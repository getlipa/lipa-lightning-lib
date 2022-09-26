use crate::errors::RuntimeError;
use bitcoin::{Script, Transaction, Txid};
use esplora_client::blocking::BlockingClient;
use lightning::chain::{Confirm, Filter, WatchedOutput};
use std::sync::{Arc, Mutex};

#[allow(dead_code)]
pub struct LipaChainAccess {
    esplora_client: Arc<BlockingClient>,
    queued_txs: Mutex<Vec<(Txid, Script)>>,
    watched_txs: Mutex<Vec<(Txid, Script)>>,
    queued_outputs: Mutex<Vec<WatchedOutput>>,
    watched_outputs: Mutex<Vec<WatchedOutput>>,
}

impl LipaChainAccess {
    pub(crate) fn new(esplora_client: Arc<BlockingClient>) -> Self {
        let queued_txs = Mutex::new(Vec::new());
        let watched_txs = Mutex::new(Vec::new());
        let queued_outputs = Mutex::new(Vec::new());
        let watched_outputs = Mutex::new(Vec::new());
        Self {
            esplora_client,
            queued_txs,
            watched_txs,
            queued_outputs,
            watched_outputs,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn sync(
        &self,
        _confirmables: Vec<&(dyn Confirm + Sync)>,
    ) -> Result<(), RuntimeError> {
        // TODO: sync with the chain
        // this will include moving queued txs and outputs to watched ones

        Ok(())
    }
}

impl Filter for LipaChainAccess {
    fn register_tx(&self, txid: &Txid, script_pubkey: &Script) {
        self.queued_txs
            .lock()
            .unwrap()
            .push((*txid, script_pubkey.clone()));
    }

    fn register_output(&self, output: WatchedOutput) -> Option<(usize, Transaction)> {
        self.queued_outputs.lock().unwrap().push(output);
        // The Filter::register_output return value has been removed, as it was very difficult to
        // correctly implement (i.e., without blocking). Users previously using it should instead
        // pass dependent transactions in via additional chain::Confirm::transactions_confirmed
        // calls (#1663).
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::LipaChainAccess;
    use bitcoin::consensus::deserialize;
    use bitcoin::hashes::hex::FromHex;
    use bitcoin::Transaction;
    use esplora_client::Builder;
    use lightning::chain::transaction::OutPoint;
    use lightning::chain::{Filter, WatchedOutput};
    use std::sync::Arc;

    fn build_filter() -> LipaChainAccess {
        // 9 is a discard port
        // See https://en.wikipedia.org/wiki/Port_(computer_networking)
        let builder = Builder::new("http://localhost:9");
        let esplora_client = Arc::new(builder.build_blocking().unwrap());
        LipaChainAccess::new(esplora_client.clone())
    }

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
    fn filter_is_initialised_empty() {
        let filter = build_filter();

        assert!(filter.queued_txs.lock().unwrap().is_empty());
        assert!(filter.queued_outputs.lock().unwrap().is_empty());
        assert!(filter.watched_txs.lock().unwrap().is_empty());
        assert!(filter.watched_outputs.lock().unwrap().is_empty());
    }

    #[test]
    fn tx_is_registered() {
        let filter = build_filter();

        let tx = build_sample_tx();

        filter.register_tx(&tx.txid(), &tx.output[0].script_pubkey);

        assert_eq!(filter.queued_txs.lock().unwrap().len(), 1);
        assert_eq!(filter.queued_outputs.lock().unwrap().len(), 0);
        assert_eq!(filter.watched_txs.lock().unwrap().len(), 0);
        assert_eq!(filter.watched_outputs.lock().unwrap().len(), 0);

        assert_eq!(
            filter.queued_txs.lock().unwrap().get(0).unwrap(),
            &(tx.txid(), tx.output[0].script_pubkey.clone())
        );
    }

    #[test]
    fn output_is_registered() {
        let filter = build_filter();

        let tx = build_sample_tx();

        let output = WatchedOutput {
            block_hash: None,
            outpoint: OutPoint {
                txid: tx.txid(),
                index: 0,
            },
            script_pubkey: tx.output[0].script_pubkey.clone(),
        };

        filter.register_output(output.clone());

        assert_eq!(filter.queued_txs.lock().unwrap().len(), 0);
        assert_eq!(filter.queued_outputs.lock().unwrap().len(), 1);
        assert_eq!(filter.watched_txs.lock().unwrap().len(), 0);
        assert_eq!(filter.watched_outputs.lock().unwrap().len(), 0);

        let filter_output = filter
            .queued_outputs
            .lock()
            .unwrap()
            .get(0)
            .unwrap()
            .clone();

        assert_eq!(filter_output.script_pubkey, output.script_pubkey);
        assert_eq!(filter_output.outpoint, output.outpoint);
        assert_eq!(filter_output.block_hash, output.block_hash);
    }
}
