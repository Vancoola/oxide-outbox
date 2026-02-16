use thiserror::Error;

#[derive(Debug, Error)]
pub enum OutboxError {
    #[error("Infrastructure error: {0}")]
    InfrastructureError(String),
}