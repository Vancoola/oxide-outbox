//! Runtime configuration for the outbox crate.
//!
//! [`OutboxConfig`] carries the tunables that both the producer-side
//! [`OutboxService`](crate::service::OutboxService) and the worker-side
//! [`OutboxManager`](crate::manager::OutboxManager) read — batch size, timer
//! intervals, lock timeout, and which [`IdempotencyStrategy`] to apply when
//! new events are written.

use crate::model::Event;
use serde::Serialize;
use std::fmt::Debug;

/// Runtime configuration shared by the producer and worker sides.
///
/// Generic over the user's domain event payload type `P` because the
/// [`Custom`](IdempotencyStrategy::Custom) strategy variant holds a function
/// pointer of type `fn(&Event<P>) -> String`.
///
/// All fields are public so callers can construct the struct with a literal or
/// start from [`default`](Self::default) and override selected fields.
///
/// # Example
///
/// ```
/// use outbox_core::prelude::*;
///
/// # #[derive(Debug, Clone, serde::Serialize)]
/// # struct MyEvent;
/// let cfg: OutboxConfig<MyEvent> = OutboxConfig {
///     batch_size: 200,
///     poll_interval_secs: 2,
///     ..OutboxConfig::default()
/// };
/// assert_eq!(cfg.batch_size, 200);
/// assert_eq!(cfg.retention_days, 7); // inherited from default
/// ```
#[derive(Clone)]
pub struct OutboxConfig<P>
where
    P: Debug + Clone + Serialize,
{
    /// Maximum number of events fetched per processing iteration.
    pub batch_size: u32,
    /// How long sent events are kept before the garbage collector deletes
    /// them, measured in days.
    pub retention_days: i64,
    /// Interval between garbage-collection passes, in seconds.
    pub gc_interval_secs: u64,
    /// Fallback polling interval for the worker loop, in seconds. Used
    /// alongside database `LISTEN`/notify to guarantee progress even when
    /// notifications are missed or unsupported.
    pub poll_interval_secs: u64,
    /// Duration a row stays locked while a worker processes it, in minutes.
    /// Once this timeout elapses, the row becomes eligible to be picked up
    /// again (recovering from a crashed or stuck worker).
    pub lock_timeout_mins: i64,

    /// How idempotency tokens are produced for newly written events. See
    /// [`IdempotencyStrategy`] for the available variants.
    pub idempotency_strategy: IdempotencyStrategy<P>,
    /// Failure count at which an event becomes eligible for quarantine. Events
    /// whose failure counter reaches this value are returned by
    /// [`DlqHeap::drain_exceeded`](crate::dlq::storage::DlqHeap::drain_exceeded)
    /// on the next reaper pass.
    ///
    /// Only consulted when the `dlq` feature is enabled.
    pub dlq_threshold: u32,
    /// Interval between dead-letter reaper passes, in seconds. Each pass drains
    /// events that have crossed [`dlq_threshold`](Self::dlq_threshold) and hands
    /// them off for quarantine.
    ///
    /// Only consulted when the `dlq` feature is enabled.
    pub dlq_interval_secs: u64,
}

impl<P> Default for OutboxConfig<P>
where
    P: Debug + Clone + Serialize,
{
    /// Returns a configuration suitable as a starting point.
    ///
    /// The defaults are:
    ///
    /// | Field | Value |
    /// |---|---|
    /// | `batch_size` | 100 |
    /// | `retention_days` | 7 |
    /// | `gc_interval_secs` | 3600 |
    /// | `poll_interval_secs` | 10 |
    /// | `lock_timeout_mins` | 5 |
    /// | `idempotency_strategy` | [`IdempotencyStrategy::None`] |
    /// | `dlq_threshold` | 10 |
    /// | `dlq_interval_secs` | 300 |
    ///
    /// These values are part of the public contract — tuning them is a
    /// deliberate behaviour change.
    fn default() -> Self {
        Self {
            batch_size: 100,
            retention_days: 7,
            gc_interval_secs: 3600,
            poll_interval_secs: 10,
            lock_timeout_mins: 5,
            idempotency_strategy: IdempotencyStrategy::None,
            dlq_threshold: 10,
            dlq_interval_secs: 300,
        }
    }
}

