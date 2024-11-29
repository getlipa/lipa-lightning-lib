use crate::async_runtime::AsyncRuntime;
use crate::backup::BackupManager;
use crate::errors::Result;
use crate::key_derivation::derive_persistence_encryption_key;
use crate::logger::init_logger_once;
use crate::{build_async_auth, permanent_failure, sanitize_input, DB_FILENAME, LOGS_DIR};
use squirrel::RemoteBackupClient;
use std::path::Path;
use std::sync::Arc;

/// Performs a recovery procedure by fetching necessary data from remote storage.
/// It should and can only be called on a fresh install of the app, if the user wants to recover a previously created wallet.
///
/// Parameters:
/// * `backend_url`
/// * `seed` - the seed from the wallet that will be recovered.
/// * `local_persistence_path` - the directory where local data will be stored.
/// * `file_logging_level` - the min log level that will be logged.
/// * `allow_external_recovery` - defines if recovery of external wallets is allowed.  
pub fn recover_lightning_node(
    backend_url: String,
    seed: Vec<u8>,
    local_persistence_path: String,
    file_logging_level: Option<log::Level>,
    allow_external_recovery: bool,
) -> Result<()> {
    if let Some(level) = file_logging_level {
        init_logger_once(level, &Path::new(&local_persistence_path).join(LOGS_DIR))?;
    }

    let db_path = format!("{local_persistence_path}/{DB_FILENAME}");
    if Path::new(&db_path).exists() {
        permanent_failure!(
            "Trying to recover when existing wallet data is present in local persistence path"
        )
    }

    let strong_typed_seed = sanitize_input::strong_type_seed(&seed)?;
    let auth = Arc::new(build_async_auth(&strong_typed_seed, &backend_url)?);

    let backup_client = RemoteBackupClient::new(backend_url, auth);
    let backup_manager = BackupManager::new(
        backup_client,
        db_path,
        derive_persistence_encryption_key(&strong_typed_seed)?,
    );

    AsyncRuntime::new()?
        .handle()
        .block_on(backup_manager.recover(allow_external_recovery))
}
