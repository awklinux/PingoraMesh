use crate::store::StoreError;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenLease {
    pub node_id: Uuid,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuedTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
}

#[async_trait]
pub trait NodeTokenStore: Send + Sync {
    async fn issue_tokens(&self, node_id: Uuid, ttl: Duration) -> Result<IssuedTokens, StoreError>;
    async fn access_lease(&self, access_token: &str) -> Result<Option<TokenLease>, StoreError>;
    async fn refresh_lease(&self, refresh_token: &str) -> Result<Option<TokenLease>, StoreError>;
    async fn revoke_tokens(&self, node_id: Uuid) -> Result<(), StoreError>;
}

#[derive(Debug, Default)]
struct MemoryTokenState {
    access: HashMap<String, TokenLease>,
    refresh: HashMap<String, TokenLease>,
    current_access_by_node: HashMap<Uuid, String>,
    current_refresh_by_node: HashMap<Uuid, String>,
}

#[derive(Debug, Default)]
pub struct MemoryTokenStore {
    inner: RwLock<MemoryTokenState>,
}

#[derive(Debug, Clone)]
pub struct RedisTokenStore {
    client: redis::Client,
    key_prefix: String,
}

impl RedisTokenStore {
    pub fn new(url: &str, key_prefix: impl Into<String>) -> Result<Self, StoreError> {
        let client =
            redis::Client::open(url).map_err(|error| StoreError::Redis(error.to_string()))?;
        Ok(Self {
            client,
            key_prefix: key_prefix.into(),
        })
    }

    fn access_key(&self, token: &str) -> String {
        format!("{}:node:access:{token}", self.key_prefix)
    }

    fn refresh_key(&self, token: &str) -> String {
        format!("{}:node:refresh:{token}", self.key_prefix)
    }

    fn current_access_key(&self, node_id: Uuid) -> String {
        format!("{}:node:current_access:{node_id}", self.key_prefix)
    }

    fn current_refresh_key(&self, node_id: Uuid) -> String {
        format!("{}:node:current_refresh:{node_id}", self.key_prefix)
    }
}

#[async_trait]
impl NodeTokenStore for MemoryTokenStore {
    async fn issue_tokens(&self, node_id: Uuid, ttl: Duration) -> Result<IssuedTokens, StoreError> {
        let mut state = self.inner.write().await;
        if let Some(previous_access) = state.current_access_by_node.remove(&node_id) {
            state.access.remove(&previous_access);
        }
        if let Some(previous_refresh) = state.current_refresh_by_node.remove(&node_id) {
            state.refresh.remove(&previous_refresh);
        }

        let access_token = format!("node-at-{}", Uuid::new_v4());
        let refresh_token = format!("node-rt-{}", Uuid::new_v4());
        let expires_at = Utc::now() + ttl;
        let lease = TokenLease {
            node_id,
            expires_at,
        };

        state.access.insert(access_token.clone(), lease.clone());
        state.refresh.insert(refresh_token.clone(), lease);
        state
            .current_access_by_node
            .insert(node_id, access_token.clone());
        state
            .current_refresh_by_node
            .insert(node_id, refresh_token.clone());

        Ok(IssuedTokens {
            access_token,
            refresh_token,
            expires_at,
        })
    }

    async fn access_lease(&self, access_token: &str) -> Result<Option<TokenLease>, StoreError> {
        let state = self.inner.read().await;
        Ok(state.access.get(access_token).cloned())
    }

    async fn refresh_lease(&self, refresh_token: &str) -> Result<Option<TokenLease>, StoreError> {
        let state = self.inner.read().await;
        Ok(state.refresh.get(refresh_token).cloned())
    }

    async fn revoke_tokens(&self, node_id: Uuid) -> Result<(), StoreError> {
        let mut state = self.inner.write().await;
        if let Some(access_token) = state.current_access_by_node.remove(&node_id) {
            state.access.remove(&access_token);
        }
        if let Some(refresh_token) = state.current_refresh_by_node.remove(&node_id) {
            state.refresh.remove(&refresh_token);
        }
        Ok(())
    }
}

