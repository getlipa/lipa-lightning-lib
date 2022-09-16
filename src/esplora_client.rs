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
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use bitcoin::consensus::{deserialize, serialize};
use bitcoin::hashes::hex::{FromHex, ToHex};
use bitcoin::{BlockHash, BlockHeader, Transaction, Txid};
use lightning::chain::chaininterface::{BroadcasterInterface, ConfirmationTarget, FeeEstimator};
use lightning::chain::Confirm;
use reqwest::{Client, StatusCode};

use crate::{ChainFilter, ChainMonitor, ChannelManager};
#[allow(unused_imports)]
use log::{debug, error, info, trace};
use rand::distributions::Exp;
use tokio::sync::Mutex;

use crate::esplora_client_api::{Error, MerkleProof, OutputStatus, TxStatus};

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

pub(crate) struct EsploraClientSync {
    esplora_client: EsploraClient,
    filter: Arc<ChainFilter>,
    channel_manager: Arc<ChannelManager>,
    chain_monitor: Arc<ChainMonitor>,
}

impl EsploraClientSync {
    pub(crate) fn new(
        url: String,
        filter: Arc<ChainFilter>,
        channel_manager: Arc<ChannelManager>,
        chain_monitor: Arc<ChainMonitor>,
    ) -> Self {
        let client = EsploraClient::new(url.as_str());
        EsploraClientSync {
            esplora_client: client,
            filter,
            channel_manager,
            chain_monitor,
        }
    }

    // pub(crate) fn sync_wallet(&self) -> Result<(), Error> {
    //     let sync_options = SyncOptions { progress: None };
    //
    //     self.wallet
    //         .lock()
    //         .unwrap()
    //         .sync(&self.blockchain, sync_options)
    //         .map_err(|e| Error::Bdk(e))?;
    //
    //     Ok(())
    // }

