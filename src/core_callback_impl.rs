use eel::callbacks::{EventsCallback, LspCallback, RemoteStorageCallback};
use eel::errors::{LipaResult, MapToLipaError};
use std::sync::Arc;
use storage_mock::Storage;

#[derive(Debug, Clone)]
pub(crate) struct StorageMock {
    storage: Arc<Storage>,
}

impl StorageMock {
    pub fn new(storage: Arc<Storage>) -> Self {
        Self { storage }
    }
}

impl Default for StorageMock {
    fn default() -> Self {
        Self::new(Arc::new(Storage::new()))
    }
}

impl RemoteStorageCallback for StorageMock {
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

pub(crate) struct LspImpl {
    pub lsp_callback: Box<dyn crate::callbacks::LspCallback>,
}

impl LspCallback for LspImpl {
    fn channel_information(&self) -> LipaResult<Vec<u8>> {
        self.lsp_callback
            .channel_information()
            .map_to_permanent_failure("LSP callback failed")
    }

    fn register_payment(&self, encrypted_payment_info_blob: Vec<u8>) -> LipaResult<()> {
        self.lsp_callback
            .register_payment(encrypted_payment_info_blob)
            .map_to_permanent_failure("LSP callback failed")
    }
}

pub(crate) struct EventsImpl {
    pub events_callback: Box<dyn crate::callbacks::EventsCallback>,
}

impl EventsCallback for EventsImpl {
    fn payment_received(&self, payment_hash: String, amount_msat: u64) -> LipaResult<()> {
        self.events_callback
            .payment_received(payment_hash, amount_msat)
            .map_to_permanent_failure("Events callback failed")
    }

    fn channel_closed(&self, channel_id: String, reason: String) -> LipaResult<()> {
        self.events_callback
            .channel_closed(channel_id, reason)
            .map_to_permanent_failure("Events callback failed")
    }

    fn payment_sent(
        &self,
        payment_hash: String,
        payment_preimage: String,
        fee_paid_msat: u64,
    ) -> LipaResult<()> {
        self.events_callback
            .payment_sent(payment_hash, payment_preimage, fee_paid_msat)
            .map_to_permanent_failure("Events callback failed")
    }

    fn payment_failed(&self, payment_hash: String) -> LipaResult<()> {
        self.events_callback
            .payment_failed(payment_hash)
            .map_to_permanent_failure("Events callback failed")
    }
}
