use std::time::Duration;
use async_trait::async_trait;
use rdkafka::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use outbox_core::prelude::{EventType, OutboxError, Payload, Transport};
use crate::config::KafkaConfig;

mod config;

pub struct KafkaTransport {
    producer: FutureProducer,
    config: KafkaConfig,
}

impl KafkaTransport {
    pub fn new(bootstrap_servers: &str, config: KafkaConfig) -> Self {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", bootstrap_servers)
            .set("message.timeout.ms", "5000")
            .create()
            .expect("Init Kafka error");
        Self {
            producer,
            config
        }
    }
}

#[async_trait]
impl Transport for KafkaTransport {
    async fn publish(&self, event_type: EventType, payload: Payload) -> Result<(), OutboxError> {

        let bytes = serde_json::to_vec(&payload)
            .map_err(|e| OutboxError::InfrastructureError(e.to_string()))?;

        let record = FutureRecord::to(&self.config.topic.as_str())
            .payload(&bytes)
            .key(event_type.as_str());

        self.producer
            .send(record, Duration::from_secs(0))
            .await
            .map(|_| ())
            .map_err(|(err, _)| OutboxError::InfrastructureError(err.to_string()))
    }
}