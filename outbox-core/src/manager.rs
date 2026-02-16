use std::cmp::max;
use std::time::Duration;
use tracing::error;
use crate::config::OutboxConfig;
use crate::error::OutboxError;
use crate::gc::GarbageCollector;
use crate::processor::OutboxProcessor;
use crate::publisher::EventPublisher;
use crate::reply::ReplyService;
use crate::storage::OutboxStorage;

pub struct OutboxManager<S, P> {
    storage: S,
    publisher: P,
    config: OutboxConfig,
    shutdown_tx: Option<tokio::sync::broadcast::Sender<()>>,
}

impl<S, P> OutboxManager<S, P>
where
    S: OutboxStorage + Clone + Send + Sync + 'static,
    P: EventPublisher + Clone + Send + Sync + 'static,
{
    pub fn new(storage: S, publisher: P, config: OutboxConfig) -> Self {
        Self {
            storage,
            publisher,
            config,
            shutdown_tx: None,
        }
    }

    //TODO: make logs
    pub async fn run(&mut self) -> Result<(), OutboxError> {
        let (tx, _) = tokio::sync::broadcast::channel(1);
        self.shutdown_tx = Some(tx.clone());

        let storage_for_listen = self.storage.clone();
        let processor = OutboxProcessor::new(self.storage.clone(), self.publisher.clone(), self.config.clone());
        let mut rx_listen = tx.subscribe();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    signal = storage_for_listen.wait_for_notification("outbox") => {
                        if let Err(e) = signal{
                            error!("Listen error: {}", e);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            continue;
                        }
                        loop {
                            match processor.process_pending_events().await {
                                Ok(0) => break,
                                Ok(_) => continue,
                                Err(e) => {
                                    error!("Pending error: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                    _ = rx_listen.recv() => {
                        break;
                    }
                }
            }
        });

        let replayer = ReplyService::new(self.storage.clone(), self.publisher.clone(), self.config.clone());
        let mut rx_replay = tx.subscribe();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                tokio::select! {
                    _ = interval.tick() => { let _ = replayer.try_replay().await; }
                    _ = rx_replay.recv() => break,
                }
            }
        });

        let gc = GarbageCollector::new(self.storage.clone(), self.config.clone());
        let mut rx_gc = tx.subscribe();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(3600));
            loop {
                tokio::select! {
                    _ = interval.tick() => { let _ = gc.collect_garbage().await; }
                    _ = rx_gc.recv() => break,
                }
            }
        });

        Ok(())
    }

}