use crate::callbacks::RemoteStorageCallback;
use crate::errors::*;

use crate::LightningLogger;
use bitcoin::hash_types::BlockHash;
use bitcoin::hashes::hex::ToHex;
use lightning::chain;
use lightning::chain::chaininterface::{BroadcasterInterface, FeeEstimator};
use lightning::chain::chainmonitor::{MonitorUpdateId, Persist};
use lightning::chain::channelmonitor::{ChannelMonitor, ChannelMonitorUpdate};
use lightning::chain::keysinterface::{InMemorySigner, KeysInterface, KeysManager, Sign};
use lightning::chain::transaction::OutPoint;
use lightning::chain::{ChannelMonitorUpdateStatus, Watch};
use lightning::ln::channelmanager::{
    ChainParameters, ChannelManagerReadArgs, SimpleArcChannelManager,
};
use lightning::routing::gossip::NetworkGraph;
use lightning::routing::scoring::WriteableScore;
use lightning::util::config::UserConfig;
use lightning::util::logger::Logger;
use lightning::util::persist::Persister;
use lightning::util::ser::{ReadableArgs, Writeable};
use log::{debug, error};
use std::io;
use std::io::{Cursor, Error};
use std::ops::Deref;
use std::sync::Arc;

static MONITORS_BUCKET: &str = "monitors";
static OBJECTS_BUCKET: &str = "objects";

static MANAGER_KEY: &str = "manager";
static GRAPH_KEY: &str = "graph";
static SCORER_KEY: &str = "scorer";

pub struct StoragePersister {
    storage: Box<dyn RemoteStorageCallback>,
}

impl StoragePersister {
    pub fn new(storage: Box<dyn RemoteStorageCallback>) -> Self {
        Self { storage }
    }

    pub fn check_health(&self) -> bool {
        self.storage.check_health()
    }

    pub fn read_channel_monitors<Signer: Sign, K: Deref>(
        &self,
        keys_manager: K,
    ) -> Vec<(BlockHash, ChannelMonitor<Signer>)>
    where
        K::Target: KeysInterface<Signer = Signer> + Sized,
    {
        let mut result = Vec::new();
        // TODO: Handle unwrap().
        for key in self
            .storage
            .list_objects(MONITORS_BUCKET.to_string())
            .unwrap()
        {
            // TODO: Handle unwrap().
            let data = self
                .storage
                .get_object(MONITORS_BUCKET.to_string(), key.clone())
                .unwrap();
            let mut buffer = Cursor::new(&data);
            match <(BlockHash, ChannelMonitor<Signer>)>::read(&mut buffer, &*keys_manager) {
                Ok((blockhash, channel_monitor)) => {
                    debug!(
                        "Successfully read ChannelMonitor {} from storage",
                        channel_monitor.get_funding_txo().0.to_channel_id().to_hex()
                    );
                    result.push((blockhash, channel_monitor));
                }
                Err(e) => {
                    error!("Failed to deserialize ChannelMonitor `{}`: {}", key, e);
                    // TODO: Should we return this information to the caller?
                }
            }
        }
        result
    }

    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    #[allow(clippy::result_large_err)]
    pub fn read_or_init_channel_manager<M, T, F, L>(
        &self,
        chain_monitor: Arc<M>,
        broadcaster: Arc<T>,
        keys_manager: Arc<KeysManager>,
        fee_estimator: Arc<F>,
        logger: Arc<L>,
        channel_monitors: Vec<&mut ChannelMonitor<InMemorySigner>>,
        user_config: UserConfig,
        chain_params: ChainParameters,
    ) -> LipaResult<(Option<BlockHash>, SimpleArcChannelManager<M, T, F, L>)>
    where
        M: Watch<InMemorySigner>,
        T: BroadcasterInterface,
        F: FeeEstimator,
        L: Logger,
    {
        if self
            .storage
            .object_exists(OBJECTS_BUCKET.to_string(), MANAGER_KEY.to_string())
            .map_to_runtime_error("Failed to check channel manager")?
        {
            let data = self
                .storage
                .get_object(OBJECTS_BUCKET.to_string(), MANAGER_KEY.to_string())
                .map_to_runtime_error("Failed to read channel manager")?;
            let read_args = ChannelManagerReadArgs::new(
                keys_manager,
                fee_estimator,
                chain_monitor,
                broadcaster,
                logger,
                user_config,
                channel_monitors,
            );
            let mut buffer = Cursor::new(&data);
            let (block_hash, channel_manager) =
                <(BlockHash, SimpleArcChannelManager<M, T, F, L>)>::read(&mut buffer, read_args)
                    .map_to_runtime_error("Failed to parse channel manager")?;
            debug!(
                "Successfully read the ChannelManager from storage. It knows of {} channels",
                channel_manager.list_channels().len()
            );
            debug!(
                "List of channels known to the read ChannelManager: {:?}",
                channel_manager
                    .list_channels()
                    .iter()
                    .map(|details| details.channel_id.to_hex())
                    .collect::<Vec<String>>()
            );
            Ok((Some(block_hash), channel_manager))
        } else {
            let channel_manager = SimpleArcChannelManager::new(
                fee_estimator,
                chain_monitor,
                broadcaster,
                logger,
                keys_manager,
                user_config,
                chain_params,
            );
            Ok((None, channel_manager))
        }
    }

