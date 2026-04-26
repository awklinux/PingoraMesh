use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificateRef {
    pub certificate_id: Uuid,
    pub version: i32,
    pub common_name: String,
    pub sans: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub cert_pem: String,
    pub key_pem_encrypted: String,
    pub chain_pem: Option<String>,
}
