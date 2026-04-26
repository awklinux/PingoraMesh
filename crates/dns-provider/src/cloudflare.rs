use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};

use crate::{
    AcmeChallenge, DnsProvider, DnsRecordChange, ProviderConfig, ProviderMetadata, ProviderZone,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudflareConfig {
    pub api_token: String,
    pub api_base: String,
}

impl CloudflareConfig {
    pub fn from_provider_config(config: &ProviderConfig) -> Result<Self> {
        let api_token = config
            .credentials
            .get("api_token")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow!("cloudflare api_token is required"))?
            .to_string();

        Ok(Self {
            api_token,
            api_base: config
                .api_endpoint
                .clone()
                .unwrap_or_else(|| "https://api.cloudflare.com/client/v4".to_string()),
        })
    }
}

#[derive(Debug, Clone)]
pub struct CloudflareDnsProvider {
    client: Client,
    config: CloudflareConfig,
}

impl CloudflareDnsProvider {
    pub fn from_config(config: CloudflareConfig) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }

    async fn get_zone_id_by_name(&self, zone_name: &str) -> Result<String> {
        let zones = self.list_zones(&[zone_name.to_string()]).await?;
        zones
            .into_iter()
            .find(|zone| zone.zone_name == zone_name)
            .map(|zone| zone.zone_id)
            .ok_or_else(|| anyhow!("cloudflare zone not found: {zone_name}"))
    }

    async fn delete_record_internal(&self, zone_id: &str, record_id: &str) -> Result<()> {
        let response = self
            .client
            .delete(format!(
                "{}/zones/{zone_id}/dns_records/{record_id}",
                self.config.api_base
            ))
            .bearer_auth(&self.config.api_token)
            .send()
            .await
            .context("failed to call cloudflare delete dns record api")?;

        ensure_success(response.status(), "delete cloudflare dns record")?;
        Ok(())
    }

    async fn find_record_id(
        &self,
        zone_id: &str,
        record_type: &str,
        name: &str,
        content: &str,
    ) -> Result<Option<String>> {
        let response = self
            .client
            .get(format!(
                "{}/zones/{zone_id}/dns_records",
                self.config.api_base
            ))
            .bearer_auth(&self.config.api_token)
            .query(&[("type", record_type), ("name", name), ("content", content)])
            .send()
            .await
            .context("failed to call cloudflare list dns records api")?;

        ensure_success(response.status(), "list cloudflare dns records")?;
        let payload: CloudflareListRecordsResponse = response
            .json()
            .await
            .context("failed to decode cloudflare dns record response")?;

        Ok(payload.result.into_iter().next().map(|record| record.id))
    }

    async fn find_record_id_by_name(
        &self,
        zone_id: &str,
        record_type: &str,
        name: &str,
    ) -> Result<Option<String>> {
        let response = self
            .client
            .get(format!(
                "{}/zones/{zone_id}/dns_records",
                self.config.api_base
            ))
            .bearer_auth(&self.config.api_token)
            .query(&[("type", record_type), ("name", name)])
            .send()
            .await
            .context("failed to call cloudflare list dns records api")?;

        ensure_success(response.status(), "list cloudflare dns records")?;
        let payload: CloudflareListRecordsResponse = response
            .json()
            .await
            .context("failed to decode cloudflare dns record response")?;

        Ok(payload.result.into_iter().next().map(|record| record.id))
    }
}

