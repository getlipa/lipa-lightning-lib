use eel::errors::Result;
use eel::interfaces::{EventHandler, RemoteStorage};
use eel::MapToError;
use std::sync::Arc;
use storage_mock::Storage;

#[derive(Debug, Clone)]
pub(crate) struct RemoteStorageMock {
    storage: Arc<Storage>,
}

impl RemoteStorageMock {
    pub fn new(storage: Arc<Storage>) -> Self {
        Self { storage }
    }
}

impl Default for RemoteStorageMock {
    fn default() -> Self {
        Self::new(Arc::new(Storage::new()))
    }
}

impl RemoteStorage for RemoteStorageMock {
    fn check_health(&self) -> bool {
        self.storage.check_health()
    }

    fn list_objects(&self, bucket: String) -> Result<Vec<String>> {
        Ok(self.storage.list_objects(bucket))
    }

    fn object_exists(&self, bucket: String, key: String) -> Result<bool> {
        Ok(self.storage.object_exists(bucket, key))
    }

    fn get_object(&self, bucket: String, key: String) -> Result<Vec<u8>> {
        Ok(self.storage.get_object(bucket, key))
    }

    fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> Result<()> {
        self.storage.put_object(bucket, key, value);
        Ok(())
    }

    fn delete_object(&self, bucket: String, key: String) -> Result<()> {
        self.storage.delete_object(bucket, key);
        Ok(())
    }
}

pub(crate) struct EventsImpl {
    pub events_callback: Box<dyn crate::callbacks::EventsCallback>,
}

impl EventHandler for EventsImpl {
    fn payment_received(&self, payment_hash: String, amount_msat: u64) -> Result<()> {
        self.events_callback
            .payment_received(payment_hash, amount_msat)
            .map_to_permanent_failure("Events callback failed")
    }

    fn channel_closed(&self, channel_id: String, reason: String) -> Result<()> {
        self.events_callback
            .channel_closed(channel_id, reason)
            .map_to_permanent_failure("Events callback failed")
    }

    fn payment_sent(
        &self,
        payment_hash: String,
        payment_preimage: String,
        fee_paid_msat: u64,
    ) -> Result<()> {
        self.events_callback
            .payment_sent(payment_hash, payment_preimage, fee_paid_msat)
            .map_to_permanent_failure("Events callback failed")
    }

    fn payment_failed(&self, payment_hash: String) -> Result<()> {
        self.events_callback
            .payment_failed(payment_hash)
            .map_to_permanent_failure("Events callback failed")
    }
}
