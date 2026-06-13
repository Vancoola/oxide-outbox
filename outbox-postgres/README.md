# Outbox Postgres

[![Crates.io](https://img.shields.io/crates/v/outbox-postgres.svg)](https://crates.io/crates/outbox-postgres)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

The official PostgreSQL storage backend for [`outbox-core`](https://crates.io/crates/outbox-core).

This crate leverages `sqlx` to provide a robust, concurrency-safe, and real-time implementation of the Transactional Outbox pattern for PostgreSQL.

## Key Features

* **True transactional outbox** (v0.3.0): `PostgresWriter` is a stateless unit struct that receives the executor handle per call. Pass `&mut *tx` from your `sqlx::Transaction` into `OutboxService::add_event` and the outbox row commits in the **same** SQL transaction as your business write — no dual-write race possible.
* **Concurrency Safe**: Uses Postgres' `FOR UPDATE SKIP LOCKED` mechanism to safely allow multiple outbox workers to process events concurrently without stepping on each other's toes.
* **Instant Processing**: Native support for PostgreSQL `LISTEN` / `NOTIFY`. The `PostgresOutbox` listens for DB triggers to wake up and process events instantly, minimizing latency and falling back to polling only as a safety net.
* **Type-Safe JSONB**: Seamlessly serializes your strongly-typed generic domain events (`Event<P>`) into PostgreSQL `jsonb` columns.
* **Built-in Garbage Collection**: Automatically cleans up old, successfully processed messages to prevent your outbox table from growing indefinitely.
* **Dead Letter Queue (feature `dlq`)**: Provides `OutboxStorage::quarantine_events` — atomic move from the active outbox table into a dedicated `dead_letter_outbox_events` table in a single transaction.

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
outbox-core = "0.6"
outbox-postgres = { version = "0.3", features = ["dlq"] } # drop the feature if you don't need DLQ
sqlx = { version = "0.8.6", features = ["postgres", "runtime-tokio", "macros", "uuid", "time"] }
```

---

## Database Schema (Migrations)

Since this crate uses `sqlx`, you need to set up the `outbox_events` table in your database. You can add the following to your `migrations/` folder.

```postgresql
create type status as enum (
    'Pending',
    'Processing',
    'Sent'
    );

create table outbox_events
(
    id                uuid primary key     default gen_random_uuid(),
    idempotency_token text                 default null,
    event_type        text        not null,
    payload           jsonb       not null,
    status            status      not null default 'Pending',
    created_at        timestamptz not null default now(),
    locked_until      timestamptz not null default '-infinity'
);
CREATE INDEX idx_outbox_processing_queue
    ON outbox_events (locked_until ASC, status)
    WHERE status IN ('Pending', 'Processing');
CREATE UNIQUE INDEX idx_outbox_idempotency
    ON outbox_events (idempotency_token);

create or replace function notify_outbox_event() returns trigger as
$$
begin
    perform pg_notify('outbox_event', 'ping');
    return new;
end;
$$ language plpgsql;

create trigger outbox_events_notify_trigger
    after insert or update
    on outbox_events
    for each row
execute function notify_outbox_event();
```

### DLQ table (feature `dlq`)

The DLQ migration ships with the crate (see `migrations/20260425120000_dlq.sql`). It adds the `outbox_dead_letters` table that `quarantine_events` writes to. Rows arrive here exclusively via the DLQ reaper — workers never read from it, it is meant to be inspected (and cleaned up) by operators.

```postgresql
create table outbox_dead_letters
(
    id                uuid        primary key,
    idempotency_token text                 default null,
    event_type        text        not null,
    payload           jsonb       not null,
    original_status   status      not null,
    created_at        timestamptz not null,
    locked_until      timestamptz not null,
    failure_count     integer     not null,
    quarantined_at    timestamptz not null default now(),
    last_error        text                 default null
);

create index idx_outbox_dlq_quarantined_at
    on outbox_dead_letters (quarantined_at desc);

create index idx_outbox_dlq_event_type
    on outbox_dead_letters (event_type);
```

Apply it together with the base outbox migration if you enable the `dlq` feature.

---

## Usage

### Initialize Storage for the Manager
To run the background worker, initialize the `PostgresOutbox` with your `PgPool` and pass it to your `OutboxManager`.

```rust
use outbox_core::prelude::*;
use outbox_postgres::PostgresOutbox;
use sqlx::PgPool;
use std::sync::Arc;

// Assuming MyEvent is your strongly-typed domain enum
let pool = PgPool::connect("postgres://user:pass@localhost/db").await?;
let config = Arc::new(OutboxConfig::default()); // Configure as needed

let postgres_storage = PostgresOutbox::<MyEvent>::new(pool, config.clone());

// Pass postgres_storage to OutboxManagerBuilder::new().storage(...)
```

### Atomic write with `PostgresWriter`

`PostgresWriter` is a stateless unit struct: every call to
`OutboxService::add_event` takes the executor as a parameter. Borrow
`&mut *tx` from your held `sqlx::Transaction` and both the business `INSERT`
and the outbox row commit (or roll back) together — that is the actual outbox
guarantee.

```rust
use outbox_core::{OutboxConfig, OutboxService};
use outbox_postgres::PostgresWriter;
use sqlx::PgPool;
use std::sync::Arc;

let writer = Arc::new(PostgresWriter); // no pool baked in
let service = OutboxService::new(writer, config.clone());

// Inside an HTTP handler / command handler / domain action:
let mut tx = pool.begin().await?;

sqlx::query("INSERT INTO orders (id, customer_id, total_cents) VALUES ($1, $2, $3)")
    .bind(order_id)
    .bind(&customer_id)
    .bind(total_cents)
    .execute(&mut *tx)
    .await?;

service
    .add_event("OrderCreated", payload, None, &mut *tx)
    .await?;

tx.commit().await?; // single fsync; both rows become durable together.
```

If you don't need transactional control (a stand-alone publisher, batch
import, etc.), acquire a connection from the pool instead:

```rust
let mut conn = pool.acquire().await?;
service.add_event("BatchItem", payload, None, &mut conn).await?;
```

See [`example/order-service`](../example/order-service) for the full
HTTP + axum + outbox + Kafka demo.