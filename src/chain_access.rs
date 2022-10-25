use crate::esplora_client::ConfirmedTransaction;
use crate::filter::FilterImpl;
use crate::ConfirmWrapper;
use crate::EsploraClient;
use bitcoin::{BlockHash, Script, Txid};
use esplora_client::Error;
use lightning::chain::transaction::OutPoint;
use lightning::chain::Confirm;
use log::debug;
use std::collections::HashSet;
use std::sync::Arc;

pub(crate) struct LipaChainAccess {
    esplora: Arc<EsploraClient>,
    filter: Arc<FilterImpl>,
    watched_txs: HashSet<(Txid, Script)>,
    // watched_outputs: HashSet<WatchedOutput>,
    watched_outputs: HashSet<WatchedOutputClone>,
    synced_tip: BlockHash,
}

impl LipaChainAccess {
    pub fn new(
        esplora_client: Arc<EsploraClient>,
        filter: Arc<FilterImpl>,
        synced_tip: BlockHash,
    ) -> Self {
        Self {
            esplora: esplora_client,
            filter,
            watched_txs: HashSet::new(),
            watched_outputs: HashSet::new(),
            synced_tip,
        }
    }

    pub(crate) fn sync(&mut self, confirm: &ConfirmWrapper) -> Result<(), Error> {
        let mut cur_tip = self.esplora.get_tip_hash()?;

        while self.synced_tip != cur_tip {
            debug!("Currently synced up to block hash: {}", self.synced_tip);
            debug!(
                "Current blockchain tip hash: {} -> Start syncing blockchain",
                cur_tip
            );

            self.sync_to_tip(confirm, &cur_tip)?;
            self.synced_tip = cur_tip;
            cur_tip = self.esplora.get_tip_hash()?;
        }

        Ok(())
    }

    fn sync_to_tip(&mut self, confirm: &ConfirmWrapper, cur_tip: &BlockHash) -> Result<(), Error> {
        self.inform_about_new_block(confirm, cur_tip)?;

        // todo: we need to make sure that the following synchronization is only done up to cur_tip.
        //       otherwise, we might end up in a situation where a block is being mined while while we're syncing
        //       which would make corrupt the order of the confirmed transactions as required by the Confirm trait:
        //       https://github.com/lightningdevkit/rust-lightning/blob/d6321e6e11b3e1b11e26617d3bab0b8c21da0b5b/lightning/src/chain/mod.rs#L128

        // The rationale behind looping here is that confirming or unconfirming transactions itself
        // might lead LDK to register additional transactions/outputs to watch.
        loop {
            self.sync_relevant_txs_for_reorgs(confirm)?;

            let mut confirmed_txs = self.sync_txs()?;
            confirmed_txs.extend(self.sync_spending_txs()?);
            debug!("{} confirmed transactions", confirmed_txs.len());
            debug!("Confirmed transaction list: {:?}", confirmed_txs);

            sort_txs(&mut confirmed_txs);

            for tx in confirmed_txs {
                confirm.transactions_confirmed(
                    &tx.block_header,
                    &[(tx.position, &tx.tx)],
                    tx.block_height,
                );
            }

            match self.filter.drain() {
                Some(filter_data) => {
                    filter_data.txs.iter().for_each(|tx| {
                        self.watched_txs.insert((tx.0, tx.1.clone()));
                    });
                    filter_data.outputs.into_iter().for_each(|output| {
                        let output = WatchedOutputClone {
                            block_hash: output.block_hash,
                            outpoint: output.outpoint,
                            script_pubkey: output.script_pubkey,
                        };
                        self.watched_outputs.insert(output);
                    });
                }
                None => break,
            }
        }

        Ok(())
    }

    fn sync_relevant_txs_for_reorgs(&self, confirm: &ConfirmWrapper) -> Result<(), Error> {
        for txid in confirm.get_relevant_txids().iter() {
            if !self.esplora.is_tx_confirmed(txid)? {
                debug!("Transactions reorged out of chain: {:?}", txid);
                confirm.transaction_unconfirmed(txid);
            }
        }

        Ok(())
    }

    fn inform_about_new_block(
        &self,
        confirm: &ConfirmWrapper,
        cur_tip: &BlockHash,
    ) -> Result<(), Error> {
        match self.esplora.get_header_with_height(cur_tip)? {
            Some((block_header, block_heigh)) => {
                confirm.best_block_updated(&block_header, block_heigh);

                Ok(())
            }
            // Block not found in best chain. Was there a reorg?
            None => Err(Error::HeaderHashNotFound(*cur_tip)),
        }
    }

