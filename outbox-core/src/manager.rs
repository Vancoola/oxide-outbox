//! Top-level worker that owns the outbox event loop.
//!
//! [`OutboxManager`] ties together storage, transport, configuration and a
//! shutdown signal. It drives the main processing loop — waiting for database
//! notifications, polling on a fixed interval, draining pending events via
//! [`OutboxProcessor`], and running a periodic [`GarbageCollector`] task on
//! the side. Construct one via [`OutboxManagerBuilder`](crate::builder::OutboxManagerBuilder).

use crate::config::OutboxConfig;
use crate::error::OutboxError;
use crate::gc::GarbageCollector;
use crate::processor::OutboxProcessor;
use crate::publisher::Transport;
use crate::storage::OutboxStorage;
use serde::Serialize;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch::Receiver;
use tracing::{debug, error, info, trace};

/// Long-running worker that publishes pending outbox events to the broker.
///
/// The manager owns one concrete [`OutboxStorage`] implementation (`S`), one
/// [`Transport`] implementation (`P`), and the user's domain event payload type
/// (`PT`). After [`run`](Self::run) is invoked, it will keep processing events
/// until the watched shutdown channel flips to `true`.
///
/// Prefer constructing through
/// [`OutboxManagerBuilder`](crate::builder::OutboxManagerBuilder) rather than
/// calling [`new`](Self::new) directly — the builder reports missing
/// dependencies with a structured [`OutboxError::ConfigError`] instead of
/// requiring all arguments to line up positionally.
pub struct OutboxManager<S, P, PT>
where
    PT: Debug + Clone + Serialize,
{
    storage: Arc<S>,
    publisher: Arc<P>,
    config: Arc<OutboxConfig<PT>>,
    shutdown_rx: Receiver<bool>,
    #[cfg(feature = "dlq")]
    dlq_heap: Arc<dyn crate::dlq::storage::DlqHeap>,
}

