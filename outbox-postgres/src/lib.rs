use std::fmt::Debug;
use async_trait::async_trait;
use outbox_core::prelude::*;
use sqlx::postgres::PgListener;
use sqlx::types::uuid;
use sqlx::{Executor, PgPool, Postgres};
use std::sync::Arc;
use serde::{Serialize};
use tokio::sync::Mutex;
use tracing::debug;

#[derive(Clone)]
pub struct PostgresOutbox<P>
where
    P: Debug + Clone + Serialize + Send + Sync,
{
    inner: Arc<PostgresOutboxInner<P>>,
}

impl<P> PostgresOutbox<P>
where
    P: Debug + Clone + Serialize + Send + Sync,
{
    pub fn new(pool: PgPool, config: Arc<OutboxConfig<P>>) -> Self {
        Self {
            inner: Arc::new(PostgresOutboxInner {
                pool,
                config,
                listener: Mutex::new(None),
            }),
        }
    }
}

struct PostgresOutboxInner<P>
where
    P: Debug + Clone + Serialize + Send + Sync,
{
    pool: PgPool,
    config: Arc<OutboxConfig<P>>,
    listener: Mutex<Option<PgListener>>,
}

#[async_trait]
impl<P> OutboxStorage<P> for PostgresOutbox<P>
where
    P: Debug + Clone + Serialize + Send + Sync + for<'de> serde::Deserialize<'de> + Unpin + 'static,
{
    async fn fetch_next_to_process(&self, limit: u32) -> Result<Vec<Event<P>>, OutboxError> {
        let record = sqlx::query_as::<_, Event<P>>(
            r"
                UPDATE outbox_events
                SET status = 'Processing',
                    locked_until = NOW() + (INTERVAL '1 minute' * $2)
                WHERE id IN (
                    SELECT id
                    FROM outbox_events
                    WHERE status='Pending'
                        OR (status='Processing' AND locked_until < NOW())
                    ORDER BY locked_until ASC
                    LIMIT $1
                    FOR UPDATE SKIP LOCKED
                )
                RETURNING
                id,
                idempotency_token,
                event_type,
                payload,
                status,
                created_at,
                locked_until
            ",
        )
        .bind(i64::from(limit))
        .bind(self.inner.config.lock_timeout_mins)
        .fetch_all(&self.inner.pool)
        .await
        .map_err(|e| OutboxError::InfrastructureError(e.to_string()))?;
        Ok(record)
    }

    async fn updates_status(
        &self,
        ids: &[EventId],
        status: EventStatus,
    ) -> Result<(), OutboxError> {
        let raw_ids: Vec<uuid::Uuid> = ids.iter().map(EventId::as_uuid).collect();

        sqlx::query(r"UPDATE outbox_events SET status = $1 WHERE id = ANY($2)")
            .bind(status)
            .bind(&raw_ids)
            .execute(&self.inner.pool)
            .await
            .map_err(|e| OutboxError::InfrastructureError(e.to_string()))?;

        Ok(())
    }

    async fn delete_garbage(&self) -> Result<(), OutboxError> {
        let result = sqlx::query(
            r"
            DELETE
            FROM outbox_events
            WHERE id IN (
                SELECT id FROM outbox_events
                WHERE status='Sent'
                    AND created_at < now() - (INTERVAL '1 day' * $1)
                LIMIT 5000
            )",
        )
        .bind(self.inner.config.retention_days)
        .execute(&self.inner.pool)
        .await
        .map_err(|e| OutboxError::InfrastructureError(e.to_string()))?;
        debug!(
            "Garbage collector: deleted {} old messages",
            result.rows_affected()
        );
        Ok(())
    }

    async fn wait_for_notification(&self, channel: &str) -> Result<(), OutboxError> {
        let mut guard = self.inner.listener.lock().await;

        if guard.is_none() {
            let mut listener = PgListener::connect_with(&self.inner.pool)
                .await
                .map_err(|e| OutboxError::InfrastructureError(e.to_string()))?;

            listener
                .listen(channel)
                .await
                .map_err(|e| OutboxError::InfrastructureError(e.to_string()))?;

            *guard = Some(listener);
        }
        match guard
            .as_mut()
            .expect("Listener initialized above")
            .recv()
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                *guard = None;
                Err(OutboxError::InfrastructureError(e.to_string()))
            }
        }
    }
}

pub struct PostgresWriter<E>(pub E);

#[async_trait]
impl<E, P> OutboxWriter<P> for PostgresWriter<E>
where
    for<'c> &'c E: Executor<'c, Database = Postgres>,
    E: Send + Sync,
    P: Debug + Clone + Serialize + Send + Sync + 'static,
{
    async fn insert_event(&self, event: Event<P>) -> Result<(), OutboxError> {
        sqlx::query(
            r"
        INSERT INTO outbox_events (id, idempotency_token, event_type, payload, status, created_at, locked_until)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ",
        )
            .bind(event.id.as_uuid())
            .bind(event.idempotency_token)
            .bind(event.event_type.as_str())
            .bind(serde_json::to_value(&event.payload).map_err(|e| OutboxError::InfrastructureError(e.to_string()))?)
            .bind(event.status)
            .bind(event.created_at)
            .bind(event.locked_until)
            .execute(&self.0)
            .await
            .map_err(|e| OutboxError::InfrastructureError(e.to_string()))?;

        Ok(())
    }
}

//TODO: Create tests:
