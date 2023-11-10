use crate::data_store::BACKUP_DB_FILENAME_SUFFIX;
use crate::errors::Result;
use crate::symmetric_encryption::{decrypt, encrypt};
use crate::{runtime_error, RuntimeErrorCode};
use graphql::GraphQlRuntimeErrorCode;
use perro::MapToError;
use squirrel::{Backup, RemoteBackupClient};
use std::fs;

const SCHEMA_NAME: &str = "COMPLETE_DB";
const SCHEMA_VERSION: &str = "0";

pub(crate) struct BackupManager {
    remote_backup_client: RemoteBackupClient,
    local_db_path: String,
    local_backup_db_path: String,
    encryption_key: [u8; 32],
}

impl BackupManager {
    pub fn new(
        remote_backup_client: RemoteBackupClient,
        local_db_path: String,
        encryption_key: [u8; 32],
    ) -> Self {
        BackupManager {
            remote_backup_client,
            local_db_path: local_db_path.clone(),
            local_backup_db_path: format!("{}{BACKUP_DB_FILENAME_SUFFIX}", local_db_path),
            encryption_key,
        }
    }

    pub async fn backup(&self) -> Result<()> {
        let local_db = fs::read(&self.local_backup_db_path)
            .map_to_permanent_failure("Failed to read db file from local filesystem")?;
        let encrypted_local_db = encrypt(&local_db, &self.encryption_key)?;
        self.remote_backup_client
            .create_backup(&Backup {
                encrypted_backup: encrypted_local_db,
                schema_name: SCHEMA_NAME.to_string(),
                schema_version: SCHEMA_VERSION.to_string(),
            })
            .await
            .map_to_permanent_failure("Failed to perform backup of local db")
    }

    pub async fn recover(&self) -> Result<()> {
        let encrypted_local_db = match self.remote_backup_client.recover_backup(SCHEMA_NAME).await {
            Ok(b) => b.encrypted_backup,
            Err(perro::Error::RuntimeError {
                code: GraphQlRuntimeErrorCode::ObjectNotFound,
                ..
            }) => {
                runtime_error!(
                    RuntimeErrorCode::BackupNotFound,
                    "No backup was found in remote"
                )
            }
            Err(e) => {
                runtime_error!(
                    RuntimeErrorCode::BackupServiceUnavailable,
                    "Failed to fetch db backup from remote: {e}"
                )
            }
        };
        let local_db = decrypt(&encrypted_local_db, &self.encryption_key)?;
        fs::write(&self.local_db_path, local_db)
            .map_to_permanent_failure("Failed to write recovered db to filesystem")
    }
}