/// How an idempotency token is produced when a new event is written.
///
/// The variant is evaluated inside
/// [`OutboxService::add_event`](crate::service::OutboxService::add_event)
/// before the event is persisted. When an
/// [`IdempotencyStorageProvider`](crate::idempotency::storage::IdempotencyStorageProvider)
/// is wired, the produced token is also used to reserve uniqueness up front.
#[derive(Clone)]
pub enum IdempotencyStrategy<P>
where
    P: Debug + Clone + Serialize,
{
    /// Uses the caller-supplied token as-is. Passing `None` at call site means
    /// the event is stored without a token and no reservation is attempted.
    Provided,
    /// Derives the token by applying the given function to the event about to
    /// be written. The `add_event` callback `get_event` must return `Some`
    /// for this variant — otherwise the service panics.
    Custom(fn(&Event<P>) -> String),
    /// Generates a fresh UUID v7 token at write time. Any caller-supplied
    /// token is ignored.
    Uuid,
    //TODO:
    //HashPayload, //BLAKE3
    /// Disables idempotency — no token is produced and no reservation is
    /// attempted. This is the default.
    None,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rstest::rstest;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct TestPayload;

    fn default_cfg() -> OutboxConfig<TestPayload> {
        OutboxConfig::default()
    }

    #[rstest]
    fn default_batch_size_is_100() {
        assert_eq!(default_cfg().batch_size, 100);
    }

    #[rstest]
    fn default_retention_days_is_7() {
        assert_eq!(default_cfg().retention_days, 7);
    }

    #[rstest]
    fn default_gc_interval_secs_is_3600() {
        assert_eq!(default_cfg().gc_interval_secs, 3600);
    }

    #[rstest]
    fn default_poll_interval_secs_is_10() {
        assert_eq!(default_cfg().poll_interval_secs, 10);
    }

    #[rstest]
    fn default_lock_timeout_mins_is_5() {
        assert_eq!(default_cfg().lock_timeout_mins, 5);
    }

    #[rstest]
    fn default_idempotency_strategy_is_none() {
        assert!(matches!(
            default_cfg().idempotency_strategy,
            IdempotencyStrategy::None
        ));
    }

    // ------------------------------- Clone -------------------------------

    #[rstest]
    fn clone_preserves_scalar_fields() {
        let cfg = OutboxConfig::<TestPayload> {
            batch_size: 42,
            retention_days: 3,
            gc_interval_secs: 99,
            poll_interval_secs: 1,
            lock_timeout_mins: 2,
            idempotency_strategy: IdempotencyStrategy::Uuid,
            dlq_threshold: 10,
            dlq_interval_secs: 1,
        };
        let cloned = cfg.clone();
        assert_eq!(cloned.batch_size, 42);
        assert_eq!(cloned.retention_days, 3);
        assert_eq!(cloned.gc_interval_secs, 99);
        assert_eq!(cloned.poll_interval_secs, 1);
        assert_eq!(cloned.lock_timeout_mins, 2);
        assert_eq!(cloned.dlq_threshold, 10);
        assert_eq!(cloned.dlq_interval_secs, 1);
        assert!(matches!(
            cloned.idempotency_strategy,
            IdempotencyStrategy::Uuid
        ));
    }

    #[rstest]
    fn clone_preserves_custom_strategy_function_pointer() {
        fn derive(_: &Event<TestPayload>) -> String {
            "fp".into()
        }
        let cfg = OutboxConfig::<TestPayload> {
            batch_size: 1,
            retention_days: 1,
            gc_interval_secs: 1,
            poll_interval_secs: 1,
            lock_timeout_mins: 1,
            idempotency_strategy: IdempotencyStrategy::Custom(derive),
            dlq_threshold: 10,
            dlq_interval_secs: 1,
        };
        let cloned = cfg.clone();
        match cloned.idempotency_strategy {
            IdempotencyStrategy::Custom(f) => {
                let e = Event::new(
                    crate::object::EventType::new("t"),
                    crate::object::Payload::new(TestPayload),
                    None,
                );
                assert_eq!(f(&e), "fp");
            }
            _ => panic!("expected Custom variant after clone"),
        }
    }
}
