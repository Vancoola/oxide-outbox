//! Shared error type for every fallible operation in the crate.
//!
//! All public APIs return `Result<T, `[`OutboxError`]`>`. The error is a flat
//! enum rather than a `Box<dyn Error>` so callers can reason about categories
//! — infrastructure, database, broker, configuration — without downcasting.
//! Display strings are deliberate and tested; they are part of the public
//! contract because they surface directly in logs and propagate to
//! integrators.

use thiserror::Error;

/// Error categories produced by the outbox crate.
///
/// The variants correspond to the layer that originated the failure. When a
/// caller needs to decide whether to retry, the variant is usually enough —
/// most transient conditions show up as [`InfrastructureError`](Self::InfrastructureError),
/// [`DatabaseError`](Self::DatabaseError), or
/// [`BrokerError`](Self::BrokerError); configuration and deduplication issues
/// are terminal.
#[derive(Debug, Error)]
pub enum OutboxError {
    /// Failure from surrounding infrastructure that is not the primary
    /// database or broker (Redis, a notification channel, DNS, etc.). Usually
    /// transient.
    #[error("Infrastructure error: {0}")]
    InfrastructureError(String),
    /// The event's idempotency token has already been reserved by a prior
    /// call. Returned by [`OutboxService::add_event`](crate::service::OutboxService::add_event)
    /// when [`IdempotencyStorageProvider::try_reserve`](crate::idempotency::storage::IdempotencyStorageProvider::try_reserve)
    /// reports the token as taken. Not retryable.
    #[error("Duplicate error")]
    DuplicateEvent,
    /// Failure from the primary event store (for example SQL errors, pool
    /// exhaustion, serialization conflicts). Often retryable by the caller.
    #[error("Database error: {0}")]
    DatabaseError(String),
    /// Failure from the message transport (Kafka, Redis Streams, etc.) when
    /// publishing an event.
    #[error("Broker error: {0}")]
    BrokerError(String),
    /// Invalid or incomplete configuration discovered at wiring time —
    /// typically raised by [`OutboxManagerBuilder::build`](crate::builder::OutboxManagerBuilder::build)
    /// when a required collaborator is missing. Not retryable.
    #[error("Config error: {0}")]
    ConfigError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    fn display_infrastructure_error_includes_inner_message() {
        let e = OutboxError::InfrastructureError("redis down".into());
        assert_eq!(format!("{e}"), "Infrastructure error: redis down");
    }

    #[rstest]
    fn display_duplicate_event_is_static_string() {
        let e = OutboxError::DuplicateEvent;
        assert_eq!(format!("{e}"), "Duplicate error");
    }

    #[rstest]
    fn display_database_error_includes_inner_message() {
        let e = OutboxError::DatabaseError("pk conflict".into());
        assert_eq!(format!("{e}"), "Database error: pk conflict");
    }

    #[rstest]
    fn display_broker_error_includes_inner_message() {
        let e = OutboxError::BrokerError("kafka timeout".into());
        assert_eq!(format!("{e}"), "Broker error: kafka timeout");
    }

    #[rstest]
    fn display_config_error_includes_inner_message() {
        let e = OutboxError::ConfigError("missing field".into());
        assert_eq!(format!("{e}"), "Config error: missing field");
    }

    #[rstest]
    fn std_error_trait_is_implemented() {
        fn takes_error<E: std::error::Error + Send + Sync + 'static>(_: E) {}
        takes_error(OutboxError::DuplicateEvent);
    }
}
