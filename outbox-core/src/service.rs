use std::sync::Arc;
use crate::error::OutboxError;
use crate::model::Event;
use crate::object::{EventType, IdempotencyToken, Payload};
use crate::prelude::{IdempotencyStorageProvider, OutboxConfig};
use crate::storage::OutboxWriter;

pub struct OutboxService<W, S> {
    writer: W,
    idempotency_storage: S,
    config: Arc<OutboxConfig>,
}

impl<W, S> OutboxService<W, S>
where
    W: OutboxWriter + Send + Sync + 'static,
    S: IdempotencyStorageProvider + Send + Sync + 'static,
{

    pub fn new(writer: W, idempotency_storage: S, config: Arc<OutboxConfig>) -> Self {
        Self {
            writer,
            idempotency_storage,
            config
        }
    }

    pub async fn add_event<F>(
        &self,
        event_type: &str,
        payload: serde_json::Value,
        provided_token: Option<String>,
        get_event: F,
    ) -> Result<(), OutboxError>
    where
        F: FnOnce() -> Option<Event>,
    {
        let i_token = self
            .config
            .idempotency_strategy
            .invoke(provided_token, get_event)
            .map(IdempotencyToken::new);

        if let Some(ref token) = i_token {
            if !self.idempotency_storage.try_reserve(token).await? {
                return Err(OutboxError::DuplicateEvent);
            }
        }

        let event = Event::new(EventType::new(event_type), Payload::new(payload), i_token);
        self.writer.insert_event(event).await
    }
}


