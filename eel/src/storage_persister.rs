use crate::errors::*;
use crate::interfaces::RemoteStorage;
use crate::types::{ChainMonitor, NetworkGraph, Scorer};
use crate::LightningLogger;

use crate::async_runtime::Handle;
use bitcoin::hash_types::BlockHash;
use bitcoin::hashes::hex::ToHex;
use lightning::chain::chaininterface::{BroadcasterInterface, FeeEstimator};
use lightning::chain::chainmonitor::{MonitorUpdateId, Persist};
use lightning::chain::channelmonitor::{ChannelMonitor, ChannelMonitorUpdate};
use lightning::chain::keysinterface::{InMemorySigner, KeysInterface, KeysManager, Sign};
use lightning::chain::transaction::OutPoint;
use lightning::chain::{ChannelMonitorUpdateStatus, Watch};
use lightning::ln::channelmanager::{
    ChainParameters, ChannelManagerReadArgs, SimpleArcChannelManager,
};
use lightning::routing::scoring::{ProbabilisticScoringParameters, WriteableScore};
use lightning::util::config::UserConfig;
use lightning::util::logger::Logger;
use lightning::util::persist::Persister;
use lightning::util::ser::{ReadableArgs, Writeable};
use lightning_persister::FilesystemPersister;
use log::{debug, error};
use perro::{permanent_failure, MapToError};
use std::fs;
use std::io::{BufReader, Cursor};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::thread::sleep;
use std::time::Duration;

static MONITORS_BUCKET: &str = "monitors";
static OBJECTS_BUCKET: &str = "objects";

static MANAGER_KEY: &str = "manager";
static GRAPH_KEY: &str = "network_graph";
static SCORER_KEY: &str = "scorer";

pub(crate) struct StoragePersister {
    storage: Arc<Box<dyn RemoteStorage>>,
    fs_persister: FilesystemPersister,
    runtime_handle: Handle,
    chain_monitor: Mutex<Weak<ChainMonitor>>,
}

impl StoragePersister {
    pub fn new(
        storage: Box<dyn RemoteStorage>,
        local_fs_path: String,
        runtime_handle: Handle,
    ) -> Self {
        let storage = Arc::new(storage);
        let fs_persister = FilesystemPersister::new(local_fs_path);
        Self {
            storage,
            fs_persister,
            runtime_handle,
            chain_monitor: Mutex::new(Weak::new()),
        }
    }

    pub fn add_chain_monitor(&self, chain_monitor: Weak<ChainMonitor>) {
        let mut mutex_chain_monitor = self.chain_monitor.lock().unwrap();
        *mutex_chain_monitor = chain_monitor;
    }

    pub fn check_health(&self) -> bool {
        self.storage.check_health()
    }

