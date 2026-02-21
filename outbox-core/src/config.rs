use crate::model::Event;

#[derive(Clone)]
pub struct OutboxConfig {
    pub batch_size: u32,
    pub retention_days: i64,
    pub gc_interval_secs: u64,
    pub poll_interval_secs: u64,
    pub lock_timeout_mins: i64,

    pub idempotency_strategy: IdempotencyStrategy,
    pub idempotency_storage: IdempotencyStorage
}

impl Default for OutboxConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            retention_days: 7,
            gc_interval_secs: 3600,
            poll_interval_secs: 10,
            lock_timeout_mins: 5,
            idempotency_strategy: IdempotencyStrategy::None,
            idempotency_storage: IdempotencyStorage::None
        }
    }
}

#[derive(Clone)]
pub enum IdempotencyStrategy {
    Provided(String),
    Custom(fn(&Event) -> String),
    Uuid, //Uuid V7
    HashPayload, //BLAKE3
    None,
}

#[derive(Clone)]
pub enum IdempotencyStorage {
    Moka, //in-memory
    Redis(String),
    //Database,
    //e.g, Custom(dyn IdempotencyStorageEngine),
    None
}