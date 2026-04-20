use crate::object::EventId;

#[derive(Debug)]
pub struct EventFail {
    pub event_id: EventId,
    pub error: String,
}
impl EventFail {
    pub fn new(event_id: EventId, error: String) -> Self {
        Self { event_id, error }
    }
}
