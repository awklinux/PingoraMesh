use anyhow::Context;
use chrono::{DateTime, Utc};
use pingorahub_application::{HeartbeatPolicy, NodeLifecycleService};
use pingorahub_domain::NodeStatus;
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use std::time::Duration;
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct StatusWorkerConfig {
    postgres_url: String,
    max_connections: u32,
    expected_heartbeat_seconds: u64,
    interval_seconds: u64,
    run_once: bool,
    heartbeat_policy: HeartbeatPolicy,
}

#[derive(Debug, Clone)]
struct NodeSnapshot {
    node_id: Uuid,
    node_code: String,
    current_status: String,
    last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Default)]
struct ReconcileSummary {
    scanned: usize,
    changed: usize,
    unchanged: usize,
    skipped: usize,
    online: usize,
    suspect: usize,
    offline: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = StatusWorkerConfig::from_env();
    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .connect(&config.postgres_url)
        .await
        .context("failed to connect status-worker postgres")?;

    info!(
        interval_seconds = config.interval_seconds,
        expected_heartbeat_seconds = config.expected_heartbeat_seconds,
        suspect_after_missed = config.heartbeat_policy.suspect_after_missed,
        offline_after_missed = config.heartbeat_policy.offline_after_missed,
        run_once = config.run_once,
        "status-worker started"
    );

    loop {
        let summary = reconcile_node_statuses(
            &pool,
            Utc::now(),
            config.expected_heartbeat_seconds,
            &config.heartbeat_policy,
        )
        .await?;
        info!(
            scanned = summary.scanned,
            changed = summary.changed,
            unchanged = summary.unchanged,
            skipped = summary.skipped,
            online = summary.online,
            suspect = summary.suspect,
            offline = summary.offline,
            "status-worker reconciled node states"
        );

        if config.run_once {
            break;
        }

        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = tokio::time::sleep(Duration::from_secs(config.interval_seconds)) => {}
        }
    }

    info!("status-worker stopped");
    Ok(())
}

impl StatusWorkerConfig {
    fn from_env() -> Self {
        Self {
            postgres_url: std::env::var("PINGORAHUB_POSTGRES_URL").unwrap_or_else(|_| {
                "postgres://postgres:postgres@127.0.0.1:5432/pingorahub".to_string()
            }),
            max_connections: std::env::var("PINGORAHUB_POSTGRES_MAX_CONNECTIONS")
                .ok()
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(5),
            expected_heartbeat_seconds: parse_env_u64(
                "PINGORAHUB_STATUS_WORKER_EXPECTED_HEARTBEAT_SECONDS",
                15,
            ),
            interval_seconds: parse_env_u64("PINGORAHUB_STATUS_WORKER_INTERVAL_SECONDS", 15),
            run_once: parse_env_bool("PINGORAHUB_STATUS_WORKER_ONCE"),
            heartbeat_policy: HeartbeatPolicy {
                suspect_after_missed: parse_env_u32(
                    "PINGORAHUB_STATUS_WORKER_SUSPECT_AFTER_MISSED",
                    HeartbeatPolicy::default().suspect_after_missed,
                ),
                offline_after_missed: parse_env_u32(
                    "PINGORAHUB_STATUS_WORKER_OFFLINE_AFTER_MISSED",
                    HeartbeatPolicy::default().offline_after_missed,
                ),
            },
        }
    }
}

