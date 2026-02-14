use crate::config::OutboxConfig;
use crate::error::OutboxError;
use crate::gc::GarbageCollector;
use crate::model::SlotStatus::{Failed, Sent};
use crate::object::SlotId;
use crate::publisher::EventPublisher;
use crate::storage::OutboxStorage;

pub struct OutboxProcessor<S, P>
where
    S: OutboxStorage + Clone + 'static,
    P: EventPublisher + Clone  + 'static,
{
    storage: S,
    publisher: P,
    config: OutboxConfig,
}

impl<S, P> OutboxProcessor<S, P>
where
    S: OutboxStorage + Clone  + 'static,
    P: EventPublisher + Clone  + 'static,
{

    pub async fn process_pending_events(&self) -> Result<(), OutboxError> {
        let events = self.storage.fetch_next_to_process(self.config.batch_size).await?;
        let mut success_ids = Vec::<SlotId>::new();
        let mut failed_ids = Vec::<SlotId>::new();
        for event in events {
            match self.publisher.publish(event.event_type, event.payload).await {
                Ok(_) => {
                    success_ids.push(event.id);
                }
                Err(_) => {
                    failed_ids.push(event.id);
                }
            }
        }
        if !success_ids.is_empty() {
            self.storage.updates_status(&success_ids, Sent).await?;
        }
        if !failed_ids.is_empty() {
            self.storage.updates_status(&failed_ids, Failed).await?;
        }
        Ok(())
    }

}