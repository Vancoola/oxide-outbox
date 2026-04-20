use async_trait::async_trait;
use crate::error::OutboxError;
use crate::object::EventId;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait DlqHeap: Send + Sync {
    async fn record_failure(&self, id: EventId) -> Result<u32, OutboxError>;
    async fn record_success(&self, id: EventId) -> Result<(), OutboxError>;
    async fn drain_exceeded(&self, threshold: u32) -> Result<Vec<EventId>, OutboxError>;
}