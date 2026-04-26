use async_trait::async_trait;
use chrono::{DateTime, Utc};
use pingorahub_config_compiler::CompiledConfigBundle;
use pingorahub_domain::{
    CacheRule, CertificateRef, NodeIdentity, NodeRuntimeState, NodeStatus,
    Protocol as SiteProtocol, SiteSpec, SiteStatus, Upstream, UpstreamBalanceMethod,
    UpstreamEndpoint,
};
use serde_json::{Map, Value, json};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use std::collections::HashMap;
use thiserror::Error;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlx error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("redis error: {0}")]
    Redis(String),
}

#[derive(Debug, Clone)]
pub struct NodeRecord {
    pub identity: NodeIdentity,
    pub runtime: NodeRuntimeState,
    pub labels: Value,
    pub hostname: Option<String>,
    pub public_ip: Option<String>,
    pub private_ip: Option<String>,
    pub bootstrap_token: Option<String>,
    pub pingora_version: Option<String>,
    pub agent_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HeartbeatRecord {
    pub node_id: Uuid,
    pub pingora_version: String,
    pub agent_version: String,
    pub active_config_version: Option<String>,
    pub site_count: u32,
    pub cpu_usage: f32,
    pub mem_usage: f32,
    pub disk_usage: f32,
    pub health_score: u8,
    pub reported_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct SiteBindingRecord {
    pub node_id: Uuid,
    pub binding_role: String,
    pub priority: i32,
}

#[derive(Debug, Clone)]
pub struct SiteRecord {
    pub id: Uuid,
    pub site_code: String,
    pub name: String,
    pub domain: String,
    pub listen_port: u16,
    pub protocol: SiteProtocol,
    pub tls_enabled: bool,
    pub status: SiteStatus,
    pub version: u32,
    pub config: Value,
    pub bindings: Vec<SiteBindingRecord>,
}

impl SiteRecord {
    pub fn to_spec(&self) -> SiteSpec {
        SiteSpec {
            id: self.id,
            site_code: self.site_code.clone(),
            name: self.name.clone(),
            domain: self.domain.clone(),
            listen_port: self.listen_port,
            protocol: self.protocol,
            tls_enabled: self.tls_enabled,
            status: self.status,
            upstreams: parse_upstreams(&self.config),
            cache_rules: parse_cache_rules(&self.config),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NodeAckRecord {
    pub apply_status: String,
    pub current_version: Option<String>,
    pub message: Option<String>,
    pub acked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct ReleaseRecord {
    pub release_id: Uuid,
    pub scope_type: String,
    pub scope_id: Option<Uuid>,
    pub release_type: String,
    pub release_version: String,
    pub reason: String,
    pub status: String,
    pub bundle: CompiledConfigBundle,
    pub target_node_ids: Vec<Uuid>,
    pub ack_status: HashMap<Uuid, NodeAckRecord>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct DnsProviderRecord {
    pub provider_id: Uuid,
    pub name: String,
    pub provider_type: String,
    pub api_endpoint: Option<String>,
    pub credential_encrypted: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct DnsZoneRecord {
    pub zone_id: Uuid,
    pub provider_id: Uuid,
    pub zone_name: String,
    pub external_zone_id: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CertificateRecord {
    pub certificate_id: Uuid,
    pub cert_code: String,
    pub common_name: String,
    pub sans: Vec<String>,
    pub fingerprint_sha256: String,
    pub status: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CertificateOrderRecord {
    pub order_id: Uuid,
    pub site_id: Option<Uuid>,
    pub certificate_id: Option<Uuid>,
    pub zone_id: Uuid,
    pub order_type: String,
    pub acme_provider: String,
    pub challenge_type: String,
    pub challenge_payload: Value,
    pub order_status: String,
    pub error_message: Option<String>,
    pub certificate_expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AdminUserRecord {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub password_hash: String,
    pub status: String,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AdminSessionRecord {
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub session_token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AdminSessionContext {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub status: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct OperationTemplateRecord {
    pub template_id: Uuid,
    pub name: String,
    pub operation_type: String,
    pub command_template: String,
    pub allowed_params: Value,
    pub timeout_seconds: u32,
    pub run_as_user: String,
    pub approval_required: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NodeOperationRecord {
    pub operation_id: Uuid,
    pub node_id: Uuid,
    pub template_id: Uuid,
    pub event_id: Option<Uuid>,
    pub input_params: Value,
    pub exec_status: String,
    pub requested_by: String,
    pub approved_by: Option<String>,
    pub exit_code: Option<i32>,
    pub stdout_log: Option<String>,
    pub stderr_log: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ClaimedNodeOperation {
    pub operation: NodeOperationRecord,
    pub template: OperationTemplateRecord,
}

#[derive(Debug, Clone, Default)]
pub struct NodeDeleteGuard {
    pub bound_site_codes: Vec<String>,
    pub failover_policy_count: usize,
}

#[async_trait]
pub trait HubRepository: Send + Sync {
    async fn save_node(&self, node: &NodeRecord) -> Result<(), StoreError>;
    async fn node(&self, node_id: Uuid) -> Result<Option<NodeRecord>, StoreError>;
    async fn node_id_by_code(&self, node_code: &str) -> Result<Option<Uuid>, StoreError>;
    async fn list_nodes(&self) -> Result<Vec<NodeRecord>, StoreError>;
    async fn node_delete_guard(&self, node_id: Uuid) -> Result<NodeDeleteGuard, StoreError>;
    async fn delete_node(&self, node_id: Uuid) -> Result<(), StoreError>;
    async fn record_heartbeat(&self, heartbeat: &HeartbeatRecord) -> Result<(), StoreError>;

    async fn save_site(&self, site: &SiteRecord) -> Result<(), StoreError>;
    async fn site(&self, site_id: Uuid) -> Result<Option<SiteRecord>, StoreError>;
    async fn site_id_by_code(&self, site_code: &str) -> Result<Option<Uuid>, StoreError>;
    async fn list_sites(&self) -> Result<Vec<SiteRecord>, StoreError>;
    async fn delete_site(&self, site_id: Uuid) -> Result<(), StoreError>;
    async fn update_active_failover_policy_targets(
        &self,
        site_id: Uuid,
        primary_node_id: Uuid,
        standby_node_id: Uuid,
    ) -> Result<u64, StoreError>;

    async fn save_release(&self, release: &ReleaseRecord) -> Result<(), StoreError>;
    async fn release(&self, release_id: Uuid) -> Result<Option<ReleaseRecord>, StoreError>;
    async fn release_by_version(&self, version: &str) -> Result<Option<ReleaseRecord>, StoreError>;
    async fn list_releases(&self) -> Result<Vec<ReleaseRecord>, StoreError>;
    async fn latest_release_id_for_node(&self, node_id: Uuid) -> Result<Option<Uuid>, StoreError>;
    async fn update_release_ack(
        &self,
        release_id: Uuid,
        node_id: Uuid,
        ack: &NodeAckRecord,
    ) -> Result<(), StoreError>;

    async fn save_dns_provider(&self, provider: &DnsProviderRecord) -> Result<(), StoreError>;
    async fn dns_provider(
        &self,
        provider_id: Uuid,
    ) -> Result<Option<DnsProviderRecord>, StoreError>;
    async fn dns_provider_by_name(
        &self,
        name: &str,
    ) -> Result<Option<DnsProviderRecord>, StoreError>;
    async fn list_dns_providers(&self) -> Result<Vec<DnsProviderRecord>, StoreError>;
    async fn delete_dns_provider(&self, provider_id: Uuid) -> Result<(), StoreError>;

    async fn save_dns_zone(&self, zone: &DnsZoneRecord) -> Result<(), StoreError>;
    async fn dns_zone(&self, zone_id: Uuid) -> Result<Option<DnsZoneRecord>, StoreError>;
    async fn dns_zone_by_provider_name(
        &self,
        provider_id: Uuid,
        zone_name: &str,
    ) -> Result<Option<DnsZoneRecord>, StoreError>;
    async fn list_dns_zones(&self) -> Result<Vec<DnsZoneRecord>, StoreError>;
    async fn delete_dns_zone(&self, zone_id: Uuid) -> Result<(), StoreError>;

    async fn save_certificate(&self, certificate: &CertificateRecord) -> Result<(), StoreError>;
    async fn site_certificates(&self, site_id: Uuid) -> Result<Vec<CertificateRef>, StoreError>;
    async fn save_certificate_order(
        &self,
        order: &CertificateOrderRecord,
    ) -> Result<(), StoreError>;
    async fn certificate_order(
        &self,
        order_id: Uuid,
    ) -> Result<Option<CertificateOrderRecord>, StoreError>;
    async fn list_certificate_orders(&self) -> Result<Vec<CertificateOrderRecord>, StoreError>;

    async fn save_admin_user(&self, user: &AdminUserRecord) -> Result<(), StoreError>;
    async fn admin_user_by_username(
        &self,
        username: &str,
    ) -> Result<Option<AdminUserRecord>, StoreError>;
    async fn touch_admin_user_login(
        &self,
        user_id: Uuid,
        last_login_at: DateTime<Utc>,
    ) -> Result<(), StoreError>;

    async fn save_admin_session(&self, session: &AdminSessionRecord) -> Result<(), StoreError>;
    async fn admin_session_by_token_hash(
        &self,
        session_token_hash: &str,
    ) -> Result<Option<AdminSessionContext>, StoreError>;
    async fn touch_admin_session(
        &self,
        session_token_hash: &str,
        last_seen_at: DateTime<Utc>,
    ) -> Result<(), StoreError>;
    async fn delete_admin_session(&self, session_token_hash: &str) -> Result<(), StoreError>;
    async fn delete_admin_sessions_for_user(
        &self,
        user_id: Uuid,
        exclude_session_token_hash: Option<&str>,
    ) -> Result<(), StoreError>;

    async fn save_operation_template(
        &self,
        template: &OperationTemplateRecord,
    ) -> Result<(), StoreError>;
    async fn operation_template(
        &self,
        template_id: Uuid,
    ) -> Result<Option<OperationTemplateRecord>, StoreError>;
    async fn operation_template_by_name(
        &self,
        name: &str,
    ) -> Result<Option<OperationTemplateRecord>, StoreError>;
    async fn list_operation_templates(&self) -> Result<Vec<OperationTemplateRecord>, StoreError>;
    async fn delete_operation_template(&self, template_id: Uuid) -> Result<(), StoreError>;

    async fn save_node_operation(&self, operation: &NodeOperationRecord) -> Result<(), StoreError>;
    async fn node_operation(
        &self,
        operation_id: Uuid,
    ) -> Result<Option<NodeOperationRecord>, StoreError>;
    async fn list_node_operations(
        &self,
        node_id: Uuid,
    ) -> Result<Vec<NodeOperationRecord>, StoreError>;
    async fn claim_next_node_operation(
        &self,
        node_id: Uuid,
    ) -> Result<Option<ClaimedNodeOperation>, StoreError>;
    async fn update_node_operation(
        &self,
        operation: &NodeOperationRecord,
    ) -> Result<(), StoreError>;
}

#[derive(Debug, Default)]
struct MemoryState {
    nodes: HashMap<Uuid, NodeRecord>,
    node_ids_by_code: HashMap<String, Uuid>,
    sites: HashMap<Uuid, SiteRecord>,
    site_ids_by_code: HashMap<String, Uuid>,
    releases: HashMap<Uuid, ReleaseRecord>,
    release_ids_by_version: HashMap<String, Uuid>,
    latest_release_by_node: HashMap<Uuid, Uuid>,
    dns_providers: HashMap<Uuid, DnsProviderRecord>,
    dns_provider_ids_by_name: HashMap<String, Uuid>,
    dns_zones: HashMap<Uuid, DnsZoneRecord>,
    certificates: HashMap<Uuid, CertificateRecord>,
    site_certificates: HashMap<Uuid, Vec<CertificateRef>>,
    certificate_orders: HashMap<Uuid, CertificateOrderRecord>,
    admin_users: HashMap<Uuid, AdminUserRecord>,
    admin_user_ids_by_username: HashMap<String, Uuid>,
    admin_sessions: HashMap<String, AdminSessionRecord>,
    operation_templates: HashMap<Uuid, OperationTemplateRecord>,
    operation_template_ids_by_name: HashMap<String, Uuid>,
    node_operations: HashMap<Uuid, NodeOperationRecord>,
}

#[derive(Debug, Default)]
pub struct MemoryRepository {
    inner: RwLock<MemoryState>,
}

#[derive(Debug, Clone)]
pub struct PostgresRepository {
    pool: PgPool,
}

impl PostgresRepository {
    pub async fn connect(url: &str, max_connections: u32) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(url)
            .await?;
        Ok(Self { pool })
    }

    fn decode_node_status(raw: &str) -> NodeStatus {
        match raw {
            "pending" => NodeStatus::Pending,
            "online" => NodeStatus::Online,
            "suspect" => NodeStatus::Suspect,
            "offline" => NodeStatus::Offline,
            "maintenance" => NodeStatus::Maintenance,
            _ => NodeStatus::Pending,
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

    fn decode_protocol(raw: &str) -> SiteProtocol {
        match raw {
            "http" => SiteProtocol::Http,
            "https" => SiteProtocol::Https,
            "tcp" => SiteProtocol::Tcp,
            _ => SiteProtocol::Http,
        }
    }

    async fn load_site_bindings(
        &self,
        site_id: Uuid,
    ) -> Result<Vec<SiteBindingRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT node_id, binding_role, priority
            FROM site_node_bindings
            WHERE site_id = $1
            ORDER BY priority ASC
            "#,
        )
        .bind(site_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| SiteBindingRecord {
                node_id: row.get("node_id"),
                binding_role: row.get("binding_role"),
                priority: row.get("priority"),
            })
            .collect())
    }

    async fn load_release_acks(
        &self,
        release_id: Uuid,
    ) -> Result<HashMap<Uuid, NodeAckRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT node_id, apply_status::text AS apply_status, current_version, apply_message, acked_at
            FROM node_release_status
            WHERE release_id = $1
            "#,
        )
        .bind(release_id)
        .fetch_all(&self.pool)
        .await?;

        let mut ack_status = HashMap::new();
        for row in rows {
            let node_id: Uuid = row.get("node_id");
            ack_status.insert(
                node_id,
                NodeAckRecord {
                    apply_status: row.get("apply_status"),
                    current_version: row.get("current_version"),
                    message: row.get("apply_message"),
                    acked_at: row.get("acked_at"),
                },
            );
        }

        Ok(ack_status)
    }
}

#[async_trait]
impl HubRepository for MemoryRepository {
    async fn save_node(&self, node: &NodeRecord) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        state
            .node_ids_by_code
            .insert(node.identity.node_code.clone(), node.identity.id);
        state.nodes.insert(node.identity.id, node.clone());
        Ok(())
    }

    async fn node(&self, node_id: Uuid) -> Result<Option<NodeRecord>, StoreError> {
        let state = self.inner.read().await;
        Ok(state.nodes.get(&node_id).cloned())
    }

    async fn node_id_by_code(&self, node_code: &str) -> Result<Option<Uuid>, StoreError> {
        let state = self.inner.read().await;
        Ok(state.node_ids_by_code.get(node_code).copied())
    }

    async fn list_nodes(&self) -> Result<Vec<NodeRecord>, StoreError> {
        let state = self.inner.read().await;
        Ok(state.nodes.values().cloned().collect())
    }

    async fn node_delete_guard(&self, node_id: Uuid) -> Result<NodeDeleteGuard, StoreError> {
        let state = self.inner.read().await;
        let mut bound_site_codes = state
            .sites
            .values()
            .filter(|site| {
                site.bindings
                    .iter()
                    .any(|binding| binding.node_id == node_id)
            })
            .map(|site| site.site_code.clone())
            .collect::<Vec<_>>();
        bound_site_codes.sort();
        bound_site_codes.dedup();
        Ok(NodeDeleteGuard {
            bound_site_codes,
            failover_policy_count: 0,
        })
    }

    async fn delete_node(&self, node_id: Uuid) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        let Some(node) = state.nodes.remove(&node_id) else {
            return Ok(());
        };

        state.node_ids_by_code.remove(&node.identity.node_code);
        state.latest_release_by_node.remove(&node_id);

        for site in state.sites.values_mut() {
            site.bindings.retain(|binding| binding.node_id != node_id);
        }

        for release in state.releases.values_mut() {
            release
                .target_node_ids
                .retain(|candidate| *candidate != node_id);
            release.ack_status.remove(&node_id);
            release.status = aggregate_release_status(&release.ack_status);
        }

        Ok(())
    }

    async fn record_heartbeat(&self, heartbeat: &HeartbeatRecord) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        if let Some(node) = state.nodes.get_mut(&heartbeat.node_id) {
            if node.identity.status != NodeStatus::Maintenance {
                node.identity.status = NodeStatus::Online;
            }
            node.pingora_version = Some(heartbeat.pingora_version.clone());
            node.agent_version = Some(heartbeat.agent_version.clone());
            node.runtime.active_config_version = heartbeat.active_config_version.clone();
            node.runtime.site_count = heartbeat.site_count;
            node.runtime.health_score = heartbeat.health_score;
            node.runtime.last_seen_at = Some(heartbeat.reported_at);
        }
        Ok(())
    }

    async fn save_site(&self, site: &SiteRecord) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        state
            .site_ids_by_code
            .insert(site.site_code.clone(), site.id);
        state.sites.insert(site.id, site.clone());
        Ok(())
    }

    async fn site(&self, site_id: Uuid) -> Result<Option<SiteRecord>, StoreError> {
        let state = self.inner.read().await;
        Ok(state.sites.get(&site_id).cloned())
    }

    async fn site_id_by_code(&self, site_code: &str) -> Result<Option<Uuid>, StoreError> {
        let state = self.inner.read().await;
        Ok(state.site_ids_by_code.get(site_code).copied())
    }

    async fn list_sites(&self) -> Result<Vec<SiteRecord>, StoreError> {
        let state = self.inner.read().await;
        Ok(state.sites.values().cloned().collect())
    }

    async fn delete_site(&self, site_id: Uuid) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        let Some(site) = state.sites.remove(&site_id) else {
            return Ok(());
        };
        state.site_ids_by_code.remove(&site.site_code);
        state.site_certificates.remove(&site_id);
        for order in state.certificate_orders.values_mut() {
            if order.site_id == Some(site_id) {
                order.site_id = None;
            }
        }
        Ok(())
    }

    async fn update_active_failover_policy_targets(
        &self,
        _site_id: Uuid,
        _primary_node_id: Uuid,
        _standby_node_id: Uuid,
    ) -> Result<u64, StoreError> {
        Ok(0)
    }

    async fn save_release(&self, release: &ReleaseRecord) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        state
            .release_ids_by_version
            .insert(release.release_version.clone(), release.release_id);
        for node_id in &release.target_node_ids {
            state
                .latest_release_by_node
                .insert(*node_id, release.release_id);
        }
        state.releases.insert(release.release_id, release.clone());
        Ok(())
    }

    async fn release(&self, release_id: Uuid) -> Result<Option<ReleaseRecord>, StoreError> {
        let state = self.inner.read().await;
        Ok(state.releases.get(&release_id).cloned())
    }

    async fn release_by_version(&self, version: &str) -> Result<Option<ReleaseRecord>, StoreError> {
        let state = self.inner.read().await;
        let Some(release_id) = state.release_ids_by_version.get(version) else {
            return Ok(None);
        };
        Ok(state.releases.get(release_id).cloned())
    }

    async fn list_releases(&self) -> Result<Vec<ReleaseRecord>, StoreError> {
        let state = self.inner.read().await;
        Ok(state.releases.values().cloned().collect())
    }

    async fn latest_release_id_for_node(&self, node_id: Uuid) -> Result<Option<Uuid>, StoreError> {
        let state = self.inner.read().await;
        Ok(state.latest_release_by_node.get(&node_id).copied())
    }

    async fn update_release_ack(
        &self,
        release_id: Uuid,
        node_id: Uuid,
        ack: &NodeAckRecord,
    ) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        if let Some(release) = state.releases.get_mut(&release_id) {
            release.ack_status.insert(node_id, ack.clone());
            release.status = aggregate_release_status(&release.ack_status);
        }
        Ok(())
    }

    async fn save_dns_provider(&self, provider: &DnsProviderRecord) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        state
            .dns_provider_ids_by_name
            .insert(provider.name.clone(), provider.provider_id);
        state
            .dns_providers
            .insert(provider.provider_id, provider.clone());
        Ok(())
    }

    async fn dns_provider(
        &self,
        provider_id: Uuid,
    ) -> Result<Option<DnsProviderRecord>, StoreError> {
        let state = self.inner.read().await;
        Ok(state.dns_providers.get(&provider_id).cloned())
    }

    async fn dns_provider_by_name(
        &self,
        name: &str,
    ) -> Result<Option<DnsProviderRecord>, StoreError> {
        let state = self.inner.read().await;
        let Some(provider_id) = state.dns_provider_ids_by_name.get(name) else {
            return Ok(None);
        };
        Ok(state.dns_providers.get(provider_id).cloned())
    }

    async fn list_dns_providers(&self) -> Result<Vec<DnsProviderRecord>, StoreError> {
        let state = self.inner.read().await;
        Ok(state.dns_providers.values().cloned().collect())
    }

    async fn delete_dns_provider(&self, provider_id: Uuid) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        let Some(provider) = state.dns_providers.remove(&provider_id) else {
            return Ok(());
        };
        state.dns_provider_ids_by_name.remove(&provider.name);
        let zone_ids = state
            .dns_zones
            .values()
            .filter(|zone| zone.provider_id == provider_id)
            .map(|zone| zone.zone_id)
            .collect::<Vec<_>>();
        for zone_id in zone_ids {
            state.dns_zones.remove(&zone_id);
        }
        Ok(())
    }

    async fn save_dns_zone(&self, zone: &DnsZoneRecord) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        state.dns_zones.insert(zone.zone_id, zone.clone());
        Ok(())
    }

    async fn dns_zone(&self, zone_id: Uuid) -> Result<Option<DnsZoneRecord>, StoreError> {
        let state = self.inner.read().await;
        Ok(state.dns_zones.get(&zone_id).cloned())
    }

    async fn dns_zone_by_provider_name(
        &self,
        provider_id: Uuid,
        zone_name: &str,
    ) -> Result<Option<DnsZoneRecord>, StoreError> {
        let state = self.inner.read().await;
        Ok(state
            .dns_zones
            .values()
            .find(|zone| zone.provider_id == provider_id && zone.zone_name == zone_name)
            .cloned())
    }

    async fn list_dns_zones(&self) -> Result<Vec<DnsZoneRecord>, StoreError> {
        let state = self.inner.read().await;
        Ok(state.dns_zones.values().cloned().collect())
    }

    async fn delete_dns_zone(&self, zone_id: Uuid) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        state.dns_zones.remove(&zone_id);
        Ok(())
    }

    async fn save_certificate(&self, certificate: &CertificateRecord) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        state
            .certificates
            .insert(certificate.certificate_id, certificate.clone());
        Ok(())
    }

