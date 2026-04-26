use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{CertificateRef, SiteSpec};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseTarget {
    pub node_id: Uuid,
    pub site_ids: Vec<Uuid>,
    pub preheated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseManifest {
    pub release_id: Uuid,
    pub release_version: String,
    pub scope: String,
    pub config_hash: String,
    pub created_at: DateTime<Utc>,
    pub sites: Vec<SiteSpec>,
    pub certificates: Vec<CertificateRef>,
    pub targets: Vec<ReleaseTarget>,
}
