# Outbox Core

[![Crates.io](https://img.shields.io/crates/v/outbox-core.svg)](https://crates.io/crates/outbox-core)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)
![PRs welocme](https://img.shields.io/badge/PRs-welcome-brightgreen)

The core logic and trait definitions for **Oxide Outbox**, a high-performance implementation of the **Transactional Outbox pattern** for Rust.

`outbox-core` provides the architectural backbone for reliable message delivery, allowing you to decouple database transactions from asynchronous event publishing with full type safety.

## Key Features

* **Trait-First Architecture**: Completely decoupled from specific storage or message brokers. Switch between Postgres, MySQL, Kafka, or RabbitMQ by implementing core traits.
* **Type-Safe Generic Payloads**: No more `serde_json::Value` overhead. Define your events using your own domain types: `Event<OrderCreated>`, `Event<UserSignedUp>`, etc.
* **Async Native**: Built from the ground up on `tokio` for maximum concurrency.
* **Flexible Idempotency**: Built-in support for multiple deduplication strategies (UUID v7, Provided tokens, or Custom logic).
* **Extensible Storage & Transport**: Interfaces designed to be implemented by specialized crates (like `outbox-postgres`).
 
---

## Core Concepts

### The `OutboxManager<S, P, PT>`
The central engine that orchestrates event discovery and dispatching. It is generic over your **Payload Type (PT)**, ensuring that every event handled by the manager respects your domain's type constraints.
* **`S` (Storage)**: Implements `OutboxStorage<PT>`. Responsible for DB operations (e.g., PostgreSQL, MySQL).
* **`P` (Publisher)**: Implements `Transport<PT>`. Responsible for sending events to brokers (e.g., Kafka, NATS).
* **`PT` (Payload Type)**: Your domain event type. Must implement `Debug + Clone + Serialize`.

### Type-Safe Events
As of v0.3.0, events use a `Payload<PT>` wrapper. This ensures zero unnecessary JSON roundtrips and provides compile-time safety from the moment you record an event until it is sent to the transport layer.

---

## Quick Start

Here is a complete, working example using `outbox-core` alongside `outbox-postgres` and a custom Tokio-channel based transport.

### 1. Define your Domain Event & Transport

First, define your event payload and implement the `Transport` trait to tell the outbox how to publish messages.

```rust
use outbox_core::prelude::*;
use serde::{Deserialize, Serialize};

// 1. Define your strongly-typed event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MyEvent {
    HiOutbox(String),
}

// 2. Implement Transport for your chosen broker (e.g., Tokio MPSC, Kafka, etc.)
struct Message(Event<MyEvent>);

#[derive(Clone)]
struct TokioEventPublisher(tokio::sync::mpsc::UnboundedSender<Message>);

#[async_trait::async_trait]
impl Transport<MyEvent> for TokioEventPublisher {
    async fn publish(&self, event: Event<MyEvent>) -> Result<(), OutboxError> {
        self.0
            .send(Message(event))
            .map_err(|e| OutboxError::InfrastructureError(e.to_string()))
    }
}
```
### 2. Wire up the Manager and Service
Set up your database pool, configure the outbox, and spawn the background manager task.

```rust
use outbox_postgres::{PostgresOutbox, PostgresWriter};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tracing::{Level, error, info};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_max_level(Level::TRACE).init();

    // 1. Setup Database Pool & Config
    let pool = PgPool::connect("postgresql://postgres:mysecretpassword@localhost:5432/outbox").await?;
    
    let config = Arc::new(OutboxConfig {
        batch_size: 100,
        retention_days: 1,
        gc_interval_secs: 10,
        poll_interval_secs: 100,
        lock_timeout_mins: 1,
        idempotency_strategy: IdempotencyStrategy::None,
    });

    // 2. Initialize Storage and Publisher
    let storage = PostgresOutbox::new(pool.clone(), config.clone());
    let writer = Arc::new(PostgresWriter(pool.clone()));
    
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<Message>();
    let publisher = TokioEventPublisher(sender);

    // 3. Initialize the Outbox Manager
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let outbox = OutboxManager::new(
        Arc::new(storage),
        Arc::new(publisher),
        config.clone(),
        shutdown_rx,
    );

    // 4. Spawn the manager in a background task
    tokio::spawn(async move {
        if let Err(e) = outbox.run().await {
            error!("Outbox critical error: {}", e);
        }
    });

    // (Optional) Dummy consumer to print received events
    tokio::spawn(async move {
        while let Some(msg) = receiver.recv().await {
            info!("Event received in Broker: type={:?}, payload={:?}", msg.0.event_type, msg.0.payload);
        }
    });

    // 5. Use the OutboxService to write events
    let service = OutboxService::new(writer, config.clone());

    info!("Inserting test event into DB...");
    service.add_event(
        "OrderCreated",
        MyEvent::HiOutbox("Hi!".into()),
        Some(String::from("r_token")), // Provided idempotency token
        || None,
    ).await?;

    info!("Testing deduplication...");
    if let Err(e) = service.add_event(
        "OrderCreated",
        MyEvent::HiOutbox("Hi!".into()),
        Some(String::from("r_token")), // Same token should trigger deduplication (if configured)
        || None,
    ).await {
        error!("Deduplication error: {}", e);
    }

    // Wait to let background tasks process
    tokio::time::sleep(Duration::from_mins(2)).await;

    // Graceful shutdown
    shutdown_tx.send(true)?;
    Ok(())
}
```

---

## Core Traits
To extend `outbox-core`, you can implement these primary traits:

| Trait               | Responsibility                                                                    |
|:--------------------|:----------------------------------------------------------------------------------|
| OutboxStorage<PT>   | Handles saving, locking, and deleting events in your DB (Postgres, Mongo, etc).   |
| Transport<PT>       | Defines how the event is published to the outside world (Kafka, RabbitMQ, HTTP).  |
| IdempotencyProvider | Checks if a request has already been processed to prevent duplicates.             |
