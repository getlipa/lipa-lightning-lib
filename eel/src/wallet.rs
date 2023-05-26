use crate::errors::{Result, RuntimeErrorCode};

use bdk::blockchain::EsploraBlockchain;
use bdk::template::Bip84;
use bdk::wallet::AddressIndex;
use bdk::SyncOptions;
use bitcoin::bech32::u5;
use bitcoin::secp256k1::ecdh::SharedSecret;
use bitcoin::secp256k1::ecdsa::{RecoverableSignature, Signature};
use bitcoin::secp256k1::{PublicKey, Scalar, Secp256k1, Signing};
use bitcoin::{Network, Script, Transaction, TxOut};
use lightning::chain::keysinterface::{
    EntropySource, InMemorySigner, KeyMaterial, KeysManager, NodeSigner, Recipient, SignerProvider,
    SpendableOutputDescriptor,
};
use lightning::ln::msgs::{DecodeError, UnsignedGossipMessage};
use lightning::ln::script::ShutdownScript;
use perro::MapToError;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

const ESPLORA_TIMEOUT_SECS: u64 = 30;
const ESPLORA_STOP_GAP: usize = 20;
const ESPLORA_CONCURRENCY: u8 = 8;

pub struct Wallet {
    blockchain: EsploraBlockchain,
    inner: Mutex<bdk::Wallet<sled::Tree>>,
}

impl Wallet {
    pub(crate) fn new(blockchain: EsploraBlockchain, wallet: bdk::Wallet<sled::Tree>) -> Self {
        let inner = Mutex::new(wallet);
        Self { blockchain, inner }
    }

    pub(crate) fn sync(&self) -> Result<()> {
        self.inner
            .lock()
            .unwrap()
            .sync(&self.blockchain, SyncOptions::default())
            .map_to_runtime_error(
                RuntimeErrorCode::EsploraServiceUnavailable,
                "Failed to sync onchain wallet",
            )
    }

    pub(crate) fn get_new_address(&self) -> Result<bitcoin::Address> {
        let address_info = self
            .inner
            .lock()
            .unwrap()
            .get_address(AddressIndex::New)
            .map_to_permanent_failure("Failed to get an on-chain address")?;
        Ok(address_info.address)
    }

    pub(crate) fn get_balance(&self) -> Result<bdk::Balance> {
        self.inner
            .lock()
            .unwrap()
            .get_balance()
            .map_to_permanent_failure("Failed to get onchain balance")
    }
}

pub struct WalletKeysManager {
    inner: KeysManager,
    wallet: Arc<Wallet>,
}

impl WalletKeysManager {
    pub fn new(
        seed: &[u8; 32],
        starting_time_secs: u64,
        starting_time_nanos: u32,
        wallet: Arc<Wallet>,
    ) -> Self {
        let inner = KeysManager::new(seed, starting_time_secs, starting_time_nanos);
        Self { inner, wallet }
    }

    #[allow(dead_code)]
    pub fn spend_spendable_outputs<C: Signing>(
        &self,
        descriptors: &[&SpendableOutputDescriptor],
        outputs: Vec<TxOut>,
        change_destination_script: Script,
        feerate_sat_per_1000_weight: u32,
        secp_ctx: &Secp256k1<C>,
    ) -> std::result::Result<Transaction, ()> {
        let only_non_static = &descriptors
            .iter()
            .filter(|desc| !matches!(desc, SpendableOutputDescriptor::StaticOutput { .. }))
            .copied()
            .collect::<Vec<_>>();
        self.inner.spend_spendable_outputs(
            only_non_static,
            outputs,
            change_destination_script,
            feerate_sat_per_1000_weight,
            secp_ctx,
        )
    }
}

impl NodeSigner for WalletKeysManager {
    fn get_inbound_payment_key_material(&self) -> KeyMaterial {
        self.inner.get_inbound_payment_key_material()
    }

    fn get_node_id(&self, recipient: Recipient) -> std::result::Result<PublicKey, ()> {
        self.inner.get_node_id(recipient)
    }

