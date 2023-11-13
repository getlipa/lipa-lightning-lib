use crate::async_runtime::AsyncRuntime;
use crate::backup::BackupManager;
use crate::environment::Environment;
use crate::errors::Result;
use crate::key_derivation::derive_persistence_encryption_key;
use crate::logger::init_logger_once;
use crate::{
    build_async_auth, permanent_failure, sanitize_input, EnvironmentCode, DB_FILENAME, LOGS_DIR,
    LOG_LEVEL,
};
use squirrel::RemoteBackupClient;
use std::path::Path;
use std::sync::Arc;

/// Performs a recovery procedure by fetching necessary data from remote storage.
/// It should and can only be called on a fresh install of the app, if the user wants to recover a previously created wallet.
/// If no existing wallet backup is found, returns an error.
pub fn recover_lightning_node(
    environment: EnvironmentCode,
    seed: Vec<u8>,
    local_persistence_path: String,
    enable_file_logging: bool,
) -> Result<()> {
    if enable_file_logging {
        init_logger_once(
            LOG_LEVEL,
            &Path::new(&local_persistence_path).join(LOGS_DIR),
        )?;
    }

    let db_path = format!("{local_persistence_path}/{DB_FILENAME}");
    if Path::new(&db_path).exists() {
        permanent_failure!(
            "Trying to recover when existing wallet data is present in local persistence path"
        )
    }

    let environment = Environment::load(environment);

    let strong_typed_seed = sanitize_input::strong_type_seed(&seed)?;
    let auth = Arc::new(build_async_auth(
        &strong_typed_seed,
        environment.backend_url.clone(),
    )?);

    let backup_client = RemoteBackupClient::new(environment.backend_url, auth);
    let backup_manager = BackupManager::new(
        backup_client,
        db_path,
        derive_persistence_encryption_key(&strong_typed_seed)?,
    );

    AsyncRuntime::new()?
        .handle()
        .block_on(backup_manager.recover())
}
