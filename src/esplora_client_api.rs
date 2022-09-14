// Note by Gabriel Comte: This file is for the most part a copy from here:
// https://github.com/bitcoindevkit/rust-esplora-client/blob/master/src/api.rs
//
// Once the functionality has been merged into BDK, we should import the functionality from there,
// respectively use the rust-esplora-client: https://github.com/bitcoindevkit/rust-esplora-client

//! structs from the esplora API
//!
//! see: <https://github.com/Blockstream/esplora/blob/master/API.md>

use bitcoin::hashes::hex::FromHex;
use bitcoin::{BlockHash, Script, Txid};
use std::{fmt, io};

use serde::Deserialize;

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct PrevOut {
    pub value: u64,
    pub scriptpubkey: Script,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Vin {
    pub txid: Txid,
    pub vout: u32,
    // None if coinbase
    pub prevout: Option<PrevOut>,
    pub scriptsig: Script,
    #[serde(deserialize_with = "deserialize_witness", default)]
    pub witness: Vec<Vec<u8>>,
    pub sequence: u32,
    pub is_coinbase: bool,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Vout {
    pub value: u64,
    pub scriptpubkey: Script,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct TxStatus {
    pub confirmed: bool,
    pub block_height: Option<u32>,
    pub block_hash: Option<BlockHash>,
    pub block_time: Option<u64>,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct MerkleProof {
    pub block_height: u32,
    pub merkle: Vec<Txid>,
    pub pos: usize,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct OutputStatus {
    pub spent: bool,
    pub txid: Option<Txid>,
    pub vin: Option<u64>,
    pub status: Option<TxStatus>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Tx {
    pub txid: Txid,
    pub version: i32,
    pub locktime: u32,
    pub vin: Vec<Vin>,
    pub vout: Vec<Vout>,
    pub status: TxStatus,
    pub fee: u64,
}

#[derive(Deserialize, Clone, Debug)]
pub struct BlockTime {
    pub timestamp: u64,
    pub height: u32,
}

impl Tx {
    // pub fn to_tx(&self) -> Transaction {
    //     Transaction {
    //         version: self.version,
    //         lock_time: self.locktime,
    //         input: self
    //             .vin
    //             .iter()
    //             .cloned()
    //             .map(|vin| TxIn {
    //                 previous_output: OutPoint {
    //                     txid: vin.txid,
    //                     vout: vin.vout,
    //                 },
    //                 script_sig: vin.scriptsig,
    //                 sequence: vin.sequence,
    //                 witness: Witness::from_vec(vin.witness),
    //             })
    //             .collect(),
    //         output: self
    //             .vout
    //             .iter()
    //             .cloned()
    //             .map(|vout| TxOut {
    //                 value: vout.value,
    //                 script_pubkey: vout.scriptpubkey,
    //             })
    //             .collect(),
    //     }
    // }

    // pub fn confirmation_time(&self) -> Option<BlockTime> {
    //     match self.status {
    //         TxStatus {
    //             confirmed: true,
    //             block_height: Some(height),
    //             block_time: Some(timestamp),
    //             ..
    //         } => Some(BlockTime { timestamp, height }),
    //         _ => None,
    //     }
    // }

    // pub fn previous_outputs(&self) -> Vec<Option<TxOut>> {
    //     self.vin
    //         .iter()
    //         .cloned()
    //         .map(|vin| {
    //             vin.prevout.map(|po| TxOut {
    //                 script_pubkey: po.scriptpubkey,
    //                 value: po.value,
    //             })
    //         })
    //         .collect()
    // }
}

fn deserialize_witness<'de, D>(d: D) -> Result<Vec<Vec<u8>>, D::Error>
where
    D: serde::de::Deserializer<'de>,
{
    let list = Vec::<String>::deserialize(d)?;
    list.into_iter()
        .map(|hex_str| Vec::<u8>::from_hex(&hex_str))
        .collect::<Result<Vec<Vec<u8>>, _>>()
        .map_err(serde::de::Error::custom)
}

/// Errors that can happen during a sync with `Esplora`
#[derive(Debug)]
pub enum Error {
    /// Error during reqwest HTTP request
    Reqwest(::reqwest::Error),
    /// IO error during ureq response read
    Io(io::Error),
    /// Invalid number returned
    Parsing(std::num::ParseIntError),
    /// Invalid Bitcoin data returned
    BitcoinEncoding(bitcoin::consensus::encode::Error),
    /// Invalid Hex data returned
    Hex(bitcoin::hashes::hex::Error),

    // /// Transaction not found
    // TransactionNotFound(Txid),
    /// Header height not found
    HeaderHeightNotFound(u32),
    // /// Header hash not found
    // HeaderHashNotFound(BlockHash),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

macro_rules! impl_error {
    ( $from:ty, $to:ident ) => {
        impl_error!($from, $to, Error);
    };
    ( $from:ty, $to:ident, $impl_for:ty ) => {
        impl std::convert::From<$from> for $impl_for {
            fn from(err: $from) -> Self {
                <$impl_for>::$to(err)
            }
        }
    };
}

impl std::error::Error for Error {}
impl_error!(::reqwest::Error, Reqwest, Error);
impl_error!(io::Error, Io, Error);
impl_error!(std::num::ParseIntError, Parsing, Error);
impl_error!(bitcoin::consensus::encode::Error, BitcoinEncoding, Error);
impl_error!(bitcoin::hashes::hex::Error, Hex, Error);
