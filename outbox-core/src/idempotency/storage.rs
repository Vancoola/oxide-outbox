use crate::error::OutboxError;
use crate::object::IdempotencyToken;
use async_trait::async_trait;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait IdempotencyStorageProvider {
    async fn try_reserve(&self, token: &IdempotencyToken) -> Result<bool, OutboxError>;
}

pub struct NoIdempotency;
#[async_trait]
impl IdempotencyStorageProvider for NoIdempotency {
    async fn try_reserve(&self, _token: &IdempotencyToken) -> Result<bool, OutboxError> {
        Ok(true)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::non_empty("any-token")]
    #[case::empty("")]
    #[case::unicode("token-🦀")]
    #[tokio::test]
    async fn no_idempotency_returns_ok_true_for_any_token(#[case] raw: &str) {
        let provider = NoIdempotency;
        let token = IdempotencyToken::new(raw.to_string());
        assert!(provider.try_reserve(&token).await.unwrap());
    }

    #[rstest]
    #[tokio::test]
    async fn no_idempotency_returns_ok_true_across_repeated_calls_with_same_token() {
        let provider = NoIdempotency;
        let token = IdempotencyToken::new("same".into());
        assert!(provider.try_reserve(&token).await.unwrap());
        assert!(provider.try_reserve(&token).await.unwrap());
    }
}
