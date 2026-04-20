use crate::dlq::{heap::DlqHeap, model::EventFail};
use crate::error::OutboxError;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tracing::error;

#[derive(Debug)]
pub struct DlqProcessor {
    source_rx: mpsc::UnboundedReceiver<EventFail>,
    shutdown_rx: watch::Receiver<bool>,
    heap: Arc<DlqHeap>,
}

impl DlqProcessor {
    pub fn new(
        source_rx: mpsc::UnboundedReceiver<EventFail>,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Self {
        Self {
            source_rx,
            shutdown_rx,
            heap: Arc::new(DlqHeap::new()),
        }
    }

    pub async fn run(mut self) -> Result<(), OutboxError> {
        let mut interval = tokio::time::interval(Duration::from_secs(10));

        let dlq_heap = self.heap.clone();
        tokio::spawn(async move {
            loop {
                interval.tick().await;
                let batch = dlq_heap.get_batch(10).await;
                match batch {
                    Ok(events) => {
                        for event in events {
                            // Process each event
                        }
                    }
                    Err(e) => {
                        error!("Failed to get batch from DLQ heap: {}", e);
                    }
                }
            }
        });

        loop {
            tokio::select! {
                _ = self.shutdown_rx.changed() => {
                    if self.shutdown_rx.has_changed().is_err(){
                        break;
                    }
                    if *self.shutdown_rx.borrow() {
                        break;
                    }
                }

                event = self.source_rx.recv() => {
                    if let Some(event) = event {
                        self.heap.add(event.event_id).await?;
                    }
                }

            }
        }

        Ok(())
    }
}
