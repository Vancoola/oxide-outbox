//! Repository layer — the transactional heart of the outbox pattern.
//!
//! [`create_order`] is the only place in this example where the business state
//! (`orders` table) and the outbox event row are touched together. Both inserts
//! happen inside the **same** SQL transaction, so either both rows commit or
//! neither does. Without that single-transaction guarantee a crash between
//! "order created" and "outbox row written" would leak orders nobody knows
//! about — exactly the dual-write problem the outbox pattern solves.
//!
//! Note how the outbox insert goes through
//! [`OutboxService::add_event`](outbox_core::OutboxService::add_event) with
//! the held transaction passed in as `&mut *tx`. The same transaction handle
//! is reused for the business `INSERT INTO orders`; both happen under one
//! commit, atomically.

use crate::domain::{Order, OrderEvent};
use outbox_core::{NoIdempotency, OutboxError, OutboxService};
use outbox_postgres::PostgresWriter;
use sqlx::PgPool;
use uuid::Uuid;

/// Concrete service type the HTTP layer holds. Spelled out once here so the
/// state and handler signatures stay readable. This example does not wire an
/// external idempotency reservation, so the `S` parameter is the in-crate
/// no-op marker.
pub type OrderOutbox = OutboxService<PostgresWriter, NoIdempotency, OrderEvent>;

/// Error returned by repository operations.
///
/// Wraps both `sqlx::Error` (raw queries, transaction lifecycle) and
/// `OutboxError` (everything that bubbles out of `add_event`).
#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("outbox error: {0}")]
    Outbox(#[from] OutboxError),
}

/// Creates a new order and emits its `OrderCreated` event atomically.
///
/// The flow:
///
/// 1. Open a transaction on the pool.
/// 2. `INSERT INTO orders ...` through the transaction.
/// 3. Call [`OutboxService::add_event`] with `&mut *tx` — the outbox row is
///    written under the same transaction. The Postgres NOTIFY trigger fires
///    when the row is *committed*, so the worker wakes up only after step 4.
/// 4. `tx.commit()` — single fsync, both rows become durable together.
///
/// If any step fails the transaction rolls back, leaving neither the order nor
/// the event in the database.
///
/// # Errors
///
/// Returns [`RepoError::Database`] if the order insert or the commit fails,
/// or [`RepoError::Outbox`] if the outbox layer rejects the event (for
/// example a configured idempotency provider reporting a duplicate).
pub async fn create_order(
    pool: &PgPool,
    outbox: &OrderOutbox,
    customer_id: String,
    total_cents: i64,
) -> Result<Order, RepoError> {
    let order = Order {
        id: Uuid::new_v4(),
        customer_id,
        total_cents,
    };

    let event = OrderEvent::OrderCreated {
        order_id: order.id,
        customer_id: order.customer_id.clone(),
        total_cents: order.total_cents,
    };

    let mut tx = pool.begin().await?;

    // 1) Business write.
    sqlx::query("INSERT INTO orders (id, customer_id, total_cents) VALUES ($1, $2, $3)")
        .bind(order.id)
        .bind(&order.customer_id)
        .bind(order.total_cents)
        .execute(&mut *tx)
        .await?;

    // 2) Outbox write — same transaction handle. `add_event` constructs the
    // event row and inserts it through the writer using the supplied executor.
    outbox
        .add_event("OrderCreated", event, None, &mut *tx)
        .await?;

    // 3) Single fsync — both rows become durable together.
    tx.commit().await?;

    Ok(order)
}

/// Reads an order by id. Plain SELECT, no outbox interaction.
///
/// # Errors
///
/// Returns the underlying [`sqlx::Error`] on database failure.
pub async fn get_order(pool: &PgPool, id: Uuid) -> Result<Option<Order>, sqlx::Error> {
    let row = sqlx::query_as::<_, (Uuid, String, i64)>(
        "SELECT id, customer_id, total_cents FROM orders WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(id, customer_id, total_cents)| Order {
        id,
        customer_id,
        total_cents,
    }))
}
