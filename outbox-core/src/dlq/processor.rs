//! Background reaper that quarantines chronically failing events.
//!
//! [`DlqProcessor`] is the worker side of the dead-letter-queue subsystem.
//! It wakes up on `config.dlq_interval_secs` and is meant to drain events
//! whose failure count has crossed `config.dlq_threshold` from the
//! [`DlqHeap`] backend. The current implementation owns the loop and the
//! shutdown plumbing; the per-tick draining/quarantine logic is being filled
//! out incrementally.
//!
//! The processor is feature-gated behind `dlq` and is only spawned when that
//! feature is enabled — see
//! [`OutboxManager::run`](crate::manager::OutboxManager::run).

use crate::config::OutboxConfig;
use crate::dlq::storage::DlqHeap;
use crate::error::OutboxError;
use crate::prelude::OutboxStorage;
use serde::Serialize;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch::Receiver;
use tracing::{debug, error, info};

/// Long-running task that drains overdue events from a [`DlqHeap`] and
/// hands them to [`OutboxStorage::quarantine_events`] for atomic move into
/// the dead-letter table.
///
/// The processor owns four collaborators:
///
/// - a [`DlqHeap`] that tracks per-event failure counts (drained on each
///   tick to fetch events that have crossed the configured threshold).
/// - an [`OutboxStorage`] used to perform the atomic move from the active
///   table into the quarantine table.
/// - an [`OutboxConfig`] from which the tick interval and threshold are
///   read.
/// - a [`Receiver<bool>`](tokio::sync::watch::Receiver) shutdown channel
///   that lets the loop exit cleanly.
///
/// Construct one with [`new`](Self::new) and drive it with [`run`](Self::run).
pub struct DlqProcessor<S, PT>
where
    PT: Debug + Clone + Serialize + Send + Sync + 'static,
    S: OutboxStorage<PT> + Send + Sync + 'static,
{
    heap: Arc<dyn DlqHeap>,
    storage: Arc<S>,
    config: Arc<OutboxConfig<PT>>,
    shutdown_rx: Receiver<bool>,
}

impl<S, PT> DlqProcessor<S, PT>
where
    PT: Debug + Clone + Serialize + Send + Sync + 'static,
    S: OutboxStorage<PT> + Send + Sync + 'static,
{
    /// Creates a new processor bound to the supplied heap, storage,
    /// configuration and shutdown channel.
    ///
    /// The processor does nothing until [`run`](Self::run) is called.
    pub fn new(
        heap: Arc<dyn DlqHeap>,
        storage: Arc<S>,
        config: Arc<OutboxConfig<PT>>,
        shutdown_rx: Receiver<bool>,
    ) -> Self {
        Self {
            heap,
            storage,
            config,
            shutdown_rx,
        }
    }

    /// Runs the dead-letter reaper loop until shutdown is observed.
    ///
    /// Each iteration races two arms in a `tokio::select!`:
    ///
    /// - the shutdown receiver — the loop exits when the watched value flips
    ///   to `true`, and also when the sender side is dropped (treated as an
    ///   implicit shutdown via [`Receiver::has_changed`]).
    /// - a periodic tick on `config.dlq_interval_secs`. On each tick the
    ///   processor drains entries above
    ///   `config.dlq_threshold` from the [`DlqHeap`] and forwards them to
    ///   [`OutboxStorage::quarantine_events`] for atomic move into the DLQ
    ///   table. Errors from either step are logged and the loop keeps
    ///   running — a transient drain/quarantine failure does not bring the
    ///   reaper down. Drained entries that fail to be quarantined are lost
    ///   from the heap but will re-accumulate on subsequent failures.
    ///
    /// This method consumes the processor and is meant to be spawned on a
    /// dedicated Tokio task. Only compiled with the `dlq` feature.
    ///
    /// # Errors
    ///
    /// Always returns `Ok(())` in the current implementation — every shutdown
    /// path is graceful. The fallible signature is preserved so future
    /// drain/quarantine logic can surface terminal failures without breaking
    /// the API.
    #[cfg(feature = "dlq")]
    pub async fn run(self) -> Result<(), OutboxError> {
        let mut rx_dlq = self.shutdown_rx.clone();
        let mut interval =
            tokio::time::interval(Duration::from_secs(self.config.dlq_interval_secs));

        info!("Starting DLQ processor");

        loop {
            tokio::select! {
                _ = rx_dlq.changed() => {
                    if rx_dlq.has_changed().is_err(){
                            break;
                        }
                        if *rx_dlq.borrow() {
                            break
                        }
                }
                _ = interval.tick() => {
                    match self.heap.drain_exceeded(self.config.dlq_threshold).await {
                        Ok(entries) if entries.is_empty() => {}
                        Ok(entries) => {
                            debug!("DLQ reaper draining {} entries", entries.len());
                            if let Err(e) = self.storage.quarantine_events(&entries).await {
                                error!(
                                    "Failed to quarantine {} events: {}",
                                    entries.len(),
                                    e
                                );
                            }
                        }
                        Err(e) => error!("DLQ drain_exceeded failed: {}", e),
                    }
                }
            }
        }

        info!("Dlq processor stopped");
        Ok(())
    }
}

