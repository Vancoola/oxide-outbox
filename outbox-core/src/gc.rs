use crate::error::OutboxError;
use crate::storage::OutboxStorage;
use std::sync::Arc;

pub(crate) struct GarbageCollector<S> {
    storage: Arc<S>,
}

impl<S> GarbageCollector<S>
where
    S: OutboxStorage + 'static,
{
    pub fn new(storage: Arc<S>) -> Self {
        Self { storage }
    }

    pub async fn collect_garbage(&self) -> Result<(), OutboxError> {
        self.storage.delete_garbage().await
    }
}
