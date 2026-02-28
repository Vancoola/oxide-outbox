use crate::error::OutboxError;
use crate::model::{Event, EventStatus};
use crate::object::EventId;
use async_trait::async_trait;
use serde::Serialize;
use std::fmt::Debug;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait OutboxStorage<P>
where
    P: Debug + Clone + Serialize + Send + Sync,
{
    ///Returns all messages with the 'Pending' status
    async fn fetch_next_to_process(&self, limit: u32) -> Result<Vec<Event<P>>, OutboxError>;
    async fn updates_status(&self, id: &[EventId], status: EventStatus) -> Result<(), OutboxError>;
    async fn delete_garbage(&self) -> Result<(), OutboxError>;
    async fn wait_for_notification(&self, channel: &str) -> Result<(), OutboxError>;
}

#[async_trait::async_trait]
pub trait OutboxWriter<P>
where
    P: Debug + Clone + Serialize + Send + Sync,
{
    async fn insert_event(&self, event: Event<P>) -> Result<(), OutboxError>;
}
