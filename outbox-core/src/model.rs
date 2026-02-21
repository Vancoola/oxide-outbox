use crate::object::{EventType, Payload, EventId, IdempotencyToken};
use time::OffsetDateTime;

#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct Event {
    pub id: EventId,
    pub idempotency_token: Option<IdempotencyToken>,
    pub event_type: EventType,
    pub payload: Payload,
    pub created_at: OffsetDateTime,
    pub locked_until: OffsetDateTime,
    pub status: EventStatus,
}
impl Event {
    pub fn new(event_type: EventType, payload: Payload, idempotency_token: Option<IdempotencyToken>) -> Self {
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
#[derive(Clone)]
pub enum EventStatus {
    // Messages that need to be sent (including Pending and Failed)
    Pending,
    Processing,
    Sent,
}
