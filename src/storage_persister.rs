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
use std::io::Error;
use std::ops::Deref;

use crate::callbacks::RedundantStorageCallback;

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

    pub fn read_channel_monitors(&self) {
        // TODO: Implement
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
            // Operating System I/O Error
            return Err(Error::from_raw_os_error(5));
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

    #[derive(Debug)]
    pub struct StorageMock;

    impl RedundantStorageCallback for StorageMock {
        fn object_exists(&self, _bucket: String, _key: String) -> bool {
            println!("object_exists()");
            false
        }

        fn get_object(&self, _bucket: String, _key: String) -> Vec<u8> {
            println!("get_object()");
            Vec::new()
        }

        fn check_health(&self, _bucket: String) -> bool {
            println!("check_health()");
            true
        }

        fn put_object(&self, _bucket: String, _key: String, _value: Vec<u8>) -> bool {
            println!("put_object()");
            false
        }

        fn list_objects(&self, _bucket: String) -> Vec<String> {
            println!("list_objects()");
            Vec::new()
        }
    }

    #[test]
    fn test_out_point() {
        let persister = StoragePersister::new(Box::new(StorageMock {}));
        assert!(persister.check_monitor_storage_health());
        assert!(persister.check_object_storage_health());
    }
}
