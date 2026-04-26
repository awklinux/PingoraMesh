use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SiteStatus {
    Draft,
    Published,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Http,
    Https,
    Tcp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamEndpoint {
    pub address: String,
    pub weight: u16,
    pub active: bool,
    #[serde(default)]
    pub backup: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Upstream {
    pub name: String,
    pub endpoints: Vec<UpstreamEndpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheRule {
    pub name: String,
    pub match_extensions: Vec<String>,
    pub expires_seconds: u64,
    pub cache_control: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteSpec {
    pub id: Uuid,
    pub site_code: String,
    pub name: String,
    pub domain: String,
    pub listen_port: u16,
    pub protocol: Protocol,
    pub tls_enabled: bool,
    pub status: SiteStatus,
    pub upstreams: Vec<Upstream>,
    #[serde(default)]
    pub cache_rules: Vec<CacheRule>,
}
