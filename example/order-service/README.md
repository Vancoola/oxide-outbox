# order-service

End-to-end example showing the transactional outbox pattern with the
`oxide-outbox` stack.

A small HTTP service accepts `POST /orders`, writes the order **and** the
corresponding `OrderCreated` outbox event in a **single SQL transaction**, and
a background worker drains the outbox table into Kafka. A demo consumer
subscribed to the same topic prints what it sees, so you can verify the
event-publication round-trip with your own eyes.

What this example demonstrates that the small fragmented examples don't:

- The producer side calls
  `service.add_event("OrderCreated", payload, None, &mut *tx)` **inside the
  same `sqlx::Transaction` as the business `INSERT`**. If either insert fails,
  both roll back — that is the actual outbox guarantee.
- The Kafka transport runs as an **idempotent producer**
  (`enable.idempotence=true`), preserving per-partition order across retries.
- The full path: HTTP request → DB transaction → outbox row → background worker
  → Kafka topic → demo consumer log line. End to end in ~150 lines of business
  code.

---

## Layout

```
example/order-service/
├── Cargo.toml
├── docker-compose.yml         # postgres + kafka (KRaft, no zookeeper)
├── .env.example               # copy to .env if you want to customize
├── migrations/
│   ├── 20260607000001_outbox.sql    # outbox_events schema (mirror of outbox-postgres init)
│   └── 20260607000002_orders.sql    # business table
└── src/
    ├── main.rs                # wiring: pool, manager, consumer, http
    ├── domain.rs              # Order, OrderEvent (with KafkaKeyExtractable)
    ├── repository.rs          # create_order: the transactional core
    ├── http.rs                # axum router
    ├── transport.rs           # KafkaTransport setup
    └── consumer.rs            # demo Kafka consumer
```

---

## Quickstart

```bash
# 1. Bring up Postgres + Kafka.
docker compose up -d

# 2. Wait a few seconds for kafka to be ready, then run the service.
cargo run -p order-service

# 3. In another terminal, create an order.
curl -X POST http://localhost:3000/orders \
  -H 'Content-Type: application/json' \
  -d '{"customer_id":"cust-42","total_cents":1599}'

# Response:
# {"id":"<uuid>","customer_id":"cust-42","total_cents":1599}

# 4. Watch the order-service logs — within ~5 seconds you should see two lines:
#    a) outbox worker publishes the event to kafka
#    b) the demo consumer receives it and decodes it back into OrderEvent

# 5. Read the order back.
curl http://localhost:3000/orders/<uuid-from-step-3>
```

### What you'll see in the logs

At the default log level (`order_service=info,outbox_core=info`):

```
INFO order_service::http: POST /orders -> 201, order + outbox event committed
    order_id=... customer_id="cust-42"
INFO order_service::consumer: consumer received header_event_type="OrderCreated"
    event=OrderCreated { order_id: ..., customer_id: "cust-42", total_cents: 1599 }
```

The first line is the HTTP handler after the transaction commits; the second is
the demo consumer reading the same event back off Kafka — end to end in about a
second.

Want to see the worker itself? Run with `RUST_LOG=order_service=info,outbox_core=debug`
and you'll also get the worker's per-batch line:

```
DEBUG outbox_core::manager: Processed 1 events
```

If you don't see the consumer line within ~10s, check `docker compose logs
kafka` — KRaft setup can take a moment on first boot, and the topic is
auto-created on the first publish (so a few `UnknownTopicOrPartition` errors on
a fresh broker are expected and stop after the first order).

---

## How the transactional write works

The whole point of the outbox pattern is in `repository::create_order`:

```rust
let mut tx = pool.begin().await?;

// 1) Business row.
sqlx::query("INSERT INTO orders ...").execute(&mut *tx).await?;

// 2) Outbox row — same transaction handle.
outbox.add_event("OrderCreated", event, None, &mut *tx).await?;

// 3) Single fsync; both rows become durable together.
tx.commit().await?;
```

`outbox-core` 0.6 lets `add_event` take an executor argument. Threading `&mut
*tx` through means the outbox row lands inside the same Postgres transaction
as the `orders` insert. Either both rows commit or neither does — no dual-write
divergence, no orphaned orders.

The Postgres trigger fires `NOTIFY outbox_event` when a row is *committed*, so
the worker wakes up as soon as the transaction lands. On startup or after a
missed notification, the worker also polls every `poll_interval_secs` seconds
as a safety net.

---

## Environment variables

All have safe defaults — copy `.env.example` to `.env` only if you want to
override them. Cargo doesn't auto-load `.env`; pass values inline or
`source .env` first if you want them in your shell.

| Variable | Default | Used for |
|---|---|---|
| `DATABASE_URL` | `postgres://orders:orders@localhost:5432/orders` | sqlx pool |
| `KAFKA_BROKERS` | `localhost:9092` | producer + consumer bootstrap |
| `KAFKA_TOPIC` | `orders.events` | both sides |
| `HTTP_BIND` | `0.0.0.0:3000` | axum listener |
| `RUST_LOG` | (unset → `order_service=info,outbox_core=info`) | tracing-subscriber filter |

---

## Tear-down

```bash
# Stop the service with ctrl-c — graceful shutdown drains the manager and consumer.

# Stop and wipe state.
docker compose down -v
```

---

## Wiring at a glance

```mermaid
flowchart TD
    client([POST /orders]) --> http["HTTP handler (axum)"]

    subgraph tx["single SQL transaction"]
        orders[("orders table")]
        outbox[("outbox_events table")]
    end

    http -->|"INSERT order"| orders
    http -->|"add_event(&mut *tx)"| outbox

    outbox -.->|"NOTIFY / poll"| manager["Outbox Manager"]
    manager -->|"publish (idempotent)"| topic{{"Kafka topic<br/>orders.events"}}
    topic --> consumer["Demo Consumer"]
    consumer -.->|"logs event"| logs([stdout])
```

Both writes inside the dashed box commit together — `orders` and
`outbox_events` are one transaction. The manager only sees the outbox row
*after* it is committed (the `NOTIFY` fires on commit), then publishes it to
Kafka, where the demo consumer reads it back.
