use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use pingorahub_application::ReleasePlanner;
use pingorahub_config_compiler::compile_site_bundle;
use pingorahub_dns_provider::{DnsRecordChange, ProviderConfig, build_provider};
use pingorahub_domain::{
    CacheRule, CertificateRef, Protocol as SiteProtocol, ReleaseTarget, RouteMatchType, SiteRoute,
    SiteSpec, SiteStatus, Upstream, UpstreamBalanceMethod, UpstreamEndpoint,
};
use serde_json::{Map, Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgPoolOptions};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Duration;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct FailoverWorkerConfig {
    postgres_url: String,
    max_connections: u32,
    interval_seconds: u64,
    expected_heartbeat_seconds: u64,
    run_once: bool,
}

#[derive(Debug, Clone)]
struct PolicySnapshot {
    policy_id: Uuid,
    scope_type: String,
    scope_id: Uuid,
    primary_node_id: Uuid,
    standby_node_id: Uuid,
    trigger_mode: String,
    failure_threshold: u32,
    precheck_policy: Value,
    primary_node_code: String,
    standby_node_code: String,
    primary_status: String,
    standby_status: String,
    standby_public_ip: Option<String>,
    primary_last_seen_at: Option<DateTime<Utc>>,
    primary_active_config_version: Option<String>,
    standby_active_config_version: Option<String>,
}

#[derive(Debug, Clone)]
struct SiteSnapshot {
    site_id: Uuid,
    site_code: String,
    name: String,
    domain: String,
    listen_port: u16,
    protocol: SiteProtocol,
    tls_enabled: bool,
    status: SiteStatus,
    config: Value,
    bindings: Vec<SiteBindingSnapshot>,
}

#[derive(Debug, Clone)]
struct SiteBindingSnapshot {
    node_id: Uuid,
    node_code: String,
    node_status: String,
    binding_role: String,
    priority: i32,
}

#[derive(Debug, Clone)]
struct DnsZoneSnapshot {
    zone_id: Uuid,
    provider_id: Uuid,
    provider_type: String,
    api_endpoint: Option<String>,
    credential_encrypted: String,
    zone_name: String,
    external_zone_id: String,
    status: String,
}

#[derive(Debug, Clone)]
struct DnsFailoverPlan {
    zone: DnsZoneSnapshot,
    change: DnsRecordChange,
}

#[derive(Debug, Clone)]
struct DnsRecordSnapshot {
    record_id: Uuid,
    value: String,
    ttl: i32,
}

#[derive(Debug, Default)]
struct FailoverRunSummary {
    scanned: usize,
    eligible: usize,
    switched: usize,
    skipped: usize,
    failed: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = FailoverWorkerConfig::from_env();
    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .connect(&config.postgres_url)
        .await
        .context("failed to connect failover-worker postgres")?;

    info!(
        interval_seconds = config.interval_seconds,
        expected_heartbeat_seconds = config.expected_heartbeat_seconds,
        run_once = config.run_once,
        "failover-worker started"
    );

    loop {
        let summary =
            process_failover_policies(&pool, Utc::now(), config.expected_heartbeat_seconds).await?;
        info!(
            scanned = summary.scanned,
            eligible = summary.eligible,
            switched = summary.switched,
            skipped = summary.skipped,
            failed = summary.failed,
            "failover-worker cycle finished"
        );

        if config.run_once {
            break;
        }

        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = tokio::time::sleep(Duration::from_secs(config.interval_seconds)) => {}
        }
    }

    info!("failover-worker stopped");
    Ok(())
}

impl FailoverWorkerConfig {
    fn from_env() -> Self {
        Self {
            postgres_url: std::env::var("PINGORAHUB_POSTGRES_URL").unwrap_or_else(|_| {
                "postgres://postgres:postgres@127.0.0.1:5432/pingorahub".to_string()
            }),
            max_connections: parse_env_u32("PINGORAHUB_POSTGRES_MAX_CONNECTIONS", 5),
            interval_seconds: parse_env_u64("PINGORAHUB_FAILOVER_WORKER_INTERVAL_SECONDS", 15),
            expected_heartbeat_seconds: parse_env_u64(
                "PINGORAHUB_FAILOVER_WORKER_EXPECTED_HEARTBEAT_SECONDS",
                15,
            ),
            run_once: parse_env_bool("PINGORAHUB_FAILOVER_WORKER_ONCE"),
        }
    }
}

async fn process_failover_policies(
    pool: &PgPool,
    now: DateTime<Utc>,
    expected_heartbeat_seconds: u64,
) -> Result<FailoverRunSummary> {
    let policies = load_active_policies(pool).await?;
    let mut summary = FailoverRunSummary {
        scanned: policies.len(),
        ..Default::default()
    };

    for policy in policies {
        let Some(reason) = evaluate_policy(&policy, now, expected_heartbeat_seconds) else {
            summary.skipped += 1;
            continue;
        };
        summary.eligible += 1;

        match trigger_policy_failover(pool, &policy, now, &reason).await {
            Ok(true) => summary.switched += 1,
            Ok(false) => summary.skipped += 1,
            Err(error) => {
                summary.failed += 1;
                warn!(
                    policy_id = %policy.policy_id,
                    site_id = %policy.scope_id,
                    error = %error,
                    "failover-worker failed to process policy"
                );
            }
        }
    }

    Ok(summary)
}

async fn load_active_policies(pool: &PgPool) -> Result<Vec<PolicySnapshot>> {
    let rows = sqlx::query(
        r#"
        SELECT
            p.id,
            p.scope_type,
            p.scope_id,
            p.primary_node_id,
            p.standby_node_id,
            p.trigger_mode::text AS trigger_mode,
            p.failure_threshold,
            p.precheck_policy,
            pn.node_code AS primary_node_code,
            sn.node_code AS standby_node_code,
            pn.status::text AS primary_status,
            sn.status::text AS standby_status,
            host(sn.public_ip) AS standby_public_ip,
            pn.last_seen_at AS primary_last_seen_at,
            phb.active_config_version AS primary_active_config_version,
            shb.active_config_version AS standby_active_config_version
        FROM failover_policies p
        JOIN nodes pn ON pn.id = p.primary_node_id
        JOIN nodes sn ON sn.id = p.standby_node_id
        LEFT JOIN LATERAL (
            SELECT active_config_version
            FROM node_heartbeats
            WHERE node_id = p.primary_node_id
            ORDER BY reported_at DESC
            LIMIT 1
        ) phb ON true
        LEFT JOIN LATERAL (
            SELECT active_config_version
            FROM node_heartbeats
            WHERE node_id = p.standby_node_id
            ORDER BY reported_at DESC
            LIMIT 1
        ) shb ON true
        WHERE p.status = 'active'
        ORDER BY p.created_at ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| PolicySnapshot {
            policy_id: row.get("id"),
            scope_type: row.get("scope_type"),
            scope_id: row.get("scope_id"),
            primary_node_id: row.get("primary_node_id"),
            standby_node_id: row.get("standby_node_id"),
            trigger_mode: row.get("trigger_mode"),
            failure_threshold: row.get::<i32, _>("failure_threshold").max(0).unsigned_abs(),
            precheck_policy: row.get("precheck_policy"),
            primary_node_code: row.get("primary_node_code"),
            standby_node_code: row.get("standby_node_code"),
            primary_status: row.get("primary_status"),
            standby_status: row.get("standby_status"),
            standby_public_ip: row.get("standby_public_ip"),
            primary_last_seen_at: row.get("primary_last_seen_at"),
            primary_active_config_version: row.get("primary_active_config_version"),
            standby_active_config_version: row.get("standby_active_config_version"),
        })
        .collect())
}

