use crate::error::OutboxError;

#[async_trait::async_trait]
pub trait EventPublisher: Send + Sync {
    /// Sends an event to an external system.
    async fn publish(&self, event_type: &str, payload: &serde_json::Value) -> Result<(), OutboxError>;
}