# Outbox Redis

[![Crates.io](https://img.shields.io/crates/v/outbox-redis.svg)](https://crates.io/crates/outbox-redis)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

The Redis-backed provider for [`outbox-core`](https://crates.io/crates/outbox-core). Covers two concerns:

* **Idempotency** — filters duplicate event requests at the edge before they ever touch your primary database.
* **Dead Letter Queue heap** *(feature `dlq`)* — tracks per-event failure counts so the `outbox-core` reaper can quarantine chronically failing events.

## Key Features

* **Distributed Idempotency**: Uses Redis `SET NX EX` to provide a global, distributed lock for idempotency tokens across multiple application instances.
* **Hybrid L1/L2 Caching**: Optional support for **Moka** (in-memory) L1 caching to drastically reduce Redis roundtrips for high-frequency duplicate requests.
* **Atomic TTLs**: Automatic cleanup of tokens via Redis expiration, ensuring your memory footprint stays lean.
* **Fail-Safe Architecture**: Designed to be used with `outbox-core`'s idempotency strategies, allowing your system to remain resilient even if the cache layer is momentarily unreachable.
* **DLQ Heap on a single ZSet**: `record_failure` is a single `ZINCRBY`, `record_success` is a single `ZREM`, and `drain_exceeded` runs `ZRANGEBYSCORE` + `ZREMRANGEBYSCORE` inside one Lua script — concurrent reapers can't see the same id twice.

## Installation

Add this to your `Cargo.toml`:
```toml
[dependencies]
outbox-core = "0.4"
outbox-redis = { version = "0.1", features = ["moka", "dlq"] } # 'moka' = L1 local cache, 'dlq' = DLQ heap
```

---

## Configuration

You can customize the behavior of the provider using `RedisTokenConfig`:

| Field                | Default     | Description                                                |
|:---------------------|:------------|:-----------------------------------------------------------|
| ttl                  | 24 hours    | How long a token remains in Redis before being purged.     |
| key_prefix           | `"i_token"` | Prefix used for all Redis keys to avoid collisions.        |
| local_cache_capacity | `10,000`    | Max entries for the Moka L1 cache (requires moka feature). |

---

## Usage
To use Redis-backed idempotency, initialize the `RedisProvider` and pass it to the `OutboxService`.

> **Note:** As of v0.1.3 the type is called `RedisProvider` (was `RedisTokenProvider`). The same instance now serves both idempotency and DLQ heap roles, sharing one Redis connection.

### 1. Setup the Provider
```rust
use outbox_redis::{RedisProvider, config::RedisTokenConfig};
use std::time::Duration;

let redis_config = RedisTokenConfig {
    ttl: Duration::from_secs(3600), // 1 hour
    ..Default::default()
};

let redis_provider = RedisProvider::new(
    "redis://127.0.0.1:6379",
    redis_config
).await?;
```

### 2. Integrate with OutboxService
When creating your service, use `with_idempotency` to attach the Redis provider.
```rust
// Use IdempotencyStrategy::Provided to respect tokens passed by the client
let config = Arc::new(OutboxConfig {
    idempotency_strategy: IdempotencyStrategy::Provided,
    ..Default::default()
});

let service = OutboxService::with_idempotency(
    writer, 
    config, 
    Arc::new(redis_provider)
);

// This event will be recorded in the DB
service.add_event(
    "OrderCreated",
    MyEvent::HiOutbox("First request".into()),
    Some("unique_token_123".into()),
    || None,
).await?;

// This second call with the same token will return an DuplicateEvent 
// (or Ok(false) depending on internal logic), preventing a duplicate DB write.
let result = service.add_event(
    "OrderCreated",
    MyEvent::HiOutbox("Duplicate request".into()),
    Some("unique_token_123".into()),
    || None,
).await;
```

---

## Efficiency: The Moka L1 Cache
If you enable the `moka` feature, outbox-redis employs a Two-Tier Check:
1. **L1 (Local)**: The provider first checks a local, high-speed in-memory cache. If the token is found here, the request is rejected immediately without a network call.
2. **L2 (Remote)**: If not in L1, it performs an atomic SET NX in Redis. If successful, it populates the L1 cache for subsequent hits.

This is particularly effective at stopping "retry storms" where a client sends dozens of identical requests in a few milliseconds.

---

## DLQ Heap (feature `dlq`)

When the `dlq` feature is enabled, `RedisProvider` also implements `outbox_core::DlqHeap`. It stores failure counts in a single Redis sorted set: key `<key_prefix>:dlq`, member = event UUID, score = current failure count.

| Method            | Redis op                                                          |
|:------------------|:------------------------------------------------------------------|
| `record_failure`  | `ZINCRBY` — atomic +1, creates the entry if missing               |
| `record_success`  | `ZREM` — clears the counter on a successful publish               |
| `drain_exceeded`  | Lua script: `ZRANGEBYSCORE` + `ZREMRANGEBYSCORE` in one call      |

The Lua script matters: without it, two concurrent `DlqProcessor` instances could read the same id between `RANGE` and `REMRANGE` and try to quarantine it twice.

### Wiring it into `OutboxManager`

```rust
use outbox_core::prelude::*;
use outbox_redis::{RedisProvider, config::RedisTokenConfig};
use std::sync::Arc;

let redis = RedisProvider::new("redis://127.0.0.1:6379", RedisTokenConfig::default()).await?;
let heap: Arc<dyn DlqHeap> = Arc::new(redis);

let outbox = OutboxManagerBuilder::new()
    .storage(Arc::new(storage))
    .publisher(Arc::new(publisher))
    .config(config)
    .dlq_heap(heap)        // required when feature `dlq` is on
    .shutdown_rx(shutdown_rx)
    .build()?;
```

> **Note on `DlqEntry::last_error`:** the `DlqHeap` trait does not currently pass an error string into `record_failure`, so the Redis backend always returns `last_error: None`. This may change in a future minor version of `outbox-core`.