fn evaluate_policy(
    policy: &PolicySnapshot,
    now: DateTime<Utc>,
    expected_heartbeat_seconds: u64,
) -> Option<String> {
    if policy.scope_type != "site" {
        return None;
    }
    if !matches!(policy.trigger_mode.as_str(), "semi_auto" | "auto") {
        return None;
    }

    let require_standby_online =
        policy_bool(&policy.precheck_policy, "require_standby_online", true);
    if require_standby_online && policy.standby_status != "online" {
        return None;
    }

    let require_config_prewarm =
        policy_bool(&policy.precheck_policy, "require_config_prewarm", false);
    if require_config_prewarm {
        let prewarmed = policy.standby_active_config_version.is_some()
            && policy.standby_active_config_version == policy.primary_active_config_version;
        if !prewarmed {
            return None;
        }
    }

    let primary_missed = policy
        .primary_last_seen_at
        .map(|last_seen_at| missed_heartbeats(now, last_seen_at, expected_heartbeat_seconds))
        .unwrap_or(u32::MAX);
    let failure_threshold = policy.failure_threshold.max(1);

    if policy.primary_status == "offline" || primary_missed >= failure_threshold {
        return Some(format!(
            "primary node {} degraded; status={} missed_heartbeats={primary_missed}",
            policy.primary_node_code, policy.primary_status
        ));
    }

    None
}

async fn trigger_policy_failover(
    pool: &PgPool,
    policy: &PolicySnapshot,
    now: DateTime<Utc>,
    reason: &str,
) -> Result<bool> {
    if has_open_failover_event(pool, policy.policy_id).await? {
        return Ok(false);
    }

    let site = load_site_snapshot(pool, policy.scope_id).await?;
    if current_primary_node_id(&site.bindings) == Some(policy.standby_node_id) {
        return Ok(false);
    }

    let event_id = create_failover_event(pool, policy, now, reason).await?;
    match execute_site_failover(pool, policy, &site, event_id, now).await {
        Ok(()) => {
            finish_failover_event(pool, event_id, "switched", now).await?;
            Ok(true)
        }
        Err(error) => {
            finish_failover_event(pool, event_id, "failed", now).await?;
            Err(error)
        }
    }
}

async fn has_open_failover_event(pool: &PgPool, policy_id: Uuid) -> Result<bool> {
    let row = sqlx::query(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM failover_events
            WHERE policy_id = $1
              AND finished_at IS NULL
              AND event_status IN ('detected', 'prechecking', 'switching')
        ) AS exists
        "#,
    )
    .bind(policy_id)
    .fetch_one(pool)
    .await?;
    Ok(row.get("exists"))
}

async fn create_failover_event(
    pool: &PgPool,
    policy: &PolicySnapshot,
    now: DateTime<Utc>,
    reason: &str,
) -> Result<Uuid> {
    let event_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO failover_events (
            id, policy_id, site_id, source_node_id, target_node_id,
            trigger_reason, event_status, started_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, 'switching', $7)
        "#,
    )
    .bind(event_id)
    .bind(policy.policy_id)
    .bind(policy.scope_id)
    .bind(policy.primary_node_id)
    .bind(policy.standby_node_id)
    .bind(reason)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(event_id)
}

async fn finish_failover_event(
    pool: &PgPool,
    event_id: Uuid,
    status: &str,
    now: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE failover_events
        SET event_status = $2::failover_event_status, finished_at = $3
        WHERE id = $1
        "#,
    )
    .bind(event_id)
    .bind(status)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

