use std::fmt::Debug;
use crate::config::OutboxConfig;
use crate::error::OutboxError;
use crate::model::Event;
use crate::model::EventStatus::Sent;
use crate::object::EventId;
use crate::publisher::Transport;
use crate::storage::OutboxStorage;
use serde::Serialize;
use std::fmt::Debug;
use std::sync::Arc;
use serde::Serialize;
use tracing::error;

pub struct OutboxProcessor<S, T, P>
where
    P: Debug + Clone + Serialize,
{
    storage: Arc<S>,
    publisher: Arc<T>,
    config: Arc<OutboxConfig<P>>,
}

impl<S, T, P> OutboxProcessor<S, T, P>
where
    S: OutboxStorage<P> + 'static,
    T: Transport<P> + 'static,
    P: Debug + Clone + Serialize + Send + Sync,
{
    pub fn new(storage: Arc<S>, publisher: Arc<T>, config: Arc<OutboxConfig<P>>) -> Self {
        Self {
            storage,
            publisher,
            config,
        }
    }

    /// We receive the event batch and send it to Transport
    /// # Errors
    /// We may get a DB error during fetch or UPDATE. publish errors are only logged.
    pub async fn process_pending_events(&self) -> Result<usize, OutboxError> {
        let events: Vec<Event<P>> = self
            .storage
            .fetch_next_to_process(self.config.batch_size)
            .await?;

        if events.is_empty() {
            return Ok(0);
        }
        let count = events.len();
        self.event_publish(events).await?;
        Ok(count)
    }

    async fn event_publish(&self, events: Vec<Event<P>>) -> Result<(), OutboxError> {
        let mut success_ids = Vec::<EventId>::new();
        for event in events {
            let id = event.id;
            match self.publisher.publish(event).await {
                Ok(()) => {
                    success_ids.push(id);
                }
                Err(e) => {
                    error!("Failed to publish event {:?}: {:?}", id, e);
                }
            }
        }
        if !success_ids.is_empty() {
            self.storage.updates_status(&success_ids, Sent).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {}
