use bitcoin::Transaction;
use lightning::chain::chaininterface::BroadcasterInterface;

#[derive(Debug)]
pub struct BroadcasterInterfaceDummy {}

impl BroadcasterInterface for BroadcasterInterfaceDummy {
    fn broadcast_transaction(&self, _tx: &Transaction) {
        // todo broadcast transaction
    }
}