async fn execute_site_failover(
    pool: &PgPool,
    policy: &PolicySnapshot,
    site: &SiteSnapshot,
    event_id: Uuid,
    now: DateTime<Utc>,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let dns_zones = load_dns_zone_snapshots(&mut tx).await?;
    let dns_failover_plan = build_dns_failover_plan(site, policy, &dns_zones)?;

    let switched_bindings = build_switched_bindings(
        &site.bindings,
        policy.primary_node_id,
        policy.standby_node_id,
    );
    save_site_bindings(&mut tx, site.site_id, &switched_bindings).await?;
    let next_policy_standby = advance_failover_policy(&mut tx, policy, &switched_bindings).await?;

    let certificates = load_site_certificates(&mut tx, site.site_id).await?;
    let release_nodes = switched_bindings.clone();
    if release_nodes.is_empty() {
        return Err(anyhow!(
            "no eligible target nodes available for site {} after failover switch",
            site.site_code
        ));
    }

    let release_version =
        ReleasePlanner::build_release_version("failover", now, now.timestamp_millis() as u64);
    let targets = release_nodes
        .iter()
        .map(|binding| ReleaseTarget {
            node_id: binding.node_id,
            site_ids: vec![site.site_id],
            preheated: binding.binding_role == "standby",
        })
        .collect::<Vec<_>>();
    let upstreams = parse_upstreams(&site.config);
    let routes = parse_routes(&site.config, &upstreams);
    let bundle = compile_site_bundle(
        SiteSpec {
            id: site.site_id,
            site_code: site.site_code.clone(),
            name: site.name.clone(),
            domain: site.domain.clone(),
            listen_port: site.listen_port,
            protocol: site.protocol,
            tls_enabled: site.tls_enabled,
            status: site.status,
            upstreams,
            routes,
            cache_rules: parse_cache_rules(&site.config),
        },
        certificates,
        targets,
        release_version.clone(),
    )?;

    let release_id = bundle.manifest.release_id;
    let reason = format!(
        "automatic failover {} -> {} for site {}",
        policy.primary_node_code, policy.standby_node_code, site.site_code
    );
    let manifest_json = serde_json::to_value(&bundle)?;

    sqlx::query(
        r#"
        INSERT INTO config_releases (
            id, release_code, scope_type, scope_id, release_type, release_version,
            manifest_json, manifest_hash, reason, status, published_at, created_by, created_at
        )
        VALUES ($1, $2, 'site', $3, 'failover', $4, $5, $6, $7, 'pending', $8, $9, $10)
        "#,
    )
    .bind(release_id)
    .bind(release_id.to_string())
    .bind(site.site_id)
    .bind(&release_version)
    .bind(manifest_json)
    .bind(&bundle.manifest.config_hash)
    .bind(&reason)
    .bind(now)
    .bind("system:failover-worker")
    .bind(now)
    .execute(&mut *tx)
    .await?;

    for binding in &release_nodes {
        sqlx::query(
            r#"
            INSERT INTO node_release_status (
                release_id, node_id, target_version, current_version, apply_status,
                apply_message, acked_at, updated_at
            )
            VALUES ($1, $2, $3, NULL, 'pending', $4, NULL, now())
            "#,
        )
        .bind(release_id)
        .bind(binding.node_id)
        .bind(&release_version)
        .bind("queued by failover-worker")
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query(
        r#"
        UPDATE sites
        SET status = 'published', updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(site.site_id)
    .execute(&mut *tx)
    .await?;

    if let Some(plan) = &dns_failover_plan {
        apply_dns_failover(&mut tx, plan, event_id, now).await?;
    }

    sqlx::query(
        r#"
        INSERT INTO audit_logs (
            operator_id, action, resource_type, resource_id, before_data, after_data, created_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind("system:failover-worker")
    .bind("failover_switch")
    .bind("site")
    .bind(site.site_id.to_string())
    .bind(json!({
        "primary_node_id": policy.primary_node_id,
        "standby_node_id": policy.standby_node_id,
        "bindings": site.bindings.iter().map(binding_to_json).collect::<Vec<_>>(),
    }))
    .bind(json!({
        "release_id": release_id,
        "release_version": release_version,
        "bindings": switched_bindings.iter().map(binding_to_json).collect::<Vec<_>>(),
        "dns_failover": dns_failover_plan.as_ref().map(dns_failover_plan_to_json),
        "next_policy_primary_node_id": policy.standby_node_id,
        "next_policy_standby_node_id": next_policy_standby,
    }))
    .bind(now)
    .execute(&mut *tx)
    .await?;

    let operation_count = enqueue_post_switch_operations(&mut tx, policy, event_id, now).await?;
    if operation_count > 0 {
        info!(
            policy_id = %policy.policy_id,
            event_id = %event_id,
            operation_count,
            "failover-worker queued post-switch operations"
        );
    }

    tx.commit().await?;
    Ok(())
}

async fn load_site_snapshot(pool: &PgPool, site_id: Uuid) -> Result<SiteSnapshot> {
    let row = sqlx::query(
        r#"
        SELECT
            s.id,
            s.site_code,
            s.name,
            s.domain,
            s.listen_port,
            s.protocol,
            s.tls_enabled,
            s.status::text AS status,
            sc.config_json
        FROM sites s
        LEFT JOIN LATERAL (
            SELECT config_json
            FROM site_configs
            WHERE site_id = s.id
            ORDER BY version DESC
            LIMIT 1
        ) sc ON true
        WHERE s.id = $1
        "#,
    )
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    let row = row.ok_or_else(|| anyhow!("site {site_id} not found"))?;
    Ok(SiteSnapshot {
        site_id: row.get("id"),
        site_code: row.get("site_code"),
        name: row.get("name"),
        domain: row.get("domain"),
        listen_port: row
            .get::<i32, _>("listen_port")
            .clamp(0, i32::from(u16::MAX)) as u16,
        protocol: decode_protocol(row.get::<String, _>("protocol").as_str()),
        tls_enabled: row.get("tls_enabled"),
        status: decode_site_status(row.get::<String, _>("status").as_str()),
        config: row
            .get::<Option<Value>, _>("config_json")
            .unwrap_or_else(|| json!({})),
        bindings: load_site_bindings(pool, site_id).await?,
    })
}

async fn load_site_bindings(pool: &PgPool, site_id: Uuid) -> Result<Vec<SiteBindingSnapshot>> {
    let rows = sqlx::query(
        r#"
        SELECT
            b.node_id,
            b.binding_role,
            b.priority,
            n.node_code,
            n.status::text AS node_status
        FROM site_node_bindings b
        JOIN nodes n ON n.id = b.node_id
        WHERE b.site_id = $1
        ORDER BY b.priority ASC, n.node_code ASC
        "#,
    )
    .bind(site_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| SiteBindingSnapshot {
            node_id: row.get("node_id"),
            node_code: row.get("node_code"),
            node_status: row.get("node_status"),
            binding_role: row.get("binding_role"),
            priority: row.get("priority"),
        })
        .collect())
}

async fn load_dns_zone_snapshots(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Vec<DnsZoneSnapshot>> {
    let rows = sqlx::query(
        r#"
        SELECT
            z.id AS zone_id,
            z.provider_id,
            z.zone_name,
            z.external_zone_id,
            z.status,
            p.provider_type::text AS provider_type,
            p.api_endpoint,
            p.credential_encrypted
        FROM dns_zones z
        JOIN dns_providers p ON p.id = z.provider_id
        WHERE z.status = 'active'
          AND p.status = 'active'
        ORDER BY length(z.zone_name) DESC, z.zone_name ASC
        "#,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| DnsZoneSnapshot {
            zone_id: row.get("zone_id"),
            provider_id: row.get("provider_id"),
            provider_type: row.get("provider_type"),
            api_endpoint: row.get("api_endpoint"),
            credential_encrypted: row.get("credential_encrypted"),
            zone_name: row.get("zone_name"),
            external_zone_id: row.get("external_zone_id"),
            status: row.get("status"),
        })
        .collect())
}

async fn load_site_certificates(
    tx: &mut Transaction<'_, Postgres>,
    site_id: Uuid,
) -> Result<Vec<CertificateRef>> {
    let rows = sqlx::query(
        r#"
        SELECT
            c.id AS certificate_id,
            c.common_name,
            c.sans,
            c.not_after,
            b.version,
            v.cert_pem,
            v.key_pem_encrypted,
            v.chain_pem
        FROM site_cert_bindings b
        JOIN certificates c ON c.id = b.certificate_id
        JOIN certificate_versions v
          ON v.certificate_id = b.certificate_id
         AND v.version = b.version
        WHERE b.site_id = $1
          AND b.is_default = TRUE
        ORDER BY b.created_at DESC
        "#,
    )
    .bind(site_id)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| CertificateRef {
            certificate_id: row.get("certificate_id"),
            version: row.get("version"),
            common_name: row.get("common_name"),
            sans: serde_json::from_value(row.get("sans")).unwrap_or_default(),
            expires_at: row.get("not_after"),
            cert_pem: row.get("cert_pem"),
            key_pem_encrypted: row.get("key_pem_encrypted"),
            chain_pem: row.get("chain_pem"),
        })
        .collect())
}

