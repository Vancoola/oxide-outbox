use async_trait::async_trait;
use sqlx::PgPool;
use outbox_core::error::OutboxError;
use outbox_core::model::{OutboxSlot, SlotStatus};
use outbox_core::storage::OutboxStorage;
pub struct PostgresOutbox {
    pool: PgPool
}

#[async_trait]
impl OutboxStorage for PostgresOutbox {
    async fn fetch_next_to_process(&self, limit: u32) -> Result<Vec<OutboxSlot>, OutboxError> {
        let record = sqlx::query!(r#"
                UPDATE outbox_events
                SET status = 'Processing'
                WHERE id IN (
                    SELECT id
                    FROM outbox_events
                    WHERE status='Pending'
                    ORDER BY created_at
                    LIMIT $1
                    FOR UPDATE SKIP LOCKED
                )
                RETURNING
                id,
                event_type,
                payload,
                status AS "status: SlotStatus",
                created_at,
                processed_at,
                retry_count,
                last_error
            "#,
        limit as i64).fetch_all(&self.pool).await.map_err(|e| OutboxError::InfrastructureError(e.to_string()))?;
        todo!()
    }

    async fn fetch_unprocessed(&self, limit: u32) -> Result<Vec<OutboxSlot>, OutboxError> {
        todo!()
    }

    async fn add_fail_counts(&self, ids: &Vec<outbox_core::object::SlotId>) -> Result<(), OutboxError> {
        todo!()
    }

    async fn update_status(&self, id: &outbox_core::object::SlotId, status: SlotStatus) -> Result<(), OutboxError> {
        todo!()
    }

    async fn updates_status(&self, id: &Vec<outbox_core::object::SlotId>, status: SlotStatus) -> Result<(), OutboxError> {
        todo!()
    }

    async fn delete_garbage(&self) -> Result<(), OutboxError> {
        todo!()
    }

    async fn wait_for_notification(&self, channel: &str) -> Result<(), OutboxError> {
        todo!()
    }
}