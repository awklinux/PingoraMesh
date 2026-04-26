use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Pending,
    Online,
    Suspect,
    Offline,
    Maintenance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeIdentity {
    pub id: Uuid,
    pub node_code: String,
    pub name: String,
    pub region: String,
    pub idc: String,
    pub status: NodeStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRuntimeState {
    pub active_config_version: Option<String>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub site_count: u32,
    pub health_score: u8,
}
