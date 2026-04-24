//! Newtype wrappers for the fields stored on an [`Event`](crate::model::Event).
//!
//! Each type hides its underlying representation and exposes a narrow,
//! intention-revealing API. Under the `sqlx` feature every newtype derives a
//! transparent `sqlx::Type`, so they round-trip through database columns
//! without additional conversion code.

use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display, Formatter};
use uuid::Uuid;

/// Primary key of an outbox row.
///
/// Wraps a [`Uuid`] so that event identifiers are not confused with other
/// UUID-valued columns. [`Default`] produces a fresh random v4 identifier.
#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(feature = "sqlx", sqlx(transparent))]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct EventId(Uuid);
impl Default for EventId {
    /// Generates a fresh random identifier (UUID v4).
    fn default() -> Self {
        Self(Uuid::new_v4())
    }
}
impl EventId {
    /// Wraps an existing [`Uuid`] — typically used by storage adapters when
    /// hydrating an [`Event`](crate::model::Event) from a database row.
    #[must_use]
    pub fn load(id: Uuid) -> Self {
        Self(id)
    }
    /// Returns the underlying [`Uuid`] for use with APIs that need one
    /// (e.g. logging or foreign-key references).
    #[must_use]
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

/// Deduplication token attached to an [`Event`](crate::model::Event).
///
/// Produced by the configured
/// [`IdempotencyStrategy`](crate::config::IdempotencyStrategy) and, when an
/// [`IdempotencyStorageProvider`](crate::idempotency::storage::IdempotencyStorageProvider)
/// is wired, used to reserve uniqueness before the event is written.
/// Accepts arbitrary strings — interpretation is left entirely to the caller.
#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(feature = "sqlx", sqlx(transparent))]
#[derive(Debug, Clone)]
pub struct IdempotencyToken(pub String);
impl IdempotencyToken {
    /// Wraps a string as a token.
    #[must_use]
    pub fn new(token: String) -> Self {
        Self(token)
    }
    /// Returns the token as a `&str` for comparison or logging.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Returns the raw bytes of the token — convenient for hashing backends
    /// such as Redis keys or BLAKE3.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

/// Domain-level event name used by transports for routing (Kafka topic
/// suffix, Redis stream key, etc).
#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(feature = "sqlx", sqlx(transparent))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventType(String);
impl EventType {
    /// Creates an [`EventType`] from a borrowed string slice.
    ///
    /// Use this at call sites that produce an event — e.g. `"order.created"`.
    #[must_use]
    pub fn new(event_type: &str) -> Self {
        Self(event_type.to_string())
    }
    /// Alternate constructor used by storage adapters when hydrating an
    /// [`Event`](crate::model::Event) from a row. Semantically identical to
    /// [`new`](Self::new); the name signals intent on the read path.
    #[must_use]
    pub fn load(value: &str) -> Self {
        Self(value.to_owned())
    }
    /// Returns the event type as a `&str`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl Display for EventType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Typed wrapper around the user's domain event value.
///
/// Marked with `#[serde(transparent)]`, so serialization produces exactly the
/// same JSON as the inner `T` — wrapping an existing type in [`Payload`] does
/// not change its on-the-wire representation.
#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(feature = "sqlx", sqlx(transparent))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Payload<T>(T);
impl<T> Payload<T>
where
    T: Debug + Clone + Serialize + Send + Sync,
{
    /// Wraps an owned payload value.
    #[must_use]
    pub fn new(payload: T) -> Self {
        Self(payload)
    }
    /// Wraps a payload by cloning from a borrowed reference.
    ///
    /// Convenient when the caller needs to keep ownership of the original
    /// value (for logging, further processing, etc.).
    #[must_use]
    pub fn from_ref(value: &T) -> Self {
        Self(value.clone())
    }
    /// Returns a borrowed reference to the inner payload.
    #[must_use]
    pub fn as_value(&self) -> &T {
        &self.0
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rstest::rstest;

    // ---------------- EventId ----------------

    #[rstest]
    fn event_id_default_generates_unique_uuids_across_calls() {
        let a = EventId::default();
        let b = EventId::default();
        assert_ne!(a, b);
        assert_ne!(a.as_uuid(), b.as_uuid());
    }

    #[rstest]
    fn event_id_load_preserves_inner_uuid() {
        let uuid = Uuid::new_v4();
        let id = EventId::load(uuid);
        assert_eq!(id.as_uuid(), uuid);
    }

    #[rstest]
    fn event_id_equality_reflects_inner_uuid() {
        let uuid = Uuid::new_v4();
        let a = EventId::load(uuid);
        let b = EventId::load(uuid);
        assert_eq!(a, b);
        // Copy: using a after copy must not move it.
        let copied = a;
        assert_eq!(copied, a);
    }

    #[rstest]
    fn event_id_default_is_v4() {
        let id = EventId::default();
        assert_eq!(id.as_uuid().get_version_num(), 4);
    }

    // ------------- IdempotencyToken -------------

    #[rstest]
    #[case("abc")]
    #[case("")]
    #[case("with spaces and 🦀")]
    fn idempotency_token_new_preserves_string(#[case] raw: &str) {
        let tok = IdempotencyToken::new(raw.to_string());
        assert_eq!(tok.as_str(), raw);
    }

    #[rstest]
    fn idempotency_token_as_bytes_matches_as_str_bytes() {
        let tok = IdempotencyToken::new("hello".into());
        assert_eq!(tok.as_bytes(), "hello".as_bytes());
        assert_eq!(tok.as_bytes(), tok.as_str().as_bytes());
    }

    // ---------------- EventType ----------------

    #[rstest]
    fn event_type_new_preserves_str() {
        let et = EventType::new("order.created");
        assert_eq!(et.as_str(), "order.created");
    }

    #[rstest]
    fn event_type_load_preserves_str() {
        let et = EventType::load("order.created");
        assert_eq!(et.as_str(), "order.created");
    }

    #[rstest]
    fn event_type_new_and_load_produce_equal_string_views() {
        let a = EventType::new("x");
        let b = EventType::load("x");
        assert_eq!(a.as_str(), b.as_str());
    }

    #[rstest]
    fn event_type_display_matches_as_str() {
        let et = EventType::new("payment.settled");
        assert_eq!(format!("{et}"), et.as_str());
    }

    // ---------------- Payload ----------------

    #[rstest]
    fn payload_new_preserves_value() {
        let p = Payload::new(42i32);
        assert_eq!(*p.as_value(), 42);
    }

    #[rstest]
    fn payload_from_ref_clones_without_consuming_source() {
        let source = String::from("keep-me");
        let p = Payload::from_ref(&source);
        assert_eq!(p.as_value(), &source);
        // Source is still usable — from_ref must clone, not move.
        assert_eq!(source, "keep-me");
    }

    #[rstest]
    fn payload_serde_is_transparent_over_inner() {
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        struct Inner {
            a: u32,
            b: String,
        }

        let inner = Inner {
            a: 7,
            b: "x".into(),
        };
        let wrapped = Payload::new(inner.clone());

        let inner_json = serde_json::to_string(&inner).unwrap();
        let wrapped_json = serde_json::to_string(&wrapped).unwrap();
        assert_eq!(inner_json, wrapped_json);
    }

    #[rstest]
    fn payload_deserialize_is_transparent_over_inner() {
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        struct Inner {
            a: u32,
        }

        let json = r#"{"a":9}"#;
        let p: Payload<Inner> = serde_json::from_str(json).unwrap();
        assert_eq!(*p.as_value(), Inner { a: 9 });
    }
}
