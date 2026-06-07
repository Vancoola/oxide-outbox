use async_trait::async_trait;
use outbox_core::prelude::{Event, OutboxError, Transport};
use rdkafka::ClientConfig;
use rdkafka::message::{Header, OwnedHeaders};
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde::Serialize;
use std::fmt::Debug;
use std::time::Duration;

pub trait KafkaKeyExtractable {
    fn kafka_key(&self) -> Vec<u8>;
}

/// Kafka transport for outbox events.
///
/// The transport enables the **idempotent producer** by default
/// (`enable.idempotence=true`) — this is required to preserve per-partition
/// ordering when `librdkafka` retries on transient errors. Without it, retries
/// combined with `max.in.flight.requests.per.connection > 1` can reorder
/// messages on the wire, which silently breaks the outbox's at-least-once,
/// in-order guarantee.
///
/// If the caller explicitly sets `enable.idempotence` in [`ClientConfig`],
/// their setting is respected and not overridden.
pub struct KafkaTransport {
    producer: FutureProducer,
    topic: String,
    send_timeout: Duration,
}

impl KafkaTransport {
    /// Default send timeout applied to each `FutureRecord` until overridden via
    /// [`with_send_timeout`](Self::with_send_timeout).
    pub const DEFAULT_SEND_TIMEOUT: Duration = Duration::from_secs(10);

    /// Creates a new [`KafkaTransport`] wired to `topic`.
    ///
    /// `enable.idempotence=true` is set on the underlying producer unless the
    /// caller has already specified it in `config`. See the type-level
    /// documentation for why this matters.
    ///
    /// # Errors
    ///
    /// Returns [`OutboxError::ConfigError`] if `librdkafka` rejects the
    /// resulting configuration (for example, conflicting settings between
    /// the caller-supplied values and the idempotence requirements).
    pub fn new(topic: &str, config: &ClientConfig) -> Result<Self, OutboxError> {
        let mut cfg = config.clone();
        if cfg.get("enable.idempotence").is_none() {
            cfg.set("enable.idempotence", "true");
        }
        let producer: FutureProducer = cfg
            .create()
            .map_err(|e| OutboxError::ConfigError(e.to_string()))?;
        Ok(Self {
            topic: topic.to_string(),
            producer,
            send_timeout: Self::DEFAULT_SEND_TIMEOUT,
        })
    }

    /// Overrides the per-record send timeout passed to
    /// [`FutureProducer::send`].
    #[must_use]
    pub fn with_send_timeout(mut self, send_timeout: Duration) -> Self {
        self.send_timeout = send_timeout;
        self
    }
}

#[async_trait]
impl<PT> Transport<PT> for KafkaTransport
where
    PT: Debug + Clone + Send + Sync + Serialize + KafkaKeyExtractable + 'static,
{
    async fn publish(&self, event: Event<PT>) -> Result<(), OutboxError> {
        let payload_bytes = serde_json::to_vec(&event.payload)
            .map_err(|e| OutboxError::InfrastructureError(e.to_string()))?;

        let mut headers = OwnedHeaders::new();

        headers = headers
            .insert(Header {
                key: "event_id",
                value: Some(&event.id.as_uuid().to_string()),
            })
            .insert(Header {
                key: "event_type",
                value: Some(event.event_type.as_str()),
            })
            .insert(Header {
                key: "created_at",
                value: Some(&event.created_at.to_string()),
            });

        if let Some(i_token) = event.idempotency_token {
            headers = headers.insert(Header {
                key: "idempotency_token",
                value: Some(i_token.as_str()),
            });
        }

        self.producer
            .send(
                FutureRecord::to(self.topic.as_str())
                    .payload(&payload_bytes)
                    .key(&event.payload.as_value().kafka_key())
                    .headers(headers),
                self.send_timeout,
            )
            .await
            .map_err(|_| OutboxError::InfrastructureError("Failed to publish event".to_string()))?;
        Ok(())
    }
}
