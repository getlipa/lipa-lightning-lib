use crate::callbacks::RedundantStorageCallback;

use bitcoin::hash_types::BlockHash;
use bitcoin::hashes::hex::ToHex;
use lightning::chain;
use lightning::chain::chaininterface::{BroadcasterInterface, FeeEstimator};
use lightning::chain::chainmonitor::MonitorUpdateId;
use lightning::chain::chainmonitor::Persist;
use lightning::chain::channelmonitor::ChannelMonitor;
use lightning::chain::channelmonitor::ChannelMonitorUpdate;
use lightning::chain::keysinterface::{KeysInterface, Sign};
use lightning::chain::transaction::OutPoint;
use lightning::chain::ChannelMonitorUpdateErr;

use lightning::ln::channelmanager::ChannelManager;
use lightning::routing::gossip::NetworkGraph;
use lightning::routing::scoring::WriteableScore;
use lightning::util::logger::Logger;
use lightning::util::persist::Persister;
use lightning::util::ser::Writeable;
use std::io;
use std::io::Error;
use std::ops::Deref;


use lightning::util::ser::ReadableArgs;
use log::error;
use std::io::Cursor;



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

    pub fn read_channel_manager(&self) {
        // TODO: Implement
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
        channel_manager: &ChannelManager<Signer, M, T, K, F, L>,
    ) -> Result<(), Error> {
        self.persist_object(
            OBJECTS_BUCKET.to_string(),
            MANAGER_KEY.to_string(),
            channel_manager.encode(),
        )
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
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::Mutex;

    #[derive(Debug)]
    pub struct Storage {
        // Put the map into RefCell to allow mutation by immutable ref in StorageMock::put_object().
        pub objects: Mutex<RefCell<HashMap<(String, String), Vec<u8>>>>,
        pub health: Mutex<HashMap<String, bool>>,
    }

    #[derive(Debug)]
    pub struct StorageMock {
        storage: Arc<Storage>,
    }

    impl Storage {
        pub fn new() -> Self {
            Self {
                objects: Mutex::new(RefCell::new(HashMap::new())),
                health: Mutex::new(HashMap::new()),
            }
        }
    }

    impl StorageMock {
        pub fn new(storage: Arc<Storage>) -> Self {
            Self { storage }
        }
    }

    impl RedundantStorageCallback for StorageMock {
        fn object_exists(&self, bucket: String, key: String) -> bool {
            self.storage
                .objects
                .lock()
                .unwrap()
                .borrow()
                .contains_key(&(bucket, key))
        }

        fn get_object(&self, bucket: String, key: String) -> Vec<u8> {
            self.storage
                .objects
                .lock()
                .unwrap()
                .borrow()
                .get(&(bucket, key))
                .unwrap()
                .clone()
        }

        fn check_health(&self, bucket: String) -> bool {
            *self.storage.health.lock().unwrap().get(&bucket).unwrap()
        }

        fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> bool {
            self.storage
                .objects
                .lock()
                .unwrap()
                .borrow_mut()
                .insert((bucket, key), value);
            true
        }

        fn list_objects(&self, bucket: String) -> Vec<String> {
            self.storage
                .objects
                .lock()
                .unwrap()
                .borrow()
                .keys()
                .filter(|(b, _)| &bucket == b)
                .map(|(_, k)| k.clone())
                .collect()
        }
    }

    #[test]
    fn test_check_monitor_storage_health() {
        let storage = Arc::new(Storage::new());
        let persister = StoragePersister::new(Box::new(StorageMock::new(storage.clone())));
        storage
            .health
            .lock()
            .unwrap()
            .insert("monitors".to_string(), true);
        assert!(persister.check_monitor_storage_health());
        assert!(persister.check_object_storage_health());

        storage
            .health
            .lock()
            .unwrap()
            .insert("monitors".to_string(), false);
        assert!(!persister.check_monitor_storage_health());
    }

    #[test]
    fn test_read_channel_monitors() {
        let storage = Arc::new(Storage::new());
        let _persister = StoragePersister::new(Box::new(StorageMock::new(storage.clone())));

        // assert_eq!(persister.read_channel_monitors().len(), 0);
    }
}
