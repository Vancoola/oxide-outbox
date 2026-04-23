use crate::config::OutboxConfig;
use crate::dlq::storage::DlqHeap;
use crate::error::OutboxError;
use crate::manager::OutboxManager;
use crate::prelude::{OutboxStorage, Transport};
use serde::Serialize;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::watch::Receiver;

pub struct OutboxManagerBuilder<S, P, PT>
where
    PT: Debug + Clone + Serialize,
{
    storage: Option<Arc<S>>,
    publisher: Option<Arc<P>>,
    config: Option<Arc<OutboxConfig<PT>>>,
    shutdown_rx: Option<Receiver<bool>>,
    #[cfg(feature = "dlq")]
    dlq_heap: Option<Arc<dyn DlqHeap>>,
}
impl<S, P, PT> Default for OutboxManagerBuilder<S, P, PT>
where
    PT: Debug + Clone + Serialize,
{
    fn default() -> Self {
        Self {
            storage: None,
            publisher: None,
            config: None,
            shutdown_rx: None,
            #[cfg(feature = "dlq")]
            dlq_heap: None,
        }
    }
}

impl<S, P, PT> OutboxManagerBuilder<S, P, PT>
where
    S: OutboxStorage<PT> + Send + Sync + 'static,
    P: Transport<PT> + Send + Sync + 'static,
    PT: Debug + Clone + Serialize + Send + Sync + 'static,
{

    pub fn new() -> Self {
        Self::default()
    }

    pub fn storage(mut self, s: Arc<S>) -> Self {
        self.storage = Some(s);
        self
    }
    pub fn publisher(mut self, p: Arc<P>) -> Self {
        self.publisher = Some(p);
        self
    }
    pub fn config(mut self, config: Arc<OutboxConfig<PT>>) -> Self {
        self.config = Some(config);
        self
    }
    pub fn shutdown_rx(mut self, rx: Receiver<bool>) -> Self {
        self.shutdown_rx = Some(rx);
        self
    }
    #[cfg(feature = "dlq")]
    pub fn dlq_heap(mut self, heap: Arc<dyn DlqHeap>) -> Self {
        self.dlq_heap = Some(heap);
        self
    }

    pub fn build(self) -> Result<OutboxManager<S, P, PT>, OutboxError> {
        #[cfg(feature = "dlq")]
        return Ok(OutboxManager::new(
            self.storage.ok_or_else(|| OutboxError::ConfigError("Storage config is missing".to_string()))?,
            self.publisher.ok_or_else(|| OutboxError::ConfigError("Publisher config is missing".to_string()))?,
            self.config.ok_or_else(|| OutboxError::ConfigError("Config config is missing".to_string()))?,
            self.dlq_heap.ok_or_else(|| OutboxError::ConfigError("Dlq heap config is missing".to_string()))?,
            self.shutdown_rx.ok_or_else(|| OutboxError::ConfigError("Shutdown channel is missing".to_string()))?,
        ));
        #[cfg(not(feature = "dlq"))]
        return Ok(OutboxManager::new(
            self.storage.ok_or_else(|| OutboxError::ConfigError("Storage config is missing".to_string()))?,
            self.publisher.ok_or_else(|| OutboxError::ConfigError("Publisher config is missing".to_string()))?,
            self.config.ok_or_else(|| OutboxError::ConfigError("Config config is missing".to_string()))?,
            self.shutdown_rx.ok_or_else(|| OutboxError::ConfigError("Shutdown channel is missing".to_string()))?,
        ));
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde::Deserialize;
    use tokio::sync::watch;
    use crate::config::IdempotencyStrategy;
    use crate::dlq::storage::MockDlqHeap;
    use crate::publisher::MockTransport;
    use crate::storage::MockOutboxStorage;
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    enum SomeDomainEvent {
        SomeEvent(String),
    }

    #[rstest]
    fn test_success_build() {

        let config = OutboxConfig {
            batch_size: 100,
            retention_days: 7,
            gc_interval_secs: 3600,
            poll_interval_secs: 5,
            lock_timeout_mins: 5,
            idempotency_strategy: IdempotencyStrategy::<SomeDomainEvent>::None,
        };

        let storage_mock = MockOutboxStorage::<SomeDomainEvent>::new();
        let transport_mock = MockTransport::<SomeDomainEvent>::new();

        #[cfg(feature = "dlq")]
        let dlq_heap_mock: MockDlqHeap = MockDlqHeap::new();

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        #[cfg(feature = "dlq")]
        let outbox_manager = OutboxManagerBuilder::new()
            .storage(Arc::new(storage_mock))
            .publisher(Arc::new(transport_mock))
            .config(Arc::new(config))
            .shutdown_rx(shutdown_rx)
            .dlq_heap(Arc::new(dlq_heap_mock))
            .build();

        #[cfg(not(feature = "dlq"))]
        let outbox_manager = OutboxManagerBuilder::new()
            .storage(Arc::new(storage_mock))
            .publisher(Arc::new(transport_mock))
            .config(Arc::new(config))
            .shutdown_rx(shutdown_rx)
            .build();


        assert!(outbox_manager.is_ok());

    }

}