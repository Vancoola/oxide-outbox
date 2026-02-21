use async_trait::async_trait;
use outbox_core::prelude::*;
use sqlx::types::uuid;
use sqlx::{Executor, PgPool, Postgres};
use std::sync::Arc;
use sqlx::postgres::PgListener;
use tokio::sync::Mutex;
use tracing::debug;

#[derive(Clone)]
pub struct PostgresOutbox {
    inner: Arc<PostgresOutboxInner>,
}

impl PostgresOutbox {
    pub fn new(pool: PgPool, config: Arc<OutboxConfig>) -> Self {
        Self {
            inner: Arc::new(PostgresOutboxInner {
                pool,
                config,
                listener: Mutex::new(None),
            }),
        }
    }
}

struct PostgresOutboxInner {
    pool: PgPool,
    config: Arc<OutboxConfig>,
    listener: Mutex<Option<PgListener>>,
}

#[async_trait]
impl OutboxStorage for PostgresOutbox {
    async fn fetch_next_to_process(&self, limit: u32) -> Result<Vec<Event>, OutboxError> {
        let record = sqlx::query_as::<_, Event>(
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
        ids: &Vec<EventId>,
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
            let mut listener = sqlx::postgres::PgListener::connect_with(&self.inner.pool)
                .await
                .map_err(|e| OutboxError::InfrastructureError(e.to_string()))?;

            listener
                .listen(channel)
                .await
                .map_err(|e| OutboxError::InfrastructureError(e.to_string()))?;

            *guard = Some(listener);
        }
        match guard.as_mut().unwrap().recv().await {
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
impl<'a, E> OutboxWriter for PostgresWriter<E>
where
    for<'c> &'c E: Executor<'c, Database = Postgres>,
    E: Send + Sync,
{
    async fn insert_event(&self, event: Event) -> Result<(), OutboxError> {
        sqlx::query(
            r"
        INSERT INTO outbox_events (id, event_type, payload, status, created_at, locked_until)
        VALUES ($1, $2, $3, $4, $5, $6)
        ",
        )
        .bind(event.id.as_uuid())
        .bind(event.event_type.as_str())
        .bind(event.payload.as_json())
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
