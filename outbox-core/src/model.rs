use crate::object::{EventId, EventType, IdempotencyToken, Payload};
use serde::Serialize;
use std::fmt::Debug;
use time::OffsetDateTime;

#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
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

#[cfg_attr(feature = "sqlx", derive(Debug, sqlx::Type))]
#[cfg_attr(
    feature = "sqlx",
    sqlx(type_name = "status", rename_all = "PascalCase")
)]
#[derive(Clone, PartialEq)]
pub enum EventStatus {
    // Messages that need to be sent (including Pending and Failed)
    Pending,
    Processing,
    Sent,
}
