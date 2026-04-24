//! Core domain types: the outbox [`Event`] row and its lifecycle
//! [`EventStatus`].
//!
//! These types are what storage adapters read and write, what transports
//! publish, and what the [`IdempotencyStrategy::Custom`](crate::config::IdempotencyStrategy::Custom)
//! function receives. When the `sqlx` feature is enabled, [`Event`] derives
//! `sqlx::FromRow` so it can be decoded directly from a database row.

use crate::object::{EventId, EventType, IdempotencyToken, Payload};
use serde::Serialize;
use std::fmt::Debug;
use time::OffsetDateTime;

/// A single outbox row representing one domain event to be published.
///
/// An event starts out with [`status`](Self::status) set to
/// [`EventStatus::Pending`] and travels through the worker loop:
/// a worker flips the row to [`EventStatus::Processing`] with a lock until
/// [`locked_until`](Self::locked_until), publishes via the transport, and
/// finally marks it [`EventStatus::Sent`]. If a worker crashes, the lock
/// expires and the row becomes eligible again.
///
/// Generic over the user's payload type `PT`; see [`Payload`].
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
#[derive(Debug, Clone)]
pub struct Event<PT> {
    /// Randomly generated primary key (UUID v4). Used to identify the row
    /// across status transitions.
    pub id: EventId,
    /// Deduplication token produced according to the configured
    /// [`IdempotencyStrategy`](crate::config::IdempotencyStrategy). May be
    /// `None` when no token is produced.
    pub idempotency_token: Option<IdempotencyToken>,
    /// Domain-level event name used for routing on the transport side.
    pub event_type: EventType,
    /// The user payload, serialized as JSON when the `sqlx` feature is on.
    #[cfg_attr(feature = "sqlx", sqlx(json))]
    pub payload: Payload<PT>,
    /// Wall-clock time the row was constructed, in UTC.
    pub created_at: OffsetDateTime,
    /// Expiration of the current processing lock. Fresh rows start with
    /// [`OffsetDateTime::UNIX_EPOCH`] (i.e. "not locked"); storage adapters
    /// update this when they claim the row for processing.
    pub locked_until: OffsetDateTime,
    /// Current lifecycle stage. See [`EventStatus`].
    pub status: EventStatus,
}
impl<PT> Event<PT>
where
    PT: Debug + Clone + Serialize,
{
    /// Constructs a new [`Event`] ready to be inserted by the storage layer.
    ///
    /// The caller supplies the domain-level fields (`event_type`, `payload`,
    /// `idempotency_token`); the remaining fields are initialised with sensible
    /// defaults:
    ///
    /// - [`id`](Event::id) — a fresh random [`EventId`]
    /// - [`created_at`](Event::created_at) — `OffsetDateTime::now_utc()`
    /// - [`locked_until`](Event::locked_until) — `OffsetDateTime::UNIX_EPOCH`
    ///   (unlocked)
    /// - [`status`](Event::status) — [`EventStatus::Pending`]
    pub fn new(
        event_type: EventType,
        payload: Payload<PT>,
        idempotency_token: Option<IdempotencyToken>,
    ) -> Self {
        Self {
            id: EventId::default(),
            idempotency_token,
            event_type,
            payload,
            created_at: OffsetDateTime::now_utc(),
            locked_until: OffsetDateTime::UNIX_EPOCH,
            status: EventStatus::Pending,
        }
    }
}

/// Lifecycle stage of an outbox [`Event`].
///
/// A row moves forward through the variants and never steps backwards on a
/// happy path:
///
/// ```text
/// Pending → Processing → Sent
/// ```
///
/// When the `sqlx` feature is enabled, this enum maps to a Postgres type
/// named `status` with `PascalCase` variant names.
#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(
    feature = "sqlx",
    sqlx(type_name = "status", rename_all = "PascalCase")
)]
#[derive(Debug, Clone, PartialEq)]
pub enum EventStatus {
    /// Newly written row awaiting a worker. Includes both freshly inserted
    /// events and rows whose processing lock expired (making them eligible
    /// for retry).
    Pending,
    /// A worker has claimed the row and is currently attempting to publish it.
    /// The lock is held until [`Event::locked_until`].
    Processing,
    /// The event has been successfully published to the transport. Rows in
    /// this state are eventually removed by the garbage collector.
    Sent,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rstest::rstest;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct TestPayload {
        value: String,
    }

    fn payload(v: &str) -> Payload<TestPayload> {
        Payload::new(TestPayload { value: v.into() })
    }

    #[rstest]
    fn event_new_sets_status_to_pending() {
        let e = Event::new(EventType::new("t"), payload("p"), None);
        assert_eq!(e.status, EventStatus::Pending);
    }

    #[rstest]
    fn event_new_sets_locked_until_to_unix_epoch() {
        let e = Event::new(EventType::new("t"), payload("p"), None);
        assert_eq!(e.locked_until, OffsetDateTime::UNIX_EPOCH);
    }

    #[rstest]
    fn event_new_sets_created_at_within_wall_clock_window() {
        let before = OffsetDateTime::now_utc();
        let e = Event::new(EventType::new("t"), payload("p"), None);
        let after = OffsetDateTime::now_utc();
        assert!(
            e.created_at >= before && e.created_at <= after,
            "created_at {} not in [{before}, {after}]",
            e.created_at
        );
    }

    #[rstest]
    fn event_new_assigns_unique_ids_across_calls() {
        let a = Event::new(EventType::new("t"), payload("p"), None);
        let b = Event::new(EventType::new("t"), payload("p"), None);
        assert_ne!(a.id, b.id);
    }

    #[rstest]
    fn event_new_preserves_event_type_and_payload() {
        let e = Event::new(EventType::new("order.created"), payload("hello"), None);
        assert_eq!(e.event_type.as_str(), "order.created");
        assert_eq!(e.payload.as_value().value, "hello");
    }

    #[rstest]
    fn event_new_preserves_idempotency_token_some() {
        let tok = IdempotencyToken::new("abc".into());
        let e = Event::new(EventType::new("t"), payload("p"), Some(tok));
        assert_eq!(
            e.idempotency_token.as_ref().map(|t| t.as_str().to_owned()),
            Some("abc".to_string())
        );
    }

    #[rstest]
    fn event_new_preserves_idempotency_token_none() {
        let e = Event::new(EventType::new("t"), payload("p"), None);
        assert!(e.idempotency_token.is_none());
    }

    #[rstest]
    #[case(EventStatus::Pending, EventStatus::Pending, true)]
    #[case(EventStatus::Processing, EventStatus::Processing, true)]
    #[case(EventStatus::Sent, EventStatus::Sent, true)]
    #[case(EventStatus::Pending, EventStatus::Processing, false)]
    #[case(EventStatus::Processing, EventStatus::Sent, false)]
    #[case(EventStatus::Pending, EventStatus::Sent, false)]
    fn event_status_partial_eq(
        #[case] a: EventStatus,
        #[case] b: EventStatus,
        #[case] expected: bool,
    ) {
        assert_eq!(a == b, expected);
    }
}
