use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostgresConfig {
    pub url: String,
    pub max_connections: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EtcdConfig {
    pub endpoints: Vec<String>,
    pub prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub backend: String,
    pub redis_key_prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificateConfig {
    pub auto_renew_before_days: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubConfig {
    pub bind: String,
    pub postgres: PostgresConfig,
    pub redis: RedisConfig,
    pub etcd: EtcdConfig,
    pub storage: StorageConfig,
    pub certificate: CertificateConfig,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:3000".to_string(),
            postgres: PostgresConfig {
                url: "postgres://postgres:postgres@127.0.0.1:5432/pingorahub".to_string(),
                max_connections: 10,
            },
            redis: RedisConfig {
                url: "redis://127.0.0.1:6379".to_string(),
            },
            etcd: EtcdConfig {
                endpoints: vec!["http://127.0.0.1:2379".to_string()],
                prefix: "/pingorahub".to_string(),
            },
            storage: StorageConfig {
                backend: "memory".to_string(),
                redis_key_prefix: "pingorahub".to_string(),
            },
            certificate: CertificateConfig {
                auto_renew_before_days: 30,
            },
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("bind address must not be empty")]
    EmptyBindAddress,
    #[error("at least one etcd endpoint is required")]
    EmptyEtcdEndpoints,
    #[error("storage backend must be one of: memory, postgres_redis")]
    InvalidStorageBackend,
    #[error("certificate auto-renew-before days must be at least 1")]
    InvalidCertificateAutoRenewBeforeDays,
}

impl HubConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.bind.trim().is_empty() {
            return Err(ConfigError::EmptyBindAddress);
        }

        if self.etcd.endpoints.is_empty() {
            return Err(ConfigError::EmptyEtcdEndpoints);
        }

        if self.storage.backend != "memory" && self.storage.backend != "postgres_redis" {
            return Err(ConfigError::InvalidStorageBackend);
        }

        if self.certificate.auto_renew_before_days < 1 {
            return Err(ConfigError::InvalidCertificateAutoRenewBeforeDays);
        }

        Ok(())
    }
}
