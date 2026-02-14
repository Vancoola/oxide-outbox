use uuid::Uuid;
use crate::config::OutboxConfig;
use crate::error::OutboxError;
use crate::object::SlotId;
use crate::Outbox;
use crate::storage::OutboxStorage;

pub(crate) struct GarbageCollector<S>
where
    S: OutboxStorage + Clone + 'static
{
    storage: S,
    config: OutboxConfig
}

impl<S> GarbageCollector<S>
where
    S: OutboxStorage + Clone + 'static
{
    pub(crate) async fn new(storage: S, config: OutboxConfig) -> Self {
        Self {
            storage,
            config
        }
    }

    pub(crate) async fn collect_garbage(&self, ids: &Vec<SlotId>) -> Result<(), OutboxError> {
        self.storage.delete_garbage(ids).await
    }

}
