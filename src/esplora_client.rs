// Note by Gabriel Comte: This file is for the most part a copy from here:
// https://github.com/bitcoindevkit/rust-esplora-client/blob/master/src/async.rs
//
// Once the functionality has been merged into BDK, we should import the functionality from there,
// respectively use the rust-esplra-client: https://github.com/bitcoindevkit/rust-esplora-client

// Bitcoin Dev Kit
// Written in 2020 by Alekos Filini <alekos.filini@gmail.com>
//
// Copyright (c) 2020-2021 Bitcoin Dev Kit Developers
//
// This file is licensed under the Apache License, Version 2.0 <LICENSE-APACHE
// or http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your option.
// You may not use this file except in accordance with one or both of these
// licenses.

//! Esplora by way of `reqwest` HTTP client.

use std::str::FromStr;
use std::thread;
use std::time::Duration;

use bitcoin::consensus::{deserialize, serialize};
use bitcoin::hashes::hex::{FromHex, ToHex};
use bitcoin::{BlockHash, BlockHeader, Transaction, Txid};
use lightning::chain::chaininterface::{BroadcasterInterface, ConfirmationTarget, FeeEstimator};
use reqwest::{Client, StatusCode};

#[allow(unused_imports)]
use log::{debug, error, info, trace};

use crate::esplora_client_api::{Error, MerkleProof, OutputStatus, TxStatus};

#[derive(Debug)]
pub struct EsploraClient {
    url: String,
    client: Client,
}

impl EsploraClient {
    /// build an async client from the base url and [`Client`]
    pub fn new(url: &str) -> Self {
        let client = Client::new();
        EsploraClient {
            url: url.to_string(),
            client,
        }
    }

    /// Get a [`Transaction`] option given its [`Txid`]
    pub async fn get_tx(&self, txid: &Txid) -> Result<Option<Transaction>, Error> {
        let resp = self
            .client
            .get(&format!("{}/tx/{}/raw", self.url, txid))
            .send()
            .await?;

        if let StatusCode::NOT_FOUND = resp.status() {
            return Ok(None);
        }

        Ok(Some(deserialize(&resp.error_for_status()?.bytes().await?)?))
    }

    // /// Get a [`Transaction`] given its [`Txid`].
    // pub async fn get_tx_no_opt(&self, txid: &Txid) -> Result<Transaction, Error> {
    //     match self.get_tx(txid).await {
    //         Ok(Some(tx)) => Ok(tx),
    //         Ok(None) => Err(Error::TransactionNotFound(*txid)),
    //         Err(e) => Err(e),
    //     }
    // }

    // /// Get a [`Txid`] of a transaction given its index in a block with a given hash.
    // pub async fn get_txid_at_block_index(
    //     &self,
    //     block_hash: &BlockHash,
    //     index: usize,
    // ) -> Result<Option<Txid>, Error> {
    //     let resp = self
    //         .client
    //         .get(&format!(
    //             "{}/block/{}/txid/{}",
    //             self.url,
    //             block_hash.to_string(),
    //             index
    //         ))
    //         .send()
    //         .await?;
    //
    //     if let StatusCode::NOT_FOUND = resp.status() {
    //         return Ok(None);
    //     }
    //
    //     Ok(Some(deserialize(&Vec::from_hex(&resp.text().await?)?)?))
    // }

    /// Get the status of a [`Transaction`] given its [`Txid`].
    pub async fn get_tx_status(&self, txid: &Txid) -> Result<Option<TxStatus>, Error> {
        let resp = self
            .client
            .get(&format!("{}/tx/{}/status", self.url, txid))
            .send()
            .await?;

        if let StatusCode::NOT_FOUND = resp.status() {
            return Ok(None);
        }

        Ok(Some(resp.error_for_status()?.json().await?))
    }

    /// Get a [`BlockHeader`] given a particular block height.
    pub async fn get_header(&self, block_height: u32) -> Result<BlockHeader, Error> {
        let resp = self
            .client
            .get(&format!("{}/block-height/{}", self.url, block_height))
            .send()
            .await?;

        if let StatusCode::NOT_FOUND = resp.status() {
            return Err(Error::HeaderHeightNotFound(block_height));
        }
        let bytes = resp.bytes().await?;
        let hash =
            std::str::from_utf8(&bytes).map_err(|_| Error::HeaderHeightNotFound(block_height))?;

        let resp = self
            .client
            .get(&format!("{}/block/{}/header", self.url, hash))
            .send()
            .await?;

        let header = deserialize(&Vec::from_hex(&resp.text().await?)?)?;

        Ok(header)
    }