impl<S, P, PT> OutboxManager<S, P, PT>
where
    S: OutboxStorage<PT> + Send + Sync + 'static,
    P: Transport<PT> + Send + Sync + 'static,
    PT: Debug + Clone + Serialize + Send + Sync + 'static,
{
    /// Direct constructor used by
    /// [`OutboxManagerBuilder`](crate::builder::OutboxManagerBuilder).
    ///
    /// Application code should normally go through the builder, which
    /// validates that every required collaborator has been supplied.
    ///
    /// This signature is compiled when the `dlq` feature is enabled and takes
    /// an extra [`DlqHeap`](crate::dlq::storage::DlqHeap) that tracks per-event
    /// failure counts.
    #[cfg(feature = "dlq")]
    pub fn new(
        storage: Arc<S>,
        publisher: Arc<P>,
        config: Arc<OutboxConfig<PT>>,
        dlq_heap: Arc<dyn crate::dlq::storage::DlqHeap>,
        shutdown_rx: Receiver<bool>,
    ) -> Self {
        Self {
            storage,
            publisher,
            config,
            shutdown_rx,
            dlq_heap,
        }
    }

    /// Direct constructor used by
    /// [`OutboxManagerBuilder`](crate::builder::OutboxManagerBuilder).
    ///
    /// Application code should normally go through the builder.
    ///
    /// This signature is compiled when the `dlq` feature is disabled and omits
    /// the DLQ heap argument.
    #[cfg(not(feature = "dlq"))]
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
    /// This method will run until a shutdown signal is received via the
    /// `shutdown_rx` channel. It coordinates three concerns:
    ///
    /// - **Event processing** — on each wake-up it drives
    ///   [`OutboxProcessor`] in an inner drain loop until the fetched batch is
    ///   empty, at which point it returns to waiting.
    /// - **Wake-up sources** — a `tokio::select!` races a storage-level
    ///   `LISTEN`/notify call, a poll interval (`config.poll_interval_secs`),
    ///   and the shutdown receiver. A notification error is logged and the
    ///   loop sleeps for 5 seconds before retrying.
    /// - **Garbage collection** — a background task is spawned that ticks on
    ///   `config.gc_interval_secs` and calls [`GarbageCollector::collect_garbage`],
    ///   exiting when the shutdown signal fires.
    ///
    /// # Errors
    ///
    /// Returns an [`OutboxError`] if the worker encounters a terminal failure
    /// that it cannot recover from. In the current implementation transient
    /// errors from the storage and transport layers are logged and the loop
    /// continues, so a returned error signals that the worker observed a
    /// graceful shutdown via `shutdown_rx`.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use std::sync::Arc;
    /// use tokio::sync::watch;
    /// use outbox_core::prelude::*;
    ///
    /// # async fn start(
    /// #     storage: Arc<impl OutboxStorage<MyEvent> + Send + Sync + 'static>,
    /// #     publisher: Arc<impl Transport<MyEvent> + Send + Sync + 'static>,
    /// # ) -> Result<(), OutboxError>
    /// # where MyEvent: std::fmt::Debug + Clone + serde::Serialize + Send + Sync + 'static,
    /// # {
    /// let (shutdown_tx, shutdown_rx) = watch::channel(false);
    /// let manager = OutboxManagerBuilder::new()
    ///     .storage(storage)
    ///     .publisher(publisher)
    ///     .config(Arc::new(OutboxConfig::default()))
    ///     .shutdown_rx(shutdown_rx)
    ///     .build()?;
    ///
    /// let handle = tokio::spawn(async move { manager.run().await });
    ///
    /// // ... later, on a signal or process exit:
    /// let _ = shutdown_tx.send(true);
    /// handle.await.expect("worker panicked")?;
    /// # Ok(()) }
    /// ```
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
        let poll_interval = self.config.poll_interval_secs;
        let mut interval = tokio::time::interval(Duration::from_secs(poll_interval));

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
                #[cfg(feature = "dlq")]
                match processor
                    .process_pending_events(self.dlq_heap.clone())
                    .await
                {
                    Ok(0) => break,
                    Ok(count) => debug!("Processed {} events", count),
                    Err(e) => {
                        error!("Processing error: {}", e);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        break;
                    }
                }
                #[cfg(not(feature = "dlq"))]
                match processor.process_pending_events().await {
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
    use crate::builder::OutboxManagerBuilder;
    use crate::config::{IdempotencyStrategy, OutboxConfig};
    #[cfg(feature = "dlq")]
    use crate::dlq::storage::MockDlqHeap;
    use crate::error::OutboxError;
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

        #[cfg(feature = "dlq")]
        let mut dlq_heap_mock: MockDlqHeap = MockDlqHeap::new();

        #[cfg(feature = "dlq")]
        dlq_heap_mock.expect_record_success().returning(|_| Ok(()));

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
            .expect_update_status()
            .withf(|ids, s| ids.len() == 4 && s == &EventStatus::Sent)
            .returning(|_, _| Ok(()));

        storage_mock.expect_delete_garbage().returning(|| Ok(()));

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

        #[cfg(feature = "dlq")]
        let manager = OutboxManagerBuilder::new()
            .storage(Arc::new(storage_mock))
            .publisher(Arc::new(transport_mock))
            .config(Arc::new(config))
            .shutdown_rx(shutdown_rx)
            .dlq_heap(Arc::new(dlq_heap_mock))
            .build()
            .unwrap();

        #[cfg(not(feature = "dlq"))]
        let manager = OutboxManagerBuilder::new()
            .storage(Arc::new(storage_mock))
            .publisher(Arc::new(transport_mock))
            .config(Arc::new(config))
            .shutdown_rx(shutdown_rx)
            .build()
            .unwrap();

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

        #[cfg(feature = "dlq")]
        let manager = OutboxManagerBuilder::new()
            .storage(Arc::new(storage_mock))
            .publisher(Arc::new(transport_mock))
            .config(Arc::new(config))
            .dlq_heap(Arc::new(MockDlqHeap::new()))
            .shutdown_rx(shutdown_rx)
            .build()
            .unwrap();

        #[cfg(not(feature = "dlq"))]
        let manager = OutboxManagerBuilder::new()
            .storage(Arc::new(storage_mock))
            .publisher(Arc::new(transport_mock))
            .config(Arc::new(config))
            .shutdown_rx(shutdown_rx)
            .build()
            .unwrap();

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
            .expect_update_status()
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

        #[cfg(feature = "dlq")]
        let dlq_heap_mock = {
            let mut m = MockDlqHeap::new();
            m.expect_record_success().returning(|_| Ok(()));
            m.expect_record_failure().returning(|_| Ok(0));
            m
        };

        #[cfg(feature = "dlq")]
        let manager = OutboxManagerBuilder::new()
            .storage(Arc::new(storage_mock))
            .publisher(Arc::new(transport_mock))
            .config(Arc::new(config))
            .dlq_heap(Arc::new(dlq_heap_mock))
            .shutdown_rx(shutdown_rx)
            .build()
            .unwrap();

        #[cfg(not(feature = "dlq"))]
        let manager = OutboxManagerBuilder::new()
            .storage(Arc::new(storage_mock))
            .publisher(Arc::new(transport_mock))
            .config(Arc::new(config))
            .shutdown_rx(shutdown_rx)
            .build()
            .unwrap();

        let result = manager.run().await;
        assert!(result.is_ok());
    }

    fn default_config() -> OutboxConfig<SomeDomainEvent> {
        OutboxConfig {
            batch_size: 100,
            retention_days: 7,
            gc_interval_secs: 3600,
            poll_interval_secs: 5,
            lock_timeout_mins: 5,
            idempotency_strategy: IdempotencyStrategy::None,
        }
    }

    #[rstest]
    #[tokio::test]
    async fn shutdown_set_before_run_exits_without_fetch() {
        let mut storage_mock = MockOutboxStorage::<SomeDomainEvent>::new();
        let transport_mock = MockTransport::<SomeDomainEvent>::new();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let _ = shutdown_tx.send(true);

        storage_mock
            .expect_wait_for_notification()
            .returning(|_| Ok(()));
        storage_mock.expect_delete_garbage().returning(|| Ok(()));
        storage_mock.expect_fetch_next_to_process().times(0);
        storage_mock.expect_update_status().times(0);

        #[cfg(feature = "dlq")]
        let manager = OutboxManagerBuilder::new()
            .storage(Arc::new(storage_mock))
            .publisher(Arc::new(transport_mock))
            .config(Arc::new(default_config()))
            .dlq_heap(Arc::new(MockDlqHeap::new()))
            .shutdown_rx(shutdown_rx)
            .build()
            .unwrap();
        #[cfg(not(feature = "dlq"))]
        let manager = OutboxManagerBuilder::new()
            .storage(Arc::new(storage_mock))
            .publisher(Arc::new(transport_mock))
            .config(Arc::new(default_config()))
            .shutdown_rx(shutdown_rx)
            .build()
            .unwrap();

        let result =
            tokio::time::timeout(tokio::time::Duration::from_secs(1), manager.run())
                .await
                .expect("manager did not stop in time");
        assert!(result.is_ok());
        drop(shutdown_tx);
    }

    #[rstest]
    #[tokio::test]
    async fn inner_loop_drains_until_empty_batch() {
        let mut storage_mock = MockOutboxStorage::<SomeDomainEvent>::new();
        let mut transport_mock = MockTransport::<SomeDomainEvent>::new();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        storage_mock
            .expect_wait_for_notification()
            .returning(|_| Ok(()));
        storage_mock.expect_delete_garbage().returning(|| Ok(()));

        let mut seq = Sequence::new();
        storage_mock
            .expect_fetch_next_to_process()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| {
                Ok(vec![
                    Event::new(
                        EventType::new("a1"),
                        Payload::new(SomeDomainEvent::SomeEvent("a1".into())),
                        None,
                    ),
                    Event::new(
                        EventType::new("a2"),
                        Payload::new(SomeDomainEvent::SomeEvent("a2".into())),
                        None,
                    ),
                ])
            });
        storage_mock
            .expect_fetch_next_to_process()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| {
                Ok(vec![Event::new(
                    EventType::new("b1"),
                    Payload::new(SomeDomainEvent::SomeEvent("b1".into())),
                    None,
                )])
            });
        storage_mock
            .expect_fetch_next_to_process()
            .times(1)
            .in_sequence(&mut seq)
            .returning(move |_| {
                let _ = shutdown_tx.send(true);
                Ok(vec![])
            });
        storage_mock
            .expect_fetch_next_to_process()
            .returning(|_| Ok(vec![]));

        storage_mock
            .expect_update_status()
            .times(2)
            .returning(|_, _| Ok(()));

        transport_mock.expect_publish().times(3).returning(|_| Ok(()));

        #[cfg(feature = "dlq")]
        let manager = {
            let mut dlq = MockDlqHeap::new();
            dlq.expect_record_success().returning(|_| Ok(()));
            dlq.expect_record_failure().returning(|_| Ok(0));
            OutboxManagerBuilder::new()
                .storage(Arc::new(storage_mock))
                .publisher(Arc::new(transport_mock))
                .config(Arc::new(default_config()))
                .dlq_heap(Arc::new(dlq))
                .shutdown_rx(shutdown_rx)
                .build()
                .unwrap()
        };
        #[cfg(not(feature = "dlq"))]
        let manager = OutboxManagerBuilder::new()
            .storage(Arc::new(storage_mock))
            .publisher(Arc::new(transport_mock))
            .config(Arc::new(default_config()))
            .shutdown_rx(shutdown_rx)
            .build()
            .unwrap();

        let handle = tokio::spawn(async move { manager.run().await.unwrap() });
        tokio::time::timeout(tokio::time::Duration::from_secs(1), handle)
            .await
            .expect("manager did not stop in time")
            .unwrap();
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn fetch_error_inside_loop_is_recoverable() {
        let mut storage_mock = MockOutboxStorage::<SomeDomainEvent>::new();
        let transport_mock = MockTransport::<SomeDomainEvent>::new();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        storage_mock
            .expect_wait_for_notification()
            .returning(|_| Ok(()));
        storage_mock.expect_delete_garbage().returning(|| Ok(()));

        let mut seq = Sequence::new();
        storage_mock
            .expect_fetch_next_to_process()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Err(OutboxError::DatabaseError("transient".into())));
        storage_mock
            .expect_fetch_next_to_process()
            .in_sequence(&mut seq)
            .returning(move |_| {
                let _ = shutdown_tx.send(true);
                Ok(vec![])
            });
        storage_mock
            .expect_fetch_next_to_process()
            .returning(|_| Ok(vec![]));

        storage_mock.expect_update_status().times(0);

        #[cfg(feature = "dlq")]
        let manager = OutboxManagerBuilder::new()
            .storage(Arc::new(storage_mock))
            .publisher(Arc::new(transport_mock))
            .config(Arc::new(default_config()))
            .dlq_heap(Arc::new(MockDlqHeap::new()))
            .shutdown_rx(shutdown_rx)
            .build()
            .unwrap();
        #[cfg(not(feature = "dlq"))]
        let manager = OutboxManagerBuilder::new()
            .storage(Arc::new(storage_mock))
            .publisher(Arc::new(transport_mock))
            .config(Arc::new(default_config()))
            .shutdown_rx(shutdown_rx)
            .build()
            .unwrap();

        let result = tokio::time::timeout(
            tokio::time::Duration::from_secs(30),
            manager.run(),
        )
        .await
        .expect("manager did not stop in time");
        assert!(result.is_ok());
    }
}
