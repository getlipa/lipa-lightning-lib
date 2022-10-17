use bitcoin::{Script, Transaction, Txid};
use lightning::chain::{Filter, WatchedOutput};
use std::sync::Mutex;

pub(crate) struct FilterData {
    pub txs: Vec<(Txid, Script)>,
    pub outputs: Vec<WatchedOutput>,
}

pub(crate) struct FilterImpl {
    data: Mutex<FilterData>,
}

impl FilterImpl {
    /// Returns `FilterData` if non empty, clean the local state.
    pub fn pop(&mut self) -> Option<FilterData> {
        None
    }
}

impl Filter for FilterImpl {
    fn register_tx(&self, txid: &Txid, script_pubkey: &Script) {}

    fn register_output(&self, output: WatchedOutput) -> Option<(usize, Transaction)> {
        None
    }
}
