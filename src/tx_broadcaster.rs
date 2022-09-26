use bitcoin::blockdata::transaction::Transaction;
use esplora_client::blocking::BlockingClient;
use lightning::chain::chaininterface::BroadcasterInterface;
use log::error;
use std::sync::Arc;

pub struct TxBroadcaster {
    esplora_client: Arc<BlockingClient>,
}

impl TxBroadcaster {
    pub fn new(esplora_client: Arc<BlockingClient>) -> Self {
        Self { esplora_client }
    }
}

impl BroadcasterInterface for TxBroadcaster {
    fn broadcast_transaction(&self, tx: &Transaction) {
        let tx = tx.clone();
        let txid = tx.txid();

        // TODO: Better handle errors. Should we retry?
        if let Err(e) = self.esplora_client.broadcast(&tx) {
            error!("Error on broadcasting txid: {} message: {}", txid, e);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use bitcoin::consensus::deserialize;
    use bitcoin_hashes::hex::FromHex;
    use esplora_client::Builder;
    use simplelog;

    #[test]
    // Run the test `cargo test test_broadcast_failure -- --nocapture` to see logs.
    fn test_broadcast_failure() {
        simplelog::TestLogger::init(log::LevelFilter::Error, simplelog::Config::default()).unwrap();

        // 9 is a discard port
        // See https://en.wikipedia.org/wiki/Port_(computer_networking)
        let builder = Builder::new("http://localhost:9");
        let esplora_client = Arc::new(builder.build_blocking().unwrap());
        let broadcaster = TxBroadcaster::new(esplora_client);

        let tx_bytes = Vec::from_hex(
            "02000000000101595895ea20179de87052b4046dfe6fd515860505d6511a9004cf12a1f93cac7c01000000\
            00ffffffff01deb807000000000017a9140f3444e271620c736808aa7b33e370bd87cb5a078702483045022\
            100fb60dad8df4af2841adc0346638c16d0b8035f5e3f3753b88db122e70c79f9370220756e6633b17fd271\
            0e626347d28d60b0a2d6cbb41de51740644b9fb3ba7751040121028fa937ca8cba2197a37c007176ed89410\
            55d3bcb8627d085e94553e62f057dcc00000000"
        ).unwrap();
        let tx: Result<Transaction, _> = deserialize(&tx_bytes);
        assert!(tx.is_ok());
        let tx = tx.unwrap();

        broadcaster.broadcast_transaction(&tx);
    }
}
