use thiserror::Error;

#[derive(Debug, Error)]
pub enum OutboxError {
    #[error("Infrastructure error: {0}")]
    InfrastructureError(String),
    #[error("Duplicate error")]
    DuplicateEvent,
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Broker error: {0}")]
    BrokerError(String),
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
