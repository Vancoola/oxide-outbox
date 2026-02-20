use async_trait::async_trait;
use outbox_core::prelude::*;
use sqlx::types::uuid;
use sqlx::{Executor, PgPool, Postgres};
use std::sync::Arc;
use tracing::debug;

#[derive(Clone)]
pub struct PostgresOutbox {
    pool: PgPool,
    config: Arc<OutboxConfig>,
}

impl PostgresOutbox {
    pub fn new(pool: PgPool, config: Arc<OutboxConfig>) -> Self {
        Self { pool, config }
    }
}

#[async_trait]
impl OutboxStorage for PostgresOutbox {
    async fn fetch_next_to_process(&self, limit: u32) -> Result<Vec<OutboxSlot>, OutboxError> {
        let record = sqlx::query!(
            r#"
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
                event_type,
                payload,
                status AS "status: SlotStatus",
                created_at,
                locked_until
            "#,
            i64::from(limit),
            self.config.lock_timeout_mins as i64,
        )
            .fetch_all(&self.pool)
            .await
            .map_err(|e| OutboxError::InfrastructureError(e.to_string()))?;
        let r = record
            .iter()
            .map(|o| {
                OutboxSlot::load(
                    o.id,
                    &o.event_type,
                    &o.payload,
                    o.created_at,
                    o.locked_until,
                    &o.status,
                )
            })
            .collect();
        Ok(r)
    }

    async fn updates_status(
        &self,
        ids: &Vec<SlotId>,
        status: SlotStatus,
    ) -> Result<(), OutboxError> {
        let raw_ids: Vec<uuid::Uuid> = ids
            .iter()
            .map(outbox_core::prelude::SlotId::as_uuid)
            .collect();
        sqlx::query!(
            r#"UPDATE outbox_events SET status = $1 WHERE id = ANY($2)"#,
            status as SlotStatus,
            &raw_ids as &[uuid::Uuid]
        )
            .execute(&self.pool)
            .await
            .map_err(|e| OutboxError::InfrastructureError(e.to_string()))?;

        Ok(())
    }

    async fn delete_garbage(&self) -> Result<(), OutboxError> {
        let result = sqlx::query!(
            r#"
            DELETE
            FROM outbox_events
            WHERE id IN (
                SELECT id FROM outbox_events
                WHERE status='Sent'
                    AND created_at < now() - (INTERVAL '1 day' * $1)
                LIMIT 5000
            )"#,
            self.config.retention_days as i64,
        )
            .execute(&self.pool)
            .await
            .map_err(|e| OutboxError::InfrastructureError(e.to_string()))?;
        debug!(
            "Garbage collector: deleted {} old messages",
            result.rows_affected()
        );
        Ok(())
    }

    async fn wait_for_notification(&self, channel: &str) -> Result<(), OutboxError> {
        let mut listener = sqlx::postgres::PgListener::connect_with(&self.pool)
            .await
            .map_err(|e| OutboxError::InfrastructureError(e.to_string()))?;

        listener
            .listen(channel)
            .await
            .map_err(|e| OutboxError::InfrastructureError(e.to_string()))?;

        listener
            .recv()
            .await
            .map_err(|e| OutboxError::InfrastructureError(e.to_string()))?;

        Ok(())
    }
}

pub struct PostgresWriter<E>(pub E);

#[async_trait]
impl<'a, E> OutboxWriter for PostgresWriter<E>
where
        for<'c> &'c E: Executor<'c, Database = Postgres>,
        E: Send + Sync,
{
    async fn insert_event(&self, event: OutboxSlot) -> Result<(), OutboxError> {
        sqlx::query!(
                    r#"
                    INSERT INTO outbox_events (id, event_type, payload, status, created_at, locked_until)
                    VALUES ($1, $2, $3, $4, $5, $6)
                    "#,
                    event.id.as_uuid(),
                    event.event_type.as_str(),
                    event.payload.as_json(),
                    event.status as SlotStatus,
                    event.created_at,
                    event.locked_until
                ).execute(&self.0).await.map_err(|e| OutboxError::InfrastructureError(e.to_string()))?;

        Ok(())
    }
}

//TODO: Create tests:
#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres;

    #[rstest]
    #[tokio::test]
    async fn data_race() {
        let db = Postgres::default()
            .start()
            .await
            .expect("Failed to start db");
        let host = db.get_host().await.expect("Failed to get host");
        let port = db
            .get_host_port_ipv4(5432)
            .await
            .expect("Failed to get port");

        let connection_string = format!("postgres://postgres:postgres@{host}:{port}/postgres");

        let pool = PgPool::connect(&connection_string)
            .await
            .expect("Failed to connect to database");
    }
}
