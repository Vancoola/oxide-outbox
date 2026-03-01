# Outbox Postgres

[![Crates.io](https://img.shields.io/crates/v/outbox-postgres.svg)](https://crates.io/crates/outbox-postgres)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

The official PostgreSQL storage backend for [`outbox-core`](https://crates.io/crates/outbox-core).

This crate leverages `sqlx` to provide a robust, concurrency-safe, and real-time implementation of the Transactional Outbox pattern for PostgreSQL.

## Key Features

* **ACID Guarantees**: Use `PostgresWriter` with an `sqlx::Transaction` to save your business data and outbox events in the exact same database transaction.
* **Concurrency Safe**: Uses Postgres' `FOR UPDATE SKIP LOCKED` mechanism to safely allow multiple outbox workers to process events concurrently without stepping on each other's toes.
* **Instant Processing**: Native support for PostgreSQL `LISTEN` / `NOTIFY`. The `PostgresOutbox` listens for DB triggers to wake up and process events instantly, minimizing latency and falling back to polling only as a safety net.
* **Type-Safe JSONB**: Seamlessly serializes your strongly-typed generic domain events (`Event<P>`) into PostgreSQL `jsonb` columns.
* **Built-in Garbage Collection**: Automatically cleans up old, successfully processed messages to prevent your outbox table from growing indefinitely.

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
outbox-core = "0.3"
outbox-postgres = "0.2"
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

// Pass postgres_storage to OutboxManager::new(...)
```