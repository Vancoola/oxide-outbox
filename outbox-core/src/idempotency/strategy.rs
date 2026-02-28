use std::fmt::Debug;
use serde::Serialize;
use crate::config::IdempotencyStrategy;
use crate::model::Event;

impl<P> IdempotencyStrategy<P>
where
    P: Debug + Clone + Serialize,
{
    /// Invokes the idempotency strategy to generate or retrieve a token.
    ///
    /// # Panics
    ///
    /// Panics if the strategy is set to `Custom`, but the provided `get_event`
    /// closure returns `None`.
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
