//! Storage abstractions backing the outbox table.
//!
//! Two traits split the read/write responsibilities:
//!
//! - [`OutboxWriter`] — producer-side insert path, used by
//!   [`OutboxService`](crate::service::OutboxService) to persist new events.
//! - [`OutboxStorage`] — worker-side read and lifecycle path, used by
//!   [`OutboxManager`](crate::manager::OutboxManager) to fetch pending rows,
//!   record status transitions, prune old data, and wait for notifications.
//!
//! Concrete implementations live in sibling crates (`outbox-postgres`,
//! `outbox-redis`). Splitting the traits lets a producer depend on the write
//! side only and keeps the worker's broader surface opt-in.

use crate::error::OutboxError;
use crate::model::{Event, EventStatus};
use crate::object::EventId;
use async_trait::async_trait;
use serde::Serialize;
use std::fmt::Debug;

/// Worker-side storage contract.
///
/// An implementation must provide the read and lifecycle operations the
/// [`OutboxManager`](crate::manager::OutboxManager) drives on every tick:
/// claiming pending rows, recording their outcome, cleaning up finished data,
/// and blocking until an external notification arrives.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait OutboxStorage<P>
where
    P: Debug + Clone + Serialize + Send + Sync,
{
    /// Claims up to `limit` rows that are eligible for processing.
    ///
    /// "Eligible" means rows whose status is
    /// [`EventStatus::Pending`](crate::model::EventStatus::Pending) — including
    /// newly inserted rows and rows whose processing lock has expired.
    /// Implementations are expected to atomically flip the returned rows to
    /// [`EventStatus::Processing`](crate::model::EventStatus::Processing)
    /// with a lock that expires after `lock_timeout_mins`, so concurrent
    /// workers cannot pick up the same row.
    ///
    /// # Errors
    ///
    /// Returns an [`OutboxError`] if the underlying datastore call fails.
    async fn fetch_next_to_process(&self, limit: u32) -> Result<Vec<Event<P>>, OutboxError>;

    /// Transitions the rows identified by `id` to `status`.
    ///
    /// Typically called after a batch publish attempt:
    /// [`EventStatus::Sent`](crate::model::EventStatus::Sent) for successful
    /// publications, or
    /// [`EventStatus::Pending`](crate::model::EventStatus::Pending) to release
    /// a row for retry.
    ///
    /// # Errors
    ///
    /// Returns an [`OutboxError`] if the underlying datastore call fails.
    async fn update_status(&self, id: &[EventId], status: EventStatus) -> Result<(), OutboxError>;

    /// Deletes rows that are past their retention window.
    ///
    /// Invoked on a timer by the [`GarbageCollector`](crate::gc::GarbageCollector)
    /// task. The retention window itself is defined by the storage
    /// implementation (it usually reads `retention_days` from the same
    /// configuration the manager holds).
    ///
    /// # Errors
    ///
    /// Returns an [`OutboxError`] if the underlying datastore call fails.
    async fn delete_garbage(&self) -> Result<(), OutboxError>;

    /// Blocks until a notification arrives on `channel`, or returns
    /// immediately on the next call if the backend does not support async
    /// notifications.
    ///
    /// Used by the manager's wake-up loop in combination with a poll
    /// interval: the backend can deliver a nudge as soon as a new row is
    /// written, while the poll interval guarantees eventual progress if the
    /// notification is missed.
    ///
    /// # Errors
    ///
    /// Returns an [`OutboxError`] if the listen call fails. The manager
    /// recovers by logging and sleeping 5 seconds before retrying.
    async fn wait_for_notification(&self, channel: &str) -> Result<(), OutboxError>;

    /// Atomically moves the given entries out of the active outbox table and
    /// into the dead-letter destination table.
    ///
    /// Called once per tick by
    /// [`DlqProcessor`](crate::dlq::processor::DlqProcessor) after
    /// [`DlqHeap::drain_exceeded`](crate::dlq::storage::DlqHeap::drain_exceeded)
    /// has returned a non-empty batch. The whole `entries` slice is expected
    /// to be moved in a single transaction so there is no observable window
    /// in which a row appears in neither table or in both.
    ///
    /// `entries` carries `failure_count` (and any future per-event metadata)
    /// alongside each [`EventId`]; the implementation persists those values
    /// onto the destination row.
    ///
    /// Ids in `entries` whose source row no longer exists (already deleted
    /// by GC, manual operator action, etc.) are silently dropped — only
    /// matched rows are moved.
    ///
    /// # Default implementation
    ///
    /// The default implementation returns an [`OutboxError::ConfigError`].
    /// Backend crates ship a real implementation behind their own `dlq`
    /// feature; this default is what callers see when the backend was built
    /// without DLQ support but `outbox-core/dlq` happens to be enabled
    /// elsewhere in the workspace (Cargo's feature unification can pull it
    /// in transitively).
    ///
    /// The method is intentionally **not** `#[cfg(feature = "dlq")]`-gated:
    /// gating it on the trait creates a workspace-level mismatch where
    /// `outbox-core` sees the method (because some other crate enabled the
    /// feature) but a backend crate built without its own `dlq` feature
    /// does not provide an implementation.
    ///
    /// # Errors
    ///
    /// Returns an [`OutboxError`] if the underlying datastore call fails.
    /// On error, the caller should assume **none** of the entries were
    /// moved — implementations must not partially commit.
    async fn quarantine_events(
        &self,
        _entries: &[crate::dlq::model::DlqEntry],
    ) -> Result<(), OutboxError> {
        Err(OutboxError::ConfigError(
            "OutboxStorage::quarantine_events: DLQ is not implemented by this backend \
             (rebuild the storage crate with its `dlq` feature enabled)"
                .to_string(),
        ))
    }
}

/// Producer-side storage contract.
///
/// Separated from [`OutboxStorage`] so a service that only writes events can
/// depend on the narrow surface it actually uses.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait OutboxWriter<P>
where
    P: Debug + Clone + Serialize + Send + Sync,
{
    /// Persists a single [`Event`] row in the outbox table.
    ///
    /// Called by [`OutboxService::add_event`](crate::service::OutboxService::add_event)
    /// after any configured idempotency reservation has succeeded.
    ///
    /// # Errors
    ///
    /// Returns an [`OutboxError`] if the insert fails — typically a
    /// [`DatabaseError`](OutboxError::DatabaseError) on a unique-constraint
    /// violation or connection issue.
    async fn insert_event(&self, event: Event<P>) -> Result<(), OutboxError>;
}
