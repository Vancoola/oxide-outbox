use uuid::Uuid;

#[derive(Debug)]
pub struct SlotId(Uuid);
impl Default for SlotId {
    fn default() -> Self {
        Self(Uuid::new_v4())
    }
}
impl SlotId {
    pub fn load(id: Uuid) -> Self {
        Self(id)
    }
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

#[derive(Debug)]
pub struct EventType(String);
impl EventType {
    pub fn load(value: &String) -> Self {
        Self(value.clone())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug)]
pub struct Payload(serde_json::Value);
impl Payload {
    pub fn load(value: &serde_json::Value) -> Self {
        Self(value.clone())
    }
    pub fn as_json(&self) -> &serde_json::Value {
        &self.0
    }
}
