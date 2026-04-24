//! Dead-letter-queue bookkeeping (feature-gated behind `dlq`).
//!
//! A [`DlqHeap`] implementation tracks how many times each event has failed to
//! publish. The worker feeds it per-event outcomes on every processing pass;
//! operators then periodically drain rows whose failure count has crossed a
//! configured threshold and move them to a separate quarantine table.

use crate::error::OutboxError;
use crate::object::EventId;
use async_trait::async_trait;

/// Tracks per-event publish failure counts for the dead-letter queue.
///
/// Used by [`OutboxProcessor`](crate::processor::OutboxProcessor) while the
/// `dlq` feature is on. Implementations typically back onto the same data
/// store as the outbox itself (a small side table keyed by [`EventId`]) but
/// the contract is intentionally narrow so in-memory implementations are
/// trivial to write for tests.
///
/// All three methods are expected to be concurrency-safe — the worker may be
/// driving multiple batches through the heap at once.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait DlqHeap: Send + Sync {
    /// Increments the failure counter for `id` and returns the new value.
    ///
    /// Called after a publish attempt returns `Err`. The returned count lets
    /// the caller decide whether this particular event has just crossed the
    /// threshold — useful for eager quarantine or alerting.
    ///
    /// # Errors
    ///
    /// Returns an [`OutboxError`] if the backing store call fails.
    async fn record_failure(&self, id: EventId) -> Result<u32, OutboxError>;

    /// Clears the failure counter for `id` after a successful publish.
    ///
    /// # Errors
    ///
    /// Returns an [`OutboxError`] if the backing store call fails.
    async fn record_success(&self, id: EventId) -> Result<(), OutboxError>;

    /// Removes and returns every event id whose failure count has reached or
    /// exceeded `threshold`.
    ///
    /// Intended to be called by an operator task that wants to quarantine
    /// chronically failing events. The implementation is expected to remove
    /// the returned ids from its tracking table atomically so the same id is
    /// not returned twice.
    ///
    /// # Errors
    ///
    /// Returns an [`OutboxError`] if the backing store call fails.
    async fn drain_exceeded(&self, threshold: u32) -> Result<Vec<EventId>, OutboxError>;
}
