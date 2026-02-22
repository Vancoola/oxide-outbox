use serde::Serialize;
use uuid::Uuid;

#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(feature = "sqlx", sqlx(transparent))]
#[derive(Debug)]
pub struct EventId(Uuid);
impl Default for EventId {
    fn default() -> Self {
        Self(Uuid::new_v4())
    }
}
impl EventId {
    pub fn load(id: Uuid) -> Self {
        Self(id)
    }
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(feature = "sqlx", sqlx(transparent))]
#[derive(Debug, Clone)]
pub struct IdempotencyToken(pub String);
impl IdempotencyToken {
    pub fn new(token: String) -> Self {
        Self(token)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(feature = "sqlx", sqlx(transparent))]
#[derive(Debug)]
pub struct EventType(String);
impl EventType {
    pub fn new(event_type: &str) -> Self {
        Self(event_type.to_string())
    }
    pub fn load(value: &str) -> Self {
        Self(value.to_owned())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(feature = "sqlx", sqlx(transparent))]
#[derive(Debug, Serialize)]
pub struct Payload(serde_json::Value);
impl Payload {
    pub fn new(payload: serde_json::Value) -> Self {
        Self(payload)
    }
    pub fn load(value: &serde_json::Value) -> Self {
        Self(value.clone())
    }
    pub fn as_json(&self) -> &serde_json::Value {
        &self.0
    }
    /// Creates a `Payload` from a static JSON string.
    ///
    /// # Panics
    ///
    /// Panics if the provided string is not valid JSON.
    pub fn from_static_str(s: &'static str) -> Self {
        let val = serde_json::from_str(s).expect("Invalid JSON in static provider");
        Self(val)
    }
}
