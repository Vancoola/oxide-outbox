use crate::config::OutboxConfig;
use crate::error::OutboxError;
use crate::model::SlotStatus::Sent;
use crate::object::SlotId;
use crate::publisher::EventPublisher;
use crate::storage::OutboxStorage;

pub(crate) struct ReplyService<S, P>
where
    S: OutboxStorage + Clone + 'static,
    P: EventPublisher + Clone + 'static,
{
    storage: S,
    publisher: P,
    config: OutboxConfig,
}

impl<S, P> ReplyService<S, P>
where
S: OutboxStorage + Clone + 'static,
P: EventPublisher + Clone + 'static,
{

    pub fn new(storage: S, publisher: P, config: OutboxConfig) -> Self {
        Self {
            storage,
            publisher,
            config
        }
    }
    
    pub(crate) async fn try_replay(&self) -> Result<(), OutboxError> {
        let events = self.storage.fetch_unprocessed(self.config.batch_size).await?;
        let mut success_ids = Vec::<SlotId>::new();
        let mut failed_ids = Vec::<SlotId>::new();
        for event in events {
            match self.publisher.publish(event.event_type, event.payload).await {
                Ok(_) => {
                    success_ids.push(event.id); 
                }
                Err(e) => {
                    failed_ids.push(event.id);
                },
            }
        }
        if !success_ids.is_empty() {
            self.storage.updates_status(&success_ids, Sent).await?;
        }
        if !failed_ids.is_empty() {
            self.storage.add_fail_counts(&failed_ids).await?;
        }
        Ok(())
    }
}