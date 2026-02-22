pub mod object;
mod storage;

use tokio::sync::mpsc::Receiver;
use tokio::sync::watch;
use crate::dlq::object::EventFail;
use crate::error::OutboxError;

pub struct OutboxDlq {
    source_rx: Receiver<EventFail>,
    shutdown_rx: watch::Receiver<bool>
}

impl OutboxDlq {
    pub fn new(source_rx: Receiver<EventFail>, shutdown_rx: watch::Receiver<bool>) -> Self {
        Self { source_rx, shutdown_rx }
    }

    pub async fn run(mut self) -> Result<(), OutboxError> {
        loop {
            tokio::select! {
                _ = self.shutdown_rx.changed() => {
                    if *self.shutdown_rx.borrow() {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

}