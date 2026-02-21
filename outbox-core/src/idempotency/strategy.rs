use crate::config::IdempotencyStrategy;
use crate::model::Event;

impl IdempotencyStrategy {
    pub fn invoke<F>(&self, provided_token: Option<String>, get_event: F) -> Option<String>
    where 
    F: FnOnce() -> Option<Event>{
        match self {
            IdempotencyStrategy::Provided => provided_token,
            IdempotencyStrategy::Custom(f) => {
                let event = get_event().expect("Strategy is Custom, but no Event context provided");
                Some(f(&event))
            }
            IdempotencyStrategy::Uuid => {
                Some(uuid::Uuid::now_v7().to_string())
            }
            // IdempotencyStrategy::HashPayload => {
            //     Some("hash_payload".to_string())
            // }
            IdempotencyStrategy::None => None,
        }
    }
}