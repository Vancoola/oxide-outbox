use crate::model::OutboxSlot;

#[derive(Clone)]
pub struct OutboxConfig {
    pub batch_size: u32,
    pub retention_days: i64,
    pub gc_interval_secs: u64,
    pub poll_interval_secs: u64,
    pub lock_timeout_mins: i64,
}

impl Default for OutboxConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            retention_days: 7,
            gc_interval_secs: 3600,
            poll_interval_secs: 10,
            lock_timeout_mins: 5,
        }
    }
}

pub enum IdempotencyStrategy {
    Provided(String),
    Custom(fn(&OutboxSlot) -> String),
    Uuid, //Uuid V7
    HashPayload, //BLAKE3
    None,
}