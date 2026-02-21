use crate::config::{OutboxConfig};
use crate::error::OutboxError;
use crate::model::Event;
use crate::model::EventStatus::Sent;
use crate::object::{EventId};
use crate::publisher::Transport;
use crate::storage::OutboxStorage;
use std::sync::Arc;
use tracing::error;

pub struct OutboxProcessor<S, T>
{
    storage: S,
    publisher: T,
    config: Arc<OutboxConfig>,
}

impl<S, T> OutboxProcessor<S, T>
where
    S: OutboxStorage + Clone  + 'static,
    T: Transport + Clone  + 'static,
{

    pub fn new(storage: S, publisher: T, config: Arc<OutboxConfig>) -> Self {
        Self {
            storage,
            publisher,
            config,
        }
    }

    pub async fn process_pending_events(&self) -> Result<usize, OutboxError> {
        let events = self.storage.fetch_next_to_process(self.config.batch_size).await?;

        if events.is_empty() {
            return Ok(0);
        }
        let count = events.len();
        self.event_publish(events).await?;
        Ok(count)
    }

    async fn event_publish(&self, events: Vec<Event>) -> Result<(), OutboxError> {
        let mut success_ids = Vec::<EventId>::new();
        for event in events {
            match self.publisher.publish(event.event_type, event.payload).await {
                Ok(()) => {
                    success_ids.push(event.id);
                }
                Err(e) => {
                    error!("Failed to publish event {:?}: {:?}", event.id, e);
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