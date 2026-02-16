use async_trait::async_trait;
use crate::error::OutboxError;
use crate::object::SlotId;
use crate::model::{OutboxSlot, SlotStatus};

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait OutboxStorage {
    async fn fetch_next_to_process(&self, limit: u32) -> Result<Vec<OutboxSlot>, OutboxError>;
    async fn fetch_unprocessed(&self, limit: u32) -> Result<Vec<OutboxSlot>, OutboxError>;
    async fn add_fail_counts(&self, ids: &Vec<SlotId>) -> Result<(), OutboxError>;
    async fn update_status(&self, id: &SlotId, status: SlotStatus) -> Result<(), OutboxError>;
    async fn updates_status(&self, id: &Vec<SlotId>, status: SlotStatus) -> Result<(), OutboxError>;
    async fn delete_garbage(&self) -> Result<(), OutboxError>;
    async fn wait_for_notification(&self, channel: &str) -> Result<(), OutboxError>;
}