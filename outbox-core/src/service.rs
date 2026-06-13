//! Producer-side entry point: writes new events into the outbox.
//!
//! [`OutboxService`] is what application code calls when a domain action
//! needs to emit an event. It applies the configured
//! [`IdempotencyStrategy`](crate::config::IdempotencyStrategy) to produce (or
//! accept) a token, optionally reserves that token through an external
//! [`IdempotencyStorageProvider`] to reject duplicates, and then persists the
//! event via an [`OutboxWriter`]. The worker side
//! ([`OutboxManager`](crate::manager::OutboxManager)) picks it up later.

use crate::error::OutboxError;
use crate::idempotency::storage::NoIdempotency;
use crate::model::Event;
use crate::object::{EventType, IdempotencyToken, Payload};
use crate::prelude::{IdempotencyStorageProvider, OutboxConfig};
use crate::storage::OutboxWriter;
use serde::Serialize;
use std::fmt::Debug;
use std::sync::Arc;

/// Producer-side facade for writing outbox events.
///
/// The service is generic over:
///
/// - `W` — [`OutboxWriter`] implementation (persists the event row)
/// - `S` — [`IdempotencyStorageProvider`] implementation used to reserve
///   tokens; set to [`NoIdempotency`] when no external reservation is needed
/// - `P` — the user's domain event payload type (`Debug + Clone + Serialize`)
///
/// Construct with [`new`](Self::new) when events should be written without an
/// external idempotency check, or with
/// [`with_idempotency`](Self::with_idempotency) to wire a reservation backend.
pub struct OutboxService<W, S, P>
where
    P: Debug + Clone + Serialize + Send + Sync,
{
    writer: Arc<W>,
    config: Arc<OutboxConfig<P>>,
    idempotency_storage: Option<Arc<S>>,
}

impl<W, P> OutboxService<W, NoIdempotency, P>
where
    W: OutboxWriter<P> + Send + Sync + 'static,
    P: Debug + Clone + Serialize + Send + Sync,
{
    /// Creates a service without any external idempotency reservation.
    ///
    /// Tokens are still produced according to `config.idempotency_strategy`
    /// and written alongside the event, so downstream consumers can deduplicate
    /// on their side, but no pre-insert uniqueness check is performed here.
    /// Use [`with_idempotency`](OutboxService::with_idempotency) to attach a
    /// reservation store (for example Redis) when at-producer deduplication
    /// is required.
    pub fn new(writer: Arc<W>, config: Arc<OutboxConfig<P>>) -> Self {
        Self {
            writer,
            config,
            idempotency_storage: None,
        }
    }
}

