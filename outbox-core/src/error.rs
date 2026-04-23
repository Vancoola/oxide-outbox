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