    async fn site_certificates(&self, site_id: Uuid) -> Result<Vec<CertificateRef>, StoreError> {
        let state = self.inner.read().await;
        Ok(state
            .site_certificates
            .get(&site_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn save_certificate_order(
        &self,
        order: &CertificateOrderRecord,
    ) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        state
            .certificate_orders
            .insert(order.order_id, order.clone());
        Ok(())
    }

    async fn certificate_order(
        &self,
        order_id: Uuid,
    ) -> Result<Option<CertificateOrderRecord>, StoreError> {
        let state = self.inner.read().await;
        Ok(state.certificate_orders.get(&order_id).cloned())
    }

    async fn list_certificate_orders(&self) -> Result<Vec<CertificateOrderRecord>, StoreError> {
        let state = self.inner.read().await;
        Ok(state.certificate_orders.values().cloned().collect())
    }

    async fn save_admin_user(&self, user: &AdminUserRecord) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        state
            .admin_user_ids_by_username
            .insert(user.username.clone(), user.user_id);
        state.admin_users.insert(user.user_id, user.clone());
        Ok(())
    }

    async fn admin_user_by_username(
        &self,
        username: &str,
    ) -> Result<Option<AdminUserRecord>, StoreError> {
        let state = self.inner.read().await;
        let Some(user_id) = state.admin_user_ids_by_username.get(username) else {
            return Ok(None);
        };
        Ok(state.admin_users.get(user_id).cloned())
    }

    async fn touch_admin_user_login(
        &self,
        user_id: Uuid,
        last_login_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        if let Some(user) = state.admin_users.get_mut(&user_id) {
            user.last_login_at = Some(last_login_at);
        }
        Ok(())
    }

    async fn save_admin_session(&self, session: &AdminSessionRecord) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        state
            .admin_sessions
            .insert(session.session_token_hash.clone(), session.clone());
        Ok(())
    }

