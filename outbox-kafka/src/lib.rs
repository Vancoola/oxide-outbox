use std::fmt::Debug;
use std::time::Duration;
use async_trait::async_trait;
use rdkafka::ClientConfig;
use rdkafka::message::{Header, OwnedHeaders};
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde::Serialize;
use outbox_core::prelude::{Event, OutboxError, Transport};

pub trait KafkaKeyExtractable {
    fn kafka_key(&self) -> Vec<u8>;
}

pub struct KafkaTransport
{
    producer: FutureProducer,
    topic: String,
}

impl KafkaTransport
{
    pub fn new(topic: &str, config: ClientConfig) -> Self {
        Self {
            topic: topic.to_string(),
            producer: config.create().expect("Failed to create Kafka producer"),
        }
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
                value: Some(&event.created_at.to_string())
            });

        if let Some(i_token) = event.idempotency_token {
            headers = headers.insert(Header {
                key: "idempotency_token",
                value: Some(i_token.as_str()),
            });
        };

        self.producer.send(
            FutureRecord::to(self.topic.as_str())
                .payload(&payload_bytes)
                .key(&event.payload.as_value().kafka_key())
                .headers(headers),
            Duration::from_secs(10),
        )
            .await.map_err(|_| OutboxError::InfrastructureError("Failed to publish event".to_string()))?;
        Ok(())
    }
}