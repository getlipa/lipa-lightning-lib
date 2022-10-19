use crate::errors::RuntimeError;
use crate::filter::FilterImpl;
use esplora_client::blocking::BlockingClient;
use lightning::chain::Confirm;
use std::sync::Arc;

#[allow(dead_code)]
pub(crate) struct LipaChainAccess {
    esplora_client: Arc<BlockingClient>,
    filter: Arc<FilterImpl>,
}

impl LipaChainAccess {
    pub fn new(esplora_client: Arc<BlockingClient>, filter: Arc<FilterImpl>) -> Self {
        Self {
            esplora_client,
            filter,
        }
    }

    pub fn sync(&self, _confirm: &(dyn Confirm + Sync)) -> Result<(), RuntimeError> {
        // TODO: sync with the chain

        if let Some(_filter_data) = self.filter.drain() {}
        Ok(())
    }
}