    fn ecdh(
        &self,
        recipient: Recipient,
        other_key: &PublicKey,
        tweak: Option<&Scalar>,
    ) -> std::result::Result<SharedSecret, ()> {
        self.inner.ecdh(recipient, other_key, tweak)
    }

    fn sign_invoice(
        &self,
        hrp_bytes: &[u8],
        invoice_data: &[u5],
        recipient: Recipient,
    ) -> std::result::Result<RecoverableSignature, ()> {
        self.inner.sign_invoice(hrp_bytes, invoice_data, recipient)
    }

    fn sign_gossip_message(
        &self,
        msg: UnsignedGossipMessage<'_>,
    ) -> std::result::Result<Signature, ()> {
        self.inner.sign_gossip_message(msg)
    }
}

impl EntropySource for WalletKeysManager {
    fn get_secure_random_bytes(&self) -> [u8; 32] {
        self.inner.get_secure_random_bytes()
    }
}

impl SignerProvider for WalletKeysManager {
    type Signer = InMemorySigner;

    fn generate_channel_keys_id(
        &self,
        inbound: bool,
        channel_value_satoshis: u64,
        user_channel_id: u128,
    ) -> [u8; 32] {
        self.inner
            .generate_channel_keys_id(inbound, channel_value_satoshis, user_channel_id)
    }

    fn derive_channel_signer(
        &self,
        channel_value_satoshis: u64,
        channel_keys_id: [u8; 32],
    ) -> Self::Signer {
        self.inner
            .derive_channel_signer(channel_value_satoshis, channel_keys_id)
    }

    fn read_chan_signer(&self, reader: &[u8]) -> std::result::Result<Self::Signer, DecodeError> {
        self.inner.read_chan_signer(reader)
    }

    fn get_destination_script(&self) -> Script {
        let address = self
            .wallet
            .get_new_address()
            .expect("Failed to retrieve new address from wallet.");
        address.script_pubkey()
    }

    fn get_shutdown_scriptpubkey(&self) -> ShutdownScript {
        let address = self
            .wallet
            .get_new_address()
            .expect("Failed to retrieve new address from wallet.");
        match address.payload {
            bitcoin::util::address::Payload::WitnessProgram { version, program } => {
                ShutdownScript::new_witness_program(version, &program)
                    .expect("Invalid shutdown script.")
            }
            _ => panic!("Tried to use a non-witness address. This must not ever happen."),
        }
    }
}

pub(crate) fn init_wallet_keys_manager(
    seed: &[u8; 32],
    wallet: Arc<Wallet>,
) -> Result<WalletKeysManager> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_to_permanent_failure("System time before Unix epoch")?;
    Ok(WalletKeysManager::new(
        seed,
        now.as_secs(),
        now.subsec_nanos(),
        wallet,
    ))
}

pub(crate) fn init_wallet(
    seed: &[u8],
    network: Network,
    wallet_db_path: &str,
    esplora_api_url: &str,
) -> Result<Wallet> {
    let xprv = bitcoin::util::bip32::ExtendedPrivKey::new_master(network, seed)
        .expect("Failed to derive wallet xpriv");

    let db = sled::open(wallet_db_path).map_to_permanent_failure("Failed to open bdk database")?;
    let db_tree = db
        .open_tree("bdk-wallet-database")
        .map_to_permanent_failure("Failed to open sled database tree")?;
    let bdk_wallet = bdk::Wallet::new(
        Bip84(xprv, bdk::KeychainKind::External),
        Some(Bip84(xprv, bdk::KeychainKind::Internal)),
        network,
        db_tree,
    )
    .map_to_permanent_failure("Failed to create wallet")?;

    let esplora_client = bdk::esplora_client::Builder::new(esplora_api_url)
        .timeout(ESPLORA_TIMEOUT_SECS)
        .build_blocking()
        .map_to_runtime_error(
            RuntimeErrorCode::EsploraServiceUnavailable,
            "Failed to build Esplora client",
        )?;

    let blockchain = EsploraBlockchain::from_client(esplora_client, ESPLORA_STOP_GAP)
        .with_concurrency(ESPLORA_CONCURRENCY);

    Ok(Wallet::new(blockchain, bdk_wallet))
}