async fn apply_dns_failover(
    tx: &mut Transaction<'_, Postgres>,
    plan: &DnsFailoverPlan,
    event_id: Uuid,
    now: DateTime<Utc>,
) -> Result<()> {
    let existing = load_dns_record_snapshot(tx, plan).await?;
    let provider_config = provider_config_from_dns_zone(&plan.zone)?;
    let provider = build_provider(&provider_config)
        .with_context(|| format!("failed to build dns provider {}", plan.zone.provider_type))?;

    if existing.is_some() {
        provider
            .update_record(&plan.change)
            .await
            .with_context(|| {
                format!(
                    "failed to update dns record {} {} to {}",
                    plan.change.record_type, plan.change.host, plan.change.value
                )
            })?;
    } else {
        provider
            .create_record(&plan.change)
            .await
            .with_context(|| {
                format!(
                    "failed to create dns record {} {} to {}",
                    plan.change.record_type, plan.change.host, plan.change.value
                )
            })?;
    }

    let record_id = upsert_dns_record_snapshot(tx, plan, existing.as_ref(), now).await?;
    insert_dns_change_log(tx, plan, existing.as_ref(), record_id, event_id, now).await?;
    info!(
        zone_name = %plan.change.zone_name,
        host = %plan.change.host,
        record_type = %plan.change.record_type,
        value = %plan.change.value,
        "failover-worker switched dns record to standby node ip"
    );
    Ok(())
}

async fn load_dns_record_snapshot(
    tx: &mut Transaction<'_, Postgres>,
    plan: &DnsFailoverPlan,
) -> Result<Option<DnsRecordSnapshot>> {
    let row = sqlx::query(
        r#"
        SELECT id, value, ttl
        FROM dns_records
        WHERE zone_id = $1
          AND record_type = $2::dns_record_type
          AND host = $3
          AND status = 'active'
        ORDER BY updated_at DESC
        LIMIT 1
        "#,
    )
    .bind(plan.zone.zone_id)
    .bind(&plan.change.record_type)
    .bind(&plan.change.host)
    .fetch_optional(&mut **tx)
    .await?;

    Ok(row.map(|row| DnsRecordSnapshot {
        record_id: row.get("id"),
        value: row.get("value"),
        ttl: row.get("ttl"),
    }))
}

async fn upsert_dns_record_snapshot(
    tx: &mut Transaction<'_, Postgres>,
    plan: &DnsFailoverPlan,
    existing: Option<&DnsRecordSnapshot>,
    now: DateTime<Utc>,
) -> Result<Uuid> {
    if let Some(existing) = existing {
        sqlx::query(
            r#"
            UPDATE dns_records
            SET value = $2,
                ttl = $3,
                last_synced_at = $4,
                updated_at = $4
            WHERE id = $1
            "#,
        )
        .bind(existing.record_id)
        .bind(&plan.change.value)
        .bind(i32::try_from(plan.change.ttl).unwrap_or(i32::MAX))
        .bind(now)
        .execute(&mut **tx)
        .await?;
        return Ok(existing.record_id);
    }

    let record_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO dns_records (
            id, zone_id, record_type, host, value, ttl, routing_policy,
            status, external_record_id, last_synced_at, created_at, updated_at
        )
        VALUES ($1, $2, $3::dns_record_type, $4, $5, $6, '{}'::jsonb,
            'active', NULL, $7, $7, $7)
        "#,
    )
    .bind(record_id)
    .bind(plan.zone.zone_id)
    .bind(&plan.change.record_type)
    .bind(&plan.change.host)
    .bind(&plan.change.value)
    .bind(i32::try_from(plan.change.ttl).unwrap_or(i32::MAX))
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(record_id)
}