async fn reconcile_node_statuses(
    pool: &PgPool,
    now: DateTime<Utc>,
    expected_heartbeat_seconds: u64,
    policy: &HeartbeatPolicy,
) -> anyhow::Result<ReconcileSummary> {
    let rows = sqlx::query(
        r#"
        SELECT id, node_code, status::text AS status, last_seen_at
        FROM nodes
        ORDER BY node_code ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let snapshots = rows
        .into_iter()
        .map(|row| NodeSnapshot {
            node_id: row.get("id"),
            node_code: row.get("node_code"),
            current_status: row.get("status"),
            last_seen_at: row.get("last_seen_at"),
        })
        .collect::<Vec<_>>();

    let mut summary = ReconcileSummary {
        scanned: snapshots.len(),
        ..Default::default()
    };

    for snapshot in snapshots {
        let Some(next_status) =
            desired_status_for_snapshot(&snapshot, now, expected_heartbeat_seconds, policy)
        else {
            summary.skipped += 1;
            continue;
        };

        match next_status {
            NodeStatus::Online => summary.online += 1,
            NodeStatus::Suspect => summary.suspect += 1,
            NodeStatus::Offline => summary.offline += 1,
            NodeStatus::Pending | NodeStatus::Maintenance => {}
        }

        if snapshot.current_status == node_status_name(next_status) {
            summary.unchanged += 1;
            continue;
        }

        sqlx::query(
            r#"
            UPDATE nodes
            SET status = $2::node_status, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(&snapshot.node_id)
        .bind(node_status_name(next_status))
        .execute(pool)
        .await
        .with_context(|| format!("failed to update node {}", snapshot.node_code))?;
        summary.changed += 1;
    }

    Ok(summary)
}

fn desired_status_for_snapshot(
    snapshot: &NodeSnapshot,
    now: DateTime<Utc>,
    expected_heartbeat_seconds: u64,
    policy: &HeartbeatPolicy,
) -> Option<NodeStatus> {
    if snapshot.current_status == "maintenance" {
        return None;
    }

    let last_seen_at = snapshot.last_seen_at?;
    let missed = missed_heartbeats(now, last_seen_at, expected_heartbeat_seconds);
    Some(NodeLifecycleService::evaluate_status(missed, policy))
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

fn node_status_name(status: NodeStatus) -> &'static str {
    match status {
        NodeStatus::Pending => "pending",
        NodeStatus::Online => "online",
        NodeStatus::Suspect => "suspect",
        NodeStatus::Offline => "offline",
        NodeStatus::Maintenance => "maintenance",
    }
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
                .unwrap_or_else(|_| "pingorahub_status_worker=info,info".into()),
        )
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missed_heartbeat_count_is_floor_division() {
        let now = DateTime::parse_from_rfc3339("2026-04-14T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let last_seen = DateTime::parse_from_rfc3339("2026-04-14T11:59:01Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(missed_heartbeats(now, last_seen, 15), 3);
    }

    #[test]
    fn maintenance_nodes_are_not_overwritten() {
        let now = Utc::now();
        let snapshot = NodeSnapshot {
            node_id: Uuid::nil(),
            node_code: "node-1".to_string(),
            current_status: "maintenance".to_string(),
            last_seen_at: Some(now - chrono::Duration::seconds(300)),
        };
        assert!(
            desired_status_for_snapshot(&snapshot, now, 15, &HeartbeatPolicy::default()).is_none()
        );
    }

    #[test]
    fn recent_heartbeat_keeps_node_online() {
        let now = Utc::now();
        let snapshot = NodeSnapshot {
            node_id: Uuid::nil(),
            node_code: "node-1".to_string(),
            current_status: "suspect".to_string(),
            last_seen_at: Some(now - chrono::Duration::seconds(10)),
        };
        assert_eq!(
            desired_status_for_snapshot(&snapshot, now, 15, &HeartbeatPolicy::default()),
            Some(NodeStatus::Online)
        );
    }

    #[test]
    fn stale_heartbeat_marks_node_offline() {
        let now = Utc::now();
        let snapshot = NodeSnapshot {
            node_id: Uuid::nil(),
            node_code: "node-1".to_string(),
            current_status: "online".to_string(),
            last_seen_at: Some(now - chrono::Duration::seconds(120)),
        };
        assert_eq!(
            desired_status_for_snapshot(&snapshot, now, 15, &HeartbeatPolicy::default()),
            Some(NodeStatus::Offline)
        );
    }
}