impl<W, S, P> OutboxService<W, S, P>
where
    W: OutboxWriter<P> + Send + Sync + 'static,
    S: IdempotencyStorageProvider + Send + Sync + 'static,
    P: Debug + Clone + Serialize + Send + Sync,
{
    /// Creates a service wired to an external idempotency reservation store.
    ///
    /// Before inserting the event, the service calls
    /// [`IdempotencyStorageProvider::try_reserve`] with the token produced by
    /// the configured strategy. If the reservation returns `false`, the insert
    /// is skipped and [`OutboxError::DuplicateEvent`] is propagated to the
    /// caller.
    pub fn with_idempotency(
        writer: Arc<W>,
        config: Arc<OutboxConfig<P>>,
        idempotency_storage: Arc<S>,
    ) -> Self {
        Self {
            writer,
            idempotency_storage: Some(idempotency_storage),
            config,
        }
    }
    /// Adds a new event to the outbox storage with idempotency checks.
    ///
    /// The token is derived from
    /// [`IdempotencyStrategy`](crate::config::IdempotencyStrategy) on the
    /// configured [`OutboxConfig`]:
    ///
    /// - `Provided` — uses `provided_token` as-is (`None` skips reservation).
    /// - `Uuid` — generates a fresh UUID v7; `provided_token` is ignored.
    /// - `Custom(callback)` — passes the event about to be written to the
    ///   callback and uses the returned `String` as the token.
    /// - `None` — no token is produced and reservation is skipped.
    ///
    /// If an idempotency provider is configured and a token was produced, it
    /// will first attempt to reserve the token to prevent duplicate processing.
    ///
    /// # Errors
    ///
    /// Returns [`OutboxError::DuplicateEvent`] if the event token has already
    /// been used. Returns any [`OutboxError`] variant propagated from the
    /// reservation call or from the writer's `insert_event`.
    ///
    /// # Transactional usage
    ///
    /// The `executor` argument is forwarded to the configured
    /// [`OutboxWriter`](crate::storage::OutboxWriter) — for sqlx-based backends
    /// this is typically `&mut *tx` borrowed from a held `sqlx::Transaction`.
    /// Threading the transaction through the call is what makes the outbox
    /// write **atomic with the caller's business write**: both rows commit or
    /// roll back together.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use std::sync::Arc;
    /// use outbox_core::prelude::*;
    ///
    /// # async fn emit(
    /// #     service: OutboxService<impl OutboxWriter<MyEvent> + Send + Sync + 'static,
    /// #                            NoIdempotency,
    /// #                            MyEvent>,
    /// #     payload: MyEvent,
    /// #     mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
    /// # ) -> Result<(), OutboxError>
    /// # where MyEvent: std::fmt::Debug + Clone + serde::Serialize + Send + Sync,
    /// # {
    /// service.add_event("order.created", payload, None, &mut *tx).await?;
    /// # Ok(()) }
    /// ```
    pub async fn add_event(
        &self,
        event_type: &str,
        payload: P,
        provided_token: Option<String>,
        executor: W::Executor<'_>,
    ) -> Result<(), OutboxError>
    where
        P: Debug + Clone + Serialize + Send + Sync,
    {
        let mut event = Event::new(EventType::new(event_type), Payload::new(payload), None);

        let i_token = self
            .config
            .idempotency_strategy
            .invoke(provided_token, &event)
            .map(IdempotencyToken::new);

        if let Some(i_provider) = &self.idempotency_storage
            && let Some(ref token) = i_token
            && !i_provider.try_reserve(token).await?
        {
            return Err(OutboxError::DuplicateEvent);
        }

        event.idempotency_token = i_token;
        self.writer.insert_event(event, executor).await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::config::IdempotencyStrategy;
    use crate::idempotency::storage::MockIdempotencyStorageProvider;
    use async_trait::async_trait;
    use rstest::rstest;
    use serde::Deserialize;
    use std::sync::Mutex;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct TestPayload {
        kind: String,
    }

    fn payload() -> TestPayload {
        TestPayload { kind: "k".into() }
    }

    fn config_with(strategy: IdempotencyStrategy<TestPayload>) -> Arc<OutboxConfig<TestPayload>> {
        Arc::new(OutboxConfig {
            batch_size: 100,
            retention_days: 7,
            gc_interval_secs: 3600,
            poll_interval_secs: 10,
            lock_timeout_mins: 5,
            idempotency_strategy: strategy,
            dlq_threshold: 10,
            dlq_interval_secs: 1,
        })
    }

    /// Test double for `OutboxWriter`: captures every event handed to
    /// `insert_event` and (optionally) returns a pre-configured error instead
    /// of succeeding. Uses `()` as the executor handle — tests don't need a
    /// real transaction.
    #[derive(Default)]
    struct CapturingWriter {
        captured: Mutex<Vec<Event<TestPayload>>>,
        error: Option<OutboxError>,
    }

    impl CapturingWriter {
        fn ok() -> Self {
            Self::default()
        }

        fn failing(error: OutboxError) -> Self {
            Self {
                captured: Mutex::new(Vec::new()),
                error: Some(error),
            }
        }

        fn events(&self) -> Vec<Event<TestPayload>> {
            self.captured.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl OutboxWriter<TestPayload> for CapturingWriter {
        type Executor<'a> = ();

        async fn insert_event<'a>(
            &self,
            event: Event<TestPayload>,
            _: (),
        ) -> Result<(), OutboxError>
        where
            'a: 'async_trait,
        {
            if let Some(err) = &self.error {
                return Err(err.clone());
            }
            self.captured.lock().unwrap().push(event);
            Ok(())
        }
    }

    #[rstest]
    #[tokio::test]
    async fn none_strategy_without_idempotency_storage_inserts_event_without_token() {
        let writer = Arc::new(CapturingWriter::ok());
        let service = OutboxService::new(writer.clone(), config_with(IdempotencyStrategy::None));
        let result = service.add_event("t", payload(), None, ()).await;
        assert!(result.is_ok());

        let events = writer.events();
        assert_eq!(events.len(), 1);
        assert!(events[0].idempotency_token.is_none());
        assert_eq!(events[0].event_type.as_str(), "t");
    }

    #[rstest]
    #[tokio::test]
    async fn uuid_strategy_without_idempotency_storage_inserts_event_with_generated_token() {
        let writer = Arc::new(CapturingWriter::ok());
        let service = OutboxService::new(writer.clone(), config_with(IdempotencyStrategy::Uuid));
        let result = service.add_event("t", payload(), None, ()).await;
        assert!(result.is_ok());

        let events = writer.events();
        assert_eq!(events.len(), 1);
        let token = events[0]
            .idempotency_token
            .as_ref()
            .expect("Uuid strategy must produce a token");
        assert!(!token.as_str().is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn uuid_strategy_with_storage_reserves_same_token_as_inserted() {
        let writer = Arc::new(CapturingWriter::ok());
        let mut idem = MockIdempotencyStorageProvider::new();

        // Capture the reserved token so we can compare it to what reached the writer.
        let reserved: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let reserved_r = reserved.clone();

        idem.expect_try_reserve().times(1).returning(move |tok| {
            *reserved_r.lock().unwrap() = Some(tok.as_str().to_owned());
            Ok(true)
        });

        let service = OutboxService::with_idempotency(
            writer.clone(),
            config_with(IdempotencyStrategy::Uuid),
            Arc::new(idem),
        );
        let result = service.add_event("t", payload(), None, ()).await;
        assert!(result.is_ok());

        let events = writer.events();
        assert_eq!(events.len(), 1);
        let inserted = events[0]
            .idempotency_token
            .as_ref()
            .map(|t| t.as_str().to_owned());
        let captured = reserved.lock().unwrap().clone();
        assert_eq!(inserted, captured);
    }

    #[rstest]
    #[tokio::test]
    async fn provided_some_passes_user_token_to_reserve_and_insert() {
        let writer = Arc::new(CapturingWriter::ok());
        let mut idem = MockIdempotencyStorageProvider::new();

        idem.expect_try_reserve()
            .withf(|t| t.as_str() == "user-tok")
            .times(1)
            .returning(|_| Ok(true));

        let service = OutboxService::with_idempotency(
            writer.clone(),
            config_with(IdempotencyStrategy::Provided),
            Arc::new(idem),
        );
        let result = service
            .add_event("t", payload(), Some("user-tok".to_string()), ())
            .await;
        assert!(result.is_ok());

        let events = writer.events();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].idempotency_token.as_ref().map(|t| t.as_str()),
            Some("user-tok")
        );
    }

    #[rstest]
    #[tokio::test]
    async fn provided_none_skips_reserve_and_inserts_without_token() {
        let writer = Arc::new(CapturingWriter::ok());
        let mut idem = MockIdempotencyStorageProvider::new();
        idem.expect_try_reserve().times(0);

        let service = OutboxService::with_idempotency(
            writer.clone(),
            config_with(IdempotencyStrategy::Provided),
            Arc::new(idem),
        );
        let result = service.add_event("t", payload(), None, ()).await;
        assert!(result.is_ok());

        let events = writer.events();
        assert_eq!(events.len(), 1);
        assert!(events[0].idempotency_token.is_none());
    }

    #[rstest]
    #[tokio::test]
    async fn custom_strategy_uses_extractor_closure_for_token() {
        fn derive(event: &Event<TestPayload>) -> String {
            format!("derived:{}", event.payload.as_value().kind)
        }

        let writer = Arc::new(CapturingWriter::ok());
        let mut idem = MockIdempotencyStorageProvider::new();

        idem.expect_try_reserve()
            .withf(|t| t.as_str() == "derived:k")
            .times(1)
            .returning(|_| Ok(true));

        let service = OutboxService::with_idempotency(
            writer.clone(),
            config_with(IdempotencyStrategy::Custom(Arc::new(derive))),
            Arc::new(idem),
        );
        let result = service.add_event("t", payload(), None, ()).await;
        assert!(result.is_ok());

        let events = writer.events();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].idempotency_token.as_ref().map(|t| t.as_str()),
            Some("derived:k")
        );
    }

    #[rstest]
    #[tokio::test]
    async fn duplicate_when_reserve_returns_false_and_insert_is_not_called() {
        let writer = Arc::new(CapturingWriter::ok());
        let mut idem = MockIdempotencyStorageProvider::new();

        idem.expect_try_reserve().times(1).returning(|_| Ok(false));

        let service = OutboxService::with_idempotency(
            writer.clone(),
            config_with(IdempotencyStrategy::Provided),
            Arc::new(idem),
        );
        let result = service
            .add_event("t", payload(), Some("dup".into()), ())
            .await;
        assert!(matches!(result, Err(OutboxError::DuplicateEvent)));
        assert!(writer.events().is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn reserve_error_propagates_and_insert_is_not_called() {
        let writer = Arc::new(CapturingWriter::ok());
        let mut idem = MockIdempotencyStorageProvider::new();

        idem.expect_try_reserve()
            .times(1)
            .returning(|_| Err(OutboxError::InfrastructureError("redis down".into())));

        let service = OutboxService::with_idempotency(
            writer.clone(),
            config_with(IdempotencyStrategy::Uuid),
            Arc::new(idem),
        );
        let result = service.add_event("t", payload(), None, ()).await;
        assert!(matches!(result, Err(OutboxError::InfrastructureError(_))));
        assert!(writer.events().is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn insert_error_propagates_after_successful_reserve() {
        let writer = Arc::new(CapturingWriter::failing(OutboxError::DatabaseError(
            "pk conflict".into(),
        )));
        let mut idem = MockIdempotencyStorageProvider::new();

        idem.expect_try_reserve().times(1).returning(|_| Ok(true));

        let service = OutboxService::with_idempotency(
            writer.clone(),
            config_with(IdempotencyStrategy::Uuid),
            Arc::new(idem),
        );
        let result = service.add_event("t", payload(), None, ()).await;
        assert!(matches!(result, Err(OutboxError::DatabaseError(_))));
        assert!(writer.events().is_empty());
    }
}
