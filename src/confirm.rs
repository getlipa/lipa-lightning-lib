use bitcoin::{blockdata::block::BlockHeader, Txid};
use lightning::chain::transaction::TransactionData;
use lightning::chain::Confirm;

pub(crate) struct ConfirmWrapper<'a> {
    members: Vec<&'a (dyn Confirm + Sync)>,
}

impl<'a> ConfirmWrapper<'a> {
    pub fn new(members: Vec<&'a (dyn Confirm + Sync)>) -> Self {
        Self { members }
    }
}

impl<'a> Confirm for ConfirmWrapper<'a> {
    fn transactions_confirmed(
        &self,
        header: &BlockHeader,
        txdata: &TransactionData<'_>,
        height: u32,
    ) {
        for member in &self.members {
            member.transactions_confirmed(header, txdata, height);
        }
    }

    fn transaction_unconfirmed(&self, txid: &Txid) {
        for member in &self.members {
            member.transaction_unconfirmed(txid);
        }
    }

    fn best_block_updated(&self, header: &BlockHeader, height: u32) {
        for member in &self.members {
            member.best_block_updated(header, height);
        }
    }

    /// Returns relevant tx ids of all members joined and deduplicated.
    fn get_relevant_txids(&self) -> Vec<Txid> {
        let mut joined = Vec::new();
        for member in &self.members {
            joined.extend_from_slice(&member.get_relevant_txids());
        }
        unique(joined)
    }
}

fn unique(mut vec: Vec<Txid>) -> Vec<Txid> {
    vec.sort_unstable();
    vec.dedup();
    vec
}