    pub fn read_or_init_graph(
        &self,
        genesis_hash: BlockHash,
        logger: Arc<LightningLogger>,
    ) -> LipaResult<NetworkGraph<Arc<LightningLogger>>> {
        if self
            .storage
            .object_exists(OBJECTS_BUCKET.to_string(), GRAPH_KEY.to_string())
            .map_to_runtime_error("Failed to check network graph")?
        {
            let data = self
                .storage
                .get_object(OBJECTS_BUCKET.to_string(), GRAPH_KEY.to_string())
                .map_to_runtime_error("Failed to read network graph")?;
            let mut buffer = Cursor::new(&data);
            let network_graph = match NetworkGraph::read(&mut buffer, Arc::clone(&logger)) {
                Ok(graph) => {
                    debug!(
                "Successfully read the NetworkGraph from storage. Last sync made at timestamp {}",
                graph
                    .get_last_rapid_gossip_sync_timestamp()
                    .unwrap_or(0)
            );
                    graph
                }
                Err(e) => {
                    error!(
                        "Failed to parse network graph data: {} - Deleting and continuing...",
                        e
                    );
                    // TODO: Handle unwrap().
                    self.storage
                        .delete_object(OBJECTS_BUCKET.to_string(), GRAPH_KEY.to_string())
                        .unwrap();
                    NetworkGraph::new(genesis_hash, logger)
                }
            };

            Ok(network_graph)
        } else {
            let network_graph = NetworkGraph::new(genesis_hash, Arc::clone(&logger));
            Ok(network_graph)
        }
    }

    pub fn read_scorer(&self) {
        // TODO: Implement
    }
}

impl<ChannelSigner: Sign> Persist<ChannelSigner> for StoragePersister {
    fn persist_new_channel(
        &self,
        channel_id: OutPoint,
        data: &ChannelMonitor<ChannelSigner>,
        _update_id: MonitorUpdateId,
    ) -> ChannelMonitorUpdateStatus {
        let key = channel_id.to_channel_id().to_hex();
        let data = data.encode();
        if self
            .storage
            .put_object(MONITORS_BUCKET.to_string(), key, data)
            .is_err()
        {
            return ChannelMonitorUpdateStatus::PermanentFailure;
        }
        ChannelMonitorUpdateStatus::Completed
    }

    fn update_persisted_channel(
        &self,
        channel_id: OutPoint,
        _update: &Option<ChannelMonitorUpdate>,
        data: &ChannelMonitor<ChannelSigner>,
        update_id: MonitorUpdateId,
    ) -> ChannelMonitorUpdateStatus {
        self.persist_new_channel(channel_id, data, update_id)
    }
}

impl<'a, Signer: Sign, M: Deref, T: Deref, K: Deref, F: Deref, L: Deref, S: WriteableScore<'a>>
    Persister<'a, Signer, M, T, K, F, L, S> for StoragePersister
