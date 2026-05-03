# Oxide Outbox 🦀

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
![PRs welocme](https://img.shields.io/badge/PRs-welcome-brightgreen)

A high-performance, flexible implementation of the **Transactional Outbox pattern** for Rust. Ensure reliable message delivery in your distributed systems by decoupling database transactions from event publishing.



## Key Features

* **Hybrid Event Discovery**: Combines real-time database notifications (e.g., Postgres `LISTEN/NOTIFY`) with fallback polling intervals to ensure zero lost events.
* **Trait-First Architecture**: Completely decoupled from specific storage or message brokers. Switch between Postgres, MySQL, Kafka, or RabbitMQ by implementing simple traits.
* **Built-in Garbage Collection**: Automatic cleanup of processed events to prevent table bloat.
* **Dead Letter Queue (DLQ)**: Chronically failing events are tracked, and after crossing a configurable threshold are moved to a dedicated quarantine store — they stop blocking healthy traffic without being silently lost.
* **Concurrency Safe**: Designed for horizontal scaling with support for row-level locking.
* **Async Native**: Built from the ground up on `tokio`.

---

## Project Structure

- `outbox-core`: Core logic, traits, `OutboxManagerBuilder` and the `OutboxService`.
- `outbox-postgres`: PostgreSQL implementation for event storage and the DLQ table using `sqlx`.
- `outbox-redis`: Redis-based idempotency provider and DLQ heap, with optional **Moka** L1 caching.
- `outbox-kafka`: Kafka transport implementation built on `rdkafka`.

---

## Distributed Idempotency & Safety

Oxide Outbox provides a robust mechanism to handle duplicate requests at the edge. By using `OutboxService`, you can ensure that an event is only recorded once, even if the client retries the request.

### Idempotency Strategies:
- **`Provided`**: Use a token supplied by the client (e.g., `X-Idempotency-Key` header).
- **`Uuid`**: Automatically generates a unique UUID v7 for every event.
- **`Custom`**: Define your own logic to generate tokens based on the event data.
- **`None`**: Skip deduplication checks (default).

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]

outbox-core = "0.4"
outbox-postgres = { version = "0.2", features = ["dlq"] } # If using Postgres + DLQ
outbox-redis = { version = "0.1", features = ["moka", "dlq"] } # Optional Redis deduplication + DLQ heap
outbox-kafka = "0.1" # Optional Kafka transport
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
- **Effectively-Once (Deduplication)**: By using the `RedisProvider`, you can achieve "effectively once" semantics. Even if a client or a producer retries an event, the idempotency layer filters out duplicates before they reach your database.
- **Fail-Open Idempotency**: If the Redis provider is unreachable, the system can be configured to allow the transaction to proceed, relying on the Database `UNIQUE` constraints as a final safety net.
- **Lazy Retry (Visibility Timeout)**: When an event is picked up, it is assigned a `lock_until` timestamp. If the worker doesn't mark it as completed within the `lock_timeout_mins` (e.g., due to a crash), the event automatically becomes visible again for the next polling cycle.
- **Transactional Integrity**: Since the outbox table lives in your business database, events are saved within the same ACID transaction as your business logic, ensuring they are never lost or orphaned.

---

## Dead Letter Queue (Optional, feature `dlq`)

Some events cannot be delivered no matter how many times you retry — broken consumer, malformed payload, downstream contract breakage. Instead of letting them block the active outbox forever, Oxide Outbox tracks failures and quarantines the bad ones.

### How it works

1. The worker calls `dlq_heap.record_failure(event_id)` after every failed publish, and `record_success` after a clean delivery.
2. A background `DlqProcessor` ticks on `dlq_interval_secs` and asks the heap for entries that crossed `dlq_threshold` failures.
3. Returned entries are atomically moved from the active outbox table to a dedicated **quarantine table** via `OutboxStorage::quarantine_events`.

### Wiring

* **Heap backend**: `outbox-redis` ships a Redis-backed `DlqHeap` (`ZSet` + atomic Lua drain). You can also implement the trait yourself for any other store.
* **Quarantine storage**: `outbox-postgres` provides the migration and `quarantine_events` impl out of the box.
* Use `OutboxManagerBuilder::dlq_heap(..)` to attach the heap to the manager.

A complete end-to-end demo lives in [`example/dlq-example`](./example/dlq-example).
