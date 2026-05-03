//! Redis-backed [`DlqHeap`] implementation.
//!
//! Failure counters are stored in a single Redis sorted set
//! (`<key_prefix>:dlq`), where:
//! - `member` = event id (UUID as string)
//! - `score`  = current failure count
//!
//! - `record_failure` → `ZINCRBY` (atomic increment, creates entry if absent)
//! - `record_success` → `ZREM`
//! - `drain_exceeded` → atomic Lua script: read entries with `score >= threshold`,
//!   remove them in the same pass. This guarantees the same id is not returned
//!   to two concurrent callers.
//!
//! `DlqEntry::last_error` is always `None` here: the [`DlqHeap`] trait does not
//! pass an error string into `record_failure`, so there is nothing to persist.

use crate::{RedisProvider};
use async_trait::async_trait;
use outbox_core::prelude::{DlqEntry, DlqHeap, EventId, OutboxError};
use tracing::error;
use uuid::Uuid;

const DLQ_KEY_SUFFIX: &str = "dlq";

#[async_trait]
impl DlqHeap for RedisProvider {
    async fn record_failure(&self, id: EventId) -> Result<(), OutboxError> {
        let mut conn = self.connection.clone();
        let key = self.dlq_key();
        let member = id.as_uuid().to_string();

        let _: f64 = redis::cmd("ZINCRBY")
            .arg(&key)
            .arg(1_i64)
            .arg(&member)
            .query_async(&mut conn)
            .await
            .map_err(map_err)?;
        Ok(())
    }

    async fn record_success(&self, id: EventId) -> Result<(), OutboxError> {
        let mut conn = self.connection.clone();
        let key = self.dlq_key();
        let member = id.as_uuid().to_string();

        let _: i64 = redis::cmd("ZREM")
            .arg(&key)
            .arg(&member)
            .query_async(&mut conn)
            .await
            .map_err(map_err)?;
        Ok(())
    }

    async fn drain_exceeded(&self, threshold: u32) -> Result<Vec<DlqEntry>, OutboxError> {
        let mut conn = self.connection.clone();
        let key = self.dlq_key();

        let script = redis::Script::new(
            r"
            local items = redis.call('ZRANGEBYSCORE', KEYS[1], ARGV[1], '+inf', 'WITHSCORES')
            if #items > 0 then
                redis.call('ZREMRANGEBYSCORE', KEYS[1], ARGV[1], '+inf')
            end
            return items
            ",
        );

        let raw: Vec<String> = script
            .key(&key)
            .arg(threshold)
            .invoke_async(&mut conn)
            .await
            .map_err(map_err)?;

        let mut out = Vec::with_capacity(raw.len() / 2);
        for pair in raw.chunks_exact(2) {
            let uuid = Uuid::parse_str(&pair[0]).map_err(|e| {
                OutboxError::InfrastructureError(format!(
                    "Invalid UUID '{}' in DLQ zset: {e}",
                    pair[0]
                ))
            })?;
            let score: f64 = pair[1].parse().map_err(|e| {
                OutboxError::InfrastructureError(format!(
                    "Invalid score '{}' in DLQ zset: {e}",
                    pair[1]
                ))
            })?;
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let count = score.max(0.0) as u32;
            out.push(DlqEntry::new(EventId::load(uuid), count, None));
        }
        Ok(out)
    }
}

impl RedisProvider {
    fn dlq_key(&self) -> String {
        format!("{}:{}", self.config.key_prefix, DLQ_KEY_SUFFIX)
    }
}

fn map_err(e: redis::RedisError) -> OutboxError {
    error!("Redis DLQ command failed: {e:?}");
    OutboxError::InfrastructureError(e.to_string())
}
