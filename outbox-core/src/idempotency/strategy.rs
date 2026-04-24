//! Resolves an [`IdempotencyStrategy`] into a concrete token at write time.
//!
//! The logic is intentionally kept out of
//! [`OutboxService::add_event`](crate::service::OutboxService::add_event) so
//! the strategy can be exercised directly in tests without spinning up a full
//! service.

use crate::config::IdempotencyStrategy;
use crate::model::Event;
use serde::Serialize;
use std::fmt::Debug;

impl<P> IdempotencyStrategy<P>
where
    P: Debug + Clone + Serialize,
{
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
    /// - [`Custom`](IdempotencyStrategy::Custom) — invokes `get_event`,
    ///   passes the resulting [`Event`] to the user-supplied function, and
    ///   wraps the returned `String` in `Some`.
    /// - [`None`](IdempotencyStrategy::None) — returns `None`; neither
    ///   `provided_token` nor `get_event` is used.
    ///
    /// `get_event` is only evaluated for the `Custom` branch, so callers can
    /// pass `|| None` for every other strategy.
    ///
    /// # Panics
    ///
    /// Panics if the strategy is set to `Custom`, but the provided `get_event`
    /// closure returns `None`. The panic message is
    /// `"Strategy is Custom, but no Event context provided"`.
    pub fn invoke<F>(&self, provided_token: Option<String>, get_event: F) -> Option<String>
    where
        F: FnOnce() -> Option<Event<P>>,
    {
        match self {
            IdempotencyStrategy::Provided => provided_token,
            IdempotencyStrategy::Custom(f) => {
                let event = get_event().expect("Strategy is Custom, but no Event context provided");
                Some(f(&event))
            }
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
    use std::cell::Cell;

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
            s.invoke(Some("abc".into()), || None),
            Some("abc".to_string())
        );
    }

    #[rstest]
    fn provided_returns_none_when_no_token_passed() {
        let s = IdempotencyStrategy::<TestPayload>::Provided;
        assert_eq!(s.invoke(None, || None), None);
    }

    #[rstest]
    fn provided_does_not_invoke_get_event() {
        let s = IdempotencyStrategy::<TestPayload>::Provided;
        let _ = s.invoke(Some("x".into()), || panic!("get_event must not be called"));
    }

    #[rstest]
    fn uuid_generates_non_empty_token() {
        let s = IdempotencyStrategy::<TestPayload>::Uuid;
        let token = s.invoke(None, || None).expect("Uuid must yield Some");
        assert!(!token.is_empty());
        // Должен парситься как UUID.
        assert!(uuid::Uuid::parse_str(&token).is_ok(), "not a valid UUID: {token}");
    }

    #[rstest]
    fn uuid_generates_unique_tokens_across_calls() {
        let s = IdempotencyStrategy::<TestPayload>::Uuid;
        let t1 = s.invoke(None, || None).unwrap();
        let t2 = s.invoke(None, || None).unwrap();
        assert_ne!(t1, t2);
    }

    #[rstest]
    fn uuid_ignores_provided_token_and_does_not_invoke_get_event() {
        let s = IdempotencyStrategy::<TestPayload>::Uuid;
        let token = s
            .invoke(Some("user-tok".into()), || {
                panic!("get_event must not be called")
            })
            .unwrap();
        assert_ne!(token, "user-tok");
    }

    #[rstest]
    fn custom_invokes_closure_and_derives_token_from_event() {
        fn derive(e: &Event<TestPayload>) -> String {
            format!("d:{}", e.payload.as_value().0)
        }
        let s = IdempotencyStrategy::<TestPayload>::Custom(derive);
        let called = Cell::new(false);
        let result = s.invoke(None, || {
            called.set(true);
            Some(test_event())
        });
        assert!(called.get());
        assert_eq!(result, Some("d:p".to_string()));
    }

    #[rstest]
    fn custom_ignores_provided_token() {
        fn derive(_: &Event<TestPayload>) -> String {
            "from-closure".into()
        }
        let s = IdempotencyStrategy::<TestPayload>::Custom(derive);
        let result = s.invoke(Some("user".into()), || Some(test_event()));
        assert_eq!(result, Some("from-closure".to_string()));
    }

    #[rstest]
    #[should_panic(expected = "Strategy is Custom, but no Event context provided")]
    fn custom_panics_when_get_event_returns_none() {
        fn derive(_: &Event<TestPayload>) -> String {
            "x".into()
        }
        let s = IdempotencyStrategy::<TestPayload>::Custom(derive);
        let _ = s.invoke(None, || None);
    }

    #[rstest]
    fn none_returns_none_and_ignores_inputs() {
        let s = IdempotencyStrategy::<TestPayload>::None;
        assert_eq!(s.invoke(Some("x".into()), || None), None);
    }

    #[rstest]
    fn none_does_not_invoke_get_event() {
        let s = IdempotencyStrategy::<TestPayload>::None;
        let _ = s.invoke(None, || panic!("get_event must not be called"));
    }
}
