//! Domain types for the order-service example.
//!
//! Two layers live here:
//!
//! - [`Order`] — the persistent business entity, written to the `orders` table.
//! - [`OrderEvent`] — the event payload published through the outbox. Multiple
//!   variants give the consumer enough context to react without re-reading the
//!   database.
//!
//! [`OrderEvent`] implements [`KafkaKeyExtractable`] so that all events for the
//! same order end up on the same Kafka partition — preserving per-order
//! ordering on the broker side.

use outbox_kafka::KafkaKeyExtractable;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Business entity written to the `orders` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: Uuid,
    pub customer_id: String,
    pub total_cents: i64,
}

/// Event payload published through the outbox.
///
/// Tagged as `{ "type": "OrderCreated", ... }` in JSON for easy consumer-side
/// pattern matching.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OrderEvent {
    OrderCreated {
        order_id: Uuid,
        customer_id: String,
        total_cents: i64,
    },
}

impl OrderEvent {
    /// Returns the order identifier the event is about. Used as the Kafka
    /// partitioning key so all events for one order travel through the same
    /// partition and stay ordered.
    fn order_id(&self) -> Uuid {
        match self {
            OrderEvent::OrderCreated { order_id, .. } => *order_id,
        }
    }
}

impl KafkaKeyExtractable for OrderEvent {
    fn kafka_key(&self) -> Vec<u8> {
        self.order_id().as_bytes().to_vec()
    }
}