    /// Get a merkle inclusion proof for a [`Transaction`] with the given [`Txid`].
    pub async fn get_merkle_proof(&self, tx_hash: &Txid) -> Result<Option<MerkleProof>, Error> {
        let resp = self
            .client
            .get(&format!("{}/tx/{}/merkle-proof", self.url, tx_hash))
            .send()
            .await?;

        if let StatusCode::NOT_FOUND = resp.status() {
            return Ok(None);
        }

        Ok(Some(resp.error_for_status()?.json().await?))
    }

    /// Get the spending status of an output given a [`Txid`] and the output index.
    pub async fn get_output_status(
        &self,
        txid: &Txid,
        index: u64,
    ) -> Result<Option<OutputStatus>, Error> {
        let resp = self
            .client
            .get(&format!("{}/tx/{}/outspend/{}", self.url, txid, index))
            .send()
            .await?;

        if let StatusCode::NOT_FOUND = resp.status() {
            return Ok(None);
        }

        Ok(Some(resp.error_for_status()?.json().await?))
    }

    /// Broadcast a [`Transaction`] to Esplora
    pub async fn broadcast(&self, transaction: &Transaction) -> Result<(), Error> {
        self.client
            .post(&format!("{}/tx", self.url))
            .body(serialize(transaction).to_hex())
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    /// Get the current height of the blockchain tip
    pub async fn get_height(&self) -> Result<u32, Error> {
        let req = self
            .client
            .get(&format!("{}/blocks/tip/height", self.url))
            .send()
            .await?;

        Ok(req.error_for_status()?.text().await?.parse()?)
    }

    // /// Get the [`BlockHash`] of the current blockchain tip.
    // pub async fn get_tip_hash(&self) -> Result<BlockHash, Error> {
    //     let resp = self
    //         .client
    //         .get(&format!("{}/blocks/tip/hash", self.url))
    //         .send()
    //         .await?;
    //
    //     Ok(BlockHash::from_str(
    //         &resp.error_for_status()?.text().await?,
    //     )?)
    // }

    /// Get the [`BlockHash`] of a specific block height
    pub async fn get_block_hash(&self, height: u32) -> Result<BlockHash, Error> {
        let resp = self
            .client
            .get(&format!("{}/block-height/{}", self.url, height))
            .send()
            .await?;

        Ok(BlockHash::from_str(
            &resp.error_for_status()?.text().await?,
        )?)
    }

    // /// Get confirmed transaction history for the specified address/scripthash,
    // /// sorted with newest first. Returns 25 transactions per page.
    // /// More can be requested by specifying the last txid seen by the previous query.
    // pub async fn scripthash_txs(
    //     &self,
    //     script: &Script,
    //     last_seen: Option<Txid>,
    // ) -> Result<Vec<Tx>, Error> {
    //     let script_hash = sha256::Hash::hash(script.as_bytes()).into_inner().to_hex();
    //     let url = match last_seen {
    //         Some(last_seen) => format!(
    //             "{}/scripthash/{}/txs/chain/{}",
    //             self.url, script_hash, last_seen
    //         ),
    //         None => format!("{}/scripthash/{}/txs", self.url, script_hash),
    //     };
    //     Ok(self
    //         .client
    //         .get(url)
    //         .send()
    //         .await?
    //         .error_for_status()?
    //         .json::<Vec<Tx>>()
    //         .await?)
    // }

    // /// Get an map where the key is the confirmation target (in number of blocks)
    // /// and the value is the estimated feerate (in sat/vB).
    // pub async fn get_fee_estimates(&self) -> Result<HashMap<String, f64>, Error> {
    //     Ok(self
    //         .client
    //         .get(&format!("{}/fee-estimates", self.url,))
    //         .send()
    //         .await?
    //         .error_for_status()?
    //         .json::<HashMap<String, f64>>()
    //         .await?)
    // }
}

impl BroadcasterInterface for EsploraClient {
    fn broadcast_transaction(&self, tx: &Transaction) {
        loop {
            let result = tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(self.broadcast(tx))
                .map_err(|e| {
                    error!("Error broadcasting transaction: {}", e);
                    e
                });

            if result.is_ok() {
                return;
            }

            // try again in 1 second
            thread::sleep(Duration::from_secs(1));
        }
    }
}
impl FeeEstimator for EsploraClient {
    fn get_est_sat_per_1000_weight(&self, _confirmation_target: ConfirmationTarget) -> u32 {
        // todo
        10000u32
    }
}
