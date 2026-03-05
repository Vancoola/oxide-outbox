use crate::dlq::model::EventFail;
use tokio::sync::{mpsc, watch};
use crate::error::OutboxError;

#[derive(Debug)]
pub struct DlqProcessor {
    source_rx: mpsc::Receiver<EventFail>,
    shutdown_rx: watch::Receiver<bool>,
}


impl DlqProcessor {
    pub fn new(source_rx: mpsc::Receiver<EventFail>, shutdown_rx: watch::Receiver<bool>) -> Self {
        Self {source_rx, shutdown_rx}
    }

    pub async fn run(mut self) -> Result<(), OutboxError> {

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
                    if let Some(event) = event {}
                }

            }
        }

        Ok(())
    }

}