#[cfg(all(test, feature = "dlq"))]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::config::IdempotencyStrategy;
    use crate::dlq::model::DlqEntry;
    use crate::dlq::storage::MockDlqHeap;
    use crate::object::EventId;
    use crate::storage::MockOutboxStorage;
    use rstest::rstest;
    use serde::{Deserialize, Serialize};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::watch;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct TestPayload;

    fn config() -> Arc<OutboxConfig<TestPayload>> {
        Arc::new(OutboxConfig {
            batch_size: 100,
            retention_days: 7,
            gc_interval_secs: 3600,
            poll_interval_secs: 5,
            lock_timeout_mins: 5,
            idempotency_strategy: IdempotencyStrategy::None,
            dlq_threshold: 10,
            dlq_interval_secs: 3600,
        })
    }

    fn empty_storage() -> Arc<MockOutboxStorage<TestPayload>> {
        Arc::new(MockOutboxStorage::<TestPayload>::new())
    }

    fn quiet_heap() -> Arc<MockDlqHeap> {
        let mut h = MockDlqHeap::new();
        h.expect_drain_exceeded().returning(|_| Ok(vec![]));
        Arc::new(h)
    }

    #[rstest]
    #[tokio::test]
    async fn run_exits_when_shutdown_flag_is_set_to_true() {
        let (tx, rx) = watch::channel(false);

        let processor = DlqProcessor::new(quiet_heap(), empty_storage(), config(), rx);
        let handle = tokio::spawn(async move { processor.run().await });

        tokio::task::yield_now().await;
        tx.send(true).unwrap();

        let result = tokio::time::timeout(Duration::from_millis(500), handle)
            .await
            .expect("run did not stop in time")
            .unwrap();
        assert!(result.is_ok());
    }

    #[rstest]
    #[tokio::test]
    async fn run_exits_when_sender_is_dropped() {
        let (tx, rx) = watch::channel(false);

        let processor = DlqProcessor::new(quiet_heap(), empty_storage(), config(), rx);
        let handle = tokio::spawn(async move { processor.run().await });

        tokio::task::yield_now().await;
        drop(tx);

        let result = tokio::time::timeout(Duration::from_millis(500), handle)
            .await
            .expect("run did not stop in time")
            .unwrap();
        assert!(result.is_ok());
    }

    #[rstest]
    #[tokio::test]
    async fn run_does_not_exit_on_shutdown_value_set_to_false() {
        let (tx, rx) = watch::channel(false);

        let processor = DlqProcessor::new(quiet_heap(), empty_storage(), config(), rx);
        let handle = tokio::spawn(async move { processor.run().await });

        tokio::task::yield_now().await;
        tx.send(false).unwrap();

        let abort = handle.abort_handle();
        let outcome = tokio::time::timeout(Duration::from_millis(100), handle).await;
        assert!(
            outcome.is_err(),
            "loop must keep running when the watched value stays falsy"
        );
        abort.abort();
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn run_drains_heap_and_forwards_to_quarantine_events() {
        let mut heap = MockDlqHeap::new();
        let entry = DlqEntry::new(EventId::load(uuid::Uuid::now_v7()), 12, None);
        let entry_for_heap = entry.clone();
        heap.expect_drain_exceeded()
            .withf(|t| *t == 10)
            .returning(move |_| Ok(vec![entry_for_heap.clone()]));

        let mut storage = MockOutboxStorage::<TestPayload>::new();
        let entry_for_storage = entry.clone();
        storage
            .expect_quarantine_events()
            .withf(move |entries| entries.len() == 1 && entries[0].id == entry_for_storage.id)
            .returning(|_| Ok(()));

        let (tx, rx) = watch::channel(false);
        let cfg = Arc::new(OutboxConfig::<TestPayload> {
            batch_size: 100,
            retention_days: 7,
            gc_interval_secs: 3600,
            poll_interval_secs: 5,
            lock_timeout_mins: 5,
            idempotency_strategy: IdempotencyStrategy::None,
            dlq_threshold: 10,
            dlq_interval_secs: 60,
        });

        let processor = DlqProcessor::new(Arc::new(heap), Arc::new(storage), cfg, rx);
        let handle = tokio::spawn(async move { processor.run().await });

        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(1)).await;
        tokio::task::yield_now().await;

        tx.send(true).unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(1), handle).await;
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn run_skips_quarantine_when_drain_returns_empty_batch() {
        let mut heap = MockDlqHeap::new();
        heap.expect_drain_exceeded().returning(|_| Ok(vec![]));

        let mut storage = MockOutboxStorage::<TestPayload>::new();
        storage.expect_quarantine_events().times(0);

        let (tx, rx) = watch::channel(false);
        let cfg = Arc::new(OutboxConfig::<TestPayload> {
            dlq_interval_secs: 60,
            ..(*config()).clone()
        });

        let processor = DlqProcessor::new(Arc::new(heap), Arc::new(storage), cfg, rx);
        let handle = tokio::spawn(async move { processor.run().await });

        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(1)).await;
        tokio::task::yield_now().await;

        tx.send(true).unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(1), handle).await;
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn run_swallows_quarantine_errors_and_keeps_running() {
        let drain_calls = Arc::new(AtomicUsize::new(0));
        let drain_calls_clone = drain_calls.clone();

        let mut heap = MockDlqHeap::new();
        heap.expect_drain_exceeded().returning(move |_| {
            drain_calls_clone.fetch_add(1, Ordering::SeqCst);
            Ok(vec![DlqEntry::new(
                EventId::load(uuid::Uuid::now_v7()),
                15,
                None,
            )])
        });

        let mut storage = MockOutboxStorage::<TestPayload>::new();
        storage
            .expect_quarantine_events()
            .returning(|_| Err(OutboxError::DatabaseError("boom".into())));

        let (tx, rx) = watch::channel(false);
        let cfg = Arc::new(OutboxConfig::<TestPayload> {
            dlq_interval_secs: 60,
            ..(*config()).clone()
        });

        let processor = DlqProcessor::new(Arc::new(heap), Arc::new(storage), cfg, rx);
        let handle = tokio::spawn(async move { processor.run().await });

        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(60)).await;
        tokio::task::yield_now().await;

        tx.send(true).unwrap();
        let result = tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("run did not stop in time")
            .unwrap();
        assert!(result.is_ok(), "loop must swallow quarantine errors");
        assert!(
            drain_calls.load(Ordering::SeqCst) >= 2,
            "expected at least 2 drain attempts despite quarantine errors"
        );
    }
}
