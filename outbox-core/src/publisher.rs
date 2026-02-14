use crate::error::OutboxError;
use crate::object::{EventType, Payload};

#[async_trait::async_trait]
pub trait EventPublisher: Send + Sync {
    /// Sends an event to an external system.
    async fn publish(&self, event_type: EventType, payload: Payload) -> Result<(), OutboxError>;
}