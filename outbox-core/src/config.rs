use crate::model::Event;
use serde::Serialize;
use std::fmt::Debug;

#[derive(Clone)]
pub struct OutboxConfig<P>
where
    P: Debug + Clone + Serialize,
{
    pub batch_size: u32,
    pub retention_days: i64,
    pub gc_interval_secs: u64,
    pub poll_interval_secs: u64,
    pub lock_timeout_mins: i64,

    pub idempotency_strategy: IdempotencyStrategy<P>,
}

impl<P> Default for OutboxConfig<P>
where
    P: Debug + Clone + Serialize,
{
    fn default() -> Self {
        Self {
            batch_size: 100,
            retention_days: 7,
            gc_interval_secs: 3600,
            poll_interval_secs: 10,
            lock_timeout_mins: 5,
            idempotency_strategy: IdempotencyStrategy::None,
        }
    }
}

#[derive(Clone)]
pub enum IdempotencyStrategy<P>
where
    P: Debug + Clone + Serialize,
{
    Provided,
    Custom(fn(&Event<P>) -> String),
    Uuid,
    //TODO:
    //HashPayload, //BLAKE3
    None,
}