    pub fn read_channel_monitors<Signer: Sign, K: Deref>(
        &self,
        keys_manager: K,
    ) -> Result<Vec<(BlockHash, ChannelMonitor<Signer>)>>
    where
        K::Target: KeysInterface<Signer = Signer> + Sized,
    {
        /*let mut result = Vec::new();
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
        result*/

        self.fs_persister
            .read_channelmonitors(&*keys_manager)
            .map_to_permanent_failure("Failed to read channel monitors from disk")
    }

    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
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
    ) -> Result<(Option<BlockHash>, SimpleArcChannelManager<M, T, F, L>)>
    where
        M: Watch<InMemorySigner>,
        T: BroadcasterInterface,
        F: FeeEstimator,
        L: Logger,
    {
        let channel_monitors_is_empty = channel_monitors.is_empty();
        let read_args = ChannelManagerReadArgs::new(
            Arc::clone(&keys_manager),
            Arc::clone(&fee_estimator),
            Arc::clone(&chain_monitor),
            Arc::clone(&broadcaster),
            Arc::clone(&logger),
            user_config,
            channel_monitors,
        );

        // Try to read from the local filesystem
        let path = PathBuf::from(self.fs_persister.get_data_dir()).join(Path::new(MANAGER_KEY));
        if let Ok(f) = fs::File::open(path) {
            let (block_hash, channel_manager) =
                <(BlockHash, SimpleArcChannelManager<M, T, F, L>)>::read(
                    &mut BufReader::new(f),
                    read_args,
                )
                .map_to_permanent_failure("Failed to parse a previously persisted ChannelManager. Could it have been corrupted?")?;
            return Ok((Some(block_hash), channel_manager));
        } else if !channel_monitors_is_empty {
            // Channel manager is not in the filesystem
            // We have ChannelMonitors, but no ChannelManager, so let's try to recover it from remote storage
            return if self
                .storage
                .object_exists(OBJECTS_BUCKET.to_string(), MANAGER_KEY.to_string())
                .map_to_runtime_error(
                    RuntimeErrorCode::RemoteStorageServiceUnavailable,
                    "Failed to check channel manager",
                )? {
                let data = self
                    .storage
                    .get_object(OBJECTS_BUCKET.to_string(), MANAGER_KEY.to_string())
                    .map_to_runtime_error(
                        RuntimeErrorCode::RemoteStorageServiceUnavailable,
                        "Failed to read channel manager",
                    )?;
                let mut buffer = Cursor::new(&data);
                let (block_hash, channel_manager) = <(
                    BlockHash,
                    SimpleArcChannelManager<M, T, F, L>,
                )>::read(&mut buffer, read_args)
                .map_to_permanent_failure("Failed to parse a previously remotely persisted ChannelManager. Could it have been corrupted?")?;
                Ok((Some(block_hash), channel_manager))
            } else {
                Err(permanent_failure(
                    "Failed to find an existing ChannelManager (both locally and remotely) when we know one should exist",
                ))
            };
        }

        debug!("Couldn't find a previously persisted channel manager. Creating a new one...");
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

    pub fn read_or_init_graph(
        &self,
        genesis_hash: BlockHash,
        logger: Arc<LightningLogger>,
    ) -> Result<NetworkGraph> {
        let path = PathBuf::from(self.fs_persister.get_data_dir()).join(Path::new(GRAPH_KEY));

        if let Ok(file) = fs::File::open(&path) {
            if let Ok(graph) = NetworkGraph::read(&mut BufReader::new(file), logger.clone()) {
                debug!("Successfully read the network graph from the local filesystem");
                return Ok(graph);
            } else {
                error!("Failed to parse network graph data. Deleting and continuing...");
                fs::remove_file(path)
                    .map_to_permanent_failure("Failed to delete an invalid network graph file")?;
            }
        }
        debug!("Couldn't find a previously persisted network graph. Creating a new one...");
        Ok(NetworkGraph::new(genesis_hash, logger))
    }

    pub fn read_or_init_scorer(
        &self,
        graph: Arc<NetworkGraph>,
        logger: Arc<LightningLogger>,
    ) -> Result<Scorer> {
        let path = PathBuf::from(self.fs_persister.get_data_dir()).join(Path::new(SCORER_KEY));

        let params = ProbabilisticScoringParameters::default();
        if let Ok(file) = fs::File::open(&path) {
            let args = (params.clone(), Arc::clone(&graph), Arc::clone(&logger));
            if let Ok(scorer) = Scorer::read(&mut BufReader::new(file), args) {
                debug!("Successfully read the scorer from the local filesystem");
                return Ok(scorer);
            } else {
                error!("Failed to parse scorer data. Deleting and continuing...");
                fs::remove_file(&path)
                    .map_to_permanent_failure("Failed to delete an invalid scorer file")?;
            }
        }
        debug!("Couldn't find a previously persisted scorer. Creating a new one...");
        Ok(Scorer::new(params, graph, logger))
    }
}

impl<ChannelSigner: Sign> Persist<ChannelSigner> for StoragePersister {
    fn persist_new_channel(
        &self,
        channel_id: OutPoint,
        data: &ChannelMonitor<ChannelSigner>,
        update_id: MonitorUpdateId,
    ) -> ChannelMonitorUpdateStatus {
        // Persist locally
        match self
            .fs_persister
            .persist_new_channel(channel_id, data, update_id)
        {
            ChannelMonitorUpdateStatus::Completed => {}
            ChannelMonitorUpdateStatus::InProgress => {
                error!("Unexpected: FilesystemPersister returned ChannelMonitorUpdateStatus::InProgress");
                return ChannelMonitorUpdateStatus::PermanentFailure;
            }
            ChannelMonitorUpdateStatus::PermanentFailure => {
                error!("Failed to persist a ChannelMonitor in the filesystem.");
                return ChannelMonitorUpdateStatus::PermanentFailure;
            }
        };

        // Launch background task that handles persisting monitor remotely
        let data = data.encode();
        let storage = Arc::clone(&self.storage);
        let key = channel_id.to_channel_id().to_hex();
        let chain_monitor = match self.chain_monitor.lock().unwrap().upgrade() {
            None => return ChannelMonitorUpdateStatus::PermanentFailure,
            Some(c) => c,
        };
        self.runtime_handle.spawn(async move {
            // The channel will block until remote persistence succeeds so let's continuously retry
            // 
            // If there is a non-runtime failure or if the node is shutdown before remote persistence 
            // succeeds, the local version of the channel monitor will be more fresh than the remote one
            // 
            // This should sort itself out as the node will get back at trying to persist remotely
            // when it gets back online (adding the ChannelMonitor to the ChainMonitor calls 
            // persist_new_channel() again).
            loop {
                match storage
                    .put_object(MONITORS_BUCKET.to_string(), key.clone(), data.clone()) {
                    Ok(_) => break,
                    Err(crate::Error::RuntimeError{..}) => {
                        error!("Temporary failure to remotely persist the ChannelMonitor {}... Retrying...", key);
                    }
                    Err(_) => {
                        error!("Failed to remotely persist the ChannelMonitor {}", key);
                        return
                    }
                }
                sleep(Duration::from_secs(5));
            }

            // Let ChainMonitor know that remote persistence has succeeded
            if chain_monitor.channel_monitor_updated(channel_id, update_id).is_err() {
                error!("Attempted to inform the ChainMonitor about a successfully persisted ChannelMonitor but the ChainMonitor doesn't know about the ChannelMonitor");
            }
            debug!("Successfully remotely persisted the ChannelMonitor {}", key);
        });

        ChannelMonitorUpdateStatus::InProgress
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

impl<'a, M: Deref, T: Deref, K: Deref, F: Deref, L: Deref, S: WriteableScore<'a>>
    Persister<'a, M, T, K, F, L, S> for StoragePersister
where
    M::Target: 'static + Watch<<K::Target as KeysInterface>::Signer>,
    T::Target: 'static + BroadcasterInterface,
    K::Target: 'static + KeysInterface,
    F::Target: 'static + FeeEstimator,
    L::Target: 'static + Logger,
{
    fn persist_manager(
        &self,
        channel_manager: &lightning::ln::channelmanager::ChannelManager<M, T, K, F, L>,
    ) -> std::result::Result<(), std::io::Error> {
        // Persist locally
        match <FilesystemPersister as Persister<'_, M, T, K, F, L, S>>::persist_manager(
            &self.fs_persister,
            channel_manager,
        ) {
            Ok(_) => {}
            Err(e) => {
                error!("Failed to persist the ChannelManager in the filesystem.");
                // Returns an error therefor stopping the Background processor.
                // We could eventually ignore this error as we do for errors in remote persistence,
                // but an error in local filesystem persistence indicates the existence
                // of a more permanent issue.
                return Err(e);
            }
        };

        // Persist remotely
        if self
            .storage
            .put_object(
                OBJECTS_BUCKET.to_string(),
                MANAGER_KEY.to_string(),
                channel_manager.encode(),
            )
            .is_err()
        {
            // We ignore errors on persisting the channel manager remotely hoping that it
            // will succeed next time and meanwhile the user will not try to
            // recover the wallet from an outdated backup (which will result in
            // force close for some new channels).
            error!("Error on remotely persisting channel manager. Ignoring.");
        }

        Ok(())
    }

    fn persist_graph(
        &self,
        network_graph: &lightning::routing::gossip::NetworkGraph<L>,
    ) -> std::result::Result<(), std::io::Error> {
        <FilesystemPersister as Persister<'_, M, T, K, F, L, S>>::persist_graph(
            &self.fs_persister,
            network_graph,
        )
    }

