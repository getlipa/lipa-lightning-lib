use crate::errors::*;
use crate::esplora_client::ConfirmedTransaction;
use crate::filter::FilterImpl;
use crate::ConfirmWrapper;
use crate::EsploraClient;

use bitcoin::{BlockHash, Txid};
use lightning::chain::{Confirm, WatchedOutput};
use log::debug;
use std::collections::HashSet;
use std::sync::Arc;

pub(crate) struct LipaChainAccess {
    esplora: Arc<EsploraClient>,
    filter: Arc<FilterImpl>,
    watched_txs: HashSet<Txid>,
    watched_outputs: HashSet<WatchedOutput>,
    synced_tip: BlockHash,
}

enum SyncResult {
    Success,
    PotentialReorg,
}

impl LipaChainAccess {
    pub fn new(
        esplora: Arc<EsploraClient>,
        filter: Arc<FilterImpl>,
        synced_tip: BlockHash,
    ) -> Self {
        Self {
            esplora,
            filter,
            watched_txs: HashSet::new(),
            watched_outputs: HashSet::new(),
            synced_tip,
        }
    }

    pub(crate) fn sync(&mut self, confirm: &ConfirmWrapper) -> LipaResult<()> {
        loop {
            let tip = self.esplora.get_tip_hash()?;
            let something_new_to_watch = match self.filter.drain() {
                Some(data) => {
                    self.watched_txs
                        .extend(data.txs.into_iter().map(|(txid, _)| txid));
                    self.watched_outputs.extend(data.outputs.into_iter());
                    true
                }
                None => false,
            };

            if tip != self.synced_tip || something_new_to_watch {
                debug!("Syncing to: {} ...", tip);
                match self.try_sync_to(tip, confirm)? {
                    SyncResult::Success => {
                        self.synced_tip = tip;
                        debug!("Synced to: {}", tip);
                    }
                    SyncResult::PotentialReorg => {
                        debug!("Potential reorg detected, sync attempt was aborted");
                    }
                };
            } else {
                break;
            }
        }

        Ok(())
    }

    fn try_sync_to(
        &mut self,
        tip_hash: BlockHash,
        confirm: &ConfirmWrapper,
    ) -> LipaResult<SyncResult> {
        let (tip_header, tip_height) = match self.esplora.get_header_with_height(&tip_hash)? {
            Some(tip) => tip,
            None => {
                return Ok(SyncResult::PotentialReorg);
            }
        };

        // 1. Query blockchain state.
        // TODO: Handle bock hash of unconfirmed txs.
        let unconfirmed_txids = self.filter_unconfirmed(confirm.get_relevant_txids())?;
        let (confirmed_txs, pending_txids) = self.split_txs_by_status(self.watched_txs.clone())?;
        let spent_outputs = self.get_spent_outputs(&self.watched_outputs)?;
        debug!("Unconfirmed txs: {:?}", unconfirmed_txids);
        debug!("Confirmed txs: {:?}", confirmed_txs.iter().map(txid));
        debug!("Still pending txs: {:?}", pending_txids);
        debug!("Spent outputs: {:?}", spent_outputs.iter().map(txid));

        // 2. Check if a potential reorg has happened while syncing.
        if tip_hash != self.esplora.get_tip_hash()? {
            return Ok(SyncResult::PotentialReorg);
        }

        // 3. Inform LDK about changes.
        for txid in &unconfirmed_txids {
            confirm.transaction_unconfirmed(txid);
        }

        for tx in join_sort_dedup(confirmed_txs, spent_outputs) {
            confirm.transactions_confirmed(
                &tx.block_header,
                &[(tx.position, &tx.tx)],
                tx.block_height,
            );
        }

        confirm.best_block_updated(&tip_header, tip_height);

        // 4. Update internal state.
        self.watched_txs.clear();
        // Keep just unconfirmed txs to check for confirmations on the next sync.
        self.watched_txs.extend(unconfirmed_txids);
        self.watched_txs.extend(pending_txids);
        // Note: We always keep watched outputs, since LDK does not inform us
        //       about outputs of interest if they were unspent (unconfirmed).

        Ok(SyncResult::Success)
    }

    fn filter_unconfirmed(&self, txids: Vec<(Txid, Option<BlockHash>)>) -> LipaResult<Vec<Txid>> {
        let mut uncofirmed = Vec::new();
        for (txid, _block_hash) in txids {
            if !self.esplora.is_tx_confirmed(&txid)? {
                uncofirmed.push(txid);
            }
        }
        Ok(uncofirmed)
    }

    fn split_txs_by_status(
        &self,
        txids: HashSet<Txid>,
    ) -> LipaResult<(Vec<ConfirmedTransaction>, Vec<Txid>)> {
        let mut confirmed = Vec::new();
        let mut pending = Vec::new();

        for txid in txids {
            match self.esplora.get_confirmed_tx_by_id(&txid)? {
                Some(tx) => {
                    confirmed.push(tx);
                }
                None => {
                    pending.push(txid);
                }
            }
        }

        Ok((confirmed, pending))
    }

    fn get_spent_outputs(
        &self,
        outputs: &HashSet<WatchedOutput>,
    ) -> LipaResult<Vec<ConfirmedTransaction>> {
        let mut spent_outputs = Vec::new();
        for output in outputs {
            if let Some(tx) = self
                .esplora
                .get_confirmed_spending_tx(&output.outpoint.txid, output.outpoint.index as u64)?
            {
                spent_outputs.push(tx);
            }
        }
        Ok(spent_outputs)
    }
}

fn join_sort_dedup(
    mut lhs: Vec<ConfirmedTransaction>,
    mut rhs: Vec<ConfirmedTransaction>,
) -> Vec<ConfirmedTransaction> {
    lhs.append(&mut rhs);
    sort_txs(&mut lhs);
    lhs.dedup();
    lhs
}

fn txid(tx: &ConfirmedTransaction) -> Txid {
    tx.tx.txid()
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
