//! Dead-letter-queue bookkeeping (feature-gated behind `dlq`).
//!
//! A [`DlqHeap`] implementation tracks how many times each event has failed to
//! publish. The worker feeds it per-event outcomes on every processing pass;
//! the [`DlqProcessor`](crate::dlq::processor::DlqProcessor) periodically
//! drains entries whose failure count has crossed a configured threshold and
//! hands them to [`OutboxStorage::quarantine_events`](crate::storage::OutboxStorage::quarantine_events)
//! for atomic move into a separate quarantine table.

use crate::dlq::model::DlqEntry;
use crate::error::OutboxError;
use crate::object::EventId;
use async_trait::async_trait;

/// Tracks per-event publish failure counts for the dead-letter queue.
///
/// Used by [`OutboxProcessor`](crate::processor::OutboxProcessor) (which
/// records per-event outcomes) and by
/// [`DlqProcessor`](crate::dlq::processor::DlqProcessor) (which drains
/// over-threshold entries on a timer) while the `dlq` feature is on.
/// Implementations typically back onto the same data store as the outbox
/// itself (a small side table keyed by [`EventId`]) but the contract is
/// intentionally narrow so in-memory implementations are trivial to write
/// for tests.
///
/// All three methods are expected to be concurrency-safe — the worker may be
/// driving multiple batches through the heap at once.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait DlqHeap: Send + Sync {
    /// Increments the failure counter for `id`.
    ///
    /// Called by the worker loop after a publish attempt returns `Err`. The
    /// method is fire-and-forget: implementations are free to emit per-call
    /// metrics or logs internally, but no aggregated state is surfaced to the
    /// caller — quarantine decisions are taken by
    /// [`DlqProcessor`](crate::dlq::processor::DlqProcessor) based on
    /// [`drain_exceeded`](Self::drain_exceeded), not by the worker.
    ///
    /// # Errors
    ///
    /// Returns an [`OutboxError`] if the backing store call fails.
    async fn record_failure(&self, id: EventId) -> Result<(), OutboxError>;

    /// Clears the failure counter for `id` after a successful publish.
    ///
    /// # Errors
    ///
    /// Returns an [`OutboxError`] if the backing store call fails.
    async fn record_success(&self, id: EventId) -> Result<(), OutboxError>;

    /// Removes and returns every [`DlqEntry`] whose failure count has reached
    /// or exceeded `threshold`.
    ///
    /// Intended to be called by [`DlqProcessor`](crate::dlq::processor::DlqProcessor)
    /// on its timer. The implementation is expected to remove the returned
    /// entries from its tracking table atomically so the same id is not
    /// returned twice on overlapping calls.
    ///
    /// # Errors
    ///
    /// Returns an [`OutboxError`] if the backing store call fails.
    async fn drain_exceeded(&self, threshold: u32) -> Result<Vec<DlqEntry>, OutboxError>;
}
