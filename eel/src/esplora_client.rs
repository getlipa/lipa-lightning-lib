use crate::errors::*;
use std::collections::HashMap;

use bitcoin::{BlockHeader, Transaction};
use esplora_client::blocking::BlockingClient;
use esplora_client::Builder;
use perro::MapToError;

static ESPLORA_TIMEOUT_SECS: u64 = 30;

pub(crate) struct EsploraClient {
    client: BlockingClient,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ConfirmedTransaction {
    pub tx: Transaction,
    pub block_height: u32,
    pub block_header: BlockHeader,
    pub position: usize, // position within the block
}

impl EsploraClient {
    pub fn new(url: &str) -> Result<Self> {
        let builder = Builder::new(url).timeout(ESPLORA_TIMEOUT_SECS);
        Ok(Self {
            client: builder.build_blocking().map_to_runtime_error(
                RuntimeErrorCode::EsploraServiceUnavailable,
                "Failed to build Esplora client",
            )?,
        })
    }

    pub fn broadcast(&self, tx: &Transaction) -> Result<()> {
        self.client.broadcast(tx).map_to_runtime_error(
            RuntimeErrorCode::EsploraServiceUnavailable,
            "Esplora failed to broadcast tx",
        )
    }

    pub fn get_fee_estimates(&self) -> Result<HashMap<String, f64>> {
        self.client.get_fee_estimates().map_to_runtime_error(
            RuntimeErrorCode::EsploraServiceUnavailable,
            "Esplora failed to get fee estimates",
        )
    }
}
