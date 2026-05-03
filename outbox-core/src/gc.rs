//! Periodic cleanup of finished outbox rows.
//!
//! [`OutboxManager`](crate::manager::OutboxManager) spawns a background task
//! that wakes up on `config.gc_interval_secs` and asks a [`GarbageCollector`]
//! to run. The collector is a thin proxy over
//! [`OutboxStorage::delete_garbage`] — the actual retention logic lives in
//! the storage implementation because what qualifies as "garbage" (age cut
//! off, status filter, batch size) is backend-specific.

use crate::error::OutboxError;
use crate::storage::OutboxStorage;
use serde::Serialize;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch::Receiver;
use tracing::{info, trace};

/// Proxy that invokes the storage layer's retention cleanup on demand.
///
/// Kept as a named type (rather than a free function) so the manager's
/// background task can own it behind an `Arc` without capturing the full
/// manager state.
pub(crate) struct GarbageCollector<S, P> {
    storage: Arc<S>,
    _marker: std::marker::PhantomData<P>,
}

impl<S, P> GarbageCollector<S, P>
where
    S: OutboxStorage<P> + 'static,
    P: Debug + Clone + Serialize + Send + Sync,
{
    /// Creates a collector that will call into the supplied storage.
    pub fn new(storage: Arc<S>) -> Self {
        Self {
            storage,
            _marker: std::marker::PhantomData,
        }
    }

    /// Runs one cleanup pass by delegating to
    /// [`OutboxStorage::delete_garbage`].
    ///
    /// The collector itself is stateless — each call hits the storage layer,
    /// which decides which rows are expired and removes them.
    ///
    /// # Errors
    ///
    /// Propagates any [`OutboxError`] returned by the storage implementation.
    pub async fn collect_garbage(&self) -> Result<(), OutboxError> {
        self.storage.delete_garbage().await
    }

    /// Runs the garbage-collection loop until shutdown is observed.
    ///
    /// Each iteration races two arms in a `tokio::select!`:
    ///
    /// - the periodic tick of an interval built from `gc_interval_secs`, on
    ///   which [`collect_garbage`](Self::collect_garbage) is invoked. Any
    ///   error returned by the storage layer is intentionally discarded so a
    ///   transient failure does not take the loop down — the next tick will
    ///   simply try again.
    /// - the shutdown receiver `rx_gc`. The loop exits when the watched value
    ///   flips to `true`, and also when the sender side is dropped (treated
    ///   as an implicit shutdown via [`Receiver::has_changed`]).
    ///
    /// This method consumes the collector. It is intended to be spawned on a
    /// dedicated Tokio task by
    /// [`OutboxManager::run`](crate::manager::OutboxManager::run); typical
    /// callers should not invoke it directly.
    ///
    /// # Errors
    ///
    /// Always returns `Ok(())` in the current implementation — every
    /// shutdown path (watched flag set to `true`, sender dropped) is graceful.
    /// The signature is kept fallible so future additions can surface a
    /// terminal failure without breaking the API.
    pub async fn run(
        self,
        gc_interval_secs: Duration,
        rx_gc: &mut Receiver<bool>,
    ) -> Result<(), OutboxError> {
        let mut interval = tokio::time::interval(gc_interval_secs);

        info!("Starting GC service");

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    trace!("GC checking garbage");
                    let _ = self.collect_garbage().await;
                }

                _ = rx_gc.changed() => {
                    if rx_gc.has_changed().is_err() {
                        break;
                    }
                    if *rx_gc.borrow() {
                        break
                    }
                }
            }
        }

        info!("Stopping GC service");

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::storage::MockOutboxStorage;
    use rstest::rstest;
    use serde::{Deserialize, Serialize};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::watch;

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

    // ------------------------------- run loop -----------------------------

    #[rstest]
    #[tokio::test]
    async fn run_exits_when_shutdown_flag_is_set_to_true() {
        let mut storage = MockOutboxStorage::<TestPayload>::new();
        // The first interval tick is immediate, so collect_garbage may be
        // called once before the shutdown wins the select; allow any count.
        storage.expect_delete_garbage().returning(|| Ok(()));

        let gc = GarbageCollector::new(Arc::new(storage));
        let (tx, mut rx) = watch::channel(false);

        let handle = tokio::spawn(async move {
            // A long interval keeps the tick branch out of the way after the
            // initial fire, so the test exits via the shutdown branch.
            gc.run(Duration::from_secs(3600), &mut rx).await
        });

        // Let the worker enter the select before flipping the flag.
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
        let mut storage = MockOutboxStorage::<TestPayload>::new();
        storage.expect_delete_garbage().returning(|| Ok(()));

        let gc = GarbageCollector::new(Arc::new(storage));
        let (tx, mut rx) = watch::channel(false);

        let handle = tokio::spawn(async move { gc.run(Duration::from_secs(3600), &mut rx).await });

        tokio::task::yield_now().await;
        // Dropping the sender is treated as an implicit shutdown — the
        // changed() arm resolves with an error and the loop breaks.
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
        // Sending the same default value (false) must not be confused with a
        // shutdown signal — the loop should keep running.
        let mut storage = MockOutboxStorage::<TestPayload>::new();
        storage.expect_delete_garbage().returning(|| Ok(()));

        let gc = GarbageCollector::new(Arc::new(storage));
        let (tx, mut rx) = watch::channel(false);

        let handle = tokio::spawn(async move { gc.run(Duration::from_secs(3600), &mut rx).await });

        tokio::task::yield_now().await;
        // Flip to a value that is still falsy. The loop must observe the
        // wake-up, see `false` in the watched cell, and keep running.
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
    async fn run_invokes_collect_garbage_on_each_interval_tick() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let mut storage = MockOutboxStorage::<TestPayload>::new();
        storage.expect_delete_garbage().returning(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });

        let gc = GarbageCollector::new(Arc::new(storage));
        let (tx, mut rx) = watch::channel(false);

        let handle = tokio::spawn(async move { gc.run(Duration::from_secs(60), &mut rx).await });

        // Initial immediate tick — let it fire.
        tokio::task::yield_now().await;
        // Two more interval boundaries.
        tokio::time::advance(Duration::from_secs(60)).await;
        tokio::time::advance(Duration::from_secs(60)).await;
        tokio::task::yield_now().await;

        tx.send(true).unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(1), handle).await;

        let n = counter.load(Ordering::SeqCst);
        assert!(n >= 2, "expected at least 2 collect_garbage calls, got {n}");
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn run_swallows_collect_garbage_errors_and_keeps_running() {
        // The loop must not abort on storage errors — transient faults
        // should be tolerated until shutdown.
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let mut storage = MockOutboxStorage::<TestPayload>::new();
        storage.expect_delete_garbage().returning(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
            Err(OutboxError::DatabaseError("transient".into()))
        });

        let gc = GarbageCollector::new(Arc::new(storage));
        let (tx, mut rx) = watch::channel(false);

        let handle = tokio::spawn(async move { gc.run(Duration::from_secs(60), &mut rx).await });

        // Drive at least two ticks so we observe the loop surviving an error
        // and proceeding to the next iteration.
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(60)).await;
        tokio::task::yield_now().await;

        tx.send(true).unwrap();
        let result = tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("run did not stop in time")
            .unwrap();
        assert!(result.is_ok(), "loop must swallow storage errors");
        assert!(
            counter.load(Ordering::SeqCst) >= 2,
            "loop should have ticked at least twice despite errors"
        );
    }
}
