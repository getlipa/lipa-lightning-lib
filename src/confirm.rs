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

    fn transaction_unconfirmed(&self, txid: &Txid) {}

    fn best_block_updated(&self, header: &BlockHeader, height: u32) {}

    fn get_relevant_txids(&self) -> Vec<Txid> {
        Vec::new()
    }
}
