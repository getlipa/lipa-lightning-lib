use crate::errors::Result;
use crate::interfaces::RemoteStorage;
use crate::key_derivation;
use crate::storage_persister::StoragePersister;
use std::sync::Arc;

pub fn recover_lightning_node(
    seed: [u8; 64],
    local_persistence_path: String,
    remote_storage: Box<dyn RemoteStorage>,
) -> Result<()> {
    let encryption_key = key_derivation::derive_persistence_encryption_key(&seed).unwrap();
    let _persister = Arc::new(StoragePersister::new(
        remote_storage,
        local_persistence_path,
        encryption_key,
    ));

    // TODO:
    //      * check if there's a local installation, if yes, return InvalidInput
    //      * check if there's files in the backend, if not, return RuntimeError with code NonExistingWallet
    //      * recover

    Ok(())
}
