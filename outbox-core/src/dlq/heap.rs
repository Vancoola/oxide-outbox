use crate::error::OutboxError;
use crate::object::EventId;

#[derive(Debug)]
pub struct DlqHeap {}
impl DlqHeap {
    pub fn new() -> Self {
        Self {}
    }
    pub async fn add(&self, event_id: EventId) -> Result<(), OutboxError> {
        Ok(())
    }
    pub async fn get_batch(&self, batch_size: usize) -> Result<Vec<EventId>, OutboxError> {
        Ok(vec![])
    }
}
