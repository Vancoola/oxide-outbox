use crate::error::OutboxError;
use crate::model::Event;
use crate::object::{EventType, Payload};
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

pub async fn add_event<W: OutboxWriter>(
    writer: &W,
    event_type: &str,
    payload: serde_json::Value,
) -> Result<(), OutboxError> {
    let event = Event::new(EventType::new(event_type), Payload::new(payload));
    writer.insert_event(event).await
}

pub mod prelude {
    pub use crate::storage::{OutboxStorage, OutboxWriter};
    pub use crate::publisher::Transport;

    pub use crate::processor::OutboxProcessor;
    pub use crate::manager::OutboxManager;
    pub use crate::config::OutboxConfig;

    pub use crate::model::{Event, EventStatus};
    pub use crate::object::{EventId, EventType, Payload};

    pub use crate::error::OutboxError;

    pub use crate::add_event;
}