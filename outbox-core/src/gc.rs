use crate::error::OutboxError;
use crate::storage::OutboxStorage;

pub(crate) struct GarbageCollector<S>
where
    S: OutboxStorage + Clone + 'static
{
    storage: S,
}

impl<S> GarbageCollector<S>
where
    S: OutboxStorage + Clone + 'static
{
    pub fn new(storage: S) -> Self {
        Self {
            storage,
        }
    }

    pub async fn collect_garbage(&self) -> Result<(), OutboxError> {
        self.storage.delete_garbage().await
    }

}
