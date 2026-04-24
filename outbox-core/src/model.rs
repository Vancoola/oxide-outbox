use crate::object::{EventId, EventType, IdempotencyToken, Payload};
use serde::Serialize;
use std::fmt::Debug;
use time::OffsetDateTime;

#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
#[derive(Debug, Clone)]
pub struct Event<PT> {
    pub id: EventId,
    pub idempotency_token: Option<IdempotencyToken>,
    pub event_type: EventType,
    #[cfg_attr(feature = "sqlx", sqlx(json))]
    pub payload: Payload<PT>,
    pub created_at: OffsetDateTime,
    pub locked_until: OffsetDateTime,
    pub status: EventStatus,
}
impl<PT> Event<PT>
where
    PT: Debug + Clone + Serialize,
{
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

#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(
    feature = "sqlx",
    sqlx(type_name = "status", rename_all = "PascalCase")
)]
#[derive(Debug, Clone, PartialEq)]
pub enum EventStatus {
    Pending,
    Processing,
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
