use uuid::Uuid;

pub struct SlotId(Uuid);
impl Default for SlotId {
    fn default() -> Self {
        Self(Uuid::new_v4())
    }
}
impl SlotId {
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

pub struct EventType(String);
impl EventType {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub struct Payload(serde_json::Value);
impl Payload {
    pub fn as_json(&self) -> &serde_json::Value {
        &self.0
    }
}