async fn insert_dns_change_log(
    tx: &mut Transaction<'_, Postgres>,
    plan: &DnsFailoverPlan,
    existing: Option<&DnsRecordSnapshot>,
    record_id: Uuid,
    event_id: Uuid,
    now: DateTime<Utc>,
) -> Result<()> {
    let before_data = existing.map(|record| {
        json!({
            "record_id": record.record_id,
            "zone_id": plan.zone.zone_id,
            "record_type": plan.change.record_type,
            "host": plan.change.host,
            "value": record.value,
            "ttl": record.ttl,
        })
    });
    let after_data = json!({
        "record_id": record_id,
        "zone_id": plan.zone.zone_id,
        "provider_id": plan.zone.provider_id,
        "failover_event_id": event_id,
        "record_type": plan.change.record_type,
        "host": plan.change.host,
        "value": plan.change.value,
        "ttl": plan.change.ttl,
    });

    sqlx::query(
        r#"
        INSERT INTO dns_change_logs (
            id, zone_id, record_id, change_type, before_data, after_data,
            change_status, operator_id, error_message, created_at
        )
        VALUES ($1, $2, $3, 'failover_dns_switch', $4, $5,
            'applied', 'system:failover-worker', NULL, $6)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(plan.zone.zone_id)
    .bind(record_id)
    .bind(before_data)
    .bind(after_data)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn provider_config_from_dns_zone(zone: &DnsZoneSnapshot) -> Result<ProviderConfig> {
    let credentials = serde_json::from_str(&zone.credential_encrypted)
        .context("failed to decode dns provider credentials")?;
    Ok(ProviderConfig {
        provider_type: zone.provider_type.clone(),
        api_endpoint: zone.api_endpoint.clone(),
        credentials,
    })
}

async fn save_site_bindings(
    tx: &mut Transaction<'_, Postgres>,
    site_id: Uuid,
    bindings: &[SiteBindingSnapshot],
) -> Result<()> {
    sqlx::query("DELETE FROM site_node_bindings WHERE site_id = $1")
        .bind(site_id)
        .execute(&mut **tx)
        .await?;

    for binding in bindings {
        sqlx::query(
            r#"
            INSERT INTO site_node_bindings (
                site_id, node_id, status, bind_mode, binding_role, priority, created_at, updated_at
            )
            VALUES ($1, $2, 'active', 'failover', $3, $4, now(), now())
            "#,
        )
        .bind(site_id)
        .bind(binding.node_id)
        .bind(&binding.binding_role)
        .bind(binding.priority)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn advance_failover_policy(
    tx: &mut Transaction<'_, Postgres>,
    policy: &PolicySnapshot,
    switched_bindings: &[SiteBindingSnapshot],
) -> Result<Option<Uuid>> {
    let next_primary_node_id = policy.standby_node_id;
    let Some(next_standby_node_id) =
        select_next_policy_standby(switched_bindings, next_primary_node_id)
    else {
        warn!(
            policy_id = %policy.policy_id,
            site_id = %policy.scope_id,
            next_primary_node_id = %next_primary_node_id,
            "failover-worker could not advance policy because no standby binding remains"
        );
        return Ok(None);
    };

    sqlx::query(
        r#"
        UPDATE failover_policies
        SET primary_node_id = $2,
            standby_node_id = $3,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(policy.policy_id)
    .bind(next_primary_node_id)
    .bind(next_standby_node_id)
    .execute(&mut **tx)
    .await?;

    info!(
        policy_id = %policy.policy_id,
        site_id = %policy.scope_id,
        primary_node_id = %next_primary_node_id,
        standby_node_id = %next_standby_node_id,
        "failover-worker advanced policy to next standby"
    );

    Ok(Some(next_standby_node_id))
}

fn build_switched_bindings(
    existing: &[SiteBindingSnapshot],
    primary_node_id: Uuid,
    standby_node_id: Uuid,
) -> Vec<SiteBindingSnapshot> {
    let current_primary_priority = existing
        .iter()
        .find(|binding| binding.node_id == primary_node_id && binding.binding_role == "primary")
        .map(|binding| binding.priority)
        .or_else(|| {
            existing
                .iter()
                .filter(|binding| binding.binding_role == "primary")
                .map(|binding| binding.priority)
                .min()
        })
        .unwrap_or(10);
    let standby_previous_priority = existing
        .iter()
        .find(|binding| binding.node_id == standby_node_id)
        .map(|binding| binding.priority)
        .unwrap_or(current_primary_priority + 10);
    let old_primary_new_priority =
        std::cmp::max(standby_previous_priority, current_primary_priority + 10);

    let mut switched = existing
        .iter()
        .filter(|binding| binding.node_id != primary_node_id && binding.node_id != standby_node_id)
        .cloned()
        .collect::<Vec<_>>();

    let standby_node = existing
        .iter()
        .find(|binding| binding.node_id == standby_node_id)
        .cloned()
        .unwrap_or_else(|| SiteBindingSnapshot {
            node_id: standby_node_id,
            node_code: standby_node_id.to_string(),
            node_status: "online".to_string(),
            binding_role: "standby".to_string(),
            priority: standby_previous_priority,
        });
    switched.push(SiteBindingSnapshot {
        binding_role: "primary".to_string(),
        priority: current_primary_priority,
        ..standby_node
    });

    let old_primary = existing
        .iter()
        .find(|binding| binding.node_id == primary_node_id)
        .cloned()
        .unwrap_or_else(|| SiteBindingSnapshot {
            node_id: primary_node_id,
            node_code: primary_node_id.to_string(),
            node_status: "offline".to_string(),
            binding_role: "primary".to_string(),
            priority: current_primary_priority,
        });
    switched.push(SiteBindingSnapshot {
        binding_role: "standby".to_string(),
        priority: old_primary_new_priority,
        ..old_primary
    });

    switched.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.node_code.cmp(&right.node_code))
    });
    switched
}

fn select_next_policy_standby(
    bindings: &[SiteBindingSnapshot],
    primary_node_id: Uuid,
) -> Option<Uuid> {
    bindings
        .iter()
        .filter(|binding| binding.node_id != primary_node_id && binding.binding_role == "standby")
        .min_by(|left, right| {
            standby_status_rank(&left.node_status)
                .cmp(&standby_status_rank(&right.node_status))
                .then_with(|| left.priority.cmp(&right.priority))
                .then_with(|| left.node_code.cmp(&right.node_code))
        })
        .map(|binding| binding.node_id)
}

fn standby_status_rank(status: &str) -> u8 {
    if status.eq_ignore_ascii_case("online") {
        0
    } else {
        1
    }
}

fn current_primary_node_id(bindings: &[SiteBindingSnapshot]) -> Option<Uuid> {
    bindings
        .iter()
        .filter(|binding| binding.binding_role == "primary")
        .min_by_key(|binding| binding.priority)
        .map(|binding| binding.node_id)
}

fn binding_to_json(binding: &SiteBindingSnapshot) -> Value {
    json!({
        "node_id": binding.node_id,
        "node_code": binding.node_code,
        "node_status": binding.node_status,
        "binding_role": binding.binding_role,
        "priority": binding.priority,
    })
}

fn build_dns_failover_plan(
    site: &SiteSnapshot,
    policy: &PolicySnapshot,
    zones: &[DnsZoneSnapshot],
) -> Result<Option<DnsFailoverPlan>> {
    let Some(config) = dns_failover_config(&policy.precheck_policy) else {
        return Ok(None);
    };
    if !config
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(None);
    }

    let standby_ip = policy
        .standby_public_ip
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow!(
                "dns failover is enabled but standby node {} has no public_ip",
                policy.standby_node_code
            )
        })?;
    let record_type = config
        .get("record_type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("A")
        .to_ascii_uppercase();
    validate_dns_failover_target(&record_type, standby_ip)?;

    let zone = select_dns_failover_zone(&site.domain, config, zones)?;
    let host = config
        .get("host")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&site.domain)
        .to_string();
    let ttl = config
        .get("ttl")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(60)
        .clamp(1, 86_400);

    Ok(Some(DnsFailoverPlan {
        zone: zone.clone(),
        change: DnsRecordChange {
            zone_id: Some(zone.external_zone_id.clone()),
            zone_name: zone.zone_name.clone(),
            record_type,
            host,
            value: standby_ip.to_string(),
            ttl,
        },
    }))
}

fn dns_failover_config(policy: &Value) -> Option<&Map<String, Value>> {
    policy.get("dns_failover").and_then(Value::as_object)
}

