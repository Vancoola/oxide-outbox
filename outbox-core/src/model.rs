use crate::object::{EventType, Payload, EventId};
use time::OffsetDateTime;
use uuid::Uuid;

#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct Event {
    pub id: EventId,
    pub event_type: EventType,
    pub payload: Payload,
    pub created_at: OffsetDateTime,
    pub locked_until: OffsetDateTime,
    pub status: EventStatus,
}
impl Event {
    pub fn new(event_type: EventType, payload: Payload) -> Self {
        Self {
            id: EventId::default(),
            event_type,
            payload,
            created_at: OffsetDateTime::now_utc(),
            locked_until: OffsetDateTime::UNIX_EPOCH,
            status: EventStatus::Pending,
        }
    }
    pub fn load(
        id: Uuid,
        event_type: &String,
        payload: &serde_json::Value,
        created_at: OffsetDateTime,
        locked_until: OffsetDateTime,
        status: &EventStatus
    ) -> Self {
        Self {
            id: EventId::load(id),
            event_type: EventType::load(event_type),
            payload: Payload::load(payload),
            created_at,
            locked_until,
            status: status.clone(),
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