    async fn admin_session_by_token_hash(
        &self,
        session_token_hash: &str,
    ) -> Result<Option<AdminSessionContext>, StoreError> {
        let state = self.inner.read().await;
        let Some(session) = state.admin_sessions.get(session_token_hash) else {
            return Ok(None);
        };
        let Some(user) = state.admin_users.get(&session.user_id) else {
            return Ok(None);
        };
        Ok(Some(AdminSessionContext {
            user_id: user.user_id,
            username: user.username.clone(),
            display_name: user.display_name.clone(),
            status: user.status.clone(),
            expires_at: session.expires_at,
        }))
    }

    async fn touch_admin_session(
        &self,
        session_token_hash: &str,
        last_seen_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        if let Some(session) = state.admin_sessions.get_mut(session_token_hash) {
            session.last_seen_at = last_seen_at;
        }
        Ok(())
    }

    async fn delete_admin_session(&self, session_token_hash: &str) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        state.admin_sessions.remove(session_token_hash);
        Ok(())
    }

    async fn delete_admin_sessions_for_user(
        &self,
        user_id: Uuid,
        exclude_session_token_hash: Option<&str>,
    ) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        state.admin_sessions.retain(|session_token_hash, session| {
            session.user_id != user_id
                || Some(session_token_hash.as_str()) == exclude_session_token_hash
        });
        Ok(())
    }

    async fn save_operation_template(
        &self,
        template: &OperationTemplateRecord,
    ) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        state
            .operation_template_ids_by_name
            .insert(template.name.clone(), template.template_id);
        state
            .operation_templates
            .insert(template.template_id, template.clone());
        Ok(())
    }

    async fn operation_template(
        &self,
        template_id: Uuid,
    ) -> Result<Option<OperationTemplateRecord>, StoreError> {
        let state = self.inner.read().await;
        Ok(state.operation_templates.get(&template_id).cloned())
    }

    async fn operation_template_by_name(
        &self,
        name: &str,
    ) -> Result<Option<OperationTemplateRecord>, StoreError> {
        let state = self.inner.read().await;
        let Some(template_id) = state.operation_template_ids_by_name.get(name) else {
            return Ok(None);
        };
        Ok(state.operation_templates.get(template_id).cloned())
    }

    async fn list_operation_templates(&self) -> Result<Vec<OperationTemplateRecord>, StoreError> {
        let state = self.inner.read().await;
        Ok(state.operation_templates.values().cloned().collect())
    }

    async fn delete_operation_template(&self, template_id: Uuid) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        let Some(template) = state.operation_templates.remove(&template_id) else {
            return Ok(());
        };
        state.operation_template_ids_by_name.remove(&template.name);
        state
            .node_operations
            .retain(|_, operation| operation.template_id != template_id);
        Ok(())
    }

    async fn save_node_operation(&self, operation: &NodeOperationRecord) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        state
            .node_operations
            .insert(operation.operation_id, operation.clone());
        Ok(())
    }

    async fn node_operation(
        &self,
        operation_id: Uuid,
    ) -> Result<Option<NodeOperationRecord>, StoreError> {
        let state = self.inner.read().await;
        Ok(state.node_operations.get(&operation_id).cloned())
    }

    async fn list_node_operations(
        &self,
        node_id: Uuid,
    ) -> Result<Vec<NodeOperationRecord>, StoreError> {
        let state = self.inner.read().await;
        let mut operations = state
            .node_operations
            .values()
            .filter(|operation| operation.node_id == node_id)
            .cloned()
            .collect::<Vec<_>>();
        operations.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(operations)
    }

    async fn claim_next_node_operation(
        &self,
        node_id: Uuid,
    ) -> Result<Option<ClaimedNodeOperation>, StoreError> {
        let mut state = self.inner.write().await;
        let next_operation_id = state
            .node_operations
            .values()
            .filter(|operation| operation.node_id == node_id && operation.exec_status == "approved")
            .min_by_key(|operation| operation.created_at)
            .map(|operation| operation.operation_id);

        let Some(operation_id) = next_operation_id else {
            return Ok(None);
        };

        let Some(operation) = state.node_operations.get_mut(&operation_id) else {
            return Ok(None);
        };
        operation.exec_status = "running".to_string();
        operation.started_at = Some(Utc::now());
        let claimed_operation = operation.clone();
        let Some(template) = state
            .operation_templates
            .get(&claimed_operation.template_id)
            .cloned()
        else {
            return Ok(None);
        };

        Ok(Some(ClaimedNodeOperation {
            operation: claimed_operation,
            template,
        }))
    }

    async fn update_node_operation(
        &self,
        operation: &NodeOperationRecord,
    ) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        state
            .node_operations
            .insert(operation.operation_id, operation.clone());
        Ok(())
    }
}

