mod object;
mod storage;

use time::OffsetDateTime;
use crate::object::{EventType, Payload, SlotId};

pub struct OutboxSlot {
    id: SlotId,
    event_type: EventType,
    payload: Payload,
    created_at: OffsetDateTime,
    processed_at: Option<OffsetDateTime>,
    retry_count: usize,
    last_error: Option<String>,
}
impl OutboxSlot {
    pub fn new(event_type: EventType, payload: Payload) -> Self {
        Self {
            id: SlotId::default(),
            event_type,
            payload,
            created_at: OffsetDateTime::now_utc(),
            processed_at: None,
            retry_count: 0,
            last_error: None,
        }
    }
}