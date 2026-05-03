//! DLQ flow demo.
//!
//! Wires Postgres as outbox storage and Redis as the [`DlqHeap`] backend,
//! then publishes through a `FlakyPublisher` that always fails for one event
//! type. The flow:
//!
//! 1. Two events are inserted via [`OutboxService::add_event`].
//! 2. The worker picks them up and calls `FlakyPublisher::publish`. The "good"
//!    event succeeds → `record_success` clears its counter. The "bad" event
//!    fails → `record_failure` bumps its counter in Redis.
//! 3. After `dlq_threshold` failures, the [`DlqProcessor`] tick drains the
//!    bad event from the Redis `ZSet` and calls
//!    [`OutboxStorage::quarantine_events`], which moves it into the DLQ
//!    table atomically.
//!
//! Run prerequisites: Postgres on `localhost:5432` (db `outbox`) and Redis on
//! `localhost:6379`. Both are in `compose.yaml`.

use async_trait::async_trait;
use outbox_core::prelude::*;
use outbox_postgres::{PostgresOutbox, PostgresWriter};
use outbox_redis::RedisProvider;
use outbox_redis::config::RedisTokenConfig;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tracing::level_filters::LevelFilter;
use tracing::{error, info, warn};
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let filter = Targets::new()
        .with_default(LevelFilter::DEBUG)
        .with_target("sqlx", LevelFilter::WARN);

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(filter)
        .init();

    let pool =
        PgPool::connect("postgresql://postgres:mysecretpassword@localhost:5432/postgres").await?;

    let redis_cfg = RedisTokenConfig {
        key_prefix: "outbox_dlq_demo".to_owned(),
        ..Default::default()
    };
    let redis = RedisProvider::new("redis://localhost:6379", redis_cfg).await?;
    let dlq_heap: Arc<dyn DlqHeap> = Arc::new(redis);

    let config = Arc::new(OutboxConfig {
        batch_size: 100,
        retention_days: 1,
        gc_interval_secs: 60,
        poll_interval_secs: 2,
        lock_timeout_mins: 1,
        idempotency_strategy: IdempotencyStrategy::None,
        dlq_threshold: 3,
        dlq_interval_secs: 5,
    });

    let storage = PostgresOutbox::new(pool.clone(), config.clone());
    let writer = Arc::new(PostgresWriter(pool.clone()));

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<Message>();
    let publisher = FlakyPublisher::new(sender);

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let outbox = OutboxManagerBuilder::new()
        .storage(Arc::new(storage))
        .publisher(Arc::new(publisher))
        .config(config.clone())
        .dlq_heap(dlq_heap)
        .shutdown_rx(shutdown_rx)
        .build()?;

    let manager = tokio::spawn(async move {
        if let Err(e) = outbox.run().await {
            error!("Outbox critical error: {e}");
        }
    });

    let receiver_task = tokio::spawn(async move {
        while let Some(msg) = receiver.recv().await {
            info!(
                "Broker delivered: type={} payload={:?}",
                msg.0.event_type.as_str(),
                msg.0.payload
            );
        }
    });

    let service = OutboxService::new(writer, config.clone());

    info!("Inserting GoodPing event...");
    service
        .add_event("GoodPing", DemoEvent::Ping("hello".into()), None, || None)
        .await?;

    info!("Inserting CursedPing event (publisher will fail it forever)...");
    service
        .add_event("CursedPing", DemoEvent::Ping("doom".into()), None, || None)
        .await?;

    info!("Waiting ~240s for worker to retry, fail, and quarantine the cursed event...");
    tokio::time::sleep(Duration::from_mins(4)).await;

    info!("Shutting down...");
    shutdown_tx.send(true)?;
    let _ = tokio::time::timeout(Duration::from_secs(5), manager).await;
    receiver_task.abort();

    info!("Done. Check the `dead_letter_outbox_events` table for the quarantined CursedPing.");
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum DemoEvent {
    Ping(String),
}

struct Message(Event<DemoEvent>);

#[derive(Clone)]
struct FlakyPublisher(tokio::sync::mpsc::UnboundedSender<Message>);

impl FlakyPublisher {
    fn new(tx: tokio::sync::mpsc::UnboundedSender<Message>) -> Self {
        Self(tx)
    }
}

#[async_trait]
impl Transport<DemoEvent> for FlakyPublisher {
    async fn publish(&self, event: Event<DemoEvent>) -> Result<(), OutboxError> {
        if event.event_type.as_str() == "CursedPing" {
            warn!(
                "FlakyPublisher: refusing to deliver cursed event id={}",
                event.id.as_uuid()
            );
            return Err(OutboxError::InfrastructureError("broker is on fire".into()));
        }
        self.0
            .send(Message(event))
            .map_err(|e| OutboxError::InfrastructureError(e.to_string()))
    }
}