fn select_dns_failover_zone(
    site_domain: &str,
    config: &Map<String, Value>,
    zones: &[DnsZoneSnapshot],
) -> Result<DnsZoneSnapshot> {
    if let Some(zone_id) = config
        .get("zone_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
    {
        return zones
            .iter()
            .find(|zone| zone.zone_id == zone_id && zone.status == "active")
            .cloned()
            .ok_or_else(|| anyhow!("dns failover zone_id {zone_id} is not active or not found"));
    }

    if let Some(zone_name) = config
        .get("zone_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return zones
            .iter()
            .find(|zone| zone.zone_name == zone_name && zone.status == "active")
            .cloned()
            .ok_or_else(|| {
                anyhow!("dns failover zone_name {zone_name} is not active or not found")
            });
    }

    zones
        .iter()
        .filter(|zone| {
            zone.status == "active" && domain_belongs_to_zone(site_domain, &zone.zone_name)
        })
        .max_by_key(|zone| zone.zone_name.len())
        .cloned()
        .ok_or_else(|| anyhow!("no active dns zone matches site domain {site_domain}"))
}

fn domain_belongs_to_zone(domain: &str, zone_name: &str) -> bool {
    domain == zone_name || domain.ends_with(&format!(".{zone_name}"))
}

fn validate_dns_failover_target(record_type: &str, value: &str) -> Result<()> {
    match record_type {
        "A" => value
            .parse::<Ipv4Addr>()
            .map(|_| ())
            .map_err(|_| anyhow!("dns failover A record requires standby public IPv4")),
        "AAAA" => value
            .parse::<Ipv6Addr>()
            .map(|_| ())
            .map_err(|_| anyhow!("dns failover AAAA record requires standby public IPv6")),
        _ => Err(anyhow!("dns failover record_type must be A or AAAA")),
    }
}

fn dns_failover_plan_to_json(plan: &DnsFailoverPlan) -> Value {
    json!({
        "zone_id": plan.zone.zone_id,
        "provider_id": plan.zone.provider_id,
        "zone_name": plan.change.zone_name,
        "record_type": plan.change.record_type,
        "host": plan.change.host,
        "value": plan.change.value,
        "ttl": plan.change.ttl,
    })
}

async fn enqueue_post_switch_operations(
    tx: &mut Transaction<'_, Postgres>,
    policy: &PolicySnapshot,
    event_id: Uuid,
    now: DateTime<Utc>,
) -> Result<usize> {
    let Some(items) = policy
        .precheck_policy
        .get("post_switch_operations")
        .and_then(Value::as_array)
    else {
        return Ok(0);
    };

    let mut queued = 0;
    for item in items {
        let Some(spec) = parse_post_switch_operation(item, policy) else {
            continue;
        };
        let Some(template) =
            load_operation_template(tx, spec.template_id, spec.template_name.as_deref()).await?
        else {
            warn!(
                policy_id = %policy.policy_id,
                event_id = %event_id,
                template_id = spec.template_id.map(|value| value.to_string()),
                template_name = spec.template_name.as_deref().unwrap_or("-"),
                "failover-worker skipped post-switch operation because template was not found"
            );
            continue;
        };
        if template.approval_required {
            warn!(
                policy_id = %policy.policy_id,
                event_id = %event_id,
                template_name = %template.name,
                "failover-worker skipped post-switch operation because template requires approval"
            );
            continue;
        }

        sqlx::query(
            r#"
            INSERT INTO node_operations (
                id, node_id, template_id, event_id, input_params, exec_status,
                requested_by, approved_by, created_at
            )
            VALUES ($1, $2, $3, $4, $5, 'approved', $6, $7, $8)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(spec.node_id)
        .bind(template.id)
        .bind(event_id)
        .bind(&spec.input_params)
        .bind("system:failover-worker")
        .bind("system:failover-worker")
        .bind(now)
        .execute(&mut **tx)
        .await?;
        queued += 1;
    }

    Ok(queued)
}

#[derive(Debug, Clone)]
struct PostSwitchOperationSpec {
    node_id: Uuid,
    template_id: Option<Uuid>,
    template_name: Option<String>,
    input_params: Value,
}

#[derive(Debug, Clone)]
struct LoadedOperationTemplate {
    id: Uuid,
    name: String,
    approval_required: bool,
}

fn parse_post_switch_operation(
    value: &Value,
    policy: &PolicySnapshot,
) -> Option<PostSwitchOperationSpec> {
    let object = value.as_object()?;
    let template_id = object
        .get("template_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok());
    let template_name = object
        .get("template_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    if template_id.is_none() && template_name.is_none() {
        return None;
    }

    let target = object
        .get("target")
        .and_then(Value::as_str)
        .unwrap_or("standby");
    let node_id = match target {
        "standby" | "target" | "new_primary" => policy.standby_node_id,
        "primary" | "source" | "old_primary" => policy.primary_node_id,
        _ => policy.standby_node_id,
    };

    let input_params = match object.get("input_params") {
        Some(Value::Object(map)) => Value::Object(map.clone()),
        _ => json!({}),
    };

    Some(PostSwitchOperationSpec {
        node_id,
        template_id,
        template_name,
        input_params,
    })
}

async fn load_operation_template(
    tx: &mut Transaction<'_, Postgres>,
    template_id: Option<Uuid>,
    template_name: Option<&str>,
) -> Result<Option<LoadedOperationTemplate>> {
    let row = match (template_id, template_name) {
        (Some(template_id), _) => {
            sqlx::query(
                r#"
                SELECT id, name, approval_required
                FROM operation_templates
                WHERE id = $1
                "#,
            )
            .bind(template_id)
            .fetch_optional(&mut **tx)
            .await?
        }
        (None, Some(template_name)) => {
            sqlx::query(
                r#"
                SELECT id, name, approval_required
                FROM operation_templates
                WHERE name = $1
                "#,
            )
            .bind(template_name)
            .fetch_optional(&mut **tx)
            .await?
        }
        (None, None) => None,
    };

    Ok(row.map(|row| LoadedOperationTemplate {
        id: row.get("id"),
        name: row.get("name"),
        approval_required: row.get("approval_required"),
    }))
}

fn decode_protocol(raw: &str) -> SiteProtocol {
    match raw {
        "http" => SiteProtocol::Http,
        "https" => SiteProtocol::Https,
        "tcp" => SiteProtocol::Tcp,
        _ => SiteProtocol::Http,
    }
}

fn decode_site_status(raw: &str) -> SiteStatus {
    match raw {
        "draft" => SiteStatus::Draft,
        "published" => SiteStatus::Published,
        "disabled" => SiteStatus::Disabled,
        _ => SiteStatus::Draft,
    }
}

fn parse_upstreams(config: &Value) -> Vec<Upstream> {
    config
        .get("upstreams")
        .and_then(Value::as_array)
        .map(|upstreams| {
            upstreams
                .iter()
                .filter_map(|upstream| {
                    let name = upstream.get("name")?.as_str()?.to_string();
                    let balance_method = parse_upstream_balance_method(upstream);
                    let endpoints = upstream
                        .get("endpoints")
                        .and_then(Value::as_array)
                        .map(|endpoints| {
                            endpoints
                                .iter()
                                .filter_map(|endpoint| match endpoint {
                                    Value::String(address) => Some(UpstreamEndpoint {
                                        address: address.clone(),
                                        weight: 100,
                                        active: true,
                                        backup: false,
                                    }),
                                    Value::Object(map) => parse_endpoint_object(map),
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();

                    Some(Upstream {
                        name,
                        balance_method,
                        endpoints,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn parse_routes(config: &Value, upstreams: &[Upstream]) -> Vec<SiteRoute> {
    match config.get("routes") {
        Some(Value::Array(routes)) => sort_site_routes(
            routes
                .iter()
                .filter_map(parse_route_value)
                .collect::<Vec<_>>(),
        ),
        Some(_) => Vec::new(),
        None => default_site_routes(upstreams),
    }
}

fn default_site_routes(upstreams: &[Upstream]) -> Vec<SiteRoute> {
    upstreams
        .first()
        .map(|upstream| {
            vec![SiteRoute {
                name: "default".to_string(),
                enabled: true,
                match_type: RouteMatchType::PathPrefix,
                path: "/".to_string(),
                upstream: upstream.name.clone(),
                priority: 1000,
                strip_prefix: false,
            }]
        })
        .unwrap_or_default()
}

fn parse_route_value(route: &Value) -> Option<SiteRoute> {
    let map = route.as_object()?;
    let name = map.get("name")?.as_str()?.trim().to_string();
    let path = map.get("path")?.as_str()?.trim().to_string();
    let upstream = map.get("upstream")?.as_str()?.trim().to_string();
    if name.is_empty() || path.is_empty() || upstream.is_empty() {
        return None;
    }
    Some(SiteRoute {
        name,
        enabled: map.get("enabled").and_then(Value::as_bool).unwrap_or(true),
        match_type: map
            .get("match_type")
            .and_then(Value::as_str)
            .map(parse_route_match_type)
            .unwrap_or(RouteMatchType::PathPrefix),
        path,
        upstream,
        priority: map
            .get("priority")
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok())
            .unwrap_or(100),
        strip_prefix: map
            .get("strip_prefix")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn parse_route_match_type(raw: &str) -> RouteMatchType {
    match raw.trim().to_ascii_lowercase().as_str() {
        "path_exact" | "exact" | "=" => RouteMatchType::PathExact,
        _ => RouteMatchType::PathPrefix,
    }
}

fn sort_site_routes(mut routes: Vec<SiteRoute>) -> Vec<SiteRoute> {
    routes.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| right.path.len().cmp(&left.path.len()))
            .then_with(|| {
                route_match_rank(left.match_type).cmp(&route_match_rank(right.match_type))
            })
            .then_with(|| left.name.cmp(&right.name))
    });
    routes
}

fn route_match_rank(match_type: RouteMatchType) -> u8 {
    match match_type {
        RouteMatchType::PathExact => 0,
        RouteMatchType::PathPrefix => 1,
    }
}

fn parse_upstream_balance_method(upstream: &Value) -> UpstreamBalanceMethod {
    upstream
        .get("balance_method")
        .or_else(|| upstream.get("distribution_method"))
        .or_else(|| upstream.get("method"))
        .and_then(Value::as_str)
        .map(normalize_upstream_balance_method)
        .unwrap_or_default()
}

fn normalize_upstream_balance_method(raw: &str) -> UpstreamBalanceMethod {
    match raw.trim().to_ascii_lowercase().as_str() {
        "weighted_round_robin" | "weighted" | "weight" | "wrr" => {
            UpstreamBalanceMethod::WeightedRoundRobin
        }
        "least_connections" | "least_conn" | "least-connections" | "least-conn" => {
            UpstreamBalanceMethod::LeastConnections
        }
        "ip_hash" | "ip-hash" | "hash" => UpstreamBalanceMethod::IpHash,
        _ => UpstreamBalanceMethod::RoundRobin,
    }
}

fn parse_cache_rules(config: &Value) -> Vec<CacheRule> {
    config
        .get("cache_rules")
        .and_then(Value::as_array)
        .map(|rules| {
            rules
                .iter()
                .filter_map(|rule| {
                    let name = rule.get("name")?.as_str()?.trim().to_string();
                    if name.is_empty() {
                        return None;
                    }

                    let expires_seconds = rule.get("expires_seconds")?.as_u64()?;
                    if expires_seconds == 0 {
                        return None;
                    }

                    let match_extensions = rule
                        .get("match_extensions")
                        .and_then(Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(Value::as_str)
                                .map(|item| item.trim().trim_start_matches('.').to_lowercase())
                                .filter(|item| !item.is_empty())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    if match_extensions.is_empty() {
                        return None;
                    }

                    let cache_control = rule
                        .get("cache_control")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("public, max-age={expires_seconds}"));

                    Some(CacheRule {
                        name,
                        match_extensions,
                        expires_seconds,
                        cache_control,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_endpoint_object(map: &Map<String, Value>) -> Option<UpstreamEndpoint> {
    let address = map.get("address")?.as_str()?.to_string();
    let weight = map
        .get("weight")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .unwrap_or(100);
    let active = map.get("active").and_then(Value::as_bool).unwrap_or(true);
    let backup = map.get("backup").and_then(Value::as_bool).unwrap_or(false);

    Some(UpstreamEndpoint {
        address,
        weight,
        active,
        backup,
    })
}

fn policy_bool(value: &Value, key: &str, default_value: bool) -> bool {
    value
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(default_value)
}

fn missed_heartbeats(
    now: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    expected_heartbeat_seconds: u64,
) -> u32 {
    let interval_seconds = expected_heartbeat_seconds.max(1);
    let elapsed_seconds = now.signed_duration_since(last_seen_at).num_seconds().max(0) as u64;
    (elapsed_seconds / interval_seconds).min(u64::from(u32::MAX)) as u32
}

fn parse_env_u64(name: &str, default_value: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default_value)
}

fn parse_env_u32(name: &str, default_value: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(default_value)
}

fn parse_env_bool(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pingorahub_failover_worker=info,info".into()),
        )
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_policy() -> PolicySnapshot {
        PolicySnapshot {
            policy_id: Uuid::new_v4(),
            scope_type: "site".to_string(),
            scope_id: Uuid::new_v4(),
            primary_node_id: Uuid::new_v4(),
            standby_node_id: Uuid::new_v4(),
            trigger_mode: "auto".to_string(),
            failure_threshold: 3,
            precheck_policy: json!({
                "require_standby_online": true,
                "require_config_prewarm": true
            }),
            primary_node_code: "primary-a".to_string(),
            standby_node_code: "standby-b".to_string(),
            primary_status: "suspect".to_string(),
            standby_status: "online".to_string(),
            standby_public_ip: Some("203.0.113.10".to_string()),
            primary_last_seen_at: Some(Utc::now() - chrono::Duration::seconds(60)),
            primary_active_config_version: Some("rel-site-001".to_string()),
            standby_active_config_version: Some("rel-site-001".to_string()),
        }
    }

    #[test]
    fn policy_is_eligible_after_failure_threshold() {
        let now = Utc::now();
        let mut policy = sample_policy();
        policy.primary_last_seen_at = Some(now - chrono::Duration::seconds(45));
        let reason = evaluate_policy(&policy, now, 15).unwrap();
        assert!(reason.contains("missed_heartbeats=3"));
    }

    #[test]
    fn policy_skips_when_standby_not_prewarmed() {
        let now = Utc::now();
        let mut policy = sample_policy();
        policy.standby_active_config_version = Some("rel-site-000".to_string());
        assert!(evaluate_policy(&policy, now, 15).is_none());
    }

    #[test]
    fn switched_bindings_promote_standby_to_primary() {
        let primary_node_id = Uuid::new_v4();
        let standby_node_id = Uuid::new_v4();
        let bindings = vec![
            SiteBindingSnapshot {
                node_id: primary_node_id,
                node_code: "primary-a".to_string(),
                node_status: "offline".to_string(),
                binding_role: "primary".to_string(),
                priority: 10,
            },
            SiteBindingSnapshot {
                node_id: standby_node_id,
                node_code: "standby-b".to_string(),
                node_status: "online".to_string(),
                binding_role: "standby".to_string(),
                priority: 100,
            },
        ];

        let switched = build_switched_bindings(&bindings, primary_node_id, standby_node_id);
        assert_eq!(current_primary_node_id(&switched), Some(standby_node_id));
        assert_eq!(
            switched
                .iter()
                .find(|binding| binding.node_id == standby_node_id)
                .unwrap()
                .priority,
            10
        );
        assert_eq!(
            switched
                .iter()
                .find(|binding| binding.node_id == primary_node_id)
                .unwrap()
                .binding_role,
            "standby"
        );
    }

    #[test]
    fn next_policy_standby_prefers_online_standby_by_priority() {
        let new_primary_node_id = Uuid::new_v4();
        let offline_old_primary_node_id = Uuid::new_v4();
        let first_online_standby_node_id = Uuid::new_v4();
        let later_online_standby_node_id = Uuid::new_v4();
        let bindings = vec![
            SiteBindingSnapshot {
                node_id: new_primary_node_id,
                node_code: "primary-b".to_string(),
                node_status: "online".to_string(),
                binding_role: "primary".to_string(),
                priority: 10,
            },
            SiteBindingSnapshot {
                node_id: offline_old_primary_node_id,
                node_code: "primary-a".to_string(),
                node_status: "offline".to_string(),
                binding_role: "standby".to_string(),
                priority: 20,
            },
            SiteBindingSnapshot {
                node_id: later_online_standby_node_id,
                node_code: "standby-c".to_string(),
                node_status: "online".to_string(),
                binding_role: "standby".to_string(),
                priority: 40,
            },
            SiteBindingSnapshot {
                node_id: first_online_standby_node_id,
                node_code: "standby-d".to_string(),
                node_status: "online".to_string(),
                binding_role: "standby".to_string(),
                priority: 30,
            },
        ];

        assert_eq!(
            select_next_policy_standby(&bindings, new_primary_node_id),
            Some(first_online_standby_node_id)
        );
    }

    #[test]
    fn next_policy_standby_falls_back_to_offline_standby() {
        let new_primary_node_id = Uuid::new_v4();
        let offline_old_primary_node_id = Uuid::new_v4();
        let bindings = vec![
            SiteBindingSnapshot {
                node_id: new_primary_node_id,
                node_code: "primary-b".to_string(),
                node_status: "online".to_string(),
                binding_role: "primary".to_string(),
                priority: 10,
            },
            SiteBindingSnapshot {
                node_id: offline_old_primary_node_id,
                node_code: "primary-a".to_string(),
                node_status: "offline".to_string(),
                binding_role: "standby".to_string(),
                priority: 20,
            },
        ];

        assert_eq!(
            select_next_policy_standby(&bindings, new_primary_node_id),
            Some(offline_old_primary_node_id)
        );
    }

    #[test]
    fn parse_routes_preserves_configured_routes() {
        let upstreams = vec![
            Upstream {
                name: "web".to_string(),
                balance_method: UpstreamBalanceMethod::RoundRobin,
                endpoints: vec![],
            },
            Upstream {
                name: "api".to_string(),
                balance_method: UpstreamBalanceMethod::RoundRobin,
                endpoints: vec![],
            },
        ];
        let routes = parse_routes(
            &json!({
                "routes": [
                    {
                        "name": "default",
                        "enabled": true,
                        "match_type": "path_prefix",
                        "path": "/",
                        "upstream": "web",
                        "priority": 1000
                    },
                    {
                        "name": "api-route",
                        "enabled": true,
                        "match_type": "path_prefix",
                        "path": "/api",
                        "upstream": "api",
                        "priority": 10
                    }
                ]
            }),
            &upstreams,
        );

        assert_eq!(routes[0].upstream, "api");
        assert_eq!(routes[1].path, "/");
    }

    #[test]
    fn parse_routes_generates_legacy_default_route() {
        let upstreams = vec![Upstream {
            name: "legacy-origin".to_string(),
            balance_method: UpstreamBalanceMethod::RoundRobin,
            endpoints: vec![],
        }];

        let routes = parse_routes(&json!({}), &upstreams);

        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].path, "/");
        assert_eq!(routes[0].upstream, "legacy-origin");
    }

    #[test]
    fn parse_post_switch_operation_defaults_to_standby_target() {
        let policy = sample_policy();
        let spec = parse_post_switch_operation(
            &json!({
                "template_name": "vip-bind",
                "input_params": {
                    "vip": "10.10.10.10/32",
                    "iface": "eth1"
                }
            }),
            &policy,
        )
        .unwrap();

        assert_eq!(spec.node_id, policy.standby_node_id);
        assert_eq!(spec.template_name.as_deref(), Some("vip-bind"));
        assert_eq!(spec.input_params["vip"], "10.10.10.10/32");
    }

    #[test]
    fn dns_failover_plan_targets_standby_public_ip() {
        let mut policy = sample_policy();
        policy.standby_public_ip = Some("203.0.113.20".to_string());
        policy.precheck_policy = json!({
            "dns_failover": {
                "enabled": true
            }
        });

        let plan = build_dns_failover_plan(
            &SiteSnapshot {
                site_id: policy.scope_id,
                site_code: "site-a".to_string(),
                name: "Site A".to_string(),
                domain: "www.example.com".to_string(),
                listen_port: 443,
                protocol: SiteProtocol::Https,
                tls_enabled: true,
                status: SiteStatus::Published,
                config: json!({}),
                bindings: vec![],
            },
            &policy,
            &[DnsZoneSnapshot {
                zone_id: Uuid::new_v4(),
                provider_id: Uuid::new_v4(),
                provider_type: "noop".to_string(),
                api_endpoint: None,
                credential_encrypted: "{}".to_string(),
                zone_name: "example.com".to_string(),
                external_zone_id: "noop-example.com".to_string(),
                status: "active".to_string(),
            }],
        )
        .unwrap()
        .unwrap();

        assert_eq!(plan.change.zone_name, "example.com");
        assert_eq!(plan.change.record_type, "A");
        assert_eq!(plan.change.host, "www.example.com");
        assert_eq!(plan.change.value, "203.0.113.20");
        assert_eq!(plan.change.ttl, 60);
    }
}
