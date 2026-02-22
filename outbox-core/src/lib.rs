use crate::config::OutboxConfig;
use crate::error::OutboxError;
use crate::model::Event;
use crate::object::{EventType, IdempotencyToken, Payload};
use crate::storage::OutboxWriter;

mod config;
mod error;
mod gc;
mod idempotency;
mod manager;
mod model;
mod object;
mod processor;
mod publisher;
mod service;
mod storage;

/// # Warning
/// This function performs a raw insert into the database WITHOUT checking the
/// idempotency storage. It is kept only for backward compatibility.
///
/// For production use with deduplication, please migrate to [`service::OutboxService`].
///
/// # Errors
///
/// Returns [`OutboxError`] if the `writer` fails to insert the event into the database.
///
/// # Panics
///
/// Panics if the idempotency strategy is set to `Custom`, but `get_event` returns `None`.
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
    let i_token = config
        .idempotency_strategy
        .invoke(provided_token, get_event)
        .map(IdempotencyToken::new);

    let event = Event::new(EventType::new(event_type), Payload::new(payload), i_token);
    writer.insert_event(event).await
}

pub mod prelude {
    pub use crate::idempotency::storage::IdempotencyStorageProvider;
    pub use crate::publisher::Transport;
    pub use crate::storage::{OutboxStorage, OutboxWriter};

    pub use crate::config::{IdempotencyStrategy, OutboxConfig};
    pub use crate::manager::OutboxManager;
    pub use crate::processor::OutboxProcessor;
    pub use crate::service::OutboxService;

    pub use crate::model::{Event, EventStatus};
    pub use crate::object::{EventId, EventType, IdempotencyToken, Payload};

    pub use crate::error::OutboxError;

    #[allow(deprecated)]
    pub use crate::add_event;
}
