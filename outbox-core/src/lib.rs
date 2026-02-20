mod object;
mod storage;
mod error;
mod config;
mod model;
mod processor;
mod publisher;
mod gc;
mod manager;

pub mod prelude {
    pub use crate::storage::OutboxStorage;
    pub use crate::publisher::EventPublisher;

    pub use crate::processor::OutboxProcessor;
    pub use crate::manager::OutboxManager;
    pub use crate::config::OutboxConfig;

    pub use crate::model::{OutboxSlot, SlotStatus};
    pub use crate::object::{SlotId};

    pub use crate::error::OutboxError;
}