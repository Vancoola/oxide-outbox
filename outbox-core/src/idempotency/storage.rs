use async_trait::async_trait;
use crate::error::OutboxError;
use crate::object::IdempotencyToken;

#[async_trait]
pub trait IdempotencyStorageProvider {
    async fn try_reserve(&self, token: &IdempotencyToken) -> Result<bool, OutboxError>;
}

pub struct NoIdempotency;
#[async_trait]
impl IdempotencyStorageProvider for NoIdempotency {
    async fn try_reserve(&self, _token: &IdempotencyToken) -> Result<bool, OutboxError> {
        Ok(true)
    }
}