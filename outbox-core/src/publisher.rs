use crate::error::OutboxError;
use crate::model::Event;
use std::fmt::Debug;

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait Transport<P>: Send + Sync
where
    P: Debug + Clone + Send + Sync,
{
    /// Sends an event to an external system.
    async fn publish(&self, event: Event<P>) -> Result<(), OutboxError>;
}
