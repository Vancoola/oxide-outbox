# Outbox Redis

[![Crates.io](https://img.shields.io/crates/v/outbox-redis.svg)](https://crates.io/crates/outbox-redis)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

The Redis-backed idempotency provider for [`outbox-core`](https://crates.io/crates/outbox-core).

`outbox-redis` ensures that your distributed system remains "effectively-once" by filtering out duplicate event requests at the edge before they ever touch your primary database.

## Key Features

* **Distributed Idempotency**: Uses Redis `SET NX EX` to provide a global, distributed lock for idempotency tokens across multiple application instances.
* **Hybrid L1/L2 Caching**: Optional support for **Moka** (in-memory) L1 caching to drastically reduce Redis roundtrips for high-frequency duplicate requests.
* **Atomic TTLs**: Automatic cleanup of tokens via Redis expiration, ensuring your memory footprint stays lean.
* **Fail-Safe Architecture**: Designed to be used with `outbox-core`'s idempotency strategies, allowing your system to remain resilient even if the cache layer is momentarily unreachable.

## Installation

Add this to your `Cargo.toml`:
```toml
[dependencies]
outbox-core = "0.3"
outbox-redis = { version = "0.1", features = ["moka"] } # Enable 'moka' for L1 local cache
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
To use Redis-backed idempotency, initialize the `RedisTokenProvider` and pass it to the `OutboxService`.

### 1. Setup the Provider
```rust
use outbox_redis::{RedisTokenProvider, config::RedisTokenConfig};
use std::time::Duration;

let redis_config = RedisTokenConfig {
    ttl: Duration::from_secs(3600), // 1 hour
    ..Default::default()
};

let redis_provider = RedisTokenProvider::new(
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