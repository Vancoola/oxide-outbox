//! Resolves an [`IdempotencyStrategy`] into a concrete token at write time.
//!
//! The logic is intentionally kept out of
//! [`OutboxService::add_event`](crate::service::OutboxService::add_event) so
//! the strategy can be exercised directly in tests without spinning up a full
//! service.

use crate::config::IdempotencyStrategy;
use crate::model::Event;

impl<P> IdempotencyStrategy<P> {
    /// Resolves the strategy into a concrete token for the event about to be
    /// written.
    ///
    /// Behaviour per variant:
    ///
    /// - [`Provided`](IdempotencyStrategy::Provided) — returns
    ///   `provided_token` as-is; `None` propagates through and means the
    ///   event will be stored without a token.
    /// - [`Uuid`](IdempotencyStrategy::Uuid) — generates a fresh UUID v7;
    ///   `provided_token` is ignored.
    /// - [`Custom`](IdempotencyStrategy::Custom) — passes `event` to the
    ///   user-supplied callback and wraps the returned `String` in `Some`.
    /// - [`None`](IdempotencyStrategy::None) — returns `None`; neither
    ///   `provided_token` nor `event` is used.
    pub fn invoke(&self, provided_token: Option<String>, event: &Event<P>) -> Option<String> {
        match self {
            IdempotencyStrategy::Provided => provided_token,
            IdempotencyStrategy::Custom(f) => Some(f(event)),
            IdempotencyStrategy::Uuid => Some(uuid::Uuid::now_v7().to_string()),
            // IdempotencyStrategy::HashPayload => {
            //     Some("hash_payload".to_string())
            // }
            IdempotencyStrategy::None => None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::object::{EventType, Payload};
    use rstest::rstest;
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct TestPayload(String);

    fn test_event() -> Event<TestPayload> {
        Event::new(
            EventType::new("t"),
            Payload::new(TestPayload("p".into())),
            None,
        )
    }

    #[rstest]
    fn provided_returns_passed_token() {
        let s = IdempotencyStrategy::<TestPayload>::Provided;
        assert_eq!(
            s.invoke(Some("abc".into()), &test_event()),
            Some("abc".to_string())
        );
    }

    #[rstest]
    fn provided_returns_none_when_no_token_passed() {
        let s = IdempotencyStrategy::<TestPayload>::Provided;
        assert_eq!(s.invoke(None, &test_event()), None);
    }

    #[rstest]
    fn uuid_generates_non_empty_token() {
        let s = IdempotencyStrategy::<TestPayload>::Uuid;
        let token = s.invoke(None, &test_event()).expect("Uuid must yield Some");
        assert!(!token.is_empty());
        // Должен парситься как UUID.
        assert!(
            uuid::Uuid::parse_str(&token).is_ok(),
            "not a valid UUID: {token}"
        );
    }

    #[rstest]
    fn uuid_generates_unique_tokens_across_calls() {
        let s = IdempotencyStrategy::<TestPayload>::Uuid;
        let t1 = s.invoke(None, &test_event()).unwrap();
        let t2 = s.invoke(None, &test_event()).unwrap();
        assert_ne!(t1, t2);
    }

    #[rstest]
    fn uuid_ignores_provided_token() {
        let s = IdempotencyStrategy::<TestPayload>::Uuid;
        let token = s.invoke(Some("user-tok".into()), &test_event()).unwrap();
        assert_ne!(token, "user-tok");
    }

    #[rstest]
    fn custom_invokes_closure_and_derives_token_from_event() {
        fn derive(e: &Event<TestPayload>) -> String {
            format!("d:{}", e.payload.as_value().0)
        }
        let s = IdempotencyStrategy::<TestPayload>::Custom(Arc::new(derive));
        let result = s.invoke(None, &test_event());
        assert_eq!(result, Some("d:p".to_string()));
    }

    #[rstest]
    fn custom_ignores_provided_token() {
        fn derive(_: &Event<TestPayload>) -> String {
            "from-closure".into()
        }
        let s = IdempotencyStrategy::<TestPayload>::Custom(Arc::new(derive));
        let result = s.invoke(Some("user".into()), &test_event());
        assert_eq!(result, Some("from-closure".to_string()));
    }

    #[rstest]
    fn custom_accepts_closure_that_captures_state() {
        let prefix = "tenant-a".to_string();
        let s = IdempotencyStrategy::<TestPayload>::Custom(Arc::new(move |e| {
            format!("{prefix}:{}", e.payload.as_value().0)
        }));
        let result = s.invoke(None, &test_event());
        assert_eq!(result, Some("tenant-a:p".to_string()));
    }

    #[rstest]
    fn none_returns_none_and_ignores_inputs() {
        let s = IdempotencyStrategy::<TestPayload>::None;
        assert_eq!(s.invoke(Some("x".into()), &test_event()), None);
    }
}
