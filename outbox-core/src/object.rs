use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use uuid::Uuid;

#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(feature = "sqlx", sqlx(transparent))]
#[derive(Debug, Copy, Clone)]
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
#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Payload<T>(T);
impl<T> Payload<T>
where
    T: Debug + Clone + Serialize + Send + Sync,
{
    pub fn new(payload: T) -> Self {
        Self(payload)
    }
    pub fn load(value: &T) -> Self {
        Self(value.clone())
    }
    pub fn as_value(&self) -> &T {
        &self.0
    }
}
