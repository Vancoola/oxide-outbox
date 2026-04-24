use crate::config::OutboxConfig;
use crate::dlq::storage::DlqHeap;
use crate::error::OutboxError;
use serde::Serialize;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch::Receiver;
use tracing::info;

pub struct DlqProcessor<PT>
where
    PT: Debug + Clone + Serialize,
{
    heap: Arc<dyn DlqHeap>,
    shutdown_rx: Receiver<bool>,
    config: Arc<OutboxConfig<PT>>,
}

impl<PT> DlqProcessor<PT>
where
    PT: Debug + Clone + Serialize,
{
    pub fn new(
        heap: Arc<dyn DlqHeap>,
        config: Arc<OutboxConfig<PT>>,
        shutdown_rx: Receiver<bool>,
    ) -> Self {
        Self {
            heap,
            config,
            shutdown_rx,
        }
    }
    #[cfg(feature = "dlq")]
    pub async fn run(self) -> Result<(), OutboxError> {
        let mut rx_dlq = self.shutdown_rx.clone();
        let mut interval =
            tokio::time::interval(Duration::from_secs(self.config.dlq_interval_secs));

        info!("Starting DLQ processor");

        loop {
            tokio::select! {
                _ = rx_dlq.changed() => {
                    if rx_dlq.has_changed().is_err(){
                            break;
                        }
                        if *rx_dlq.borrow() {
                            break
                        }
                }
                _ = interval.tick() => {

                }
            }
        }

        info!("Dlq processor stopped");
        Ok(())
    }
}
