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
use tracing::error;
use crate::dlq::storage::DlqHeap;

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
    pub async fn process_pending_events(
        &self,
        #[cfg(feature = "dlq")]
        dlq_heap: Arc<dyn DlqHeap>
    ) -> Result<usize, OutboxError> {
        let events: Vec<Event<P>> = self
            .storage
            .fetch_next_to_process(self.config.batch_size)
            .await?;

        if events.is_empty() {
            return Ok(0);
        }
        let count = events.len();
        #[cfg(feature = "dlq")]
        self.event_publish(events, dlq_heap).await?;
        #[cfg(not(feature = "dlq"))]
        self.event_publish(events).await?;
        Ok(count)
    }
    
    async fn event_publish(
        &self,
        events: Vec<Event<P>>,
        #[cfg(feature = "dlq")]
        dlq_heap: Arc<dyn DlqHeap>
    ) -> Result<(), OutboxError> {
        let mut success_ids = Vec::<EventId>::new();
        for event in events {
            let id = event.id;

            #[cfg(feature = "metrics")]
            let start = std::time::Instant::now();

            let event_type = event.event_type.to_string();

            match self.publisher.publish(event).await {
                Ok(()) => {
                    success_ids.push(id);
                    #[cfg(feature = "dlq")]
                    dlq_heap.record_success(id).await?;
                    #[cfg(feature = "metrics")]
                    {
                        let delta = start.elapsed().as_secs_f64();

                        metrics::counter!("outbox.events_total",
                            "status" => "success",
                            "event_type" => event_type.clone()
                        )
                        .increment(1);

                        metrics::histogram!(
                            "outbox.publish_duration_seconds",
                            "event_type" => event_type.clone()
                        )
                        .record(delta);
                    }
                }
                Err(e) => {
                    error!("Failed to publish event {:?}: {:?}", id, e);
                    #[cfg(feature = "dlq")]
                    dlq_heap.record_success(id).await?;
                    
                    #[cfg(feature = "metrics")]
                    {
                        let delta = start.elapsed().as_secs_f64();

                        metrics::counter!("outbox.events_total",
                            "status" => "error",
                            "event_type" => event_type.clone()
                        )
                        .increment(1);

                        metrics::histogram!(
                            "outbox.publish_duration_seconds",
                            "status" => "error",
                            "event_type" => event_type
                        )
                        .record(delta);
                    }
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