#[async_trait]
impl DnsProvider for CloudflareDnsProvider {
    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            provider_type: "cloudflare".to_string(),
            supports_dns01: true,
        }
    }

    async fn list_zones(&self, names: &[String]) -> Result<Vec<ProviderZone>> {
        if names.is_empty() {
            let response = self
                .client
                .get(format!("{}/zones", self.config.api_base))
                .bearer_auth(&self.config.api_token)
                .send()
                .await
                .context("failed to call cloudflare list zones api")?;
            ensure_success(response.status(), "list cloudflare zones")?;
            let payload: CloudflareZonesResponse = response
                .json()
                .await
                .context("failed to decode cloudflare zones response")?;
            return Ok(payload
                .result
                .into_iter()
                .map(|zone| ProviderZone {
                    zone_id: zone.id,
                    zone_name: zone.name,
                    status: zone.status,
                })
                .collect());
        }

        let mut zones = Vec::with_capacity(names.len());
        for name in names {
            let response = self
                .client
                .get(format!("{}/zones", self.config.api_base))
                .bearer_auth(&self.config.api_token)
                .query(&[("name", name.as_str())])
                .send()
                .await
                .with_context(|| format!("failed to call cloudflare list zone api for {name}"))?;
            ensure_success(response.status(), "list cloudflare zone by name")?;
            let payload: CloudflareZonesResponse = response
                .json()
                .await
                .context("failed to decode cloudflare zone lookup response")?;
            zones.extend(payload.result.into_iter().map(|zone| ProviderZone {
                zone_id: zone.id,
                zone_name: zone.name,
                status: zone.status,
            }));
        }

        Ok(zones)
    }

    async fn create_record(&self, change: &DnsRecordChange) -> Result<()> {
        let zone_id = match &change.zone_id {
            Some(zone_id) => zone_id.clone(),
            None => self.get_zone_id_by_name(&change.zone_name).await?,
        };
        let name = normalize_record_name(&change.host, &change.zone_name);

        let response = self
            .client
            .post(format!(
                "{}/zones/{zone_id}/dns_records",
                self.config.api_base
            ))
            .bearer_auth(&self.config.api_token)
            .json(&serde_json::json!({
                "type": change.record_type,
                "name": name,
                "content": change.value,
                "ttl": change.ttl,
            }))
            .send()
            .await
            .context("failed to call cloudflare create dns record api")?;

        ensure_success(response.status(), "create cloudflare dns record")?;
        Ok(())
    }

    async fn update_record(&self, change: &DnsRecordChange) -> Result<()> {
        let zone_id = match &change.zone_id {
            Some(zone_id) => zone_id.clone(),
            None => self.get_zone_id_by_name(&change.zone_name).await?,
        };
        let name = normalize_record_name(&change.host, &change.zone_name);
        let record_id = self
            .find_record_id_by_name(&zone_id, &change.record_type, &name)
            .await?
            .ok_or_else(|| anyhow!("cloudflare dns record not found for update"))?;

        let response = self
            .client
            .put(format!(
                "{}/zones/{zone_id}/dns_records/{record_id}",
                self.config.api_base
            ))
            .bearer_auth(&self.config.api_token)
            .json(&serde_json::json!({
                "type": change.record_type,
                "name": name,
                "content": change.value,
                "ttl": change.ttl,
            }))
            .send()
            .await
            .context("failed to call cloudflare update dns record api")?;

        ensure_success(response.status(), "update cloudflare dns record")?;
        Ok(())
    }

    async fn delete_record(&self, change: &DnsRecordChange) -> Result<()> {
        let zone_id = match &change.zone_id {
            Some(zone_id) => zone_id.clone(),
            None => self.get_zone_id_by_name(&change.zone_name).await?,
        };
        let name = normalize_record_name(&change.host, &change.zone_name);
        if let Some(record_id) = self
            .find_record_id(&zone_id, &change.record_type, &name, &change.value)
            .await?
        {
            self.delete_record_internal(&zone_id, &record_id).await?;
        }
        Ok(())
    }

    async fn present_dns01_challenge(&self, challenge: &AcmeChallenge) -> Result<()> {
        self.create_record(&DnsRecordChange {
            zone_id: challenge.zone_id.clone(),
            zone_name: challenge.zone_name.clone(),
            record_type: "TXT".to_string(),
            host: challenge.fqdn.clone(),
            value: challenge.value.clone(),
            ttl: challenge.ttl,
        })
        .await
    }

    async fn cleanup_dns01_challenge(&self, challenge: &AcmeChallenge) -> Result<()> {
        self.delete_record(&DnsRecordChange {
            zone_id: challenge.zone_id.clone(),
            zone_name: challenge.zone_name.clone(),
            record_type: "TXT".to_string(),
            host: challenge.fqdn.clone(),
            value: challenge.value.clone(),
            ttl: challenge.ttl,
        })
        .await
    }
}

#[derive(Debug, Deserialize)]
struct CloudflareZonesResponse {
    result: Vec<CloudflareZone>,
}

#[derive(Debug, Deserialize)]
struct CloudflareZone {
    id: String,
    name: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct CloudflareListRecordsResponse {
    result: Vec<CloudflareRecord>,
}

#[derive(Debug, Deserialize)]
struct CloudflareRecord {
    id: String,
}

fn normalize_record_name(host: &str, zone_name: &str) -> String {
    if host == "@" {
        zone_name.to_string()
    } else if host.ends_with(zone_name) {
        host.to_string()
    } else {
        format!("{host}.{zone_name}")
    }
}

fn ensure_success(status: StatusCode, action: &str) -> Result<()> {
    if status.is_success() {
        Ok(())
    } else {
        Err(anyhow!("{action} failed with status {status}"))
    }
}
