use crate::error::OutboxError;
use crate::storage::OutboxStorage;
use serde::Serialize;
use std::fmt::Debug;
use std::sync::Arc;

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
        Self {
            storage,
            _marker: std::marker::PhantomData,
        }
    }

    pub async fn collect_garbage(&self) -> Result<(), OutboxError> {
        self.storage.delete_garbage().await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::storage::MockOutboxStorage;
    use rstest::rstest;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct TestPayload;

    #[rstest]
    #[tokio::test]
    async fn collect_garbage_ok_proxies_to_delete_garbage() {
        let mut storage = MockOutboxStorage::<TestPayload>::new();
        storage
            .expect_delete_garbage()
            .times(1)
            .returning(|| Ok(()));

        let gc = GarbageCollector::new(Arc::new(storage));
        assert!(gc.collect_garbage().await.is_ok());
    }

    #[rstest]
    #[tokio::test]
    async fn collect_garbage_propagates_storage_error() {
        let mut storage = MockOutboxStorage::<TestPayload>::new();
        storage
            .expect_delete_garbage()
            .times(1)
            .returning(|| Err(OutboxError::DatabaseError("gc failed".into())));

        let gc = GarbageCollector::new(Arc::new(storage));
        let result = gc.collect_garbage().await;
        assert!(matches!(result, Err(OutboxError::DatabaseError(_))));
    }

    #[rstest]
    #[tokio::test]
    async fn collect_garbage_invokes_storage_each_call_with_no_caching() {
        let mut storage = MockOutboxStorage::<TestPayload>::new();
        // Stateless: two invocations must trigger two storage calls.
        storage
            .expect_delete_garbage()
            .times(2)
            .returning(|| Ok(()));

        let gc = GarbageCollector::new(Arc::new(storage));
        assert!(gc.collect_garbage().await.is_ok());
        assert!(gc.collect_garbage().await.is_ok());
    }
}
