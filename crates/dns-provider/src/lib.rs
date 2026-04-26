mod cloudflare;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub use cloudflare::{CloudflareConfig, CloudflareDnsProvider};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMetadata {
    pub provider_type: String,
    pub supports_dns01: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderZone {
    pub zone_id: String,
    pub zone_name: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsRecordChange {
    pub zone_id: Option<String>,
    pub zone_name: String,
    pub record_type: String,
    pub host: String,
    pub value: String,
    pub ttl: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcmeChallenge {
    pub zone_id: Option<String>,
    pub zone_name: String,
    pub fqdn: String,
    pub value: String,
    pub ttl: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub provider_type: String,
    pub api_endpoint: Option<String>,
    pub credentials: serde_json::Value,
}

#[async_trait]
pub trait DnsProvider: Send + Sync {
    fn metadata(&self) -> ProviderMetadata;
    async fn list_zones(&self, names: &[String]) -> Result<Vec<ProviderZone>>;
    async fn create_record(&self, change: &DnsRecordChange) -> Result<()>;
    async fn update_record(&self, change: &DnsRecordChange) -> Result<()>;
    async fn delete_record(&self, change: &DnsRecordChange) -> Result<()>;
    async fn present_dns01_challenge(&self, challenge: &AcmeChallenge) -> Result<()>;
    async fn cleanup_dns01_challenge(&self, challenge: &AcmeChallenge) -> Result<()>;
}

#[derive(Debug, Default)]
pub struct NoopDnsProvider;

#[async_trait]
impl DnsProvider for NoopDnsProvider {
    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            provider_type: "noop".to_string(),
            supports_dns01: true,
        }
    }

    async fn list_zones(&self, names: &[String]) -> Result<Vec<ProviderZone>> {
        Ok(names
            .iter()
            .map(|name| ProviderZone {
                zone_id: format!("noop-{name}"),
                zone_name: name.clone(),
                status: "active".to_string(),
            })
            .collect())
    }

    async fn create_record(&self, _change: &DnsRecordChange) -> Result<()> {
        Ok(())
    }

    async fn update_record(&self, _change: &DnsRecordChange) -> Result<()> {
        Ok(())
    }

    async fn delete_record(&self, _change: &DnsRecordChange) -> Result<()> {
        Ok(())
    }

    async fn present_dns01_challenge(&self, _challenge: &AcmeChallenge) -> Result<()> {
        Ok(())
    }

    async fn cleanup_dns01_challenge(&self, _challenge: &AcmeChallenge) -> Result<()> {
        Ok(())
    }
}

pub fn build_provider(config: &ProviderConfig) -> Result<Box<dyn DnsProvider>> {
    match config.provider_type.as_str() {
        "cloudflare" => Ok(Box::new(CloudflareDnsProvider::from_config(
            CloudflareConfig::from_provider_config(config)?,
        ))),
        "noop" | "custom" => Ok(Box::new(NoopDnsProvider)),
        other => anyhow::bail!("unsupported dns provider type: {other}"),
    }
}
