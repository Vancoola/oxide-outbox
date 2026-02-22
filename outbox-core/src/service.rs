use std::sync::Arc;
use crate::error::OutboxError;
use crate::idempotency::storage::NoIdempotency;
use crate::model::Event;
use crate::object::{EventType, IdempotencyToken, Payload};
use crate::prelude::{IdempotencyStorageProvider, OutboxConfig};
use crate::storage::OutboxWriter;

pub struct OutboxService<W, S> {
    writer: Arc<W>,
    config: Arc<OutboxConfig>,
    idempotency_storage: Option<Arc<S>>,
}


impl<W> OutboxService<W, NoIdempotency>
where W: OutboxWriter + Send + Sync + 'static
{
    pub fn new(writer: Arc<W>, config: Arc<OutboxConfig>) -> Self {
        Self {
            writer,
            config,
            idempotency_storage: None, 
        }
    }
}


impl<W, S> OutboxService<W, S>
where
    W: OutboxWriter + Send + Sync + 'static,
    S: IdempotencyStorageProvider + Send + Sync + 'static,
{

    pub fn with_idempotency(writer: Arc<W>, config: Arc<OutboxConfig>, idempotency_storage: Arc<S>) -> Self {
        Self {
            writer,
            idempotency_storage: Some(idempotency_storage),
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

        if let Some(i_provider) = &self.idempotency_storage
            && let Some(ref token) = i_token
                && !i_provider.try_reserve(token).await? {
                return Err(OutboxError::DuplicateEvent);
            }

        let event = Event::new(EventType::new(event_type), Payload::new(payload), i_token);
        self.writer.insert_event(event).await
    }
}