    fn sync_txs(&mut self) -> Result<Vec<ConfirmedTransaction>, Error> {
        let mut confirmed_txs = Vec::new();
        let mut not_yet_confirmed_txs = HashSet::new();

        debug!("{} transactions to sync", self.watched_txs.len());
        debug!(
            "List of transactions to sync: {:?}",
            self.watched_txs
                .iter()
                .map(|tx| tx.0)
                .collect::<Vec<Txid>>()
        );

        for tx in self.watched_txs.iter() {
            let txid = tx.0;
            if let Some(confirmed_tx) = self.esplora.get_confirmed_tx_by_id(&txid)? {
                confirmed_txs.push(confirmed_tx);
                continue;
            }

            not_yet_confirmed_txs.insert((txid, tx.1.clone()));
        }
        self.watched_txs = not_yet_confirmed_txs;

        Ok(confirmed_txs)
    }

    fn sync_spending_txs(&mut self) -> Result<Vec<ConfirmedTransaction>, Error> {
        let mut confirmed_txs = Vec::new();
        let mut unspent_registered_outputs = HashSet::new();

        debug!("{} outputs to sync", self.watched_outputs.len());
        debug!(
            "List of outputs to sync: {:?}",
            self.watched_outputs
                .iter()
                .map(|output| output.outpoint)
                .collect::<Vec<OutPoint>>()
        );

        for output in self.watched_outputs.iter() {
            let txid = output.outpoint.txid;
            let index = output.outpoint.index as u64;
            if let Some(confirmed_tx) = self.esplora.get_confirmed_spending_tx(&txid, index)? {
                confirmed_txs.push(confirmed_tx);
                continue;
            }
            unspent_registered_outputs.insert(output.clone());
        }

        self.watched_outputs = unspent_registered_outputs;

        Ok(confirmed_txs)
    }
}

// Sorting by blocks and by transaction position within the each block
// From the Confirm trait documentation:
// - Transactions confirmed in a block must be given before transactions confirmed in a later
//   block.
// - Dependent transactions within the same block must be given in topological order, possibly in
//   separate calls.
fn sort_txs(txs: &mut [ConfirmedTransaction]) {
    txs.sort_unstable_by_key(|tx| (tx.block_height, tx.position));
}

#[cfg(test)]
mod tests {
    use crate::chain_access::sort_txs;
    use crate::esplora_client::ConfirmedTransaction;
    use bitcoin::consensus::deserialize;
    use bitcoin::hashes::hex::FromHex;
    use bitcoin::hashes::Hash;
    use bitcoin::{BlockHash, BlockHeader, Transaction, TxMerkleNode};

    fn build_sample_tx() -> Transaction {
        let tx_bytes = Vec::from_hex(
            "02000000000101595895ea20179de87052b4046dfe6fd515860505d6511a9004cf12a1f93cac7c01000000\
            00ffffffff01deb807000000000017a9140f3444e271620c736808aa7b33e370bd87cb5a078702483045022\
            100fb60dad8df4af2841adc0346638c16d0b8035f5e3f3753b88db122e70c79f9370220756e6633b17fd271\
            0e626347d28d60b0a2d6cbb41de51740644b9fb3ba7751040121028fa937ca8cba2197a37c007176ed89410\
            55d3bcb8627d085e94553e62f057dcc00000000"
        ).unwrap();
        let tx: Result<Transaction, _> = deserialize(&tx_bytes);
        tx.unwrap()
    }

    fn build_default_block_header() -> BlockHeader {
        BlockHeader {
            version: 0,
            prev_blockhash: BlockHash::all_zeros(),
            merkle_root: TxMerkleNode::all_zeros(),
            time: 0,
            bits: 0,
            nonce: 0,
        }
    }

    #[test]
    fn txs_are_sorted_by_block_and_position_within_block() {
        let mut txs = vec![
            ConfirmedTransaction {
                tx: build_sample_tx(),
                block_height: 10,
                block_header: build_default_block_header(),
                position: 10,
            },
            ConfirmedTransaction {
                tx: build_sample_tx(),
                block_height: 5,
                block_header: build_default_block_header(),
                position: 5,
            },
            ConfirmedTransaction {
                tx: build_sample_tx(),
                block_height: 10,
                block_header: build_default_block_header(),
                position: 5,
            },
            ConfirmedTransaction {
                tx: build_sample_tx(),
                block_height: 5,
                block_header: build_default_block_header(),
                position: 10,
            },
        ];

        let unsorted: Vec<(u32, usize)> = txs
            .iter()
            .map(|ctx| (ctx.block_height, ctx.position))
            .collect();

        sort_txs(&mut txs);

        let sorted: Vec<(u32, usize)> = txs
            .iter()
            .map(|ctx| (ctx.block_height, ctx.position))
            .collect();

        assert_eq!(unsorted, vec![(10, 10), (5, 5), (10, 5), (5, 10)]);
        assert_eq!(sorted, vec![(5, 5), (5, 10), (10, 5), (10, 10)]);
    }
}

// todo this is only required until an LDK version is used, that ships this PR:
//      https://github.com/lightningdevkit/rust-lightning/pull/1763
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct WatchedOutputClone {
    /// First block where the transaction output may have been spent.
    pub block_hash: Option<BlockHash>,

    /// Outpoint identifying the transaction output.
    pub outpoint: OutPoint,

    /// Spending condition of the transaction output.
    pub script_pubkey: Script,
}
