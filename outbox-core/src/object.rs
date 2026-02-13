use uuid::Uuid;

pub struct SlotId(Uuid);
impl Default for SlotId {
    fn default() -> Self {
        Self(Uuid::new_v4())
    }
}

pub struct EventType(String);

pub struct Payload(serde_json::Value);