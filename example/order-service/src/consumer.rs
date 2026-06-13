//! Demo Kafka consumer.
//!
//! Subscribes to the configured topic, decodes each message as an
//! [`OrderEvent`], and logs it. The point is to give you a visible signal that
//! the outbox actually published — once an order is created via HTTP, you'll
//! see a `consumer received` line in the logs within a second or two.
//!
//! This is *not* a production consumer: it commits offsets automatically,
//! does no retry book-keeping, and panics on no payload. It's a demo
//! observer, nothing more.

use crate::domain::OrderEvent;
use anyhow::Result;
use rdkafka::ClientConfig;
use rdkafka::Message;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::Headers;
use tokio::sync::watch;
use tracing::{error, info, warn};

pub async fn run(
    brokers: &str,
    topic: &str,
    group_id: &str,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", brokers)
        .set("group.id", group_id)
        .set("enable.auto.commit", "true")
        .set("auto.offset.reset", "earliest")
        .set("session.timeout.ms", "6000")
        .create()?;

    consumer.subscribe(&[topic])?;
    info!(topic, group_id, "consumer started");

    loop {
        tokio::select! {
            biased;

            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    info!("consumer shutdown signal received");
                    break;
                }
            }

            msg = consumer.recv() => {
                match msg {
                    Ok(borrowed) => {
                        let Some(payload) = borrowed.payload() else {
                            warn!("consumer received empty payload");
                            continue;
                        };
                        match serde_json::from_slice::<OrderEvent>(payload) {
                            Ok(event) => {
                                let event_type = borrowed.headers().and_then(|h| {
                                    h.iter()
                                        .find(|h| h.key == "event_type")
                                        .and_then(|h| h.value)
                                        .and_then(|v| std::str::from_utf8(v).ok())
                                        .map(str::to_owned)
                                });
                                info!(
                                    header_event_type = event_type.as_deref().unwrap_or("?"),
                                    ?event,
                                    "consumer received"
                                );
                            }
                            Err(e) => {
                                warn!(error = ?e, raw = %String::from_utf8_lossy(payload), "consumer: failed to decode payload");
                            }
                        }
                    }
                    Err(e) => {
                        error!(error = ?e, "consumer poll error");
                    }
                }
            }
        }
    }

    Ok(())
}
