use crate::object::EventId;

pub struct EventFail {
    pub event_id: EventId,
    pub error_text: String,
}