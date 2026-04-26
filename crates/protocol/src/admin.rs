use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminLoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminSessionInfo {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminLoginResponse {
    pub session: AdminSessionInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminMeResponse {
    pub session: AdminSessionInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeAdminPasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeAdminPasswordResponse {
    pub user_id: Uuid,
    pub username: String,
    pub changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateNodeRequest {
    pub node_code: String,
    pub name: String,
    pub region: String,
    pub idc: String,
    pub labels: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateNodeResponse {
    pub node_id: Uuid,
    pub bootstrap_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteNodeResponse {
    pub node_id: Uuid,
    pub node_code: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeOverview {
    pub node_id: Uuid,
    pub node_code: String,
    pub name: String,
    pub region: String,
    pub idc: String,
    pub labels: Value,
    pub status: String,
    pub active_config_version: Option<String>,
    pub last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSiteOverview {
    pub site_id: Uuid,
    pub site_code: String,
    pub name: String,
    pub domain: String,
    pub status: String,
    pub version: u32,
    pub binding_role: String,
    pub priority: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDetail {
    pub node_id: Uuid,
    pub node_code: String,
    pub name: String,
    pub region: String,
    pub idc: String,
    pub labels: Value,
    pub status: String,
    pub hostname: Option<String>,
    pub public_ip: Option<String>,
    pub private_ip: Option<String>,
    pub pingora_version: Option<String>,
    pub agent_version: Option<String>,
    pub active_config_version: Option<String>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub runtime_site_count: u32,
    pub health_score: u8,
    pub sites: Vec<NodeSiteOverview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSiteRequest {
    pub site_code: Option<String>,
    pub name: String,
    pub domain: String,
    pub listen_port: u16,
    pub protocol: String,
    pub tls_enabled: bool,
    pub config: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSiteRequest {
    pub name: String,
    pub domain: String,
    pub listen_port: u16,
    pub protocol: String,
    pub tls_enabled: bool,
    pub config: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSiteResponse {
    pub site_id: Uuid,
    pub site_code: String,
    pub version: u32,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteDetail {
    pub site_id: Uuid,
    pub site_code: String,
    pub name: String,
    pub domain: String,
    pub listen_port: u16,
    pub protocol: String,
    pub tls_enabled: bool,
    pub status: String,
    pub version: u32,
    pub config: Value,
    pub bindings: Vec<SiteBindingItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteOverview {
    pub site_id: Uuid,
    pub site_code: String,
    pub name: String,
    pub domain: String,
    pub protocol: String,
    pub status: String,
    pub version: u32,
    pub binding_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_node_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_node_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_node_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_node_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteBindingItem {
    pub node_id: Uuid,
    pub binding_role: String,
    pub priority: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSiteBindingsRequest {
    pub bindings: Vec<SiteBindingItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSiteBindingsResponse {
    pub site_id: Uuid,
    pub binding_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteSiteResponse {
    pub site_id: Uuid,
    pub site_code: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSiteStatusResponse {
    pub site_id: Uuid,
    pub site_code: String,
    pub status: String,
    pub version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenewSiteCertificateResponse {
    pub site_id: Uuid,
    pub domain: String,
    pub zone_id: Uuid,
    pub order_id: Uuid,
    pub certificate_id: Option<Uuid>,
    pub order_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchSitePrimaryRequest {
    pub target_node_id: Option<Uuid>,
    pub keep_previous_as_standby: Option<bool>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchSitePrimaryResponse {
    pub site_id: Uuid,
    pub previous_primary_node_id: Option<Uuid>,
    pub current_primary_node_id: Uuid,
    pub binding_count: usize,
    pub release_id: Uuid,
    pub release_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateReleaseRequest {
    pub scope_type: String,
    pub scope_id: Uuid,
    pub release_type: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateReleaseResponse {
    pub release_id: Uuid,
    pub release_version: String,
    pub config_hash: String,
    pub target_node_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReleaseStatusCounts {
    pub pending: usize,
    pub in_progress: usize,
    pub success: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseOverview {
    pub release_id: Uuid,
    pub scope_type: String,
    pub scope_id: Option<Uuid>,
    pub release_type: String,
    pub release_version: String,
    pub status: String,
    pub reason: String,
    pub counts: ReleaseStatusCounts,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateDnsProviderRequest {
    pub name: String,
    pub provider_type: String,
    pub api_endpoint: Option<String>,
    pub credentials: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateDnsProviderResponse {
    pub provider_id: Uuid,
    pub name: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteDnsProviderResponse {
    pub provider_id: Uuid,
    pub name: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsProviderOverview {
    pub provider_id: Uuid,
    pub name: String,
    pub provider_type: String,
    pub api_endpoint: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncDnsZonesRequest {
    pub provider_id: Uuid,
    pub zone_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncDnsZonesResponse {
    pub provider_id: Uuid,
    pub zone_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsZoneOverview {
    pub zone_id: Uuid,
    pub provider_id: Uuid,
    pub zone_name: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateDnsZoneStatusResponse {
    pub zone_id: Uuid,
    pub zone_name: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteDnsZoneResponse {
    pub zone_id: Uuid,
    pub zone_name: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCertificateOrderRequest {
    #[serde(default)]
    pub site_id: Option<Uuid>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub sans: Vec<String>,
    pub acme_provider: String,
    pub challenge_type: String,
    pub zone_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCertificateOrderResponse {
    pub order_id: Uuid,
    pub certificate_id: Option<Uuid>,
    pub order_status: String,
    pub challenge_payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificateOrderOverview {
    pub order_id: Uuid,
    pub site_id: Option<Uuid>,
    pub certificate_id: Option<Uuid>,
    pub zone_id: Uuid,
    pub order_type: String,
    pub acme_provider: String,
    pub challenge_type: String,
    pub order_status: String,
    pub challenge_payload: Value,
    pub error_message: Option<String>,
    pub certificate_expires_at: Option<DateTime<Utc>>,
    pub next_renew_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFailoverPolicyRequest {
    pub scope_type: String,
    pub scope_id: Uuid,
    pub primary_node_id: Uuid,
    pub standby_node_id: Uuid,
    pub trigger_mode: String,
    pub failure_threshold: u32,
    pub recover_threshold: u32,
    pub precheck_policy: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateNodeOperationRequest {
    pub template_id: Uuid,
    pub input_params: Value,
    pub approval_ticket: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOperationTemplateRequest {
    pub name: String,
    pub operation_type: String,
    pub command_template: String,
    pub allowed_params: Vec<String>,
    pub timeout_seconds: u32,
    pub run_as_user: String,
    pub approval_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationTemplateOverview {
    pub template_id: Uuid,
    pub name: String,
    pub operation_type: String,
    pub command_template: String,
    pub allowed_params: Vec<String>,
    pub timeout_seconds: u32,
    pub run_as_user: String,
    pub approval_required: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteOperationTemplateResponse {
    pub template_id: Uuid,
    pub name: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeOperationOverview {
    pub operation_id: Uuid,
    pub node_id: Uuid,
    pub template_id: Uuid,
    pub template_name: String,
    pub operation_type: String,
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
