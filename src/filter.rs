use bitcoin::{Script, Txid};
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
    pub fn new() -> Self {
        Self {
            data: Mutex::new(FilterData {
                txs: Vec::new(),
                outputs: Vec::new(),
            }),
        }
    }

    /// Returns `FilterData` if non empty, clean the local state.
    pub fn drain(&self) -> Option<FilterData> {
        let mut data = self.data.lock().unwrap();
        if !data.txs.is_empty() || !data.outputs.is_empty() {
            return Some(FilterData {
                txs: data.txs.drain(..).collect(),
                outputs: data.outputs.drain(..).collect(),
            });
        }
        None
    }
}

impl Filter for FilterImpl {
    fn register_tx(&self, txid: &Txid, script_pubkey: &Script) {
        self.data
            .lock()
            .unwrap()
            .txs
            .push((*txid, script_pubkey.clone()));
    }

    fn register_output(&self, output: WatchedOutput) {
        self.data.lock().unwrap().outputs.push(output);
    }
}

#[cfg(test)]
mod tests {
    use super::FilterImpl;
    use bitcoin::consensus::deserialize;
    use bitcoin::hashes::hex::FromHex;
    use bitcoin::Transaction;
    use lightning::chain::transaction::OutPoint;
    use lightning::chain::{Filter, WatchedOutput};

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

    #[test]
    fn test_drain_empty_filter() {
        let filter = FilterImpl::new();
        assert!(filter.drain().is_none());
        // The filter is still empty.
        assert!(filter.drain().is_none());
    }

    #[test]
    fn test_register_tx() {
        let filter = FilterImpl::new();
        let tx = build_sample_tx();
        filter.register_tx(&tx.txid(), &tx.output[0].script_pubkey);

        let data = filter.drain().unwrap();
        assert_eq!(data.txs.len(), 1);
        assert_eq!(data.outputs.len(), 0);

        // Now the filter is empty.
        assert!(filter.drain().is_none());
    }

    #[test]
    fn test_register_output() {
        let filter = FilterImpl::new();
        let tx = build_sample_tx();
        let output = WatchedOutput {
            block_hash: None,
            outpoint: OutPoint {
                txid: tx.txid(),
                index: 0,
            },
            script_pubkey: tx.output[0].script_pubkey.clone(),
        };
        filter.register_output(output);

        let data = filter.drain().unwrap();
        assert_eq!(data.txs.len(), 0);
        assert_eq!(data.outputs.len(), 1);

        // Now the filter is empty.
        assert!(filter.drain().is_none());
    }
}
