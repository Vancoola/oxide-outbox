use async_trait::async_trait;
use crate::error::OutboxError;
use crate::object::EventId;
use crate::model::{Event, EventStatus};

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait OutboxStorage {
    ///Returns all messages with the 'Pending' status
    async fn fetch_next_to_process(&self, limit: u32) -> Result<Vec<Event>, OutboxError>;
    async fn updates_status(&self, id: &Vec<EventId>, status: EventStatus) -> Result<(), OutboxError>;
    async fn delete_garbage(&self) -> Result<(), OutboxError>;
    async fn wait_for_notification(&self, channel: &str) -> Result<(), OutboxError>;
}

#[async_trait::async_trait]
pub trait OutboxWriter {
    async fn insert_event(
        &self,
        event: Event
    ) -> Result<(), OutboxError>;
}