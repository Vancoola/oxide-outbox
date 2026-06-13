//! Producer side of the Kafka stack: builds the [`KafkaTransport`] the outbox
//! worker hands events to.
//!
//! The transport itself enforces `enable.idempotence=true` by default (see
//! `outbox-kafka` 0.2 release notes), so the [`ClientConfig`] only needs to
//! describe the broker address and any operational knobs. Everything else is
//! safe defaults.

use anyhow::Result;
use outbox_kafka::KafkaTransport;
use rdkafka::ClientConfig;

pub fn build(brokers: &str, topic: &str) -> Result<KafkaTransport> {
    let mut config = ClientConfig::new();
    config
        .set("bootstrap.servers", brokers)
        // Bound how long a record sits in the producer queue before send() fails.
        .set("message.timeout.ms", "10000")
        // Optional: identify this app in broker logs.
        .set("client.id", "order-service");

    let transport = KafkaTransport::new(topic, &config)?;
    Ok(transport)
}