#[async_trait]
impl HubRepository for PostgresRepository {
    async fn save_node(&self, node: &NodeRecord) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        let status = node_status_name(node.identity.status);
        sqlx::query(
            r#"
            INSERT INTO nodes (
                id, node_code, name, region, idc, labels, hostname,
                public_ip, private_ip, status, pingora_version, agent_version,
                last_seen_at, updated_at
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7,
                $8::inet, $9::inet, $10::node_status, $11, $12,
                $13, now()
            )
            ON CONFLICT (id) DO UPDATE SET
                node_code = EXCLUDED.node_code,
                name = EXCLUDED.name,
                region = EXCLUDED.region,
                idc = EXCLUDED.idc,
                labels = EXCLUDED.labels,
                hostname = EXCLUDED.hostname,
                public_ip = EXCLUDED.public_ip,
                private_ip = EXCLUDED.private_ip,
                status = EXCLUDED.status,
                pingora_version = EXCLUDED.pingora_version,
                agent_version = EXCLUDED.agent_version,
                last_seen_at = EXCLUDED.last_seen_at,
                updated_at = now()
            "#,
        )
        .bind(node.identity.id)
        .bind(&node.identity.node_code)
        .bind(&node.identity.name)
        .bind(&node.identity.region)
        .bind(&node.identity.idc)
        .bind(&node.labels)
        .bind(node.hostname.as_deref())
        .bind(node.public_ip.as_deref())
        .bind(node.private_ip.as_deref())
        .bind(status)
        .bind(node.pingora_version.as_deref())
        .bind(node.agent_version.as_deref())
        .bind(node.runtime.last_seen_at)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO node_credentials (node_id, bootstrap_token_hash, client_id)
            VALUES ($1, $2, $3)
            ON CONFLICT (node_id) DO UPDATE SET
                bootstrap_token_hash = EXCLUDED.bootstrap_token_hash
            "#,
        )
        .bind(node.identity.id)
        .bind(node.bootstrap_token.as_deref())
        .bind(node.identity.id.to_string())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    async fn node(&self, node_id: Uuid) -> Result<Option<NodeRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT
                n.id,
                n.node_code,
                n.name,
                n.region,
                n.idc,
                n.labels,
                n.hostname,
                n.public_ip::text AS public_ip,
                n.private_ip::text AS private_ip,
                n.status::text AS status,
                n.pingora_version,
                n.agent_version,
                n.last_seen_at,
                nc.bootstrap_token_hash,
                hb.active_config_version,
                hb.site_count,
                hb.health_score
            FROM nodes n
            LEFT JOIN node_credentials nc ON nc.node_id = n.id
            LEFT JOIN LATERAL (
                SELECT active_config_version, site_count, health_score
                FROM node_heartbeats
                WHERE node_id = n.id
                ORDER BY reported_at DESC
                LIMIT 1
            ) hb ON true
            WHERE n.id = $1
            "#,
        )
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| NodeRecord {
            identity: NodeIdentity {
                id: row.get("id"),
                node_code: row.get("node_code"),
                name: row.get("name"),
                region: row.get("region"),
                idc: row.get("idc"),
                status: Self::decode_node_status(row.get::<String, _>("status").as_str()),
            },
            runtime: NodeRuntimeState {
                active_config_version: row.get("active_config_version"),
                last_seen_at: row.get("last_seen_at"),
                site_count: row
                    .get::<Option<i32>, _>("site_count")
                    .unwrap_or_default()
                    .max(0) as u32,
                health_score: row
                    .get::<Option<i16>, _>("health_score")
                    .unwrap_or(100)
                    .clamp(0, 100) as u8,
            },
            labels: row.get("labels"),
            hostname: row.get("hostname"),
            public_ip: row.get("public_ip"),
            private_ip: row.get("private_ip"),
            bootstrap_token: row.get("bootstrap_token_hash"),
            pingora_version: row.get("pingora_version"),
            agent_version: row.get("agent_version"),
        }))
    }

    async fn node_id_by_code(&self, node_code: &str) -> Result<Option<Uuid>, StoreError> {
        let row = sqlx::query("SELECT id FROM nodes WHERE node_code = $1")
            .bind(node_code)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| row.get("id")))
    }

    async fn list_nodes(&self) -> Result<Vec<NodeRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT
                n.id,
                n.node_code,
                n.name,
                n.region,
                n.idc,
                n.labels,
                n.hostname,
                n.public_ip::text AS public_ip,
                n.private_ip::text AS private_ip,
                n.status::text AS status,
                n.pingora_version,
                n.agent_version,
                n.last_seen_at,
                nc.bootstrap_token_hash,
                hb.active_config_version,
                hb.site_count,
                hb.health_score
            FROM nodes n
            LEFT JOIN node_credentials nc ON nc.node_id = n.id
            LEFT JOIN LATERAL (
                SELECT active_config_version, site_count, health_score
                FROM node_heartbeats
                WHERE node_id = n.id
                ORDER BY reported_at DESC
                LIMIT 1
            ) hb ON true
            ORDER BY n.node_code ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| NodeRecord {
                identity: NodeIdentity {
                    id: row.get("id"),
                    node_code: row.get("node_code"),
                    name: row.get("name"),
                    region: row.get("region"),
                    idc: row.get("idc"),
                    status: Self::decode_node_status(row.get::<String, _>("status").as_str()),
                },
                runtime: NodeRuntimeState {
                    active_config_version: row.get("active_config_version"),
                    last_seen_at: row.get("last_seen_at"),
                    site_count: row
                        .get::<Option<i32>, _>("site_count")
                        .unwrap_or_default()
                        .max(0) as u32,
                    health_score: row
                        .get::<Option<i16>, _>("health_score")
                        .unwrap_or(100)
                        .clamp(0, 100) as u8,
                },
                labels: row.get("labels"),
                hostname: row.get("hostname"),
                public_ip: row.get("public_ip"),
                private_ip: row.get("private_ip"),
                bootstrap_token: row.get("bootstrap_token_hash"),
                pingora_version: row.get("pingora_version"),
                agent_version: row.get("agent_version"),
            })
            .collect())
    }

    async fn node_delete_guard(&self, node_id: Uuid) -> Result<NodeDeleteGuard, StoreError> {
        let site_rows = sqlx::query(
            r#"
            SELECT DISTINCT s.site_code
            FROM site_node_bindings b
            JOIN sites s ON s.id = b.site_id
            WHERE b.node_id = $1
            ORDER BY s.site_code ASC
            "#,
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await?;

        let failover_policy_count = sqlx::query(
            r#"
            SELECT COUNT(*) AS count
            FROM failover_policies
            WHERE primary_node_id = $1 OR standby_node_id = $1
            "#,
        )
        .bind(node_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(NodeDeleteGuard {
            bound_site_codes: site_rows
                .into_iter()
                .map(|row| row.get("site_code"))
                .collect(),
            failover_policy_count: failover_policy_count.get::<i64, _>("count").max(0) as usize,
        })
    }

    async fn delete_node(&self, node_id: Uuid) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM nodes WHERE id = $1")
            .bind(node_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn record_heartbeat(&self, heartbeat: &HeartbeatRecord) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO node_heartbeats (
                node_id, cpu_usage, mem_usage, disk_usage, load_status,
                active_config_version, site_count, health_score, reported_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(heartbeat.node_id)
        .bind(f64::from(heartbeat.cpu_usage))
        .bind(f64::from(heartbeat.mem_usage))
        .bind(f64::from(heartbeat.disk_usage))
        .bind(json!({}))
        .bind(heartbeat.active_config_version.as_deref())
        .bind(i32::try_from(heartbeat.site_count).unwrap_or(i32::MAX))
        .bind(i16::from(heartbeat.health_score))
        .bind(heartbeat.reported_at)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE nodes
            SET
                status = CASE
                    WHEN status = 'maintenance'::node_status THEN 'maintenance'::node_status
                    ELSE 'online'::node_status
                END,
                pingora_version = $2,
                agent_version = $3,
                last_seen_at = $4,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(heartbeat.node_id)
        .bind(&heartbeat.pingora_version)
        .bind(&heartbeat.agent_version)
        .bind(heartbeat.reported_at)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    async fn save_site(&self, site: &SiteRecord) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        let status = site_status_name(site.status);
        let protocol = protocol_name(site.protocol);

        sqlx::query(
            r#"
            INSERT INTO sites (
                id, site_code, name, domain, listen_port, protocol,
                tls_enabled, status, metadata, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8::site_status, $9, now(), now())
            ON CONFLICT (id) DO UPDATE SET
                site_code = EXCLUDED.site_code,
                name = EXCLUDED.name,
                domain = EXCLUDED.domain,
                listen_port = EXCLUDED.listen_port,
                protocol = EXCLUDED.protocol,
                tls_enabled = EXCLUDED.tls_enabled,
                status = EXCLUDED.status,
                metadata = EXCLUDED.metadata,
                updated_at = now()
            "#,
        )
        .bind(site.id)
        .bind(&site.site_code)
        .bind(&site.name)
        .bind(&site.domain)
        .bind(i32::from(site.listen_port))
        .bind(protocol)
        .bind(site.tls_enabled)
        .bind(status)
        .bind(json!({}))
        .execute(&mut *tx)
        .await?;

        let config_hash = format!("{}-v{}", site.site_code, site.version);
        sqlx::query(
            r#"
            INSERT INTO site_configs (site_id, version, config_json, config_hash, created_by)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (site_id, version) DO UPDATE SET
                config_json = EXCLUDED.config_json,
                config_hash = EXCLUDED.config_hash,
                created_by = EXCLUDED.created_by
            "#,
        )
        .bind(site.id)
        .bind(i32::try_from(site.version).unwrap_or(i32::MAX))
        .bind(&site.config)
        .bind(config_hash)
        .bind("system")
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM site_node_bindings WHERE site_id = $1")
            .bind(site.id)
            .execute(&mut *tx)
            .await?;

        for binding in &site.bindings {
            sqlx::query(
                r#"
                INSERT INTO site_node_bindings (
                    site_id, node_id, status, bind_mode, binding_role, priority, created_at, updated_at
                )
                VALUES ($1, $2, 'active', 'manual', $3, $4, now(), now())
                "#,
            )
            .bind(site.id)
            .bind(binding.node_id)
            .bind(&binding.binding_role)
            .bind(binding.priority)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn site(&self, site_id: Uuid) -> Result<Option<SiteRecord>, StoreError> {
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
                sc.version,
                sc.config_json
            FROM sites s
            LEFT JOIN LATERAL (
                SELECT version, config_json
                FROM site_configs
                WHERE site_id = s.id
                ORDER BY version DESC
                LIMIT 1
            ) sc ON true
            WHERE s.id = $1
            "#,
        )
        .bind(site_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        Ok(Some(SiteRecord {
            id: row.get("id"),
            site_code: row.get("site_code"),
            name: row.get("name"),
            domain: row.get("domain"),
            listen_port: row
                .get::<i32, _>("listen_port")
                .clamp(0, i32::from(u16::MAX)) as u16,
            protocol: Self::decode_protocol(row.get::<String, _>("protocol").as_str()),
            tls_enabled: row.get("tls_enabled"),
            status: Self::decode_site_status(row.get::<String, _>("status").as_str()),
            version: row.get::<Option<i32>, _>("version").unwrap_or(1).max(1) as u32,
            config: row
                .get::<Option<Value>, _>("config_json")
                .unwrap_or_else(|| json!({})),
            bindings: self.load_site_bindings(site_id).await?,
        }))
    }

    async fn site_id_by_code(&self, site_code: &str) -> Result<Option<Uuid>, StoreError> {
        let row = sqlx::query("SELECT id FROM sites WHERE site_code = $1")
            .bind(site_code)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| row.get("id")))
    }

    async fn list_sites(&self) -> Result<Vec<SiteRecord>, StoreError> {
        let rows = sqlx::query(
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
                sc.version,
                sc.config_json
            FROM sites s
            LEFT JOIN LATERAL (
                SELECT version, config_json
                FROM site_configs
                WHERE site_id = s.id
                ORDER BY version DESC
                LIMIT 1
            ) sc ON true
            ORDER BY s.site_code ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut sites = Vec::with_capacity(rows.len());
        for row in rows {
            let site_id: Uuid = row.get("id");
            sites.push(SiteRecord {
                id: site_id,
                site_code: row.get("site_code"),
                name: row.get("name"),
                domain: row.get("domain"),
                listen_port: row
                    .get::<i32, _>("listen_port")
                    .clamp(0, i32::from(u16::MAX)) as u16,
                protocol: Self::decode_protocol(row.get::<String, _>("protocol").as_str()),
                tls_enabled: row.get("tls_enabled"),
                status: Self::decode_site_status(row.get::<String, _>("status").as_str()),
                version: row.get::<Option<i32>, _>("version").unwrap_or(1).max(1) as u32,
                config: row
                    .get::<Option<Value>, _>("config_json")
                    .unwrap_or_else(|| json!({})),
                bindings: self.load_site_bindings(site_id).await?,
            });
        }

        Ok(sites)
    }

    async fn delete_site(&self, site_id: Uuid) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM sites WHERE id = $1")
            .bind(site_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn update_active_failover_policy_targets(
        &self,
        site_id: Uuid,
        primary_node_id: Uuid,
        standby_node_id: Uuid,
    ) -> Result<u64, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE failover_policies
            SET primary_node_id = $2,
                standby_node_id = $3,
                updated_at = now()
            WHERE scope_type = 'site'
              AND scope_id = $1
              AND status = 'active'
            "#,
        )
        .bind(site_id)
        .bind(primary_node_id)
        .bind(standby_node_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    async fn save_release(&self, release: &ReleaseRecord) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        let manifest_json = serde_json::to_value(&release.bundle)?;
        sqlx::query(
            r#"
            INSERT INTO config_releases (
                id, release_code, scope_type, scope_id, release_type, release_version,
                manifest_json, manifest_hash, reason, status, published_at, created_by, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::release_status, now(), 'system', $11)
            ON CONFLICT (id) DO UPDATE SET
                release_code = EXCLUDED.release_code,
                scope_type = EXCLUDED.scope_type,
                scope_id = EXCLUDED.scope_id,
                release_type = EXCLUDED.release_type,
                release_version = EXCLUDED.release_version,
                manifest_json = EXCLUDED.manifest_json,
                manifest_hash = EXCLUDED.manifest_hash,
                reason = EXCLUDED.reason,
                status = EXCLUDED.status,
                published_at = EXCLUDED.published_at
            "#,
        )
        .bind(release.release_id)
        .bind(release.release_id.to_string())
        .bind(&release.scope_type)
        .bind(release.scope_id)
        .bind(&release.release_type)
        .bind(&release.release_version)
        .bind(manifest_json)
        .bind(&release.bundle.manifest.config_hash)
        .bind(&release.reason)
        .bind(&release.status)
        .bind(release.created_at)
        .execute(&mut *tx)
        .await?;

        for node_id in &release.target_node_ids {
            let ack = release
                .ack_status
                .get(node_id)
                .cloned()
                .unwrap_or(NodeAckRecord {
                    apply_status: "pending".to_string(),
                    current_version: None,
                    message: None,
                    acked_at: None,
                });
            sqlx::query(
                r#"
                INSERT INTO node_release_status (
                    release_id, node_id, target_version, current_version, apply_status,
                    apply_message, acked_at, updated_at
                )
                VALUES ($1, $2, $3, $4, $5::apply_status, $6, $7, now())
                ON CONFLICT (release_id, node_id) DO UPDATE SET
                    target_version = EXCLUDED.target_version,
                    current_version = EXCLUDED.current_version,
                    apply_status = EXCLUDED.apply_status,
                    apply_message = EXCLUDED.apply_message,
                    acked_at = EXCLUDED.acked_at,
                    updated_at = now()
                "#,
            )
            .bind(release.release_id)
            .bind(*node_id)
            .bind(&release.release_version)
            .bind(ack.current_version.as_deref())
            .bind(&ack.apply_status)
            .bind(ack.message.as_deref())
            .bind(ack.acked_at)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn release(&self, release_id: Uuid) -> Result<Option<ReleaseRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT
                id, scope_type, scope_id, release_type, release_version,
                reason, status::text AS status, created_at, manifest_json
            FROM config_releases
            WHERE id = $1
            "#,
        )
        .bind(release_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let bundle: CompiledConfigBundle = serde_json::from_value(row.get("manifest_json"))?;
        let ack_status = self.load_release_acks(release_id).await?;
        let target_node_ids = ack_status.keys().copied().collect::<Vec<_>>();

        Ok(Some(ReleaseRecord {
            release_id: row.get("id"),
            scope_type: row.get("scope_type"),
            scope_id: row.get("scope_id"),
            release_type: row.get("release_type"),
            release_version: row.get("release_version"),
            reason: row.get::<Option<String>, _>("reason").unwrap_or_default(),
            status: row.get("status"),
            bundle,
            target_node_ids,
            ack_status,
            created_at: row.get("created_at"),
        }))
    }

    async fn release_by_version(&self, version: &str) -> Result<Option<ReleaseRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT
                id, scope_type, scope_id, release_type, release_version,
                reason, status::text AS status, created_at, manifest_json
            FROM config_releases
            WHERE release_version = $1
            "#,
        )
        .bind(version)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let release_id: Uuid = row.get("id");
        let bundle: CompiledConfigBundle = serde_json::from_value(row.get("manifest_json"))?;
        let ack_status = self.load_release_acks(release_id).await?;
        let target_node_ids = ack_status.keys().copied().collect::<Vec<_>>();

        Ok(Some(ReleaseRecord {
            release_id,
            scope_type: row.get("scope_type"),
            scope_id: row.get("scope_id"),
            release_type: row.get("release_type"),
            release_version: row.get("release_version"),
            reason: row.get::<Option<String>, _>("reason").unwrap_or_default(),
            status: row.get("status"),
            bundle,
            target_node_ids,
            ack_status,
            created_at: row.get("created_at"),
        }))
    }

    async fn list_releases(&self) -> Result<Vec<ReleaseRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT
                id, scope_type, scope_id, release_type, release_version,
                reason, status::text AS status, created_at, manifest_json
            FROM config_releases
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut releases = Vec::with_capacity(rows.len());
        for row in rows {
            let release_id: Uuid = row.get("id");
            let bundle: CompiledConfigBundle = serde_json::from_value(row.get("manifest_json"))?;
            let ack_status = self.load_release_acks(release_id).await?;
            let target_node_ids = ack_status.keys().copied().collect::<Vec<_>>();
            releases.push(ReleaseRecord {
                release_id,
                scope_type: row.get("scope_type"),
                scope_id: row.get("scope_id"),
                release_type: row.get("release_type"),
                release_version: row.get("release_version"),
                reason: row.get::<Option<String>, _>("reason").unwrap_or_default(),
                status: row.get("status"),
                bundle,
                target_node_ids,
                ack_status,
                created_at: row.get("created_at"),
            });
        }

        Ok(releases)
    }

    async fn latest_release_id_for_node(&self, node_id: Uuid) -> Result<Option<Uuid>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT nrs.release_id
            FROM node_release_status nrs
            JOIN config_releases cr ON cr.id = nrs.release_id
            WHERE nrs.node_id = $1
            ORDER BY cr.created_at DESC
            LIMIT 1
            "#,
        )
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| row.get("release_id")))
    }

    async fn update_release_ack(
        &self,
        release_id: Uuid,
        node_id: Uuid,
        ack: &NodeAckRecord,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            UPDATE node_release_status
            SET
                apply_status = $3::apply_status,
                current_version = $4,
                apply_message = $5,
                acked_at = $6,
                updated_at = now()
            WHERE release_id = $1 AND node_id = $2
            "#,
        )
        .bind(release_id)
        .bind(node_id)
        .bind(&ack.apply_status)
        .bind(ack.current_version.as_deref())
        .bind(ack.message.as_deref())
        .bind(ack.acked_at)
        .execute(&self.pool)
        .await?;

        let status = aggregate_release_status(&self.load_release_acks(release_id).await?);
        sqlx::query(
            r#"
            UPDATE config_releases
            SET status = $2::release_status
            WHERE id = $1
            "#,
        )
        .bind(release_id)
        .bind(status)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn save_dns_provider(&self, provider: &DnsProviderRecord) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO dns_providers (
                id, name, provider_type, api_endpoint, credential_encrypted, status, created_at, updated_at
            )
            VALUES ($1, $2, $3::dns_provider_type, $4, $5, $6, $7, now())
            ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name,
                provider_type = EXCLUDED.provider_type,
                api_endpoint = EXCLUDED.api_endpoint,
                credential_encrypted = EXCLUDED.credential_encrypted,
                status = EXCLUDED.status,
                updated_at = now()
            "#,
        )
        .bind(provider.provider_id)
        .bind(&provider.name)
        .bind(&provider.provider_type)
        .bind(provider.api_endpoint.as_deref())
        .bind(&provider.credential_encrypted)
        .bind(&provider.status)
        .bind(provider.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn delete_dns_provider(&self, provider_id: Uuid) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM dns_providers WHERE id = $1")
            .bind(provider_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn dns_provider(
        &self,
        provider_id: Uuid,
    ) -> Result<Option<DnsProviderRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, name, provider_type::text AS provider_type, api_endpoint, credential_encrypted, status, created_at
            FROM dns_providers
            WHERE id = $1
            "#,
        )
        .bind(provider_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| DnsProviderRecord {
            provider_id: row.get("id"),
            name: row.get("name"),
            provider_type: row.get("provider_type"),
            api_endpoint: row.get("api_endpoint"),
            credential_encrypted: row.get("credential_encrypted"),
            status: row.get("status"),
            created_at: row.get("created_at"),
        }))
    }

    async fn dns_provider_by_name(
        &self,
        name: &str,
    ) -> Result<Option<DnsProviderRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, name, provider_type::text AS provider_type, api_endpoint, credential_encrypted, status, created_at
            FROM dns_providers
            WHERE name = $1
            "#,
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| DnsProviderRecord {
            provider_id: row.get("id"),
            name: row.get("name"),
            provider_type: row.get("provider_type"),
            api_endpoint: row.get("api_endpoint"),
            credential_encrypted: row.get("credential_encrypted"),
            status: row.get("status"),
            created_at: row.get("created_at"),
        }))
    }

    async fn list_dns_providers(&self) -> Result<Vec<DnsProviderRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, provider_type::text AS provider_type, api_endpoint, credential_encrypted, status, created_at
            FROM dns_providers
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| DnsProviderRecord {
                provider_id: row.get("id"),
                name: row.get("name"),
                provider_type: row.get("provider_type"),
                api_endpoint: row.get("api_endpoint"),
                credential_encrypted: row.get("credential_encrypted"),
                status: row.get("status"),
                created_at: row.get("created_at"),
            })
            .collect())
    }

    async fn save_dns_zone(&self, zone: &DnsZoneRecord) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO dns_zones (
                id, provider_id, zone_name, external_zone_id, status, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, now())
            ON CONFLICT (provider_id, zone_name) DO UPDATE SET
                external_zone_id = EXCLUDED.external_zone_id,
                status = EXCLUDED.status,
                updated_at = now()
            "#,
        )
        .bind(zone.zone_id)
        .bind(zone.provider_id)
        .bind(&zone.zone_name)
        .bind(&zone.external_zone_id)
        .bind(&zone.status)
        .bind(zone.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn dns_zone(&self, zone_id: Uuid) -> Result<Option<DnsZoneRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, provider_id, zone_name, external_zone_id, status, created_at
            FROM dns_zones
            WHERE id = $1
            "#,
        )
        .bind(zone_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| DnsZoneRecord {
            zone_id: row.get("id"),
            provider_id: row.get("provider_id"),
            zone_name: row.get("zone_name"),
            external_zone_id: row.get("external_zone_id"),
            status: row.get("status"),
            created_at: row.get("created_at"),
        }))
    }

    async fn dns_zone_by_provider_name(
        &self,
        provider_id: Uuid,
        zone_name: &str,
    ) -> Result<Option<DnsZoneRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, provider_id, zone_name, external_zone_id, status, created_at
            FROM dns_zones
            WHERE provider_id = $1 AND zone_name = $2
            "#,
        )
        .bind(provider_id)
        .bind(zone_name)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| DnsZoneRecord {
            zone_id: row.get("id"),
            provider_id: row.get("provider_id"),
            zone_name: row.get("zone_name"),
            external_zone_id: row.get("external_zone_id"),
            status: row.get("status"),
            created_at: row.get("created_at"),
        }))
    }

    async fn list_dns_zones(&self) -> Result<Vec<DnsZoneRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, provider_id, zone_name, external_zone_id, status, created_at
            FROM dns_zones
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| DnsZoneRecord {
                zone_id: row.get("id"),
                provider_id: row.get("provider_id"),
                zone_name: row.get("zone_name"),
                external_zone_id: row.get("external_zone_id"),
                status: row.get("status"),
                created_at: row.get("created_at"),
            })
            .collect())
    }

    async fn delete_dns_zone(&self, zone_id: Uuid) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM dns_zones WHERE id = $1")
            .bind(zone_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn save_certificate(&self, certificate: &CertificateRecord) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO certificates (
                id, cert_code, common_name, sans, fingerprint_sha256, status, not_after, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6::certificate_status, $7, $8, now())
            ON CONFLICT (id) DO UPDATE SET
                cert_code = EXCLUDED.cert_code,
                common_name = EXCLUDED.common_name,
                sans = EXCLUDED.sans,
                fingerprint_sha256 = EXCLUDED.fingerprint_sha256,
                status = EXCLUDED.status,
                not_after = EXCLUDED.not_after,
                updated_at = now()
            "#,
        )
        .bind(certificate.certificate_id)
        .bind(&certificate.cert_code)
        .bind(&certificate.common_name)
        .bind(json!(certificate.sans))
        .bind(&certificate.fingerprint_sha256)
        .bind(&certificate.status)
        .bind(certificate.expires_at)
        .bind(certificate.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn site_certificates(&self, site_id: Uuid) -> Result<Vec<CertificateRef>, StoreError> {
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
        .fetch_all(&self.pool)
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

    async fn save_certificate_order(
        &self,
        order: &CertificateOrderRecord,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO certificate_orders (
                id, site_id, certificate_id, zone_id, order_type, acme_provider,
                challenge_type, challenge_payload, order_status, error_message, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, now())
            ON CONFLICT (id) DO UPDATE SET
                site_id = EXCLUDED.site_id,
                certificate_id = EXCLUDED.certificate_id,
                zone_id = EXCLUDED.zone_id,
                order_type = EXCLUDED.order_type,
                acme_provider = EXCLUDED.acme_provider,
                challenge_type = EXCLUDED.challenge_type,
                challenge_payload = EXCLUDED.challenge_payload,
                order_status = EXCLUDED.order_status,
                error_message = EXCLUDED.error_message,
                updated_at = now()
            "#,
        )
        .bind(order.order_id)
        .bind(order.site_id)
        .bind(order.certificate_id)
        .bind(order.zone_id)
        .bind(&order.order_type)
        .bind(&order.acme_provider)
        .bind(&order.challenge_type)
        .bind(&order.challenge_payload)
        .bind(&order.order_status)
        .bind(order.error_message.as_deref())
        .bind(order.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn certificate_order(
        &self,
        order_id: Uuid,
    ) -> Result<Option<CertificateOrderRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT
                o.id, o.site_id, o.certificate_id, o.zone_id, o.order_type,
                o.acme_provider, o.challenge_type, o.challenge_payload, o.order_status,
                o.error_message, o.created_at, c.not_after AS certificate_expires_at
            FROM certificate_orders o
            LEFT JOIN certificates c ON c.id = o.certificate_id
            WHERE o.id = $1
            "#,
        )
        .bind(order_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| CertificateOrderRecord {
            order_id: row.get("id"),
            site_id: row.get("site_id"),
            certificate_id: row.get("certificate_id"),
            zone_id: row.get("zone_id"),
            order_type: row.get("order_type"),
            acme_provider: row.get("acme_provider"),
            challenge_type: row.get("challenge_type"),
            challenge_payload: row.get("challenge_payload"),
            order_status: row.get("order_status"),
            error_message: row.get("error_message"),
            certificate_expires_at: row.get("certificate_expires_at"),
            created_at: row.get("created_at"),
        }))
    }

    async fn list_certificate_orders(&self) -> Result<Vec<CertificateOrderRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT
                o.id, o.site_id, o.certificate_id, o.zone_id, o.order_type,
                o.acme_provider, o.challenge_type, o.challenge_payload, o.order_status,
                o.error_message, o.created_at, c.not_after AS certificate_expires_at
            FROM certificate_orders o
            LEFT JOIN certificates c ON c.id = o.certificate_id
            ORDER BY o.created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| CertificateOrderRecord {
                order_id: row.get("id"),
                site_id: row.get("site_id"),
                certificate_id: row.get("certificate_id"),
                zone_id: row.get("zone_id"),
                order_type: row.get("order_type"),
                acme_provider: row.get("acme_provider"),
                challenge_type: row.get("challenge_type"),
                challenge_payload: row.get("challenge_payload"),
                order_status: row.get("order_status"),
                error_message: row.get("error_message"),
                certificate_expires_at: row.get("certificate_expires_at"),
                created_at: row.get("created_at"),
            })
            .collect())
    }

    async fn save_admin_user(&self, user: &AdminUserRecord) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO admin_users (
                id, username, display_name, password_hash, status, last_login_at, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, now())
            ON CONFLICT (username) DO UPDATE SET
                display_name = EXCLUDED.display_name,
                password_hash = EXCLUDED.password_hash,
                status = EXCLUDED.status,
                last_login_at = EXCLUDED.last_login_at,
                updated_at = now()
            "#,
        )
        .bind(user.user_id)
        .bind(&user.username)
        .bind(&user.display_name)
        .bind(&user.password_hash)
        .bind(&user.status)
        .bind(user.last_login_at)
        .bind(user.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn admin_user_by_username(
        &self,
        username: &str,
    ) -> Result<Option<AdminUserRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, username, display_name, password_hash, status, last_login_at, created_at
            FROM admin_users
            WHERE username = $1
            "#,
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| AdminUserRecord {
            user_id: row.get("id"),
            username: row.get("username"),
            display_name: row.get("display_name"),
            password_hash: row.get("password_hash"),
            status: row.get("status"),
            last_login_at: row.get("last_login_at"),
            created_at: row.get("created_at"),
        }))
    }

    async fn touch_admin_user_login(
        &self,
        user_id: Uuid,
        last_login_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            UPDATE admin_users
            SET last_login_at = $2, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(user_id)
        .bind(last_login_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn save_admin_session(&self, session: &AdminSessionRecord) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO admin_sessions (
                id, user_id, session_token_hash, expires_at, created_at, last_seen_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (session_token_hash) DO UPDATE SET
                user_id = EXCLUDED.user_id,
                expires_at = EXCLUDED.expires_at,
                last_seen_at = EXCLUDED.last_seen_at
            "#,
        )
        .bind(session.session_id)
        .bind(session.user_id)
        .bind(&session.session_token_hash)
        .bind(session.expires_at)
        .bind(session.created_at)
        .bind(session.last_seen_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn admin_session_by_token_hash(
        &self,
        session_token_hash: &str,
    ) -> Result<Option<AdminSessionContext>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT
                s.id AS session_id,
                s.user_id,
                s.expires_at,
                u.username,
                u.display_name,
                u.status
            FROM admin_sessions s
            JOIN admin_users u ON u.id = s.user_id
            WHERE s.session_token_hash = $1
            "#,
        )
        .bind(session_token_hash)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| AdminSessionContext {
            user_id: row.get("user_id"),
            username: row.get("username"),
            display_name: row.get("display_name"),
            status: row.get("status"),
            expires_at: row.get("expires_at"),
        }))
    }

    async fn touch_admin_session(
        &self,
        session_token_hash: &str,
        last_seen_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            UPDATE admin_sessions
            SET last_seen_at = $2
            WHERE session_token_hash = $1
            "#,
        )
        .bind(session_token_hash)
        .bind(last_seen_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn delete_admin_session(&self, session_token_hash: &str) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM admin_sessions WHERE session_token_hash = $1")
            .bind(session_token_hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn delete_admin_sessions_for_user(
        &self,
        user_id: Uuid,
        exclude_session_token_hash: Option<&str>,
    ) -> Result<(), StoreError> {
        if let Some(exclude_session_token_hash) = exclude_session_token_hash {
            sqlx::query(
                r#"
                DELETE FROM admin_sessions
                WHERE user_id = $1
                  AND session_token_hash <> $2
                "#,
            )
            .bind(user_id)
            .bind(exclude_session_token_hash)
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query("DELETE FROM admin_sessions WHERE user_id = $1")
                .bind(user_id)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    async fn save_operation_template(
        &self,
        template: &OperationTemplateRecord,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO operation_templates (
                id, name, operation_type, command_template, allowed_params,
                timeout_seconds, run_as_user, approval_required, created_at, updated_at
            )
            VALUES ($1, $2, $3::operation_type, $4, $5, $6, $7, $8, $9, now())
            ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name,
                operation_type = EXCLUDED.operation_type,
                command_template = EXCLUDED.command_template,
                allowed_params = EXCLUDED.allowed_params,
                timeout_seconds = EXCLUDED.timeout_seconds,
                run_as_user = EXCLUDED.run_as_user,
                approval_required = EXCLUDED.approval_required,
                updated_at = now()
            "#,
        )
        .bind(template.template_id)
        .bind(&template.name)
        .bind(&template.operation_type)
        .bind(&template.command_template)
        .bind(&template.allowed_params)
        .bind(i32::try_from(template.timeout_seconds).unwrap_or(i32::MAX))
        .bind(&template.run_as_user)
        .bind(template.approval_required)
        .bind(template.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn operation_template(
        &self,
        template_id: Uuid,
    ) -> Result<Option<OperationTemplateRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT
                id, name, operation_type::text AS operation_type, command_template,
                allowed_params, timeout_seconds, run_as_user, approval_required, created_at
            FROM operation_templates
            WHERE id = $1
            "#,
        )
        .bind(template_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| OperationTemplateRecord {
            template_id: row.get("id"),
            name: row.get("name"),
            operation_type: row.get("operation_type"),
            command_template: row.get("command_template"),
            allowed_params: row.get("allowed_params"),
            timeout_seconds: row.get::<i32, _>("timeout_seconds").clamp(0, i32::MAX) as u32,
            run_as_user: row.get("run_as_user"),
            approval_required: row.get("approval_required"),
            created_at: row.get("created_at"),
        }))
    }

    async fn operation_template_by_name(
        &self,
        name: &str,
    ) -> Result<Option<OperationTemplateRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT
                id, name, operation_type::text AS operation_type, command_template,
                allowed_params, timeout_seconds, run_as_user, approval_required, created_at
            FROM operation_templates
            WHERE name = $1
            "#,
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| OperationTemplateRecord {
            template_id: row.get("id"),
            name: row.get("name"),
            operation_type: row.get("operation_type"),
            command_template: row.get("command_template"),
            allowed_params: row.get("allowed_params"),
            timeout_seconds: row.get::<i32, _>("timeout_seconds").clamp(0, i32::MAX) as u32,
            run_as_user: row.get("run_as_user"),
            approval_required: row.get("approval_required"),
            created_at: row.get("created_at"),
        }))
    }

    async fn list_operation_templates(&self) -> Result<Vec<OperationTemplateRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT
                id, name, operation_type::text AS operation_type, command_template,
                allowed_params, timeout_seconds, run_as_user, approval_required, created_at
            FROM operation_templates
            ORDER BY created_at DESC, name ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| OperationTemplateRecord {
                template_id: row.get("id"),
                name: row.get("name"),
                operation_type: row.get("operation_type"),
                command_template: row.get("command_template"),
                allowed_params: row.get("allowed_params"),
                timeout_seconds: row.get::<i32, _>("timeout_seconds").clamp(0, i32::MAX) as u32,
                run_as_user: row.get("run_as_user"),
                approval_required: row.get("approval_required"),
                created_at: row.get("created_at"),
            })
            .collect())
    }

    async fn delete_operation_template(&self, template_id: Uuid) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM operation_templates WHERE id = $1")
            .bind(template_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn save_node_operation(&self, operation: &NodeOperationRecord) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO node_operations (
                id, node_id, template_id, event_id, input_params, exec_status,
                requested_by, approved_by, exit_code, stdout_log, stderr_log,
                started_at, finished_at, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6::operation_exec_status, $7, $8, $9, $10, $11, $12, $13, $14)
            ON CONFLICT (id) DO UPDATE SET
                input_params = EXCLUDED.input_params,
                exec_status = EXCLUDED.exec_status,
                requested_by = EXCLUDED.requested_by,
                approved_by = EXCLUDED.approved_by,
                exit_code = EXCLUDED.exit_code,
                stdout_log = EXCLUDED.stdout_log,
                stderr_log = EXCLUDED.stderr_log,
                started_at = EXCLUDED.started_at,
                finished_at = EXCLUDED.finished_at
            "#,
        )
        .bind(operation.operation_id)
        .bind(operation.node_id)
        .bind(operation.template_id)
        .bind(operation.event_id)
        .bind(&operation.input_params)
        .bind(&operation.exec_status)
        .bind(&operation.requested_by)
        .bind(operation.approved_by.as_deref())
        .bind(operation.exit_code)
        .bind(operation.stdout_log.as_deref())
        .bind(operation.stderr_log.as_deref())
        .bind(operation.started_at)
        .bind(operation.finished_at)
        .bind(operation.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn node_operation(
        &self,
        operation_id: Uuid,
    ) -> Result<Option<NodeOperationRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT
                id, node_id, template_id, event_id, input_params,
                exec_status::text AS exec_status, requested_by, approved_by,
                exit_code, stdout_log, stderr_log, started_at, finished_at, created_at
            FROM node_operations
            WHERE id = $1
            "#,
        )
        .bind(operation_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| NodeOperationRecord {
            operation_id: row.get("id"),
            node_id: row.get("node_id"),
            template_id: row.get("template_id"),
            event_id: row.get("event_id"),
            input_params: row.get("input_params"),
            exec_status: row.get("exec_status"),
            requested_by: row.get("requested_by"),
            approved_by: row.get("approved_by"),
            exit_code: row.get("exit_code"),
            stdout_log: row.get("stdout_log"),
            stderr_log: row.get("stderr_log"),
            started_at: row.get("started_at"),
            finished_at: row.get("finished_at"),
            created_at: row.get("created_at"),
        }))
    }

    async fn list_node_operations(
        &self,
        node_id: Uuid,
    ) -> Result<Vec<NodeOperationRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT
                id, node_id, template_id, event_id, input_params,
                exec_status::text AS exec_status, requested_by, approved_by,
                exit_code, stdout_log, stderr_log, started_at, finished_at, created_at
            FROM node_operations
            WHERE node_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| NodeOperationRecord {
                operation_id: row.get("id"),
                node_id: row.get("node_id"),
                template_id: row.get("template_id"),
                event_id: row.get("event_id"),
                input_params: row.get("input_params"),
                exec_status: row.get("exec_status"),
                requested_by: row.get("requested_by"),
                approved_by: row.get("approved_by"),
                exit_code: row.get("exit_code"),
                stdout_log: row.get("stdout_log"),
                stderr_log: row.get("stderr_log"),
                started_at: row.get("started_at"),
                finished_at: row.get("finished_at"),
                created_at: row.get("created_at"),
            })
            .collect())
    }

    async fn claim_next_node_operation(
        &self,
        node_id: Uuid,
    ) -> Result<Option<ClaimedNodeOperation>, StoreError> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            r#"
            SELECT
                o.id AS operation_id,
                o.node_id,
                o.template_id,
                o.event_id,
                o.input_params,
                o.exec_status::text AS exec_status,
                o.requested_by,
                o.approved_by,
                o.exit_code,
                o.stdout_log,
                o.stderr_log,
                o.started_at,
                o.finished_at,
                o.created_at,
                t.id AS template_record_id,
                t.name,
                t.operation_type::text AS operation_type,
                t.command_template,
                t.allowed_params,
                t.timeout_seconds,
                t.run_as_user,
                t.approval_required,
                t.created_at AS template_created_at
            FROM node_operations o
            JOIN operation_templates t ON t.id = o.template_id
            WHERE o.node_id = $1
              AND o.exec_status = 'approved'
            ORDER BY o.created_at ASC
            LIMIT 1
            FOR UPDATE OF o SKIP LOCKED
            "#,
        )
        .bind(node_id)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };

        let operation_id: Uuid = row.get("operation_id");
        let started_at = Utc::now();
        sqlx::query(
            r#"
            UPDATE node_operations
            SET exec_status = 'running', started_at = COALESCE(started_at, $2)
            WHERE id = $1
            "#,
        )
        .bind(operation_id)
        .bind(started_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        Ok(Some(ClaimedNodeOperation {
            operation: NodeOperationRecord {
                operation_id,
                node_id: row.get("node_id"),
                template_id: row.get("template_id"),
                event_id: row.get("event_id"),
                input_params: row.get("input_params"),
                exec_status: "running".to_string(),
                requested_by: row.get("requested_by"),
                approved_by: row.get("approved_by"),
                exit_code: row.get("exit_code"),
                stdout_log: row.get("stdout_log"),
                stderr_log: row.get("stderr_log"),
                started_at: Some(started_at),
                finished_at: row.get("finished_at"),
                created_at: row.get("created_at"),
            },
            template: OperationTemplateRecord {
                template_id: row.get("template_record_id"),
                name: row.get("name"),
                operation_type: row.get("operation_type"),
                command_template: row.get("command_template"),
                allowed_params: row.get("allowed_params"),
                timeout_seconds: row.get::<i32, _>("timeout_seconds").clamp(0, i32::MAX) as u32,
                run_as_user: row.get("run_as_user"),
                approval_required: row.get("approval_required"),
                created_at: row.get("template_created_at"),
            },
        }))
    }

    async fn update_node_operation(
        &self,
        operation: &NodeOperationRecord,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            UPDATE node_operations
            SET
                exec_status = $2::operation_exec_status,
                approved_by = $3,
                exit_code = $4,
                stdout_log = $5,
                stderr_log = $6,
                started_at = $7,
                finished_at = $8
            WHERE id = $1
            "#,
        )
        .bind(operation.operation_id)
        .bind(&operation.exec_status)
        .bind(operation.approved_by.as_deref())
        .bind(operation.exit_code)
        .bind(operation.stdout_log.as_deref())
        .bind(operation.stderr_log.as_deref())
        .bind(operation.started_at)
        .bind(operation.finished_at)
        .execute(&self.pool)
        .await?;
        Ok(())
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