    pub(crate) async fn sync(&self) -> Result<(), Error> {
        let confirmables: Vec<Arc<dyn Confirm + Sync + Send>> =
            vec![self.channel_manager.clone(), self.chain_monitor.clone()];

        let cur_height = self.esplora_client.get_height().await?;

        // todo last_sync_height needs to be persisted
        // let mut locked_last_sync_height = self.last_sync_height.lock().unwrap();
        // Todo: mocking some value for now ...
        let current_sync_height_mock = Mutex::new(Some(10));

        let mut locked_last_sync_height = current_sync_height_mock.lock().await;
        if cur_height >= locked_last_sync_height.unwrap_or(0) {
            {
                // First, inform the interface of the new block.
                let cur_block_header = self.esplora_client.get_header(cur_height).await?;

                for c in &confirmables {
                    c.best_block_updated(&cur_block_header, cur_height);
                }

                *locked_last_sync_height = Some(cur_height);
            }

            {
                // First, check the confirmation status of registered transactions as well as the
                // status of dependent transactions of registered outputs.
                // let mut locked_queued_transactions = self.queued_transactions.lock().unwrap();
                // let mut locked_queued_outputs = self.queued_outputs.lock().unwrap();
                // let mut locked_watched_transactions = self.watched_transactions.lock().unwrap();
                // let mut locked_watched_outputs = self.watched_outputs.lock().unwrap();

                let mut confirmed_txs = Vec::new();

                // Check in the current queue, as well as in registered transactions leftover from
                // previous iterations.
                // let mut registered_txs: Vec<Txid> = locked_watched_transactions
                //     .iter()
                //     .chain(locked_queued_transactions.iter())
                //     .cloned()
                //     .collect();

                let mut tx_filter = self.filter.filter.lock().unwrap();

                tx_filter
                    .watched_transactions
                    .sort_unstable_by(|txid1, txid2| txid1.cmp(&txid2));
                tx_filter
                    .watched_transactions
                    .dedup_by(|txid1, txid2| txid1.eq(&txid2));

                // Remember all registered but unconfirmed transactions for future processing.
                let mut unconfirmed_registered_txs = Vec::new();

                for txid in tx_filter.watched_transactions.iter() {
                    let txid = txid.0;
                    if let Some(tx_status) = self.esplora_client.get_tx_status(&txid).await? {
                        if tx_status.confirmed {
                            if let Some(tx) = self.esplora_client.get_tx(&txid).await? {
                                if let Some(block_height) = tx_status.block_height {
                                    let block_header =
                                        self.esplora_client.get_header(block_height).await?;
                                    if let Some(merkle_proof) =
                                        self.esplora_client.get_merkle_proof(&txid).await?
                                    {
                                        confirmed_txs.push((
                                            tx,
                                            block_height,
                                            block_header,
                                            merkle_proof.pos,
                                        ));
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                    unconfirmed_registered_txs.push(txid);
                }

                // Check all registered outputs for dependent spending transactions.
                // let registered_outputs: Vec<WatchedOutput> = locked_watched_outputs
                //     .iter()
                //     .chain(locked_queued_outputs.iter())
                //     .cloned()
                //     .collect();

                // Remember all registered outputs that haven't been spent for future processing.
                let mut unspent_registered_outputs = Vec::new();

                for output in tx_filter.watched_outputs.iter() {
                    if let Some(output_status) = self
                        .esplora_client
                        .get_output_status(&output.outpoint.txid, output.outpoint.index as u64)
                        .await?
                    {
                        if output_status.spent {
                            if let Some(spending_tx_status) = output_status.status {
                                if spending_tx_status.confirmed {
                                    let spending_txid = output_status.txid.unwrap();
                                    if let Some(spending_tx) =
                                        self.esplora_client.get_tx(&spending_txid).await?
                                    {
                                        let block_height = spending_tx_status.block_height.unwrap();
                                        let block_header =
                                            self.esplora_client.get_header(block_height).await?;
                                        if let Some(merkle_proof) = self
                                            .esplora_client
                                            .get_merkle_proof(&spending_txid)
                                            .await?
                                        {
                                            confirmed_txs.push((
                                                spending_tx,
                                                block_height,
                                                block_header,
                                                merkle_proof.pos,
                                            ));
                                            continue;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    unspent_registered_outputs.push(output);
                }

                // Sort all confirmed transactions by block height and feed them to the interface
                // in order.
                confirmed_txs.sort_unstable_by(
                    |(_, block_height1, _, _), (_, block_height2, _, _)| {
                        block_height1.cmp(&block_height2)
                    },
                );
                for (tx, block_height, block_header, pos) in confirmed_txs {
                    for c in &confirmables {
                        c.transactions_confirmed(&block_header, &[(pos, &tx)], block_height);
                    }
                }

                // *locked_watched_transactions = unconfirmed_registered_txs;
                // *locked_queued_transactions = Vec::new();
                // *locked_watched_outputs = unspent_registered_outputs;
                // *locked_queued_outputs = Vec::new();
            }

            {
                /* todo check for reorgs

                // Query the interface for relevant txids and check whether they have been
                // reorged-out of the chain.
                let unconfirmed_txids_unfiltered = confirmables
                    .iter()
                    .flat_map(|c| c.get_relevant_txids())
                    .collect::<Vec<Txid>>();

                let mut unconfirmed_txids = Vec::new();

                for unconfirmed_txid in unconfirmed_txids_unfiltered.iter() {
                    if client
                        .get_tx_status(unconfirmed_txid).await
                        .ok()
                        .unwrap_or(None)
                        .map_or(true, |status| !status.confirmed) {
                        unconfirmed_txids.push(*unconfirmed_txid);
                    }
                }

                // Mark all relevant unconfirmed transactions as unconfirmed.
                for txid in &unconfirmed_txids {
                    for c in &confirmables {
                        c.transaction_unconfirmed(txid);
                    }
                }

                 */
            }
        }

        // TODO: check whether new outputs have been registered by now and process them
        Ok(())
    }

    // pub(crate) fn create_funding_transaction(
    //     &self, output_script: &Script, value_sats: u64, confirmation_target: ConfirmationTarget,
    // ) -> Result<Transaction, Error> {
    //     let num_blocks = num_blocks_from_conf_target(confirmation_target);
    //     let fee_rate = self.blockchain.estimate_fee(num_blocks)?;
    //
    //     let locked_wallet = self.wallet.lock().unwrap();
    //     let mut tx_builder = locked_wallet.build_tx();
    //
    //     tx_builder.add_recipient(output_script.clone(), value_sats).fee_rate(fee_rate).enable_rbf();
    //
    //     let (mut psbt, _) = tx_builder.finish()?;
    //     log_trace!(self.logger, "Created funding PSBT: {:?}", psbt);
    //
    //     // We double-check that no inputs try to spend non-witness outputs. As we use a SegWit
    //     // wallet descriptor this technically shouldn't ever happen, but better safe than sorry.
    //     for input in &psbt.inputs {
    //         if input.witness_utxo.is_none() {
    //             return Err(Error::FundingTxNonWitnessOuputSpend);
    //         }
    //     }
    //
    //     let finalized = locked_wallet.sign(&mut psbt, SignOptions::default())?;
    //     if !finalized {
    //         return Err(Error::FundingTxNotFinalized);
    //     }
    //
    //     Ok(psbt.extract_tx())
    // }
    //
    // pub(crate) fn get_new_address(&self) -> Result<bitcoin::Address, Error> {
    //     let address_info = self.wallet.lock().unwrap().get_address(AddressIndex::New)?;
    //     Ok(address_info.address)
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
