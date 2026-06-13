//! End-to-end order-service example for the oxide-outbox stack.
//!
//! Wires together:
//!
//! - **HTTP layer** ([`http`]) — Axum router with `POST /orders` and
//!   `GET /orders/{id}`. The POST handler invokes the transactional repository
//!   that writes an order row *and* an outbox event row in the same SQL
//!   transaction.
//! - **Outbox worker** ([`OutboxManager`](outbox_core::OutboxManager)) — runs
//!   in a background task, drains pending rows from `outbox_events`, and
//!   publishes them through the Kafka transport.
//! - **Demo consumer** ([`consumer`]) — subscribes to the Kafka topic and
//!   logs every event it sees, so you can confirm visually that the outbox
//!   actually published.
//!
//! Bring the stack up with `docker compose up -d`, then `cargo run -p
//! order-service`. The README walks through the end-to-end demo.

mod consumer;
mod domain;
mod http;
mod repository;
mod transport;

use anyhow::{Context, Result};
use outbox_core::{OutboxConfig, OutboxManagerBuilder, OutboxService};
use outbox_postgres::{PostgresOutbox, PostgresWriter};
use sqlx::PgPool;
use std::env;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::http::AppState;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let cfg = AppConfig::from_env();
    info!(?cfg, "starting order-service");

    // 1. Database pool + migrations.
    let pool = PgPool::connect(&cfg.database_url)
        .await
        .with_context(|| format!("connecting to {}", cfg.database_url))?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    info!("migrations applied");

    // 2. Outbox configuration. We override only what differs from defaults.
    let mut outbox_config = OutboxConfig::<domain::OrderEvent>::default();
    outbox_config.poll_interval_secs = 5; // fallback poll even if LISTEN drops
    outbox_config.retention_days = 1; // keep `Sent` rows around for a day
    outbox_config.gc_interval_secs = 60;
    let outbox_config = Arc::new(outbox_config);

    // 3. Pieces of the worker side: storage + transport.
    let storage = Arc::new(PostgresOutbox::new(pool.clone(), outbox_config.clone()));
    let transport = Arc::new(transport::build(&cfg.kafka_brokers, &cfg.kafka_topic)?);

    // 4. Shutdown channel shared by the worker, the consumer, and HTTP.
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // 5. Build and spawn the OutboxManager. From this point on, any row
    // committed into `outbox_events` will be picked up and published to Kafka
    // within a poll interval (or sooner via NOTIFY).
    let manager = OutboxManagerBuilder::new()
        .storage(storage)
        .publisher(transport)
        .config(outbox_config.clone())
        .shutdown_rx(shutdown_rx.clone())
        .build()?;

    let manager_task = tokio::spawn(async move {
        if let Err(e) = manager.run().await {
            error!(error = ?e, "outbox manager exited with error");
        }
    });

    // 6. Spawn the demo Kafka consumer so you can see events delivered.
    let consumer_brokers = cfg.kafka_brokers.clone();
    let consumer_topic = cfg.kafka_topic.clone();
    let consumer_shutdown = shutdown_rx.clone();
    let consumer_task = tokio::spawn(async move {
        if let Err(e) = consumer::run(
            &consumer_brokers,
            &consumer_topic,
            "order-service-demo-consumer",
            consumer_shutdown,
        )
        .await
        {
            error!(error = ?e, "consumer exited with error");
        }
    });

    // 7. Producer-side service used by HTTP handlers. Stateless writer —
    // every add_event call expects the caller to thread in its own
    // transaction handle (see repository::create_order).
    let outbox_service = Arc::new(OutboxService::new(
        Arc::new(PostgresWriter),
        outbox_config.clone(),
    ));

    // 8. HTTP server with graceful shutdown wired to ctrl-c.
    let state = AppState {
        pool: Arc::new(pool),
        outbox: outbox_service,
    };
    let app = http::router(state);

    let listener = tokio::net::TcpListener::bind(&cfg.http_bind)
        .await
        .with_context(|| format!("binding {}", cfg.http_bind))?;
    info!(bind = %cfg.http_bind, "http server listening");

    let mut shutdown_for_axum = shutdown_rx.clone();
    let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        // Either ctrl-c or someone else flipped the shutdown channel.
        tokio::select! {
            _ = tokio::signal::ctrl_c() => info!("ctrl-c received"),
            _ = shutdown_for_axum.changed() => info!("shutdown signal observed by http"),
        }
    });

    if let Err(e) = server.await {
        error!(error = ?e, "http server exited with error");
    }

    // Flip the shared shutdown channel so worker and consumer also wind down.
    let _ = shutdown_tx.send(true);

    let _ = tokio::join!(manager_task, consumer_task);
    info!("order-service stopped");

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("order_service=info,outbox_core=debug"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

#[derive(Debug, Clone)]
struct AppConfig {
    database_url: String,
    kafka_brokers: String,
    kafka_topic: String,
    http_bind: String,
}

impl AppConfig {
    fn from_env() -> Self {
        Self {
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgres://orders:orders@localhost:5432/orders".into()),
            kafka_brokers: env::var("KAFKA_BROKERS").unwrap_or_else(|_| "localhost:9092".into()),
            kafka_topic: env::var("KAFKA_TOPIC").unwrap_or_else(|_| "orders.events".into()),
            http_bind: env::var("HTTP_BIND").unwrap_or_else(|_| "0.0.0.0:3000".into()),
        }
    }
}