pub fn node_status_name(status: NodeStatus) -> String {
    match status {
        NodeStatus::Pending => "pending",
        NodeStatus::Online => "online",
        NodeStatus::Suspect => "suspect",
        NodeStatus::Offline => "offline",
        NodeStatus::Maintenance => "maintenance",
    }
    .to_string()
}

pub fn site_status_name(status: SiteStatus) -> String {
    match status {
        SiteStatus::Draft => "draft",
        SiteStatus::Published => "published",
        SiteStatus::Disabled => "disabled",
    }
    .to_string()
}

pub fn protocol_name(protocol: SiteProtocol) -> &'static str {
    match protocol {
        SiteProtocol::Http => "http",
        SiteProtocol::Https => "https",
        SiteProtocol::Tcp => "tcp",
    }
}

#[cfg(test)]
impl MemoryRepository {
    pub async fn set_site_certificates(&self, site_id: Uuid, certificates: Vec<CertificateRef>) {
        let mut state = self.inner.write().await;
        state.site_certificates.insert(site_id, certificates);
    }
}

pub fn aggregate_release_status(ack_status: &HashMap<Uuid, NodeAckRecord>) -> String {
    if ack_status.is_empty() {
        return "pending".to_string();
    }

    let mut has_in_progress = false;
    let mut has_failed = false;
    let mut all_success = true;

    for ack in ack_status.values() {
        match ack.apply_status.as_str() {
            "success" => {}
            "failed" => {
                has_failed = true;
                all_success = false;
            }
            "downloading" | "applying" => {
                has_in_progress = true;
                all_success = false;
            }
            _ => {
                all_success = false;
            }
        }
    }

    if has_failed {
        "failed".to_string()
    } else if all_success {
        "success".to_string()
    } else if has_in_progress {
        "publishing".to_string()
    } else {
        "pending".to_string()
    }
}
