# Oxide Outbox 🦀

[![Crates.io](https://img.shields.io/crates/v/outbox-core.svg)](https://crates.io/crates/outbox-core)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A high-performance, flexible implementation of the **Transactional Outbox pattern** for Rust. Ensure reliable message delivery in your distributed systems by decoupling database transactions from event publishing.



## Key Features

* **Hybrid Event Discovery**: Combines real-time database notifications (e.g., Postgres `LISTEN/NOTIFY`) with fallback polling intervals to ensure zero lost events.
* **Trait-First Architecture**: Completely decoupled from specific storage or message brokers. Switch between Postgres, MySQL, Kafka, or RabbitMQ by implementing simple traits.
* **Built-in Garbage Collection**: Automatic cleanup of processed events to prevent table bloat.
* **Concurrency Safe**: Designed for horizontal scaling with support for row-level locking.
* **Async Native**: Built from the ground up on `tokio`.

---

## Project Structure

- `outbox-core`: The heart of the library containing core logic, traits, and the `OutboxManager`.
- `outbox-postgres`: Default implementation for PostgreSQL using `sqlx`.

---

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]

outbox-core = "0.1"
outbox-postgres = "0.1" # If using Postgres
```

---

## How It Works
`Oxide Outbox` uses a dual-trigger mechanism to process events:
- Notification Trigger: The manager listens for specific DB signals (like NOTIFY in Postgres). When a transaction commits an outbox entry, the worker wakes up immediately.
- Interval Trigger: A safety net that periodically checks for "stale" events that might have been missed during network blips or worker restarts.
- Automatic GC: A background task periodically removes successfully processed events based on your retention policy.

---

## Reliability & Resilience

- **At-Least-Once Delivery**: Messages are guaranteed to be delivered at least once. If a worker fails while processing an event, the message remains in the database.
- **Lazy Retry (Visibility Timeout)**: When an event is picked up, it is assigned a `lock_until` timestamp. If the worker doesn't mark it as completed within the `lock_timeout_mins` (e.g., due to a crash), the event automatically becomes visible again for the next polling cycle or notification.
- **Transactional Integrity**: Since the outbox table lives in your business database, events are saved within the same ACID transaction as your business logic, ensuring they are never lost or orphaned.

---

## Quick Start

```rust
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
        lock_timeout_mins: 1,
        idempotency_strategy: IdempotencyStrategy::Uuid,
        idempotency_storage: IdempotencyStorage::None
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
    add_event(&writer, "OrderCreated", serde_json::json!({"id": 123}), &config, || None).await?;
    tokio::time::sleep(Duration::from_secs(20)).await;
    info!("Inserting test 2 event into DB...");
    add_event(&writer, "OrderCreated", serde_json::json!({"id": 321}), &config, || None).await?;
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
```

