//! Thin HTTP layer — Axum router with two endpoints.
//!
//! - `POST /orders` accepts a JSON body, calls
//!   [`repository::create_order`](crate::repository::create_order) (which does
//!   the atomic order + outbox insert), and returns the new order id.
//! - `GET /orders/{id}` reads an order by id for verification.
//!
//! The handlers do not talk to Kafka directly. Publishing is the outbox
//! worker's job — the HTTP layer's only obligation is to land the event row
//! in `outbox_events` in the same transaction as the order.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::repository::{self, OrderOutbox};

#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<PgPool>,
    pub outbox: Arc<OrderOutbox>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/orders", post(create_order_handler))
        .route("/orders/{id}", get(get_order_handler))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
pub struct CreateOrderRequest {
    pub customer_id: String,
    pub total_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct OrderResponse {
    pub id: Uuid,
    pub customer_id: String,
    pub total_cents: i64,
}

async fn create_order_handler(
    State(state): State<AppState>,
    Json(req): Json<CreateOrderRequest>,
) -> Result<(StatusCode, Json<OrderResponse>), (StatusCode, String)> {
    if req.total_cents < 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "total_cents must be non-negative".to_string(),
        ));
    }

    let order =
        repository::create_order(&state.pool, &state.outbox, req.customer_id, req.total_cents)
            .await
            .map_err(internal_error)?;

    tracing::info!(
        order_id = %order.id,
        customer_id = %order.customer_id,
        "POST /orders -> 201, order + outbox event committed"
    );

    Ok((
        StatusCode::CREATED,
        Json(OrderResponse {
            id: order.id,
            customer_id: order.customer_id,
            total_cents: order.total_cents,
        }),
    ))
}

async fn get_order_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<OrderResponse>, (StatusCode, String)> {
    let order = repository::get_order(&state.pool, id)
        .await
        .map_err(internal_error)?
        .ok_or((StatusCode::NOT_FOUND, "order not found".to_string()))?;

    Ok(Json(OrderResponse {
        id: order.id,
        customer_id: order.customer_id,
        total_cents: order.total_cents,
    }))
}

fn internal_error<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
