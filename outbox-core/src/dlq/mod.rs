//! Dead-letter-queue subsystem.
//!
//! [`model`] holds [`DlqEntry`](model::DlqEntry), which is referenced by
//! [`OutboxStorage::quarantine_events`](crate::storage::OutboxStorage::quarantine_events)
//! and therefore always compiled. The active machinery —
//! [`storage::DlqHeap`] and [`processor::DlqProcessor`] — is only compiled
//! when the `dlq` feature is enabled.

pub mod model;

#[cfg(feature = "dlq")]
pub mod processor;
#[cfg(feature = "dlq")]
pub mod storage;
