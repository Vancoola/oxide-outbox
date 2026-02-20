use async_trait::async_trait;
use crate::error::OutboxError;
use crate::object::SlotId;
use crate::model::{OutboxSlot, SlotStatus};

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait OutboxStorage {
    ///Returns all messages with the 'Pending' status
    async fn fetch_next_to_process(&self, limit: u32) -> Result<Vec<OutboxSlot>, OutboxError>;
    async fn updates_status(&self, id: &Vec<SlotId>, status: SlotStatus) -> Result<(), OutboxError>;
    async fn delete_garbage(&self) -> Result<(), OutboxError>;
    async fn wait_for_notification(&self, channel: &str) -> Result<(), OutboxError>;
}

#[async_trait::async_trait]
pub trait OutboxWriter {
    async fn insert_event(
        &self,
        event: OutboxSlot
    ) -> Result<(), OutboxError>;
}