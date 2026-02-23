use crate::error::OutboxError;
use crate::model::Event;

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait Transport: Send + Sync + 'static {
    /// Sends an event to an external system.
    async fn publish(&self, event: Event) -> Result<(), OutboxError>;
}
