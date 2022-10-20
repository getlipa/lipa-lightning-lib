use crate::callbacks::RedundantStorageCallback;
use crate::errors::InitializationError;

use bitcoin::hash_types::BlockHash;
use bitcoin::hashes::hex::ToHex;
use lightning::chain;
use lightning::chain::chaininterface::{BroadcasterInterface, FeeEstimator};
use lightning::chain::chainmonitor::{MonitorUpdateId, Persist};
use lightning::chain::channelmonitor::{ChannelMonitor, ChannelMonitorUpdate};
use lightning::chain::keysinterface::{InMemorySigner, KeysInterface, KeysManager, Sign};
use lightning::chain::transaction::OutPoint;
use lightning::chain::{ChannelMonitorUpdateErr, Watch};
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
    storage: Box<dyn RedundantStorageCallback>,
}

impl StoragePersister {
    pub fn new(storage: Box<dyn RedundantStorageCallback>) -> Self {
        Self { storage }
    }

    pub fn check_monitor_storage_health(&self) -> bool {
        self.storage.check_health(MONITORS_BUCKET.to_string())
    }

    pub fn check_object_storage_health(&self) -> bool {
        self.storage.check_health(OBJECTS_BUCKET.to_string())
    }

    pub fn read_channel_monitors<Signer: Sign, K: Deref>(
        &self,
        keys_manager: K,
    ) -> Vec<(BlockHash, ChannelMonitor<Signer>)>
    where
        K::Target: KeysInterface<Signer = Signer> + Sized,
    {
        let mut result = Vec::new();
        for key in self.storage.list_objects(MONITORS_BUCKET.to_string()) {
            let data = self
                .storage
                .get_object(MONITORS_BUCKET.to_string(), key.clone());
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
    ) -> Result<(Option<BlockHash>, SimpleArcChannelManager<M, T, F, L>), InitializationError>
    where
        M: Watch<InMemorySigner>,
        T: BroadcasterInterface,
        F: FeeEstimator,
        L: Logger,
    {
        if self
            .storage
            .object_exists(OBJECTS_BUCKET.to_string(), MANAGER_KEY.to_string())
        {
            let data = self
                .storage
                .get_object(OBJECTS_BUCKET.to_string(), MANAGER_KEY.to_string());
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
                    .map_err(|e| InitializationError::ChannelMonitorBackup {
                        message: e.to_string(),
                    })?;
            debug!(
                "Successfully read the ChannelManager from storage. It knows of {} channels",
                channel_manager.list_channels().len()
            );
            debug!(
                "List of channels known to the read ChannelManager: {}",
                channel_manager
                    .list_channels()
                    .iter()
                    .map(|details| details.channel_id.to_hex())
                    .collect::<String>()
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

    pub fn read_graph(&self) {
        // TODO: Implement
    }

    pub fn read_scorer(&self) {
        // TODO: Implement
    }

    fn persist_object(&self, bucket: String, key: String, data: Vec<u8>) -> Result<(), Error> {
        if !self.storage.put_object(bucket, key, data) {
            return Err(Error::new(
                io::ErrorKind::Other,
                "Failed to persist object using storage callback",
            ));
        }
        Ok(())
    }
}

impl<ChannelSigner: Sign> Persist<ChannelSigner> for StoragePersister {
    fn persist_new_channel(
        &self,
        funding_txo: OutPoint,
        monitor: &ChannelMonitor<ChannelSigner>,
        _update_id: MonitorUpdateId,
    ) -> Result<(), ChannelMonitorUpdateErr> {
        let key = funding_txo.to_channel_id().to_hex();
        let data = monitor.encode();
        if !self
            .storage
            .put_object(MONITORS_BUCKET.to_string(), key, data)
        {
            return Err(lightning::chain::ChannelMonitorUpdateErr::PermanentFailure);
        }
        Ok(())
    }

    fn update_persisted_channel(
        &self,
        funding_txo: OutPoint,
        _update: &Option<ChannelMonitorUpdate>,
        monitor: &ChannelMonitor<ChannelSigner>,
        update_id: MonitorUpdateId,
    ) -> Result<(), ChannelMonitorUpdateErr> {
        self.persist_new_channel(funding_txo, monitor, update_id)
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
            .persist_object(
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
        self.persist_object(
            OBJECTS_BUCKET.to_string(),
            GRAPH_KEY.to_string(),
            network_graph.encode(),
        )
    }

    fn persist_scorer(&self, scorer: &S) -> Result<(), Error> {
        self.persist_object(
            OBJECTS_BUCKET.to_string(),
            SCORER_KEY.to_string(),
            scorer.encode(),
        )
    }
}

#[cfg(test)]
mod test {
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

    impl RedundantStorageCallback for StorageMock {
        fn object_exists(&self, bucket: String, key: String) -> bool {
            self.storage.object_exists(bucket, key)
        }

        fn get_object(&self, bucket: String, key: String) -> Vec<u8> {
            self.storage.get_object(bucket, key)
        }

        fn check_health(&self, bucket: String) -> bool {
            self.storage.check_health(bucket)
        }

        fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> bool {
            self.storage.put_object(bucket, key, value)
        }

        fn list_objects(&self, bucket: String) -> Vec<String> {
            self.storage.list_objects(bucket)
        }
    }

    #[test]
    fn test_check_storage_health() {
        let storage = Arc::new(Storage::new());
        let persister = StoragePersister::new(Box::new(StorageMock::new(storage.clone())));
        storage
            .health
            .lock()
            .unwrap()
            .insert("monitors".to_string(), true);
        storage
            .health
            .lock()
            .unwrap()
            .insert("objects".to_string(), true);
        assert!(persister.check_monitor_storage_health());
        assert!(persister.check_object_storage_health());

        storage
            .health
            .lock()
            .unwrap()
            .insert("monitors".to_string(), false);
        assert!(!persister.check_monitor_storage_health());
        assert!(persister.check_object_storage_health());
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
