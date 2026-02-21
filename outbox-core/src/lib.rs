use crate::config::OutboxConfig;
use crate::error::OutboxError;
use crate::model::Event;
use crate::object::{EventType, IdempotencyToken, Payload};
use crate::storage::OutboxWriter;

mod object;
mod storage;
mod error;
mod config;
mod model;
mod processor;
mod publisher;
mod gc;
mod manager;
mod idempotency;
mod service;

/// # Warning
/// This function performs a raw insert into the database WITHOUT checking the
/// idempotency storage. It is kept only for backward compatibility.
///
/// For production use with deduplication, please migrate to [service::OutboxService].
#[deprecated(
    since = "0.2.0",
    note = "SECURITY WARNING: This standalone function ignores idempotency checks.
           Use `OutboxService::add_event` for safe, deduplicated event publishing.
           This function is no longer recommended and will be removed in 0.3.0."
)]
pub async fn add_event<W, F>(
    writer: &W,
    event_type: &str,
    payload: serde_json::Value,
    config: &OutboxConfig,
    provided_token: Option<String>,
    get_event: F,
) -> Result<(), OutboxError>
where
    W: OutboxWriter,
    F: FnOnce() -> Option<Event>,
{

    let i_token = match config.idempotency_strategy.invoke(provided_token, get_event) {
        Some(i) => Some(IdempotencyToken::new(i)),
        None => None,
    };

    let event = Event::new(EventType::new(event_type), Payload::new(payload), i_token);
    writer.insert_event(event).await
}

pub mod prelude {
    pub use crate::storage::{OutboxStorage, OutboxWriter};
    pub use crate::idempotency::storage::IdempotencyStorageProvider;
    pub use crate::publisher::Transport;

    pub use crate::processor::OutboxProcessor;
    pub use crate::manager::OutboxManager;
    pub use crate::config::{OutboxConfig, IdempotencyStrategy};
    pub use crate::service::OutboxService;

    pub use crate::model::{Event, EventStatus};
    pub use crate::object::{EventId, EventType, Payload, IdempotencyToken};

    pub use crate::error::OutboxError;

    pub use crate::add_event;
}