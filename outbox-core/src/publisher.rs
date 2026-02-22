use crate::error::OutboxError;
use crate::object::{EventType, Payload};

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    /// Sends an event to an external system.
    async fn publish(&self, event_type: EventType, payload: Payload) -> Result<(), OutboxError>;
}
