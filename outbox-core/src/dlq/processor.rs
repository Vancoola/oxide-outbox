use tokio::sync::{watch, mpsc};
use crate::dlq::model::EventFail;

#[derive(Debug)]
pub struct DlqProcessor {
    source_rx: mpsc::Receiver<EventFail>,
    shutdown_rx: watch::Receiver<bool>,
}

impl DlqProcessor {
    pub fn new(source_rx: mpsc::Receiver<EventFail>, shutdown_rx: watch::Receiver<bool>) -> Self {
        Self {source_rx, shutdown_rx}
    }
    pub async fn run(mut self) {
        loop {
            tokio::select! {
                _ = self.shutdown_rx.changed() => {
                    if *self.shutdown_rx.borrow() {
                        break;
                    }
                },
                
                
                
            }
        }
    }
}