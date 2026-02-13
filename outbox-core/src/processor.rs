use crate::config::OutboxConfig;
use crate::error::OutboxError;
use crate::publisher::EventPublisher;
use crate::storage::OutboxStorage;

pub struct OutboxProcessor<S, P>
where
    S: OutboxStorage,
    P: EventPublisher,
{
    storage: S,
    publisher: P,
    config: OutboxConfig,
}

impl<S, P> OutboxProcessor<S, P>
where
    S: OutboxStorage,
    P: EventPublisher,
{

    pub async fn process_pending_events(&self) -> Result<(), OutboxError> {
        let events = self.storage.fetch_next_to_process(self.config.batch_size).await?;
        for mut event in events {
            match self.publisher.publish(event.event_type.as_str(), event.payload.as_json()).await {
                Ok(_) => {
                    //TODO: GC logic
                }
                Err(_) => {
                    //TODO: replay logic
                }
            }
        }
        Ok(())
    }

}