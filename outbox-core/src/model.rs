use crate::object::{EventType, Payload, SlotId};
use time::OffsetDateTime;
use uuid::Uuid;

#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct OutboxSlot {
    pub id: SlotId,
    pub event_type: EventType,
    pub payload: Payload,
    pub created_at: OffsetDateTime,
    pub locked_until: OffsetDateTime,
    pub status: SlotStatus,
}
impl OutboxSlot {
    pub fn new(event_type: EventType, payload: Payload) -> Self {
        Self {
            id: SlotId::default(),
            event_type,
            payload,
            created_at: OffsetDateTime::now_utc(),
            locked_until: OffsetDateTime::UNIX_EPOCH,
            status: SlotStatus::Pending,
        }
    }
    pub fn load(
        id: Uuid,
        event_type: &String,
        payload: &serde_json::Value,
        created_at: OffsetDateTime,
        locked_until: OffsetDateTime,
        status: &SlotStatus
    ) -> Self {
        Self {
            id: SlotId::load(id),
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
pub enum SlotStatus {
    // Messages that need to be sent (including Pending and Failed)
    Pending,
    Processing,
    Sent,
}
