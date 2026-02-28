use std::fmt::Debug;
use crate::error::OutboxError;
use crate::storage::OutboxStorage;
use std::sync::Arc;
use serde::Serialize;

pub(crate) struct GarbageCollector<S, P> {
    storage: Arc<S>,
    _marker: std::marker::PhantomData<P>,
}

impl<S, P> GarbageCollector<S, P>
where
    S: OutboxStorage<P> + 'static,
    P: Debug + Clone + Serialize + Send + Sync,
{
    pub fn new(storage: Arc<S>) -> Self {
        Self { storage, _marker: std::marker::PhantomData }
    }

    pub async fn collect_garbage(&self) -> Result<(), OutboxError> {
        self.storage.delete_garbage().await
    }
}
