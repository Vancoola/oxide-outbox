# Outbox Kafka

[![Crates.io](https://img.shields.io/crates/v/outbox-kafka.svg)](https://crates.io/crates/outbox-kafka)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

The Apache Kafka transport implementation for [`outbox-core`](https://crates.io/crates/outbox-core).

`outbox-kafka` provides a reliable bridge between your transactional database and Kafka topics, ensuring that every event saved in your outbox table is eventually published to a broker.

## Key Features

* **Async-First**: Built on `rdkafka`'s `FutureProducer` for high-throughput, non-blocking event publishing.
* **Automatic Metadata Propagation**: Maps Outbox event metadata (ID, Type, CreatedAt) directly to Kafka record headers.
* **Custom Partitioning**: Uses the `KafkaKeyExtractable` trait to allow you to define business-logic keys for Kafka partitioning.
* **At-Least-Once Delivery**: Works with `outbox-core` to ensure messages are only marked as "sent" after a successful Kafka ACK.

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
outbox-core = "0.3"
outbox-kafka = "0.1"
```

---

## Usage

### 1. **Implement KafkaKeyExtractable**
Your event payload must define how it should be keyed in Kafka (e.g., by `user_id` to ensure ordering).

```rust
use outbox_kafka::KafkaKeyExtractable;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderEvent {
    Created { order_id: String, customer_id: String },
    StatusUpdated { order_id: String, new_status: String },
    Cancelled { order_id: String, reason: String },
}

impl KafkaKeyExtractable for OrderEvent {
    fn kafka_key(&self) -> Vec<u8> {
        match self {
            Self::Created { order_id, .. } => order_id.as_bytes().to_vec(),
            Self::StatusUpdated { order_id, .. } => order_id.as_bytes().to_vec(),
            Self::Cancelled { order_id, .. } => order_id.as_bytes().to_vec(),
        }
    }
}
```

### 2. **Initialize the Transport**
```rust
use outbox_kafka::KafkaTransport;
use rdkafka::config::ClientConfig;

let mut kafka_config = ClientConfig::new();
kafka_config
    .set("bootstrap.servers", "localhost:9092")
    .set("message.timeout.ms", "5000");

// All variants of OrderEvent will be sent to this topic
let transport = KafkaTransport::new("orders.events", &kafka_config);
```

### 3. **Run with OutboxProcessor**
```rust
use outbox_core::prelude::*;

// Assuming you have a repository (e.g., Postgres)
let outbox = OutboxManager::new(
    Arc::new(storage),
    Arc::new(transport),
    config.clone(),
    shutdown_rx,
);

processor.run().await?;
```

---

## Header Mapping
The following headers are automatically attached to every Kafka message:

| Header Key        | Description                                         |
|:------------------|:----------------------------------------------------|
| event_id          | Id of the outbox record.                            |
| event_type        | String name of the event type (e.g., "OrderEvent"). |
| created_at        | Timestamp of when the event was recorded.           |
| idempotency_token | Optional token for downstream deduplication.        | 