where
    M::Target: 'static + chain::Watch<Signer>,
    T::Target: 'static + BroadcasterInterface,
    K::Target: 'static + KeysInterface<Signer = Signer>,
    F::Target: 'static + FeeEstimator,
    L::Target: 'static + Logger,
{
    fn persist_manager(
        &self,
        channel_manager: &lightning::ln::channelmanager::ChannelManager<Signer, M, T, K, F, L>,
    ) -> Result<(), Error> {
        if self
            .storage
            .put_object(
                OBJECTS_BUCKET.to_string(),
                MANAGER_KEY.to_string(),
                channel_manager.encode(),
            )
            .is_err()
        {
            // We ignore errors on persisting the channel manager hoping that it
            // will succeed next time and meanwhile the user will not try to
            // recover the wallet from an outdated backup (what will result in
            // force close for some new channels).
            error!("Error on persisting channel manager. Ignoring.");
        }
        Ok(())
    }

    fn persist_graph(&self, network_graph: &NetworkGraph<L>) -> Result<(), Error> {
        self.storage
            .put_object(
                OBJECTS_BUCKET.to_string(),
                GRAPH_KEY.to_string(),
                network_graph.encode(),
            )
            .map_err(|_| {
                Error::new(
                    io::ErrorKind::Other,
                    "Failed to persist graph using storage callback",
                )
            })
    }

    fn persist_scorer(&self, scorer: &S) -> Result<(), Error> {
        self.storage
            .put_object(
                OBJECTS_BUCKET.to_string(),
                SCORER_KEY.to_string(),
                scorer.encode(),
            )
            .map_err(|_| {
                Error::new(
                    io::ErrorKind::Other,
                    "Failed to persist socrer using storage callback",
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::keys_manager::init_keys_manager;

    use std::fs;
    use std::path::PathBuf;
    use storage_mock::Storage;

    #[derive(Debug)]
    pub struct StorageMock {
        storage: Arc<Storage>,
    }

    impl StorageMock {
        pub fn new(storage: Arc<Storage>) -> Self {
            Self { storage }
        }
    }

    impl RemoteStorageCallback for StorageMock {
        fn object_exists(&self, bucket: String, key: String) -> CallbackResult<bool> {
            Ok(self.storage.object_exists(bucket, key))
        }

        fn get_object(&self, bucket: String, key: String) -> CallbackResult<Vec<u8>> {
            Ok(self.storage.get_object(bucket, key))
        }

        fn check_health(&self) -> bool {
            self.storage.check_health()
        }

        fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> CallbackResult<()> {
            self.storage.put_object(bucket, key, value);
            Ok(())
        }

        fn list_objects(&self, bucket: String) -> CallbackResult<Vec<String>> {
            Ok(self.storage.list_objects(bucket))
        }

        fn delete_object(&self, bucket: String, key: String) -> CallbackResult<()> {
            self.storage.delete_object(bucket, key);
            Ok(())
        }
    }

    #[test]
    fn test_check_storage_health() {
        let storage = Arc::new(Storage::new());
        let persister = StoragePersister::new(Box::new(StorageMock::new(storage.clone())));
        *storage.health.lock().unwrap() = true;
        assert!(persister.check_health());

        *storage.health.lock().unwrap() = false;
        assert!(!persister.check_health());
    }

    #[test]
    fn test_read_channel_monitors() {
        let storage = Arc::new(Storage::new());
        let persister = StoragePersister::new(Box::new(StorageMock::new(storage.clone())));
        let keys_manager = init_keys_manager(&[0u8; 32].to_vec()).unwrap();

        assert_eq!(persister.read_channel_monitors(&keys_manager).len(), 0);

        // With invalid object.
        storage.objects.lock().unwrap().borrow_mut().insert(
            ("monitors".to_string(), "invalid_object".to_string()),
            Vec::new(),
        );
        assert_eq!(persister.read_channel_monitors(&keys_manager).len(), 0);

        // With valid object.
        let mut monitors_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        monitors_path.push("tests/resources/monitors");
        monitors_path.push("739f39903ea426645bd6650c55a568653c4d3c275bcbda17befc468f64c76a58_1");
        let data = fs::read(monitors_path).unwrap();
        storage
            .objects
            .lock()
            .unwrap()
            .borrow_mut()
            .insert(("monitors".to_string(), "valid_object".to_string()), data);
        let monitors = persister.read_channel_monitors(&keys_manager);
        assert_eq!(monitors.len(), 1);
        let (blockhash, monitor) = &monitors[0];
        assert_eq!(
            blockhash.as_hash().to_hex(),
            "4c1a044c14d1c506431707ad671721cd8b637760ecc26e1975ad71b79721660d"
        );
        assert_eq!(monitor.get_latest_update_id(), 0);
        let txo = monitor.get_funding_txo().0;
        assert_eq!(
            txo.txid.as_hash().to_hex(),
            "739f39903ea426645bd6650c55a568653c4d3c275bcbda17befc468f64c76a58"
        );
        assert_eq!(txo.index, 1);
    }
}
