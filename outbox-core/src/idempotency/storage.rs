//! Producer-side token reservation backend.
//!
//! [`IdempotencyStorageProvider`] is the narrow async contract an external
//! store (Redis, another SQL table, an in-memory set for tests) must satisfy
//! so [`OutboxService`](crate::service::OutboxService) can reject duplicate
//! events at insert time. [`NoIdempotency`] is the trivial "always succeeds"
//! implementation used when no such check is required.

use crate::error::OutboxError;
use crate::object::IdempotencyToken;
use async_trait::async_trait;

/// Async contract for reserving an idempotency token before an event is
/// written.
///
/// The implementation is expected to perform an atomic "claim if absent"
/// operation (e.g. `SET NX` in Redis) so concurrent producers cannot both
/// reserve the same token.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait IdempotencyStorageProvider {
    /// Attempts to reserve `token` as unused.
    ///
    /// Returns `true` if the token has been reserved by this call and the
    /// event may proceed, or `false` if the token was already taken — in
    /// which case [`OutboxService::add_event`](crate::service::OutboxService::add_event)
    /// surfaces a [`DuplicateEvent`](OutboxError::DuplicateEvent).
    ///
    /// # Errors
    ///
    /// Returns an [`OutboxError`] — typically
    /// [`InfrastructureError`](OutboxError::InfrastructureError) — when the
    /// reservation backend itself fails (connection lost, timeout, etc.).
    /// The error is propagated to the caller without inserting the event.
    async fn try_reserve(&self, token: &IdempotencyToken) -> Result<bool, OutboxError>;
}

/// No-op [`IdempotencyStorageProvider`] that accepts every token.
///
/// Used when no external reservation backend is configured — i.e. when
/// [`OutboxService::new`](crate::service::OutboxService::new) is called
/// instead of
/// [`with_idempotency`](crate::service::OutboxService::with_idempotency).
/// Tokens produced by the strategy are still stored on the event row, so
/// downstream consumers can deduplicate on their side if they need to.
pub struct NoIdempotency;
#[async_trait]
impl IdempotencyStorageProvider for NoIdempotency {
    /// Always returns `Ok(true)` — the token is considered "reserved"
    /// unconditionally.
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
