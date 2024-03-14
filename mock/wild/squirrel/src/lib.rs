use graphql::errors::*;
use honey_badger::asynchronous::Auth;
use std::sync::Arc;

pub use squirrel::Backup;
pub struct RemoteBackupClient {}

impl RemoteBackupClient {
    pub fn new(_backend_url: String, _auth: Arc<Auth>) -> Self {
        Self {}
    }

    pub async fn create_backup(&self, _backup: &Backup) -> Result<()> {
        Ok(())
    }

    pub async fn recover_backup(&self, _schema_name: &str) -> Result<Backup> {
        Err(Error::RuntimeError {
            code: GraphQlRuntimeErrorCode::ObjectNotFound,
            msg: "No backup found with the provided schema name".to_string(),
        })
    }
}
