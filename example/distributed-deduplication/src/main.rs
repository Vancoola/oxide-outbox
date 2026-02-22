use outbox_core::prelude::*;
use outbox_postgres::{PostgresOutbox, PostgresWriter};
use outbox_redis::RedisTokenProvider;
use outbox_redis::config::RedisTokenConfig;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tracing::{Level, error, info};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .init();

    let pool =
        PgPool::connect("postgresql://postgres:mysecretpassword@localhost:5432/outbox").await?;
    let config = Arc::new(OutboxConfig {
        batch_size: 100,
        retention_days: 1,
        gc_interval_secs: 10,
        poll_interval_secs: 100,
        lock_timeout_mins: 1,
        idempotency_strategy: IdempotencyStrategy::Provided,
    });
    let regis_config = RedisTokenConfig::default();

    let storage = PostgresOutbox::new(pool.clone(), config.clone());
    let writer = Arc::new(PostgresWriter(pool.clone()));
    let redis_provider = RedisTokenProvider::new("redis://127.0.0.1:6379", regis_config).await?;

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<Message>();
    let publisher = TokioEventPublisher(sender);

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let outbox = OutboxManager::new(
        Arc::new(storage),
        Arc::new(publisher),
        config.clone(),
        shutdown_rx,
    );

    tokio::spawn(async move {
        if let Err(e) = outbox.run().await {
            error!("Outbox critical error: {}", e);
        }
    });

    tokio::spawn(async move {
        while let Some(msg) = receiver.recv().await {
            info!(
                "Event received in Broker: type={:?}, payload={:?}",
                msg.0, msg.1
            );
        }
    });

    let service = OutboxService::with_idempotency(writer, config.clone(), Arc::new(redis_provider));


    info!("Inserting test event into DB...");
    service
        .add_event(
            "OrderCreated",
            serde_json::json!({"id": 123}),
            Some(String::from("r_token")),
            || None,
        )
        .await?;
    tokio::time::sleep(Duration::from_secs(10)).await;
    info!("Deduplication! Inserting test 2 event into DB...");
    if let Err(e) = service
        .add_event(
            "OrderCreated",
            serde_json::json!({"id": 123}),
            Some(String::from("r_token")),
            || None,
        )
        .await
    {
        error!("Deduplication error: {}", e);
    }

    tokio::time::sleep(Duration::from_mins(2)).await;

    shutdown_tx.send(true)?;
    Ok(())
}
struct Message(EventType, Payload);

#[derive(Clone)]
struct TokioEventPublisher(tokio::sync::mpsc::UnboundedSender<Message>);
#[async_trait::async_trait]
impl Transport for TokioEventPublisher {
    async fn publish(&self, event_type: EventType, payload: Payload) -> Result<(), OutboxError> {
        self.0
            .send(Message(event_type, payload))
            .map_err(|e| OutboxError::InfrastructureError(e.to_string()))
    }
}
