use bitcoin::{BlockHash, BlockHeader, Transaction, Txid};
use esplora_client::blocking::BlockingClient;
use esplora_client::Builder;
use esplora_client::{Error, TxStatus};
use log::error;

static ESPLORA_TIMEOUT_SECS: u64 = 30;

pub(crate) struct EsploraClient {
    client: BlockingClient,
}

#[derive(Debug)]
pub(crate) struct ConfirmedTransaction {
    pub tx: Transaction,
    pub block_height: u32,
    pub block_header: BlockHeader,
    pub position: usize, // position within the block
}

impl EsploraClient {
    pub fn new(url: &str) -> Result<Self, Error> {
        let builder = Builder::new(url).timeout(ESPLORA_TIMEOUT_SECS);
        Ok(Self {
            client: builder.build_blocking()?,
        })
    }

    fn get_height_by_hash(&self, hash: &BlockHash) -> Result<Option<u32>, Error> {
        Ok(self.client.get_block_status(hash)?.height)
    }

    pub fn is_tx_confirmed(&self, txid: &Txid) -> Result<bool, Error> {
        Ok(self
            .client
            .get_tx_status(txid)?
            .map_or(false, |status| status.confirmed))
    }

    pub fn get_header_with_height(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<(BlockHeader, u32)>, Error> {
        if let Some(height) = self.get_height_by_hash(block_hash)? {
            let header = self.client.get_header_by_hash(block_hash)?;

            return Ok(Some((header, height)));
        }

        Ok(None)
    }

    pub fn get_confirmed_tx_by_id(
        &self,
        txid: &Txid,
    ) -> Result<Option<ConfirmedTransaction>, Error> {
        if let Some(tx_status) = self.client.get_tx_status(txid)? {
            return self.get_confirmed_tx(txid, &tx_status);
        }

        Ok(None)
    }

    pub fn get_confirmed_spending_tx(
        &self,
        txid: &Txid,
        index: u64,
    ) -> Result<Option<ConfirmedTransaction>, Error> {
        if let Some(output_status) = self.client.get_output_status(txid, index)? {
            if output_status.spent {
                if let (Some(spending_tx_id), Some(spending_tx_status)) =
                    (output_status.txid, output_status.status)
                {
                    return self.get_confirmed_tx(&spending_tx_id, &spending_tx_status);
                } else {
                    error!("Esplora sees output {}:{} as spent, yet its spending transaction does not have any id and/or status attributed to it.", txid, index);
                }
            }
        }

        Ok(None)
    }

    pub fn get_confirmed_tx(
        &self,
        txid: &Txid,
        tx_status: &TxStatus,
    ) -> Result<Option<ConfirmedTransaction>, Error> {
        if tx_status.confirmed {
            if let (Some(block_hash), Some(block_height)) =
                (tx_status.block_hash, tx_status.block_height)
            {
                if let Some(tx) = self.client.get_tx(txid)? {
                    let block_header = self.client.get_header_by_hash(&block_hash)?;
                    if let Some(merkle_proof) = self.client.get_merkle_proof(txid)? {
                        return Ok(Some(ConfirmedTransaction {
                            tx,
                            block_height,
                            block_header,
                            position: merkle_proof.pos,
                        }));
                    } else {
                        error!("Esplora sees transaction {} as confirmed, but does not have a merkle proof for it", txid);
                    }
                } else {
                    error!(
                        "Expected transaction {} to be confirmed, but couldn't find it on Esplora",
                        txid
                    );
                }
            } else {
                error!("Esplora sees transaction {} as confirmed, yet there is no block hash and/or block height attributed to it.", txid);
            }
        }

        Ok(None)
    }

    pub fn get_tip_hash(&self) -> Result<BlockHash, Error> {
        self.client.get_tip_hash()
    }

    pub fn broadcast(&self, tx: &Transaction) -> Result<(), Error> {
        self.client.broadcast(tx)
    }
}
