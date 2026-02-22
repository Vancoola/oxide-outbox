pub mod config;

use crate::config::RedisTokenConfig;
use async_trait::async_trait;
use outbox_core::prelude::{IdempotencyStorageProvider, IdempotencyToken, OutboxError};
use redis::aio::MultiplexedConnection;
use tracing::error;

pub struct RedisTokenProvider {
    connection: MultiplexedConnection,
    #[cfg(feature = "moka")]
    local_cache: moka::future::Cache<String, ()>,
    config: RedisTokenConfig,
}

impl RedisTokenProvider {
    /// Creates a new `RedisTokenProvider` and establishes a connection to Redis.
    ///
    /// # Errors
    ///
    /// Returns [`OutboxError::InfrastructureError`] if:
    /// - The `connection_info` string is not a valid Redis URL.
    /// - The connection to the Redis server cannot be established.
    pub async fn new(connection_info: &str, config: RedisTokenConfig) -> Result<Self, OutboxError> {
        let client = redis::Client::open(connection_info)
            .map_err(|e| OutboxError::InfrastructureError(format!("Invalid Redis URL: {e}")))?;
        let conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| {
                error!("Redis connection failed: {:?}", e);
                OutboxError::InfrastructureError("Redis connection failed".to_string())
            })?;
        Ok(Self {
            connection: conn,
            #[cfg(feature = "moka")]
            local_cache: moka::future::Cache::builder()
                .time_to_live(config.ttl)
                .build(),
            config,
        })
    }
}

#[async_trait]
impl IdempotencyStorageProvider for RedisTokenProvider {
    async fn try_reserve(&self, token: &IdempotencyToken) -> Result<bool, OutboxError> {
        let token_str = token.as_str();
        let redis_key = format!("{}:{}", self.config.key_prefix, token_str);
        #[cfg(feature = "moka")]
        {
            if self.local_cache.contains_key(token_str) {
                return Ok(false);
            }
        }

        let mut conn = self.connection.clone();

        let result: Option<String> = redis::cmd("SET")
            .arg(&redis_key)
            .arg(1)
            .arg("NX")
            .arg("EX")
            .arg(self.config.ttl.as_secs())
            .query_async(&mut conn)
            .await
            .map_err(|e| {
                error!("Redis query failed: {:?}", e);
                OutboxError::InfrastructureError(e.to_string())
            })?;

        let is_new = result.is_some();

        if is_new {
            #[cfg(feature = "moka")]
            {
                self.local_cache.insert(token_str.to_string(), ()).await;
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
