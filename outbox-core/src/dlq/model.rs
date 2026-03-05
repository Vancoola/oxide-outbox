use crate::object::EventId;

#[derive(Debug)]
pub struct EventFail {
    pub event_id: EventId,
    pub error: String,
}