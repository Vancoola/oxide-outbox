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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rstest::rstest;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct TestPayload;

    fn default_cfg() -> OutboxConfig<TestPayload> {
        OutboxConfig::default()
    }

    #[rstest]
    fn default_batch_size_is_100() {
        assert_eq!(default_cfg().batch_size, 100);
    }

    #[rstest]
    fn default_retention_days_is_7() {
        assert_eq!(default_cfg().retention_days, 7);
    }

    #[rstest]
    fn default_gc_interval_secs_is_3600() {
        assert_eq!(default_cfg().gc_interval_secs, 3600);
    }

    #[rstest]
    fn default_poll_interval_secs_is_10() {
        assert_eq!(default_cfg().poll_interval_secs, 10);
    }

    #[rstest]
    fn default_lock_timeout_mins_is_5() {
        assert_eq!(default_cfg().lock_timeout_mins, 5);
    }

    #[rstest]
    fn default_idempotency_strategy_is_none() {
        assert!(matches!(
            default_cfg().idempotency_strategy,
            IdempotencyStrategy::None
        ));
    }

    // ------------------------------- Clone -------------------------------

    #[rstest]
    fn clone_preserves_scalar_fields() {
        let cfg = OutboxConfig::<TestPayload> {
            batch_size: 42,
            retention_days: 3,
            gc_interval_secs: 99,
            poll_interval_secs: 1,
            lock_timeout_mins: 2,
            idempotency_strategy: IdempotencyStrategy::Uuid,
        };
        let cloned = cfg.clone();
        assert_eq!(cloned.batch_size, 42);
        assert_eq!(cloned.retention_days, 3);
        assert_eq!(cloned.gc_interval_secs, 99);
        assert_eq!(cloned.poll_interval_secs, 1);
        assert_eq!(cloned.lock_timeout_mins, 2);
        assert!(matches!(
            cloned.idempotency_strategy,
            IdempotencyStrategy::Uuid
        ));
    }

    #[rstest]
    fn clone_preserves_custom_strategy_function_pointer() {
        fn derive(_: &Event<TestPayload>) -> String {
            "fp".into()
        }
        let cfg = OutboxConfig::<TestPayload> {
            batch_size: 1,
            retention_days: 1,
            gc_interval_secs: 1,
            poll_interval_secs: 1,
            lock_timeout_mins: 1,
            idempotency_strategy: IdempotencyStrategy::Custom(derive),
        };
        let cloned = cfg.clone();
        match cloned.idempotency_strategy {
            IdempotencyStrategy::Custom(f) => {
                let e = Event::new(
                    crate::object::EventType::new("t"),
                    crate::object::Payload::new(TestPayload),
                    None,
                );
                assert_eq!(f(&e), "fp");
            }
            _ => panic!("expected Custom variant after clone"),
        }
    }
}
