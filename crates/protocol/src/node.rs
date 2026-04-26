use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRegisterRequest {
    pub node_code: String,
    pub bootstrap_token: String,
    pub hostname: String,
    pub public_ip: Option<String>,
    pub private_ip: Option<String>,
    pub agent_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRegisterResponse {
    pub node_id: Uuid,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRefreshRequest {
    pub node_id: Uuid,
    pub refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRefreshResponse {
    pub node_id: Uuid,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeHeartbeatRequest {
    pub node_id: Uuid,
    pub pingora_version: String,
    pub agent_version: String,
    pub active_config_version: Option<String>,
    pub site_count: u32,
    pub cpu_usage: f32,
    pub mem_usage: f32,
    pub disk_usage: f32,
    pub health_score: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeHeartbeatResponse {
    pub server_time: DateTime<Utc>,
    pub next_heartbeat_after_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestReleaseResponse {
    pub release_id: Uuid,
    pub release_version: String,
    pub config_hash: String,
    pub download_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigPackageResponse {
    pub release_id: Uuid,
    pub release_version: String,
    pub manifest: Value,
    pub rendered_config: Value,
    pub certificates: Value,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseAckRequest {
    pub node_id: Uuid,
    pub apply_status: String,
    pub current_version: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseAckResponse {
    pub release_id: Uuid,
    pub node_id: Uuid,
    pub apply_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResultRequest {
    pub node_id: Uuid,
    pub exec_status: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub finished_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingOperationResponse {
    pub operation_id: Uuid,
    pub node_id: Uuid,
    pub template_id: Uuid,
    pub template_name: String,
    pub operation_type: String,
    pub rendered_command: String,
    pub timeout_seconds: u64,
    pub run_as_user: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResultResponse {
    pub operation_id: Uuid,
    pub node_id: Uuid,
    pub exec_status: String,
}
