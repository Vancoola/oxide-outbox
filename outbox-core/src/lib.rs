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
mod dlq;

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
}
