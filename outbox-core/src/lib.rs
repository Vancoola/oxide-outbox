//! Transactional outbox primitives shared by every storage and transport
//! adapter in the workspace.
//!
//! The crate is split into two sides:
//!
//! - **Producer** — [`OutboxService`](prelude::OutboxService) persists new
//!   events into the outbox table, applying the configured
//!   [`IdempotencyStrategy`](prelude::IdempotencyStrategy) and optionally
//!   reserving the token through an external
//!   [`IdempotencyStorageProvider`](prelude::IdempotencyStorageProvider).
//! - **Worker** — [`OutboxManager`](prelude::OutboxManager), constructed via
//!   [`OutboxManagerBuilder`](prelude::OutboxManagerBuilder), drives the
//!   processing loop: it waits for notifications, fetches pending rows,
//!   publishes each through a [`Transport`](prelude::Transport), and runs a
//!   background garbage collector on the side.
//!
//! Storage and transport backends live in sibling crates (`outbox-postgres`,
//! `outbox-redis`, `outbox-kafka`). This crate only defines the traits they
//! must satisfy.
//!
//! # Features
//!
//! - `sqlx` — derives `sqlx::Type` / `sqlx::FromRow` on the domain types so
//!   storage adapters can map rows without manual conversion.
//! - `dlq` — enables the dead-letter-queue heap (see
//!   [`DlqHeap`](crate::dlq::storage::DlqHeap)); the worker then tracks
//!   per-event failure counts on every publish attempt.
//! - `metrics` — emits `outbox.events_total` and
//!   `outbox.publish_duration_seconds` via the `metrics` crate.
//! - `full` — turns on `sqlx`, `dlq`, and `metrics` together.
//!
//! # Getting started
//!
//! Import the common types via the [`prelude`] module:
//!
//! ```ignore
//! use outbox_core::prelude::*;
//! ```

mod builder;
mod config;
mod dlq;
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

// Root re-exports for explicit-import users. The [`prelude`] module exposes
// the same set as a glob-import shortcut for integrators who prefer it.
pub use crate::builder::OutboxManagerBuilder;
pub use crate::config::{IdempotencyDeriver, IdempotencyStrategy, OutboxConfig};
pub use crate::error::OutboxError;
pub use crate::idempotency::storage::IdempotencyStorageProvider;
pub use crate::manager::OutboxManager;
pub use crate::model::{Event, EventStatus};
pub use crate::object::{EventId, EventType, IdempotencyToken, Payload};
pub use crate::publisher::Transport;
pub use crate::service::OutboxService;
pub use crate::storage::{OutboxStorage, OutboxWriter};

#[cfg(feature = "dlq")]
pub use crate::dlq::model::DlqEntry;
#[cfg(feature = "dlq")]
pub use crate::dlq::storage::DlqHeap;

/// Curated set of re-exports for typical integrator code.
///
/// Importing `outbox_core::prelude::*` brings in the types you need to build
/// and run an outbox without having to reach into individual modules. Pulls
/// in both the public-facing APIs (service, manager, builder, config, errors)
/// and the traits a storage/transport adapter has to implement.
///
/// Every item in the prelude is also available as a direct re-export at the
/// crate root, so explicit-import users can write
/// `use outbox_core::{OutboxConfig, OutboxService};` instead of glob-importing.
pub mod prelude {
    pub use crate::{
        Event, EventId, EventStatus, EventType, IdempotencyDeriver,
        IdempotencyStorageProvider, IdempotencyStrategy, IdempotencyToken, OutboxConfig,
        OutboxError, OutboxManager, OutboxManagerBuilder, OutboxService, OutboxStorage,
        OutboxWriter, Payload, Transport,
    };

    pub use crate::processor::OutboxProcessor;

    #[cfg(feature = "dlq")]
    pub use crate::{DlqEntry, DlqHeap};
}
