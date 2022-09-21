use crate::{
    ChainMonitor, FeeEstimator, LightningLogger, LipaEventHandler, PeerManager, StoragePersister,
    TxBroadcaster,
};
use lightning::ln::channelmanager::SimpleArcChannelManager;
use lightning::routing::gossip::NetworkGraph;
use lightning::routing::scoring::ProbabilisticScorer;
use lightning_background_processor::GossipSync;
use lightning_rapid_gossip_sync::RapidGossipSync;
use log::error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;

pub(crate) struct BackgroundProcessor {
    stop_thread: Arc<AtomicBool>,
    thread_handle: Option<JoinHandle<Result<(), std::io::Error>>>,
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
impl BackgroundProcessor {
    pub(crate) fn start(
        persister: Arc<StoragePersister>,
        event_handler: Arc<LipaEventHandler>,
        chain_monitor: Arc<ChainMonitor>,
        channel_manager: Arc<
            SimpleArcChannelManager<ChainMonitor, TxBroadcaster, FeeEstimator, LightningLogger>,
        >,
        rapid_gossip: Arc<
            RapidGossipSync<Arc<NetworkGraph<Arc<LightningLogger>>>, Arc<LightningLogger>>,
        >,
        peer_manager: Arc<PeerManager>,
        logger: Arc<LightningLogger>,
        scorer: Arc<
            Mutex<
                ProbabilisticScorer<Arc<NetworkGraph<Arc<LightningLogger>>>, Arc<LightningLogger>>,
            >,
        >,
    ) -> Self {
        let stop_thread = Arc::new(AtomicBool::new(false));
        let stop_thread_clone = stop_thread.clone();
        let handle = thread::spawn(move || -> Result<(), std::io::Error> {
            loop {
                let background_processor =
                    lightning_background_processor::BackgroundProcessor::start(
                        persister.clone(),
                        event_handler.clone(),
                        chain_monitor.clone(),
                        channel_manager.clone(),
                        GossipSync::rapid(rapid_gossip.clone()),
                        peer_manager.clone(),
                        logger.clone(),
                        Some(scorer.clone()),
                    );
                match background_processor.join() {
                    Ok(_) => break,
                    Err(e) => {
                        error!("The background processor stopped due to: {}. Starting the background processor again", e.to_string());
                    }
                }
                if stop_thread.load(Ordering::Acquire) {
                    break;
                }
            }
            Ok(())
        });
        Self {
            stop_thread: stop_thread_clone,
            thread_handle: Some(handle),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn stop(mut self) -> Result<(), std::io::Error> {
        assert!(self.thread_handle.is_some());

        self.stop_thread.store(true, Ordering::Release);
        match self.thread_handle.take() {
            Some(handle) => handle.join().unwrap(),
            None => Ok(()),
        }
    }
}
