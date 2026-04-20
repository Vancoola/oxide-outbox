use crate::dlq::processor::DlqProcessor;
use crate::error::OutboxError;
use crate::gc::GarbageCollector;
use crate::processor::OutboxProcessor;
use crate::publisher::Transport;
use crate::storage::OutboxStorage;
use crate::{config::OutboxConfig, dlq::model::EventFail};
use serde::Serialize;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch::Receiver;
use tracing::{debug, error, info, trace};

pub struct OutboxManager<S, P, PT>
where
    PT: Debug + Clone + Serialize,
{
    storage: Arc<S>,
    publisher: Arc<P>,
    config: Arc<OutboxConfig<PT>>,
    shutdown_rx: Receiver<bool>,
}

impl<S, P, PT> OutboxManager<S, P, PT>
where
    S: OutboxStorage<PT> + Send + Sync + 'static,
    P: Transport<PT> + Send + Sync + 'static,
    PT: Debug + Clone + Serialize + Send + Sync + 'static,
{
    pub fn new(
        storage: Arc<S>,
        publisher: Arc<P>,
        config: Arc<OutboxConfig<PT>>,
        shutdown_rx: Receiver<bool>,
    ) -> Self {
        Self {
            storage,
            publisher,
            config,
            shutdown_rx,
        }
    }

    /// Starts the main outbox worker loop.
    ///
    /// This method will run until a shutdown signal is received via the `shutdown_rx` channel.
    /// It handles event processing, database notifications, and periodic garbage collection.
    ///
    /// # Errors
    ///
    /// Returns an [`OutboxError`] if the worker encounters a terminal failure that it cannot
    /// recover from (though currently the loop primarily logs errors and continues).
    pub async fn run(self) -> Result<(), OutboxError> {
        let storage_for_listen = self.storage.clone();
        let processor = OutboxProcessor::new(
            self.storage.clone(),
            self.publisher.clone(),
            self.config.clone(),
        );

        let gc = GarbageCollector::new(self.storage.clone());
        let mut rx_gc = self.shutdown_rx.clone();
        let gc_interval_secs = self.config.gc_interval_secs;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(gc_interval_secs));
            loop {
                tokio::select! {
                    _ = interval.tick() => { let _ = gc.collect_garbage().await; }
                    _ = rx_gc.changed() => {
                            if rx_gc.has_changed().is_err(){
                            break;
                        }
                        if *rx_gc.borrow() {
                            break
                        }
                    },
                }
            }
        });

        let mut rx_listen = self.shutdown_rx.clone();
        let rx_dlq = self.shutdown_rx.clone();
        let poll_interval = self.config.poll_interval_secs;
        let mut interval = tokio::time::interval(Duration::from_secs(poll_interval));
        let (fail_tx, fail_rx) = tokio::sync::mpsc::unbounded_channel::<EventFail>();
        let dlq_processor = DlqProcessor::new(fail_rx, rx_dlq);

        info!("Starting DLQ processor");

        tokio::spawn(async move {
            if let Err(e) = dlq_processor.run().await {
                error!("DLQ processor error: {}", e);
            }
        });

        info!("Outbox worker loop started");

        loop {
            tokio::select! {
                signal = storage_for_listen.wait_for_notification("outbox_event") => {
                    if let Err(e) = signal {
                        error!("Listen error: {}", e);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                }
                _ = interval.tick() => {
                    trace!("Checking for stale or pending events via interval");
                }
                _ = rx_listen.changed() => {
                    if rx_listen.has_changed().is_err(){
                        break;
                    }
                    if *rx_listen.borrow() {
                        break
                    }
                }
            }
            loop {
                if *rx_listen.borrow() {
                    return Ok(());
                }
                match processor.process_pending_events(fail_tx.clone()).await {
                    Ok(0) => break,
                    Ok(count) => debug!("Processed {} events", count),
                    Err(e) => {
                        error!("Processing error: {}", e);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        break;
                    }
                }
            }
        }
        debug!("Outbox worker loop stopped");
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::config::{IdempotencyStrategy, OutboxConfig};
    use crate::error::OutboxError;
    use crate::manager::OutboxManager;
    use crate::model::{Event, EventStatus};
    use crate::object::EventType;
    use crate::prelude::Payload;
    use crate::publisher::MockTransport;
    use crate::storage::MockOutboxStorage;
    use mockall::Sequence;
    use rstest::rstest;
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;
    use tokio::sync::watch;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    enum SomeDomainEvent {
        SomeEvent(String),
    }

    #[rstest]
    #[tokio::test]
    async fn test_event_send_success() {
        let config = OutboxConfig {
            batch_size: 100,
            retention_days: 7,
            gc_interval_secs: 3600,
            poll_interval_secs: 5,
            lock_timeout_mins: 5,
            idempotency_strategy: IdempotencyStrategy::None,
        };

        let mut storage_mock = MockOutboxStorage::<SomeDomainEvent>::new();
        let mut transport_mock = MockTransport::<SomeDomainEvent>::new();

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        storage_mock
            .expect_wait_for_notification()
            .returning(|_| Ok(()));

        storage_mock
            .expect_fetch_next_to_process()
            .withf(move |l| l == &config.batch_size)
            .times(1)
            .returning(move |_| {
                let _ = shutdown_tx.send(true);
                Ok(vec![
                    Event::new(
                        EventType::new("1"),
                        Payload::new(SomeDomainEvent::SomeEvent("test1".to_string())),
                        None,
                    ),
                    Event::new(
                        EventType::new("2"),
                        Payload::new(SomeDomainEvent::SomeEvent("test2".to_string())),
                        None,
                    ),
                    Event::new(
                        EventType::new("3"),
                        Payload::new(SomeDomainEvent::SomeEvent("test3".to_string())),
                        None,
                    ),
                    Event::new(
                        EventType::new("4"),
                        Payload::new(SomeDomainEvent::SomeEvent("test4".to_string())),
                        None,
                    ),
                ])
            });

        storage_mock
            .expect_fetch_next_to_process()
            .withf(move |l| l == &config.batch_size)
            .returning(move |_| Ok(vec![]));

        storage_mock
            .expect_updates_status()
            .withf(|ids, s| ids.len() == 4 && s == &EventStatus::Sent)
            .returning(|_, _| Ok(()));

        let mut seq = Sequence::new();

        for i in 1..=4 {
            let expected_type = i.to_string();
            let expected_val = SomeDomainEvent::SomeEvent(format!("test{}", i));

            transport_mock
                .expect_publish()
                .withf(move |event| {
                    let type_matches = event.event_type.as_str() == expected_type;
                    let payload_matches = event.payload.as_value() == &expected_val;
                    type_matches && payload_matches
                })
                .times(1)
                .in_sequence(&mut seq)
                .returning(|_| Ok(()));
        }

        let manager = OutboxManager::new(
            Arc::new(storage_mock),
            Arc::new(transport_mock),
            Arc::new(config),
            shutdown_rx,
        );

        let handle = tokio::spawn(async move {
            manager.run().await.unwrap();
        });

        tokio::time::timeout(tokio::time::Duration::from_secs(1), handle)
            .await
            .expect("Manager did not stop in time")
            .unwrap();
    }

    #[rstest]
    #[tokio::test]
    async fn test_recovery_after_storage_error() {
        let mut storage_mock = MockOutboxStorage::<SomeDomainEvent>::new();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        storage_mock
            .expect_wait_for_notification()
            .times(1)
            .returning(|_| Err(OutboxError::InfrastructureError("Connection lost".into())));

        storage_mock
            .expect_wait_for_notification()
            .times(1)
            .returning(move |_| {
                let _ = shutdown_tx.send(true);
                Ok(())
            });

        storage_mock
            .expect_fetch_next_to_process()
            .returning(|_| Ok(vec![]));

        storage_mock
            .expect_delete_garbage()
            .times(1)
            .returning(|| Ok(()));

        let transport_mock = MockTransport::<SomeDomainEvent>::new();

        let config = OutboxConfig::<SomeDomainEvent> {
            batch_size: 100,
            retention_days: 7,
            gc_interval_secs: 3600,
            poll_interval_secs: 5,
            lock_timeout_mins: 5,
            idempotency_strategy: IdempotencyStrategy::None,
        };

        let manager = OutboxManager::new(
            Arc::new(storage_mock),
            Arc::new(transport_mock),
            Arc::new(config),
            shutdown_rx,
        );

        let result = manager.run().await;
        assert!(result.is_ok());
    }

    #[rstest]
    #[tokio::test]
    async fn test_publish_failure() {
        let mut storage_mock = MockOutboxStorage::<SomeDomainEvent>::new();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let e1 = Event::new(
            EventType::new("1"),
            Payload::new(SomeDomainEvent::SomeEvent("test1".to_string())),
            None,
        );
        let e2 = Event::new(
            EventType::new("2"),
            Payload::new(SomeDomainEvent::SomeEvent("test2".to_string())),
            None,
        );
        let e3 = Event::new(
            EventType::new("3"),
            Payload::new(SomeDomainEvent::SomeEvent("test3".to_string())),
            None,
        );
        let e4 = Event::new(
            EventType::new("4"),
            Payload::new(SomeDomainEvent::SomeEvent("test4".to_string())),
            None,
        );

        let id1 = e1.id.clone();
        let id2 = e2.id.clone();
        let id3 = e3.id.clone();
        let id4 = e4.id.clone();

        storage_mock
            .expect_wait_for_notification()
            .returning(|_| Ok(()));

        storage_mock
            .expect_fetch_next_to_process()
            .times(1)
            .returning(move |_| Ok(vec![e1.clone(), e2.clone(), e3.clone(), e4.clone()]));

        storage_mock
            .expect_fetch_next_to_process()
            .returning(|_| Ok(vec![]));

        storage_mock.expect_delete_garbage().returning(|| Ok(()));

        storage_mock
            .expect_updates_status()
            .withf(move |ids, status| {
                if status != &EventStatus::Sent {
                    return false;
                }

                let ids_set: std::collections::HashSet<_> = ids.iter().cloned().collect();

                ids_set.len() == 3
                    && ids_set.contains(&id1)
                    && ids_set.contains(&id2)
                    && ids_set.contains(&id4)
                    && !ids_set.contains(&id3)
            })
            .returning(move |_, _| {
                let _ = shutdown_tx.send(true);
                Ok(())
            });

        let mut transport_mock = MockTransport::<SomeDomainEvent>::new();

        let mut seq = Sequence::new();

        transport_mock
            .expect_publish()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Ok(()));

        transport_mock
            .expect_publish()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Ok(()));

        transport_mock
            .expect_publish()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Err(OutboxError::InfrastructureError("Connection lost".into())));

        transport_mock
            .expect_publish()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Ok(()));

        let config = OutboxConfig::<SomeDomainEvent> {
            batch_size: 100,
            retention_days: 7,
            gc_interval_secs: 3600,
            poll_interval_secs: 5,
            lock_timeout_mins: 5,
            idempotency_strategy: IdempotencyStrategy::None,
        };

        let manager = OutboxManager::new(
            Arc::new(storage_mock),
            Arc::new(transport_mock),
            Arc::new(config),
            shutdown_rx,
        );

        let result = manager.run().await;
        assert!(result.is_ok());
    }
}
