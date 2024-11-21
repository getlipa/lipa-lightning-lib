use crate::errors::Result;
use crate::locker::Locker;
use crate::support::Support;
use crate::{with_status, EnableStatus, RuntimeErrorCode};
use perro::MapToError;
use std::sync::Arc;

pub struct LightningAddress {
    support: Arc<Support>,
}

impl LightningAddress {
    pub(crate) fn new(support: Arc<Support>) -> Self {
        Self { support }
    }

    /// Register a human-readable lightning address or return the previously
    /// registered one.
    ///
    /// Requires network: **yes**
    pub fn register(&self) -> Result<String> {
        let address = self
            .support
            .rt
            .handle()
            .block_on(pigeon::assign_lightning_address(
                &self.support.node_config.remote_services_config.backend_url,
                &self.support.async_auth,
            ))
            .map_to_runtime_error(
                RuntimeErrorCode::AuthServiceUnavailable,
                "Failed to register a lightning address",
            )?;
        self.support
            .data_store
            .lock_unwrap()
            .store_lightning_address(&address)?;
        Ok(address)
    }

    /// Get the registered lightning address.
    ///
    /// Requires network: **no**
    pub fn get(&self) -> Result<Option<String>> {
        Ok(self
            .support
            .data_store
            .lock_unwrap()
            .retrieve_lightning_addresses()?
            .into_iter()
            .filter_map(with_status(EnableStatus::Enabled))
            .find(|a| !a.starts_with('-')))
    }
}
