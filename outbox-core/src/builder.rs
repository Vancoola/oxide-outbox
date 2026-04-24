//! Fluent builder for [`OutboxManager`].
//!
//! The manager has several collaborators that must all be provided before it
//! can run: a storage backend, a message transport, a configuration, and a
//! shutdown channel — plus a DLQ heap when the `dlq` feature is enabled.
//! [`OutboxManagerBuilder`] assembles these pieces step by step and validates
//! them once at [`build`](OutboxManagerBuilder::build) time, returning a
//! structured [`OutboxError::ConfigError`] if anything is missing rather than
//! panicking.
//!
//! # Example
//!
//! ```ignore
//! use std::sync::Arc;
//! use tokio::sync::watch;
//! use outbox_core::prelude::*;
//!
//! # async fn run(
//! #     storage: Arc<impl OutboxStorage<MyEvent> + Send + Sync + 'static>,
//! #     publisher: Arc<impl Transport<MyEvent> + Send + Sync + 'static>,
//! # ) -> Result<(), OutboxError>
//! # where MyEvent: std::fmt::Debug + Clone + serde::Serialize + Send + Sync + 'static,
//! # {
//! let (_tx, rx) = watch::channel(false);
//! let manager = OutboxManagerBuilder::new()
//!     .storage(storage)
//!     .publisher(publisher)
//!     .config(Arc::new(OutboxConfig::default()))
//!     .shutdown_rx(rx)
//!     .build()?;
//! manager.run().await
//! # }
//! ```

use crate::config::OutboxConfig;
#[cfg(feature = "dlq")]
use crate::dlq::storage::DlqHeap;
use crate::error::OutboxError;
use crate::manager::OutboxManager;
use crate::prelude::{OutboxStorage, Transport};
use serde::Serialize;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::watch::Receiver;

