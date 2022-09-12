use bitcoin::hashes::hex::ToHex;
use lightning::chain::chainmonitor::MonitorUpdateId;
use lightning::chain::chainmonitor::Persist;
use lightning::chain::channelmonitor::ChannelMonitor;
use lightning::chain::channelmonitor::ChannelMonitorUpdate;
use lightning::chain::keysinterface::Sign;
use lightning::chain::transaction::OutPoint;
use lightning::chain::ChannelMonitorUpdateErr;
use lightning::util::ser::Writeable;

use crate::callbacks::RedundantStorageCallback;

static MONITORS_BUCKET: &str = "monitors";

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

    pub fn read_channel_monitors(&self) {
        // TODO: Implement
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
    }
}
