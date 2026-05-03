//! Publishing abstraction between the outbox worker and the external broker.
//!
//! [`Transport`] is the narrow boundary an implementation crate (such as
//! `outbox-kafka` or `outbox-rabbit`) has to satisfy for
//! [`OutboxManager`](crate::manager::OutboxManager) to deliver events to a
//! message bus.

use crate::error::OutboxError;
use crate::model::Event;
use std::fmt::Debug;

/// Publishes a single [`Event`] to an external system.
///
/// Implementations are expected to be self-contained — everything the
/// transport needs (connection pool, topic mapping, serializer) should live
/// inside the `impl` so that the worker can keep its loop simple and focused
/// on orchestration.
///
/// Implementations must be `Send + Sync` because the manager holds them
/// behind an `Arc` and may drive them from arbitrary tokio tasks.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait Transport<P>: Send + Sync
where
    P: Debug + Clone + Send + Sync,
{
    /// Sends an event to an external system.
    ///
    /// Called per-event by [`OutboxProcessor`](crate::processor::OutboxProcessor).
    /// A successful return means the broker has accepted responsibility for
    /// the message; any retry semantics beyond that are an implementation
    /// detail of the concrete transport.
    ///
    /// # Errors
    ///
    /// Returns an [`OutboxError`] — usually
    /// [`BrokerError`](OutboxError::BrokerError) — if the broker call fails.
    /// The manager treats this as a per-event failure: the event is left in
    /// the processing state (and will be retried when its lock expires or, if
    /// the `dlq` feature is on, tracked via the DLQ heap) while sibling
    /// events in the same batch are still processed.
    async fn publish(&self, event: Event<P>) -> Result<(), OutboxError>;
}
