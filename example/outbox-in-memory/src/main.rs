use std::sync::Arc;
use std::time::Duration;
use sqlx::PgPool;
use tracing::{error, info, Level};
use outbox_core::prelude::*;
use outbox_postgres::{PostgresOutbox, PostgresWriter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_max_level(Level::TRACE).init();

    let pool = PgPool::connect("postgresql://postgres:mysecretpassword@localhost:5432/outbox").await?;
    let config = Arc::new(OutboxConfig {
        batch_size: 100,
        retention_days: 1,
        gc_interval_secs: 10,
        poll_interval_secs: 100,
        lock_timeout_mins: 1
    });

    let storage = PostgresOutbox::new(pool.clone(), config.clone());
    let writer = PostgresWriter(pool.clone());

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<Message>();
    let publisher = TokioEventPublisher(sender);

    let mut outbox = OutboxManager::new(storage, publisher, config.clone());

    tokio::spawn(async move {
        if let Err(e) = outbox.run().await {
            error!("Outbox critical error: {}", e);
        }
    });

    tokio::spawn(async move {
        while let Some(msg) = receiver.recv().await {
            info!("Event received in Broker: type={:?}, payload={:?}", msg.0, msg.1);
        }
    });

    info!("Inserting test event into DB...");
    add_event(&writer, "OrderCreated", serde_json::json!({"id": 123})).await?;
    tokio::time::sleep(Duration::from_secs(20)).await;
    info!("Inserting test 2 event into DB...");
    add_event(&writer, "OrderCreated", serde_json::json!({"id": 321})).await?;
    tokio::time::sleep(Duration::from_mins(2)).await;
    Ok(())
}
struct Message(EventType, Payload);

#[derive(Clone)]
struct TokioEventPublisher(tokio::sync::mpsc::UnboundedSender<Message>);
#[async_trait::async_trait]
impl Transport for TokioEventPublisher {
    async fn publish(&self, event_type: EventType, payload: Payload) -> Result<(), OutboxError> {
        self.0.send(Message(event_type, payload)).map_err(|e| OutboxError::InfrastructureError(e.to_string()))
    }
}