/// Fluent builder that assembles an [`OutboxManager`].
///
/// All fields are optional during construction — validation happens once in
/// [`build`](Self::build). Setter methods consume and return `self`, so a
/// builder can be constructed in a single chained expression. The builder is
/// generic over:
///
/// - `S` — [`OutboxStorage`] implementation (typically a database adapter)
/// - `P` — [`Transport`] implementation (message broker publisher)
/// - `PT` — the user's domain event payload type (`Debug + Clone + Serialize`)
///
/// # Required vs optional
///
/// | Setter | Required | Notes |
/// |---|---|---|
/// | [`storage`](Self::storage) | yes | fails `build()` if missing |
/// | [`publisher`](Self::publisher) | yes | fails `build()` if missing |
/// | [`config`](Self::config) | yes | fails `build()` if missing |
/// | [`shutdown_rx`](Self::shutdown_rx) | yes | fails `build()` if missing |
/// | [`dlq_heap`](Self::dlq_heap) | yes *(feature `dlq` only)* | fails `build()` if missing when feature is on |
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
    /// Creates an empty builder with all fields unset.
    ///
    /// Equivalent to [`Default::default`]; kept as a discoverable entry point
    /// so callers do not need to import the `Default` trait.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the storage backend used for reading pending events, locking them,
    /// persisting status transitions, and deleting expired rows.
    #[must_use]
    pub fn storage(mut self, s: Arc<S>) -> Self {
        self.storage = Some(s);
        self
    }

    /// Sets the transport (broker publisher) that delivers events to the
    /// outside world.
    #[must_use]
    pub fn publisher(mut self, p: Arc<P>) -> Self {
        self.publisher = Some(p);
        self
    }

    /// Sets the runtime configuration (batch size, poll interval, GC cadence,
    /// lock timeout, idempotency strategy).
    #[must_use]
    pub fn config(mut self, config: Arc<OutboxConfig<PT>>) -> Self {
        self.config = Some(config);
        self
    }

    /// Sets the shutdown channel. The manager stops its worker loop as soon as
    /// `true` is observed on this receiver.
    #[must_use]
    pub fn shutdown_rx(mut self, rx: Receiver<bool>) -> Self {
        self.shutdown_rx = Some(rx);
        self
    }

    /// Sets the Dead-Letter-Queue heap that tracks per-event failure counts.
    ///
    /// Required when the crate is built with `--features dlq`.
    #[cfg(feature = "dlq")]
    #[must_use]
    pub fn dlq_heap(mut self, heap: Arc<dyn DlqHeap>) -> Self {
        self.dlq_heap = Some(heap);
        self
    }

    /// Consumes the builder and returns a fully wired [`OutboxManager`].
    ///
    /// # Errors
    ///
    /// Returns [`OutboxError::ConfigError`] with a message identifying the
    /// first missing dependency if any required field has not been set.
    /// The diagnostic mentions one of: `Storage`, `Publisher`, `Config`,
    /// `Shutdown channel`, or — under feature `dlq` — `Dlq heap`.
    pub fn build(self) -> Result<OutboxManager<S, P, PT>, OutboxError> {
        #[cfg(feature = "dlq")]
        return Ok(OutboxManager::new(
            self.storage
                .ok_or_else(|| OutboxError::ConfigError("Storage config is missing".to_string()))?,
            self.publisher.ok_or_else(|| {
                OutboxError::ConfigError("Publisher config is missing".to_string())
            })?,
            self.config
                .ok_or_else(|| OutboxError::ConfigError("Config config is missing".to_string()))?,
            self.dlq_heap.ok_or_else(|| {
                OutboxError::ConfigError("Dlq heap config is missing".to_string())
            })?,
            self.shutdown_rx.ok_or_else(|| {
                OutboxError::ConfigError("Shutdown channel is missing".to_string())
            })?,
        ));
        #[cfg(not(feature = "dlq"))]
        return Ok(OutboxManager::new(
            self.storage
                .ok_or_else(|| OutboxError::ConfigError("Storage config is missing".to_string()))?,
            self.publisher.ok_or_else(|| {
                OutboxError::ConfigError("Publisher config is missing".to_string())
            })?,
            self.config
                .ok_or_else(|| OutboxError::ConfigError("Config config is missing".to_string()))?,
            self.shutdown_rx.ok_or_else(|| {
                OutboxError::ConfigError("Shutdown channel is missing".to_string())
            })?,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::IdempotencyStrategy;
    use crate::dlq::storage::MockDlqHeap;
    use crate::publisher::MockTransport;
    use crate::storage::MockOutboxStorage;
    use rstest::rstest;
    use serde::Deserialize;
    use tokio::sync::watch;

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

        let (_shutdown_tx, shutdown_rx) = watch::channel(false);

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

    type Builder = OutboxManagerBuilder<
        MockOutboxStorage<SomeDomainEvent>,
        MockTransport<SomeDomainEvent>,
        SomeDomainEvent,
    >;

    fn default_config() -> Arc<OutboxConfig<SomeDomainEvent>> {
        Arc::new(OutboxConfig {
            batch_size: 100,
            retention_days: 7,
            gc_interval_secs: 3600,
            poll_interval_secs: 5,
            lock_timeout_mins: 5,
            idempotency_strategy: IdempotencyStrategy::None,
        })
    }

    #[rstest]
    fn default_builder_fails_on_build_with_config_error() {
        let result = Builder::default().build();
        assert!(matches!(result, Err(OutboxError::ConfigError(_))));
    }

    fn assert_config_error_with(
        result: Result<
            OutboxManager<
                MockOutboxStorage<SomeDomainEvent>,
                MockTransport<SomeDomainEvent>,
                SomeDomainEvent,
            >,
            OutboxError,
        >,
        needle: &str,
    ) {
        match result {
            Err(OutboxError::ConfigError(msg)) => {
                assert!(msg.contains(needle), "expected '{needle}' in '{msg}'");
            }
            Err(other) => panic!("expected ConfigError, got {other:?}"),
            Ok(_) => panic!("expected ConfigError, got Ok(..)"),
        }
    }

    #[rstest]
    fn new_matches_default() {
        let r1 = Builder::new().build();
        let r2 = Builder::default().build();
        let m1 = match r1 {
            Err(OutboxError::ConfigError(m)) => m,
            _ => panic!("new().build() should fail with ConfigError"),
        };
        let m2 = match r2 {
            Err(OutboxError::ConfigError(m)) => m,
            _ => panic!("default().build() should fail with ConfigError"),
        };
        assert_eq!(m1, m2);
    }

    #[rstest]
    fn build_fails_without_storage() {
        let (_tx, rx) = watch::channel(false);
        let b = Builder::new()
            .publisher(Arc::new(MockTransport::new()))
            .config(default_config())
            .shutdown_rx(rx);
        #[cfg(feature = "dlq")]
        let b = b.dlq_heap(Arc::new(MockDlqHeap::new()));

        assert_config_error_with(b.build(), "Storage");
    }

    #[rstest]
    fn build_fails_without_publisher() {
        let (_tx, rx) = watch::channel(false);
        let b = Builder::new()
            .storage(Arc::new(MockOutboxStorage::new()))
            .config(default_config())
            .shutdown_rx(rx);
        #[cfg(feature = "dlq")]
        let b = b.dlq_heap(Arc::new(MockDlqHeap::new()));

        assert_config_error_with(b.build(), "Publisher");
    }

    #[rstest]
    fn build_fails_without_config() {
        let (_tx, rx) = watch::channel(false);
        let b = Builder::new()
            .storage(Arc::new(MockOutboxStorage::new()))
            .publisher(Arc::new(MockTransport::new()))
            .shutdown_rx(rx);
        #[cfg(feature = "dlq")]
        let b = b.dlq_heap(Arc::new(MockDlqHeap::new()));

        assert_config_error_with(b.build(), "Config");
    }

    #[rstest]
    fn build_fails_without_shutdown_rx() {
        let b = Builder::new()
            .storage(Arc::new(MockOutboxStorage::new()))
            .publisher(Arc::new(MockTransport::new()))
            .config(default_config());
        #[cfg(feature = "dlq")]
        let b = b.dlq_heap(Arc::new(MockDlqHeap::new()));

        assert_config_error_with(b.build(), "Shutdown");
    }

    #[cfg(feature = "dlq")]
    #[rstest]
    fn build_fails_without_dlq_heap() {
        let (_tx, rx) = watch::channel(false);
        let result = Builder::new()
            .storage(Arc::new(MockOutboxStorage::new()))
            .publisher(Arc::new(MockTransport::new()))
            .config(default_config())
            .shutdown_rx(rx)
            .build();
        assert_config_error_with(result, "Dlq");
    }

    #[rstest]
    fn build_is_insensitive_to_setter_order() {
        let (_tx, rx) = watch::channel(false);
        let b = Builder::new()
            .shutdown_rx(rx)
            .config(default_config())
            .publisher(Arc::new(MockTransport::new()))
            .storage(Arc::new(MockOutboxStorage::new()));
        #[cfg(feature = "dlq")]
        let b = b.dlq_heap(Arc::new(MockDlqHeap::new()));

        assert!(b.build().is_ok());
    }
}
