//! One iteration of the worker loop: fetch a batch of pending events,
//! publish each of them, and record the outcome.
//!
//! [`OutboxProcessor`] is the unit [`OutboxManager`](crate::manager::OutboxManager)
//! drives on every wake-up. It keeps the orchestration inside the manager
//! simple (drain until a fetch returns an empty batch) and leaves the
//! per-event work — publishing and status bookkeeping — encapsulated here.

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

/// Processes one batch of pending outbox events per invocation.
///
/// Holds handles to storage, transport, and configuration; the manager
/// constructs a single processor at startup and reuses it across iterations.
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
    /// Creates a processor wired to the supplied storage, transport, and
    /// configuration.
    pub fn new(storage: Arc<S>, publisher: Arc<T>, config: Arc<OutboxConfig<P>>) -> Self {
        Self {
            storage,
            publisher,
            config,
        }
    }

    /// Processes one batch of pending events.
    ///
    /// Fetches up to `config.batch_size` rows via
    /// [`OutboxStorage::fetch_next_to_process`], publishes each through the
    /// [`Transport`], and then marks every successfully published row as
    /// [`Sent`](crate::model::EventStatus::Sent) in a single
    /// [`update_status`](OutboxStorage::update_status) call. Rows whose
    /// `publish` call failed are left in `Processing`; their lock will expire
    /// and make them eligible for retry.
    ///
    /// Returns the number of events fetched in the batch — `0` signals to the
    /// caller (typically the manager's drain loop) that there is nothing left
    /// to do right now and it can go back to waiting.
    ///
    /// When the `dlq` feature is enabled, takes a shared
    /// [`DlqHeap`](crate::dlq::storage::DlqHeap) and records success or
    /// failure per event so the heap can track repeat-offender rows.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](OutboxError::DatabaseError) propagated from
    /// `fetch_next_to_process` or `update_status`. Per-event publish failures
    /// are *not* propagated — they are logged via `tracing::error!` and the
    /// rest of the batch continues.
    pub async fn process_pending_events(
        &self,
        #[cfg(feature = "dlq")] dlq_heap: Arc<dyn crate::dlq::storage::DlqHeap>,
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
        #[cfg(feature = "dlq")] dlq_heap: Arc<dyn crate::dlq::storage::DlqHeap>,
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
                    dlq_heap.record_failure(id).await?;

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
            self.storage.update_status(&success_ids, Sent).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::config::{IdempotencyStrategy, OutboxConfig};
    #[cfg(feature = "dlq")]
    use crate::dlq::storage::MockDlqHeap;
    use crate::model::EventStatus;
    use crate::object::EventType;
    use crate::prelude::Payload;
    use crate::publisher::MockTransport;
    use crate::storage::MockOutboxStorage;
    use mockall::Sequence;
    use rstest::rstest;
    use serde::{Deserialize, Serialize};
    use std::collections::HashSet;
    use std::sync::Arc;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    enum TestEvent {
        A(String),
    }

    fn config() -> Arc<OutboxConfig<TestEvent>> {
        Arc::new(OutboxConfig {
            batch_size: 100,
            retention_days: 7,
            gc_interval_secs: 3600,
            poll_interval_secs: 5,
            lock_timeout_mins: 5,
            idempotency_strategy: IdempotencyStrategy::None,
        })
    }

    fn make_event(n: u32) -> Event<TestEvent> {
        Event::new(
            EventType::new(&format!("t{n}")),
            Payload::new(TestEvent::A(format!("v{n}"))),
            None,
        )
    }

    #[rstest]
    #[tokio::test]
    async fn empty_batch_returns_zero_and_skips_status_update() {
        let mut storage = MockOutboxStorage::<TestEvent>::new();
        let mut transport = MockTransport::<TestEvent>::new();

        storage
            .expect_fetch_next_to_process()
            .withf(|limit| *limit == 100)
            .times(1)
            .returning(|_| Ok(vec![]));

        storage.expect_update_status().times(0);
        transport.expect_publish().times(0);

        let processor = OutboxProcessor::new(Arc::new(storage), Arc::new(transport), config());

        #[cfg(not(feature = "dlq"))]
        let result = processor.process_pending_events().await;

        #[cfg(feature = "dlq")]
        let result = {
            let mut dlq = MockDlqHeap::new();
            dlq.expect_record_success().times(0);
            dlq.expect_record_failure().times(0);
            dlq.expect_drain_exceeded().times(0);
            processor.process_pending_events(Arc::new(dlq)).await
        };

        assert!(matches!(result, Ok(0)));
    }

    #[rstest]
    #[tokio::test]
    async fn all_publishes_succeed_updates_status_with_all_ids() {
        let mut storage = MockOutboxStorage::<TestEvent>::new();
        let mut transport = MockTransport::<TestEvent>::new();

        let events = vec![make_event(1), make_event(2), make_event(3)];
        let expected_ids: HashSet<EventId> = events.iter().map(|e| e.id).collect();

        storage
            .expect_fetch_next_to_process()
            .times(1)
            .returning(move |_| Ok(events.clone()));

        storage
            .expect_update_status()
            .withf(move |ids, status| {
                let got: HashSet<EventId> = ids.iter().copied().collect();
                got == expected_ids && *status == EventStatus::Sent
            })
            .times(1)
            .returning(|_, _| Ok(()));

        transport.expect_publish().times(3).returning(|_| Ok(()));

        let processor = OutboxProcessor::new(Arc::new(storage), Arc::new(transport), config());

        #[cfg(not(feature = "dlq"))]
        let result = processor.process_pending_events().await;

        #[cfg(feature = "dlq")]
        let result = {
            let mut dlq = MockDlqHeap::new();
            dlq.expect_record_success().times(3).returning(|_| Ok(()));
            dlq.expect_record_failure().times(0);
            processor.process_pending_events(Arc::new(dlq)).await
        };

        assert!(matches!(result, Ok(3)));
    }

    #[rstest]
    #[tokio::test]
    async fn partial_publish_failure_updates_only_successful() {
        let mut storage = MockOutboxStorage::<TestEvent>::new();
        let mut transport = MockTransport::<TestEvent>::new();

        let e1 = make_event(1);
        let e2 = make_event(2);
        let e3 = make_event(3);
        let id1 = e1.id;
        let id2 = e2.id;
        let id3 = e3.id;

        storage
            .expect_fetch_next_to_process()
            .times(1)
            .returning(move |_| Ok(vec![e1.clone(), e2.clone(), e3.clone()]));

        storage
            .expect_update_status()
            .withf(move |ids, status| {
                let got: HashSet<EventId> = ids.iter().copied().collect();
                got.len() == 2
                    && got.contains(&id1)
                    && got.contains(&id3)
                    && !got.contains(&id2)
                    && *status == EventStatus::Sent
            })
            .times(1)
            .returning(|_, _| Ok(()));

        let mut seq = Sequence::new();
        transport
            .expect_publish()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Ok(()));
        transport
            .expect_publish()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Err(OutboxError::BrokerError("boom".into())));
        transport
            .expect_publish()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Ok(()));

        let processor = OutboxProcessor::new(Arc::new(storage), Arc::new(transport), config());

        #[cfg(not(feature = "dlq"))]
        let result = processor.process_pending_events().await;

        #[cfg(feature = "dlq")]
        let result = {
            let mut dlq = MockDlqHeap::new();
            dlq.expect_record_success()
                .withf(move |id| *id == id1 || *id == id3)
                .times(2)
                .returning(|_| Ok(()));
            dlq.expect_record_failure()
                .withf(move |id| *id == id2)
                .times(1)
                .returning(|_| Ok(1));
            processor.process_pending_events(Arc::new(dlq)).await
        };

        assert!(matches!(result, Ok(3)));
    }

    #[rstest]
    #[tokio::test]
    async fn all_publishes_fail_skips_status_update() {
        let mut storage = MockOutboxStorage::<TestEvent>::new();
        let mut transport = MockTransport::<TestEvent>::new();

        let events = vec![make_event(1), make_event(2)];

        storage
            .expect_fetch_next_to_process()
            .times(1)
            .returning(move |_| Ok(events.clone()));
        storage.expect_update_status().times(0);

        transport
            .expect_publish()
            .times(2)
            .returning(|_| Err(OutboxError::BrokerError("x".into())));

        let processor = OutboxProcessor::new(Arc::new(storage), Arc::new(transport), config());

        #[cfg(not(feature = "dlq"))]
        let result = processor.process_pending_events().await;

        #[cfg(feature = "dlq")]
        let result = {
            let mut dlq = MockDlqHeap::new();
            dlq.expect_record_success().times(0);
            dlq.expect_record_failure().times(2).returning(|_| Ok(1));
            processor.process_pending_events(Arc::new(dlq)).await
        };

        assert!(matches!(result, Ok(2)));
    }

    #[rstest]
    #[tokio::test]
    async fn fetch_error_propagates_without_publishing() {
        let mut storage = MockOutboxStorage::<TestEvent>::new();
        let mut transport = MockTransport::<TestEvent>::new();

        storage
            .expect_fetch_next_to_process()
            .times(1)
            .returning(|_| Err(OutboxError::DatabaseError("boom".into())));
        storage.expect_update_status().times(0);
        transport.expect_publish().times(0);

        let processor = OutboxProcessor::new(Arc::new(storage), Arc::new(transport), config());

        #[cfg(not(feature = "dlq"))]
        let result = processor.process_pending_events().await;

        #[cfg(feature = "dlq")]
        let result = {
            let mut dlq = MockDlqHeap::new();
            dlq.expect_record_success().times(0);
            dlq.expect_record_failure().times(0);
            processor.process_pending_events(Arc::new(dlq)).await
        };

        assert!(matches!(result, Err(OutboxError::DatabaseError(_))));
    }

    #[rstest]
    #[tokio::test]
    async fn update_status_error_propagates_after_publish() {
        let mut storage = MockOutboxStorage::<TestEvent>::new();
        let mut transport = MockTransport::<TestEvent>::new();

        storage
            .expect_fetch_next_to_process()
            .times(1)
            .returning(move |_| Ok(vec![make_event(1)]));
        storage
            .expect_update_status()
            .times(1)
            .returning(|_, _| Err(OutboxError::DatabaseError("boom".into())));

        transport.expect_publish().times(1).returning(|_| Ok(()));

        let processor = OutboxProcessor::new(Arc::new(storage), Arc::new(transport), config());

        #[cfg(not(feature = "dlq"))]
        let result = processor.process_pending_events().await;

        #[cfg(feature = "dlq")]
        let result = {
            let mut dlq = MockDlqHeap::new();
            dlq.expect_record_success().times(1).returning(|_| Ok(()));
            processor.process_pending_events(Arc::new(dlq)).await
        };

        assert!(matches!(result, Err(OutboxError::DatabaseError(_))));
    }
}