#[async_trait]
impl NodeTokenStore for RedisTokenStore {
    async fn issue_tokens(&self, node_id: Uuid, ttl: Duration) -> Result<IssuedTokens, StoreError> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|error| StoreError::Redis(error.to_string()))?;

        let current_access_key = self.current_access_key(node_id);
        let current_refresh_key = self.current_refresh_key(node_id);

        let previous_access: Option<String> = conn
            .get(&current_access_key)
            .await
            .map_err(|error| StoreError::Redis(error.to_string()))?;
        let previous_refresh: Option<String> = conn
            .get(&current_refresh_key)
            .await
            .map_err(|error| StoreError::Redis(error.to_string()))?;

        if let Some(previous_access) = previous_access {
            let _: usize = conn
                .del(self.access_key(&previous_access))
                .await
                .map_err(|error| StoreError::Redis(error.to_string()))?;
        }
        if let Some(previous_refresh) = previous_refresh {
            let _: usize = conn
                .del(self.refresh_key(&previous_refresh))
                .await
                .map_err(|error| StoreError::Redis(error.to_string()))?;
        }

        let access_token = format!("node-at-{}", Uuid::new_v4());
        let refresh_token = format!("node-rt-{}", Uuid::new_v4());
        let expires_at = Utc::now() + ttl;
        let lease = TokenLease {
            node_id,
            expires_at,
        };
        let ttl_seconds = ttl.num_seconds().max(1) as u64;
        let serialized = serde_json::to_string(&lease)?;

        let _: () = conn
            .set_ex(
                self.access_key(&access_token),
                serialized.clone(),
                ttl_seconds,
            )
            .await
            .map_err(|error| StoreError::Redis(error.to_string()))?;
        let _: () = conn
            .set_ex(self.refresh_key(&refresh_token), serialized, ttl_seconds)
            .await
            .map_err(|error| StoreError::Redis(error.to_string()))?;
        let _: () = conn
            .set_ex(&current_access_key, &access_token, ttl_seconds)
            .await
            .map_err(|error| StoreError::Redis(error.to_string()))?;
        let _: () = conn
            .set_ex(&current_refresh_key, &refresh_token, ttl_seconds)
            .await
            .map_err(|error| StoreError::Redis(error.to_string()))?;

        Ok(IssuedTokens {
            access_token,
            refresh_token,
            expires_at,
        })
    }

    async fn access_lease(&self, access_token: &str) -> Result<Option<TokenLease>, StoreError> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|error| StoreError::Redis(error.to_string()))?;
        let raw: Option<String> = conn
            .get(self.access_key(access_token))
            .await
            .map_err(|error| StoreError::Redis(error.to_string()))?;
        raw.map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(StoreError::from)
    }

    async fn refresh_lease(&self, refresh_token: &str) -> Result<Option<TokenLease>, StoreError> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|error| StoreError::Redis(error.to_string()))?;
        let raw: Option<String> = conn
            .get(self.refresh_key(refresh_token))
            .await
            .map_err(|error| StoreError::Redis(error.to_string()))?;
        raw.map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(StoreError::from)
    }

    async fn revoke_tokens(&self, node_id: Uuid) -> Result<(), StoreError> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|error| StoreError::Redis(error.to_string()))?;

        let current_access_key = self.current_access_key(node_id);
        let current_refresh_key = self.current_refresh_key(node_id);

        let current_access: Option<String> = conn
            .get(&current_access_key)
            .await
            .map_err(|error| StoreError::Redis(error.to_string()))?;
        let current_refresh: Option<String> = conn
            .get(&current_refresh_key)
            .await
            .map_err(|error| StoreError::Redis(error.to_string()))?;

        let _: usize = conn
            .del(&current_access_key)
            .await
            .map_err(|error| StoreError::Redis(error.to_string()))?;
        let _: usize = conn
            .del(&current_refresh_key)
            .await
            .map_err(|error| StoreError::Redis(error.to_string()))?;

        if let Some(access_token) = current_access {
            let _: usize = conn
                .del(self.access_key(&access_token))
                .await
                .map_err(|error| StoreError::Redis(error.to_string()))?;
        }
        if let Some(refresh_token) = current_refresh {
            let _: usize = conn
                .del(self.refresh_key(&refresh_token))
                .await
                .map_err(|error| StoreError::Redis(error.to_string()))?;
        }

        Ok(())
    }
}
