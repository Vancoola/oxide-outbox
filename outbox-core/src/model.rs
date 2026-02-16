use time::OffsetDateTime;
use crate::object::{EventType, Payload, SlotId};

pub struct OutboxSlot {
    pub id: SlotId,
    pub event_type: EventType,
    pub payload: Payload,
    pub created_at: OffsetDateTime,
    pub processed_at: Option<OffsetDateTime>,
    pub status: SlotStatus,
    pub retry_count: usize,
    pub last_error: Option<String>,
}
impl OutboxSlot {
    pub fn new(event_type: EventType, payload: Payload) -> Self {
        Self {
            id: SlotId::default(),
            event_type,
            payload,
            created_at: OffsetDateTime::now_utc(),
            processed_at: None,
            status: SlotStatus::Pending,
            retry_count: 0,
            last_error: None,
        }
    }
}

#[derive(Debug, sqlx::Type)]
#[sqlx(type_name = "status", rename_all = "PascalCase")]
pub enum SlotStatus {
    Pending,
    Processing,
    Sent,
    Failed,
}