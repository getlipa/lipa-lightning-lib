use crate::errors::Result;
use crate::interfaces::RemoteStorage;
use crate::key_derivation;
use crate::keys_manager::init_keys_manager;
use crate::storage_persister::{StoragePersister, MANAGER_KEY, MONITORS_BUCKET};
use bitcoin::hashes::hex::ToHex;
use lightning::chain::channelmonitor::ChannelMonitor;
use lightning::chain::keysinterface::WriteableEcdsaChannelSigner;
use lightning::chain::transaction::OutPoint;
use lightning::util::persist::KVStorePersister;
use lightning_persister::FilesystemPersister;
use log::info;
use perro::{invalid_input, MapToError};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn recover_lightning_node(
    seed: [u8; 64],
    local_persistence_path: String,
    remote_storage: Box<dyn RemoteStorage>,
) -> Result<()> {
    let encryption_key = key_derivation::derive_persistence_encryption_key(&seed).unwrap();
    let storage = Arc::new(StoragePersister::new(
        remote_storage,
        local_persistence_path.clone(),
        encryption_key,
    ));

    verify_no_local_install(&local_persistence_path)?;

    let _ = fs::create_dir_all(&local_persistence_path);

    // Fetch and persist ChannelManager
    let remote_channel_manager = storage.fetch_remote_channel_manager_serialized()?;
    fs::write(
        get_local_channel_manager_path(&local_persistence_path),
        remote_channel_manager,
    )
    .map_to_permanent_failure(
        "Failed to locally persist the ChannelManager recovered from remote storage",
    )?;

    // Fetch and persist ChannelMonitors
    let mut seed_first_half = [0u8; 32];
    seed_first_half.copy_from_slice(&seed[..32]);
    let keys_manager = Arc::new(init_keys_manager(&seed_first_half)?);

    let remote_channel_monitors =
        storage.fetch_remote_channel_monitors(&*keys_manager, &*keys_manager)?;
    info!(
        "Fetched {} channel monitors from remote storage during recovery procedure",
        remote_channel_monitors.len()
    );
    let local_channel_monitors_dir_path =
        get_local_channel_monitors_dir_path(&local_persistence_path);
    let persister = FilesystemPersister::new(local_persistence_path);

    let _ = fs::create_dir_all(local_channel_monitors_dir_path);
    for (_, monitor) in remote_channel_monitors {
        persist_channel_monitor(&persister, monitor.get_funding_txo().0, &monitor)?;
    }

    Ok(())
}

fn verify_no_local_install(local_persistence_path: &str) -> Result<()> {
    // ChannelManager
    let channel_manager_path = get_local_channel_manager_path(local_persistence_path);
    if fs::File::open(channel_manager_path).is_ok() {
        return Err(invalid_input(
            "Invalid local persistence path: an existing wallet installation was found!",
        ));
    }
    // ChannelMonitors
    let channel_monitors_dir_path = get_local_channel_monitors_dir_path(local_persistence_path);
    if let Ok(mut dir_entries) = fs::read_dir(channel_monitors_dir_path) {
        if dir_entries.next().is_some() {
            return Err(invalid_input(
                "Invalid local persistence path: an existing wallet installation was found!",
            ));
        }
    }
    Ok(())
}

fn get_local_channel_manager_path(local_persistence_path: &str) -> PathBuf {
    PathBuf::from(local_persistence_path).join(Path::new(MANAGER_KEY))
}

fn get_local_channel_monitors_dir_path(local_persistence_path: &str) -> PathBuf {
    PathBuf::from(local_persistence_path).join(Path::new(MONITORS_BUCKET))
}

fn persist_channel_monitor<ChannelSigner: WriteableEcdsaChannelSigner>(
    persister: &FilesystemPersister,
    funding_txo: OutPoint,
    monitor: &ChannelMonitor<ChannelSigner>,
) -> Result<()> {
    let key = format!(
        "monitors/{}_{}",
        funding_txo.txid.to_hex(),
        funding_txo.index
    );
    persister.persist(&key, monitor).map_to_permanent_failure(
        "Failed to locally persist a ChannelMonitor recovered from remote storage",
    )?;
    Ok(())
}
