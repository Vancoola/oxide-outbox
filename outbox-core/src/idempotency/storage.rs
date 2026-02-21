use async_trait::async_trait;
use crate::error::OutboxError;
use crate::object::IdempotencyToken;

#[async_trait]
pub trait IdempotencyStorageProvider {
    async fn try_reserve(&self, token: &IdempotencyToken) -> Result<bool, OutboxError>;
}