    fn persist_scorer(&self, scorer: &S) -> std::result::Result<(), std::io::Error> {
        <FilesystemPersister as Persister<'_, M, T, K, F, L, S>>::persist_scorer(
            &self.fs_persister,
            scorer,
        )
    }
}

#[cfg(test)]
mod tests {
    /*use super::*;

    use crate::keys_manager::init_keys_manager;

    use crate::async_runtime::AsyncRuntime;
    use bitcoin::hashes::hex::ToHex;
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

    impl RemoteStorage for StorageMock {
        fn check_health(&self) -> bool {
            self.storage.check_health()
        }

        fn list_objects(&self, bucket: String) -> LipaResult<Vec<String>> {
            Ok(self.storage.list_objects(bucket))
        }

        fn object_exists(&self, bucket: String, key: String) -> LipaResult<bool> {
            Ok(self.storage.object_exists(bucket, key))
        }

        fn get_object(&self, bucket: String, key: String) -> LipaResult<Vec<u8>> {
            Ok(self.storage.get_object(bucket, key))
        }

        fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> LipaResult<()> {
            self.storage.put_object(bucket, key, value);
            Ok(())
        }

        fn delete_object(&self, bucket: String, key: String) -> LipaResult<()> {
            self.storage.delete_object(bucket, key);
            Ok(())
        }
    }

    #[test]
    fn test_check_storage_health() {
        let storage = Arc::new(Storage::new());
        let rt = AsyncRuntime::new().unwrap();
        let persister = StoragePersister::new(
            Box::new(StorageMock::new(storage.clone())),
            "".to_string(),
            rt.handle(),
        );
        *storage.health.lock().unwrap() = true;
        assert!(persister.check_health());

        *storage.health.lock().unwrap() = false;
        assert!(!persister.check_health());
    }

    #[test]
    fn test_read_channel_monitors() {
        let storage = Arc::new(Storage::new());
        let rt = AsyncRuntime::new().unwrap();
        let persister = StoragePersister::new(
            Box::new(StorageMock::new(storage.clone())),
            "".to_string(),
            rt.handle(),
        );
        let keys_manager = init_keys_manager(&[0u8; 32].to_vec()).unwrap();

        assert_eq!(
            persister
                .read_channel_monitors(&keys_manager)
                .unwrap()
                .len(),
            0
        );

        // With invalid object.
        storage.objects.lock().unwrap().borrow_mut().insert(
            ("monitors".to_string(), "invalid_object".to_string()),
            Vec::new(),
        );
        assert_eq!(
            persister
                .read_channel_monitors(&keys_manager)
                .unwrap()
                .len(),
            0
        );

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
        let monitors = persister.read_channel_monitors(&keys_manager).unwrap();
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
    }*/
}
