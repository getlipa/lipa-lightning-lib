//! # lipa-lightning-lib (aka 3L)
//!
//! This crate implements all the main business logic of the lipa wallet.
//!
//! Most functionality can be accessed by creating an instance of [`LightningNode`] and using its methods.

#![allow(clippy::let_unit_value)]

extern crate core;

mod amount;
mod async_runtime;
mod callbacks;
mod config;
mod data_store;
mod environment;
mod errors;
mod exchange_rate_provider;
mod fiat_topup;
mod fund_migration;
mod invoice_details;
mod limits;
mod locker;
mod logger;
mod migrations;
mod random;
mod recovery;
mod sanitize_input;
mod secret;
mod task_manager;
mod util;

use crate::amount::ToAmount;
pub use crate::amount::{Amount, FiatValue};
use crate::async_runtime::AsyncRuntime;
pub use crate::callbacks::EventsCallback;
pub use crate::config::{Config, TzConfig, TzTime};
use crate::environment::Environment;
pub use crate::environment::EnvironmentCode;
use crate::errors::{to_mnemonic_error, Error, SimpleError};
pub use crate::errors::{DecodeInvoiceError, MnemonicError, PayError, PayErrorCode, PayResult};
pub use crate::errors::{Error as LnError, Result, RuntimeErrorCode};
pub use crate::exchange_rate_provider::ExchangeRate;
use crate::exchange_rate_provider::ExchangeRateProviderImpl;
pub use crate::fiat_topup::TopupCurrency;
use crate::fiat_topup::{FiatTopupInfo, PocketClient};
pub use crate::invoice_details::InvoiceDetails;
pub use crate::limits::{LiquidityLimit, PaymentAmountLimits};
use crate::locker::Locker;
pub use crate::recovery::recover_lightning_node;
use crate::secret::Secret;
use crate::task_manager::{TaskManager, TaskPeriods};
use crate::util::unix_timestamp_to_system_time;
use amount::{AsSats, Sats};
pub use crow::{PermanentFailureCode, TemporaryFailureCode};

use bip39::{Language, Mnemonic};
use bitcoin::hashes::hex::ToHex;
use bitcoin::secp256k1::{PublicKey, SECP256K1};
use bitcoin::util::bip32::{DerivationPath, ExtendedPrivKey};
use bitcoin::Network;
use breez_sdk_core::{
    parse, BreezEvent, BreezServices, EventListener, GreenlightCredentials, GreenlightNodeConfig,
    InputType, ListPaymentsRequest, LnUrlPayRequestData, LnUrlPayResult, LnUrlWithdrawResult,
    NodeConfig, OpenChannelFeeRequest, OpeningFeeParams, PaymentDetails, PaymentStatus,
    PaymentTypeFilter, ReceiveOnchainRequest, SweepRequest,
};
use cipher::generic_array::typenum::U32;
use crow::{CountryCode, LanguageCode, OfferManager, TopupError, TopupInfo, TopupStatus};
use data_store::DataStore;
use email_address::EmailAddress;
use honey_badger::secrets::{generate_keypair, KeyPair};
use honey_badger::{Auth, AuthLevel, CustomTermsAndConditions};
use iban::Iban;
use log::{info, trace};
use logger::init_logger_once;
use num_enum::TryFromPrimitive;
use perro::Error::RuntimeError;
use perro::{
    invalid_input, permanent_failure, runtime_error, MapToError, OptionToError, ResultTrait,
};
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use std::{env, fs};
use uuid::Uuid;

const LOG_LEVEL: log::Level = log::Level::Trace;
const LOGS_DIR: &str = "logs";

const BACKEND_AUTH_DERIVATION_PATH: &str = "m/76738065'/0'/0";

/// The fee charged by the Lightning Service Provider (LSP) for opening a channel with the node.
/// This fee is being charged at the time of the channel creation.
/// The LSP simply subtracts this fee from an incoming payment (if this incoming payment leads to a channel creation).
pub struct LspFee {
    pub channel_minimum_fee: Amount,
    /// Parts per myriad (aka basis points) -> 100 is 1%
    pub channel_fee_permyriad: u64,
}

/// The type returned by [`LightningNode::calculate_lsp_fee`]
pub struct CalculateLspFeeResponse {
    /// Indicates the amount that will be charged
    pub lsp_fee: Amount,
    /// Should be passed in to [`LightningNode::create_invoice`]
    pub lsp_fee_params: Option<OpeningFeeParams>,
}

/// Information about the Lightning node running in the background
pub struct NodeInfo {
    /// Lightning network public key of the node (also known as node id)
    pub node_pubkey: String,
    /// List of node ids of all the peers the node is connected to
    pub peers: Vec<String>,
    /// Amount of on-chain balance the node has
    pub onchain_balance: Amount,
    /// Information about the channels of the node
    pub channels_info: ChannelsInfo,
}

/// Information about the channels of the node
pub struct ChannelsInfo {
    /// The balance of the local node
    pub local_balance: Amount,
    /// Capacity the node can actually receive.
    /// It excludes non usable channels, pending HTLCs, channels reserves, etc.
    pub inbound_capacity: Amount,
    /// Capacity the node can actually send.
    /// It excludes non usable channels, pending HTLCs, channels reserves, etc.
    pub outbound_capacity: Amount,
}

#[derive(PartialEq, Eq, Debug, TryFromPrimitive, Clone)]
#[repr(u8)]
pub enum PaymentType {
    Receiving,
    Sending,
}

#[derive(PartialEq, Eq, Debug, TryFromPrimitive, Clone)]
#[repr(u8)]
pub enum PaymentState {
    /// The payment was created and is in progress.
    Created,
    /// The payment succeeded.
    Succeeded,
    /// The payment failed. If it is a [`PaymentType::Sending`] payment, it can be retried.
    Failed,
    /// A payment retrial is in progress.
    Retried,
    /// The invoice associated with this payment has expired.
    InvoiceExpired,
}

/// Information about an incoming or outgoing payment
#[derive(PartialEq)]
pub struct Payment {
    pub payment_type: PaymentType,
    pub payment_state: PaymentState,
    /// For now, will always be empty
    pub fail_reason: Option<PayErrorCode>,
    /// Hex representation of payment hash
    pub hash: String,
    /// Nominal amount specified in the invoice
    pub amount: Amount,
    pub invoice_details: InvoiceDetails,
    pub created_at: TzTime,
    /// The description embedded in the invoice. Given the length limit of this data,
    /// it is possible that a hex hash of the description is provided instead, but that is uncommon.
    pub description: String,
    /// Hex representation of the preimage. Will only be present on successful payments.
    pub preimage: Option<String>,
    /// Routing fees paid in a [`PaymentType::Sending`] payment. Will only be present if the payment
    /// was successful.
    /// The cost of sending a payment is `amount` + `network_fees`.
    pub network_fees: Option<Amount>,
    /// LSP fees paid in a [`PaymentType::Receiving`] payment. Will never be present for
    /// [`PaymentType::Sending`] payments but might be 0 for [`PaymentType::Receiving`] payments.
    /// The amount is only paid if successful.
    /// The value that is received in practice is given by `amount` - `lsp_fees`.
    pub lsp_fees: Option<Amount>,
    /// An offer a [`PaymentType::Receiving`] payment came from if any.
    pub offer: Option<OfferKind>,
    pub metadata: String,
}

/// Indicates the max routing fee mode used to restrict fees of a payment of a given size
pub enum MaxRoutingFeeMode {
    /// `max_fee_permyriad` Parts per myriad (aka basis points) -> 100 is 1%
    Relative {
        max_fee_permyriad: u16,
    },
    Absolute {
        max_fee_amount: Amount,
    },
}

#[derive(Debug)]
pub enum OfferStatus {
    READY,
    /// Claiming the offer failed, but it can be retried.
    FAILED,
    /// The offer could not be claimed, so the user got refunded.
    /// Specific info for Pocket offers:
    /// - The Refund happened over the Fiat rails
    /// - Reasons for why the offer was refunded: <https://pocketbitcoin.com/developers/docs/rest/v1/webhooks#refund-reasons>
    REFUNDED,
    SETTLED,
}

/// An error associated with a specific PocketOffer. Can be temporary, indicating there was an issue
/// with a previous withdrawal attempt and it can be retried, or it can be permanent.
///
/// More information on each specific error can be found on
/// [Pocket's Documentation Page](<https://pocketbitcoin.com/developers/docs/rest/v1/webhooks>).
pub type PocketOfferError = TopupError;

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum OfferKind {
    /// An offer related to a topup using the Pocket exchange
    /// Values are denominated in the fiat currency the user sent to the exchange.
    /// The currency code can be found in `exchange_rate`.
    Pocket {
        id: String,
        /// The exchange rate used by the exchange to exchange fiat to sats
        exchange_rate: ExchangeRate,
        /// The original fiat amount sent to the exchange
        topup_value_minor_units: u64,
        /// The fee paid to perform the exchange from fiat to sats
        exchange_fee_minor_units: u64,
        /// The rate of the fee expressed in permyriad (e.g. 1.5% would be 150)
        exchange_fee_rate_permyriad: u16,
        /// The optional error that might have occurred in the offer withdrawal process
        error: Option<PocketOfferError>,
    },
}

/// Information on a funds offer that can be claimed
/// using [`LightningNode::request_offer_collection`].
#[derive(Debug)]
pub struct OfferInfo {
    pub offer_kind: OfferKind,
    /// Amount available for withdrawal
    pub amount: Amount,
    /// The lnurlw string that will be used to withdraw this offer. Can be empty if the offer isn't
    /// available anymore (i.e `status` is [`OfferStatus::REFUNDED`])
    pub lnurlw: Option<String>,
    pub created_at: SystemTime,
    /// The time this offer expires at. Can be empty if the offer isn't available anymore
    /// (i.e `status` is [`OfferStatus::REFUNDED`]).
    pub expires_at: Option<SystemTime>,
    pub status: OfferStatus,
}

/// Information about a generated swap address
pub struct SwapAddressInfo {
    /// Funds sent to this address will be swapped into LN to be received by the local wallet
    pub address: String,
    /// Minimum amount to be sent to `address`
    pub min_deposit: Amount,
    /// Maximum amount to be sent to `address`
    pub max_deposit: Amount,
}

/// Information about a failed swap
pub struct FailedSwapInfo {
    pub address: String,
    /// The amount that is available to be refunded. The refund will involve paying some
    /// on-chain fees so it isn't possible to recover the entire amount.
    pub amount: Amount,
    pub created_at: SystemTime,
}

#[derive(Clone, PartialEq, Debug)]
pub(crate) struct UserPreferences {
    fiat_currency: String,
    timezone_config: TzConfig,
}

/// Information about an LNURL-pay.
pub struct LnUrlPayDetails {
    pub min_sendable: Amount,
    pub max_sendable: Amount,
    /// An internal struct is not supposed to be inspected, but only passed to [`LightningNode::pay_lnurlp`].
    pub request_data: LnUrlPayRequestData,
}

impl LnUrlPayDetails {
    fn from_lnurl_pay_request_data(
        request_data: LnUrlPayRequestData,
        exchange_rate: &Option<ExchangeRate>,
    ) -> Self {
        Self {
            min_sendable: request_data
                .min_sendable
                .as_msats()
                .to_amount_up(exchange_rate),
            max_sendable: request_data
                .max_sendable
                .as_msats()
                .to_amount_up(exchange_rate),
            request_data,
        }
    }
}

/// Decoded data that can be obtained using [`LightningNode::decode_data`].
pub enum DecodedData {
    Bolt11Invoice { invoice_details: InvoiceDetails },
    LnUrlPay { lnurl_pay_details: LnUrlPayDetails },
}

struct LipaEventListener {
    events_callback: Box<dyn EventsCallback>,
}

impl EventListener for LipaEventListener {
    fn on_event(&self, e: BreezEvent) {
        match e {
            BreezEvent::NewBlock { .. } => {}
            BreezEvent::InvoicePaid { details } => {
                self.events_callback.payment_received(details.payment_hash)
            }
            BreezEvent::Synced => {}
            BreezEvent::PaymentSucceed { details } => {
                if let PaymentDetails::Ln { data } = details.details {
                    self.events_callback
                        .payment_sent(data.payment_hash, data.payment_preimage)
                }
            }
            BreezEvent::PaymentFailed { details } => {
                if let Some(invoice) = details.invoice {
                    self.events_callback.payment_failed(invoice.payment_hash)
                }
            }
            BreezEvent::BackupStarted => {}
            BreezEvent::BackupSucceeded => {}
            BreezEvent::BackupFailed { .. } => {}
        }
    }
}

const MAX_FEE_PERMYRIAD: u16 = 50;
const EXEMPT_FEE: Sats = Sats::new(21);

const FOREGROUND_PERIODS: TaskPeriods = TaskPeriods {
    update_exchange_rates: Some(Duration::from_secs(10 * 60)),
    sync_breez: Some(Duration::from_secs(10 * 60)),
    update_lsp_fee: Some(Duration::from_secs(10 * 60)),
};

const BACKGROUND_PERIODS: TaskPeriods = TaskPeriods {
    update_exchange_rates: None,
    sync_breez: Some(Duration::from_secs(30 * 60)),
    update_lsp_fee: None,
};

/// The main class/struct of this library. Constructing an instance will initiate the Lightning node and
/// run it in the background. As long as an instance of `LightningNode` is held, the node will continue to run
/// in the background. Dropping the instance will start a deinit process.  
pub struct LightningNode {
    user_preferences: Mutex<UserPreferences>,
    sdk: Arc<BreezServices>,
    auth: Arc<Auth>,
    fiat_topup_client: PocketClient,
    offer_manager: OfferManager,
    rt: AsyncRuntime,
    data_store: Arc<Mutex<DataStore>>,
    task_manager: Arc<Mutex<TaskManager>>,
}

impl LightningNode {
    pub fn new(config: Config, events_callback: Box<dyn EventsCallback>) -> Result<Self> {
        enable_backtrace();
        fs::create_dir_all(&config.local_persistence_path).map_to_permanent_failure(format!(
            "Failed to create directory: {}",
            &config.local_persistence_path,
        ))?;
        if config.enable_file_logging {
            init_logger_once(
                LOG_LEVEL,
                &Path::new(&config.local_persistence_path).join(LOGS_DIR),
            )?;
        }

        let rt = AsyncRuntime::new()?;

        let environment = Environment::load(config.environment);

        let strong_typed_seed = sanitize_input::strong_type_seed(&config.seed)?;
        let auth = Arc::new(build_auth(
            &strong_typed_seed,
            environment.backend_url.clone(),
        )?);

        let device_cert = env!("BREEZ_SDK_PARTNER_CERTIFICATE").as_bytes().to_vec();
        let device_key = env!("BREEZ_SDK_PARTNER_KEY").as_bytes().to_vec();
        let partner_credentials = GreenlightCredentials {
            device_cert,
            device_key,
        };

        let mut breez_config = BreezServices::default_config(
            environment.environment_type,
            env!("BREEZ_SDK_API_KEY").to_string(),
            NodeConfig::Greenlight {
                config: GreenlightNodeConfig {
                    partner_credentials: Some(partner_credentials),
                    invite_code: None,
                },
            },
        );

        breez_config.working_dir = config.local_persistence_path.clone();
        breez_config.exemptfee_msat = EXEMPT_FEE.msats;
        breez_config.maxfee_percent = MAX_FEE_PERMYRIAD as f64 / 100_f64;

        let sdk = rt
            .handle()
            .block_on(BreezServices::connect(
                breez_config,
                config.seed.clone(),
                Box::new(LipaEventListener { events_callback }),
            ))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to initialize a breez sdk instance",
            )?;

        rt.handle().block_on(async {
            if sdk
                .lsp_id()
                .await
                .map_to_runtime_error(
                    RuntimeErrorCode::NodeUnavailable,
                    "Failed to get current lsp id",
                )?
                .is_none()
            {
                let lsps = sdk.list_lsps().await.map_to_runtime_error(
                    RuntimeErrorCode::NodeUnavailable,
                    "Failed to list lsps",
                )?;
                let lsp = lsps
                    .first()
                    .ok_or_runtime_error(RuntimeErrorCode::NodeUnavailable, "No lsp available")?;
                sdk.connect_lsp(lsp.id.clone()).await.map_to_runtime_error(
                    RuntimeErrorCode::NodeUnavailable,
                    "Failed to connect to lsp",
                )?;
            }
            Ok::<(), Error>(())
        })?;

        let exchange_rate_provider = Box::new(ExchangeRateProviderImpl::new(
            environment.backend_url.clone(),
            Arc::clone(&auth),
        ));

        let fiat_topup_client =
            PocketClient::new(environment.pocket_url, Arc::clone(&sdk), rt.handle())?;
        let offer_manager = OfferManager::new(environment.backend_url.clone(), Arc::clone(&auth));

        let db_path = format!("{}/db2.db3", config.local_persistence_path);

        let user_preferences = Mutex::new(UserPreferences {
            fiat_currency: config.fiat_currency,
            timezone_config: config.timezone_config,
        });

        let data_store = Arc::new(Mutex::new(DataStore::new(&db_path)?));

        let task_manager = Arc::new(Mutex::new(TaskManager::new(
            rt.handle(),
            exchange_rate_provider,
            Arc::clone(&data_store),
            Arc::clone(&sdk),
        )?));
        task_manager
            .lock_unwrap()
            .restart(Self::get_foreground_periods());

        let data_store_clone = Arc::clone(&data_store);
        let auth_clone = Arc::clone(&auth);
        fund_migration::migrate_funds(
            rt.handle(),
            &strong_typed_seed,
            data_store_clone,
            &sdk,
            auth_clone,
            &environment.backend_url,
        )
        .map_runtime_error_to(RuntimeErrorCode::FailedFundMigration)?;

        Ok(LightningNode {
            user_preferences,
            sdk,
            auth,
            fiat_topup_client,
            offer_manager,
            rt,
            data_store,
            task_manager,
        })
    }

    fn get_foreground_periods() -> TaskPeriods {
        match env::var("TESTING_TASK_PERIODS") {
            Ok(period) => {
                let period: u64 = period
                    .parse()
                    .expect("TESTING_TASK_PERIODS should be an integer number");
                let period = Duration::from_secs(period);
                TaskPeriods {
                    update_exchange_rates: Some(period),
                    sync_breez: Some(period),
                    update_lsp_fee: Some(period),
                }
            }
            Err(_) => FOREGROUND_PERIODS,
        }
    }

    /// Request some basic info about the node
    pub fn get_node_info(&self) -> Result<NodeInfo> {
        let node_state = self.sdk.node_info().map_to_runtime_error(
            RuntimeErrorCode::NodeUnavailable,
            "Failed to read node info",
        )?;
        let rate = self.get_exchange_rate();

        Ok(NodeInfo {
            node_pubkey: node_state.id,
            peers: node_state.connected_peers,
            onchain_balance: node_state
                .onchain_balance_msat
                .as_msats()
                .to_amount_down(&rate),
            channels_info: ChannelsInfo {
                local_balance: node_state
                    .channels_balance_msat
                    .as_msats()
                    .to_amount_down(&rate),
                inbound_capacity: node_state
                    .inbound_liquidity_msats
                    .as_msats()
                    .to_amount_down(&rate),
                outbound_capacity: node_state.max_payable_msat.as_msats().to_amount_down(&rate),
            },
        })
    }

    /// When *receiving* payments, a new channel MAY be required. A fee will be charged to the user.
    /// This does NOT impact *sending* payments.
    /// Get information about the fee charged by the LSP for opening new channels
    /// Please keep in mind that this method doesn't make any network calls. It simply retrieves
    /// previously fetched values that are frequently updated by a background task.
    pub fn query_lsp_fee(&self) -> Result<LspFee> {
        let exchange_rate = self.get_exchange_rate();
        let lsp_fee = self.task_manager.lock_unwrap().get_lsp_fee()?;
        Ok(LspFee {
            channel_minimum_fee: lsp_fee.min_msat.as_msats().to_amount_up(&exchange_rate),
            channel_fee_permyriad: lsp_fee.proportional as u64 / 100,
        })
    }

    /// Calculate the actual LSP fee for the given amount of an incoming payment.
    /// If the already existing inbound capacity is enough, no new channel is required.
    ///
    /// Parameters:
    /// * `amount_sat` - amount in sats to compute LSP fee for
    ///
    /// For the returned fees to be guaranteed to be accurate, the returned `lsp_fee_params` must be
    /// provided to [`LightningNode::create_invoice`]
    pub fn calculate_lsp_fee(&self, amount_sat: u64) -> Result<CalculateLspFeeResponse> {
        let req = OpenChannelFeeRequest {
            amount_msat: amount_sat * 1_000,
            expiry: None,
        };
        let res = self
            .rt
            .handle()
            .block_on(self.sdk.open_channel_fee(req))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to compute opening channel fee",
            )?;
        Ok(CalculateLspFeeResponse {
            lsp_fee: res
                .fee_msat
                .as_msats()
                .to_amount_up(&self.get_exchange_rate()),
            lsp_fee_params: res.used_fee_params,
        })
    }

    /// Get the current limits for the amount that can be transferred in a single payment.
    /// Currently there are only limits for receiving payments.
    /// The limits (partly) depend on the channel situation of the node, so it should be called
    /// again every time the user is about to receive a payment.
    /// The limits stay the same regardless of what amount wants to receive (= no changes while
    /// he's typing the amount)
    pub fn get_payment_amount_limits(&self) -> Result<PaymentAmountLimits> {
        // TODO: try to move this logic inside the SDK
        let lsp_min_fee_amount = self.query_lsp_fee()?.channel_minimum_fee;
        let max_inbound_amount = self.get_node_info()?.channels_info.inbound_capacity;
        Ok(PaymentAmountLimits::calculate(
            max_inbound_amount.sats,
            lsp_min_fee_amount.sats,
            &self.get_exchange_rate(),
        ))
    }

    /// Create an invoice to receive a payment with:
    ///    - `amount_sat` - the smallest amount of sats required for the node to accept the incoming
    /// payment (sender will have to pay fees on top of that amount)
    ///    - `lsp_fee_params` - the params that will be used to determine the lsp fee.
    /// Can be obtained from [`LightningNode::calculate_lsp_fee`] to guarantee predicted fees
    /// are the ones charged.
    ///    - `description` - a description to be embedded into the created invoice
    ///    - `metadata` - a metadata string that gets tied up to this payment. It can be used by
    /// the user of this library to store data that is relevant to this payment. It is provided
    /// together with the respective payment in [`LightningNode::get_latest_payments`]
    pub fn create_invoice(
        &self,
        amount_sat: u64,
        lsp_fee_params: Option<OpeningFeeParams>,
        description: String,
        _metadata: String,
    ) -> Result<InvoiceDetails> {
        let response = self
            .rt
            .handle()
            .block_on(
                self.sdk
                    .receive_payment(breez_sdk_core::ReceivePaymentRequest {
                        amount_sats: amount_sat,
                        description,
                        preimage: None,
                        opening_fee_params: lsp_fee_params,
                        use_description_hash: None,
                        expiry: None,
                        cltv: None,
                    }),
            )
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to create an invoice",
            )?;

        self.store_payment_info(&response.ln_invoice.payment_hash, None)
            .map_to_permanent_failure("Failed to persist payment info")?;
        // TODO: persist metadata

        Ok(InvoiceDetails::from_ln_invoice(
            response.ln_invoice,
            &self.get_exchange_rate(),
        ))
    }

    /// Decode a user-provided string (usually obtained from QR-code or pasted)
    ///
    /// Can be used instead of [`LightningNode::decode_invoice`].
    pub fn decode_data(&self, data: String) -> Result<DecodedData> {
        match self.rt.handle().block_on(parse(&data)) {
            Ok(InputType::Bolt11 { invoice }) => Ok(DecodedData::Bolt11Invoice {
                invoice_details: InvoiceDetails::from_ln_invoice(
                    invoice,
                    &self.get_exchange_rate(),
                ),
            }),
            Ok(InputType::LnUrlPay { data }) => Ok(DecodedData::LnUrlPay {
                lnurl_pay_details: LnUrlPayDetails::from_lnurl_pay_request_data(
                    data,
                    &self.get_exchange_rate(),
                ),
            }),
            Ok(_) => Err(invalid_input("Unsupported data type")), // TODO: improve returned error when new error model is introduced
            Err(e) => Err(invalid_input(format!("Unrecognized data type {e}"))), // TODO: improve returned error when new error model is introduced
        }
    }

    /// Decodes an invoice returning detailed information
    ///
    /// Parameters:
    /// * `invoice` - a BOLT-11 invoice (Mainnet invoices start with "lnbc")
    pub fn decode_invoice(
        &self,
        invoice: String,
    ) -> std::result::Result<InvoiceDetails, DecodeInvoiceError> {
        match self.rt
            .handle()
            .block_on(parse(&invoice)) {
            Ok(InputType::Bolt11 { invoice }) => Ok(InvoiceDetails::from_ln_invoice(invoice, &self.get_exchange_rate())),
            Ok(_) => Err(DecodeInvoiceError::SemanticError {
                msg: "Failed to decode invoice - provided string was recognized but not as a Bolt11 invoice".to_string(),
            }),
            Err(e) => Err(DecodeInvoiceError::ParseError {
                msg: format!("Failed to parse invoice: {e}"),
            }),
        }
    }

    /// Get the max routing fee mode that will be employed to restrict the fees for paying a given amount in sats
    pub fn get_payment_max_routing_fee_mode(&self, amount_sat: u64) -> MaxRoutingFeeMode {
        get_payment_max_routing_fee_mode(amount_sat, &self.get_exchange_rate())
    }

    /// Start an attempt to pay an invoice. Can immediately fail, meaning that the payment couldn't be started.
    /// If successful, it doesn't mean that the payment itself was successful (funds received by the payee).
    /// After this method returns, the consumer of this library will learn about a successful/failed payment through the
    /// callbacks [`EventsCallback::payment_sent`] and [`EventsCallback::payment_failed`].
    ///
    /// Parameters:
    /// * `invoice` - a BOLT-11 invoice (normally starts with lnbc). The invoice must:
    ///         - use the same network as the one this node operates on
    ///         - have not expired
    /// * `metadata` - a metadata string that gets tied up to this payment. It can be used by the user of this library
    ///  to store data that is relevant to this payment. It is provided together with the respective payment in [`LightningNode::get_latest_payments`].
    pub fn pay_invoice(&self, invoice: String, _metadata: String) -> PayResult<()> {
        match self.rt.handle().block_on(parse(&invoice)) {
            Ok(InputType::Bolt11 { invoice }) => self
                .store_payment_info(&invoice.payment_hash, None)
                .map_to_permanent_failure("Failed to persist payment info"),
            _ => Err(invalid_input("Invalid invoice")),
        }?;
        // TODO: persist metadata

        match self
            .rt
            .handle()
            .block_on(self.sdk.send_payment(invoice, None))
        {
            Ok(_) => Ok(()),
            // TODO: properly handle errors (requires changing either ours or the SDK's error model)
            Err(e) => Err(RuntimeError {
                code: PayErrorCode::UnexpectedError,
                msg: format!("Failed to start paying invoice: {e}"),
            }),
        }
    }

    /// Similar to [`LightningNode::pay_invoice`] with the difference that the passed in invoice
    /// does not have any payment amount specified, and allows the caller of the method to
    /// specify an amount instead.
    ///
    /// Additional Parameters:
    /// * `amount_sat` - amount in sats to be paid
    pub fn pay_open_invoice(
        &self,
        invoice: String,
        amount_sat: u64,
        _metadata: String,
    ) -> PayResult<()> {
        match self.rt.handle().block_on(parse(&invoice)) {
            Ok(InputType::Bolt11 { invoice }) => self
                .store_payment_info(&invoice.payment_hash, None)
                .map_to_permanent_failure("Failed to persist payment info"),
            _ => Err(invalid_input("Invalid invoice")),
        }?;
        // TODO: persist metadata

        match self
            .rt
            .handle()
            .block_on(self.sdk.send_payment(invoice, Some(amount_sat)))
        {
            Ok(_) => Ok(()),
            // TODO: properly handle errors (requires changing either ours or the SDK's error model)
            Err(e) => Err(RuntimeError {
                code: PayErrorCode::UnexpectedError,
                msg: format!("Failed to start paying invoice: {e}"),
            }),
        }
    }

    /// Pay an LNURL-pay the provided amount.
    ///
    /// Parameters:
    /// * `amount_sat` - amount to be paid
    /// * `lnurl_pay_request_data` - LNURL-pay request data as obtained from [`LightningNode::decode_data`]
    ///
    /// Returns the payment hash of the payment.
    pub fn pay_lnurlp(
        &self,
        amount_sat: u64,
        lnurl_pay_request_data: LnUrlPayRequestData,
    ) -> Result<String> {
        let initial_latest_payments = self.get_latest_payments(1)?;
        let initial_latest_payment = initial_latest_payments.get(0);

        match self
            .rt
            .handle()
            .block_on(self.sdk.lnurl_pay(amount_sat, None, lnurl_pay_request_data)) // TODO: return payment hash directly when Breez SDK allows for it https://github.com/breez/breez-sdk/pull/550
            .map_to_invalid_input("Invalid parameters provided to pay_lnurlp()")?
        {
            LnUrlPayResult::EndpointSuccess { .. } => {}
            LnUrlPayResult::EndpointError { .. } => {
                return Err(runtime_error(
                    // TODO: return a more specific error
                    RuntimeErrorCode::NodeUnavailable,
                    "Failed to get an invoice from the LNURL-pay service",
                ));
            }
        }

        let final_latest_payments = self.get_latest_payments(1)?;
        let final_latest_payment = final_latest_payments.get(0);

        // Temporary hack: check if there is a new payment to know if a payment attempt has been started
        if initial_latest_payment != final_latest_payment {
            Ok(final_latest_payment
                .ok_or_permanent_failure("Expected at least one payment to exist")?
                .hash
                .clone())
        } else {
            Err(runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to get an invoice from LNURL-pay service",
            ))
        }
    }

    /// Get a list of the latest payments
    ///
    /// Parameters:
    /// * `number_of_payments` - the maximum number of payments that will be returned
    pub fn get_latest_payments(&self, number_of_payments: u32) -> Result<Vec<Payment>> {
        let list_payments_request = ListPaymentsRequest {
            filter: PaymentTypeFilter::All,
            from_timestamp: None,
            to_timestamp: None,
            include_failures: Some(true),
        };
        self.rt
            .handle()
            .block_on(self.sdk.list_payments(list_payments_request))
            .map_to_runtime_error(RuntimeErrorCode::NodeUnavailable, "Failed to list payments")?
            .into_iter()
            .filter(|p| p.payment_type != breez_sdk_core::PaymentType::ClosedChannel)
            .take(number_of_payments as usize)
            .map(|p| self.payment_from_breez_payment(p))
            .collect::<Result<Vec<Payment>>>()
    }

    /// Get a payment given its payment hash
    ///
    /// Parameters:
    /// * `hash` - hex representation of payment hash
    pub fn get_payment(&self, hash: String) -> Result<Payment> {
        let breez_payment = self
            .rt
            .handle()
            .block_on(self.sdk.payment_by_hash(hash))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to get payment by hash",
            )?
            .ok_or_invalid_input("Invalid hash: no payment with provided hash was found")?;

        self.payment_from_breez_payment(breez_payment)
    }

    fn payment_from_breez_payment(
        &self,
        breez_payment: breez_sdk_core::Payment,
    ) -> Result<Payment> {
        let payment_details = match breez_payment.details {
            PaymentDetails::Ln { data } => data,
            _ => {
                return Err(permanent_failure(
                    "Current interface doesn't support PaymentDetails::ClosedChannel",
                ))
            }
        };

        let local_payment_data = self
            .data_store
            .lock_unwrap()
            .retrieve_payment_info(&payment_details.payment_hash)?;

        let (exchange_rate, time, offer) = match local_payment_data {
            None => {
                let exchange_rate = self.get_exchange_rate();
                let user_preferences = self.user_preferences.lock_unwrap();
                let time = TzTime {
                    time: unix_timestamp_to_system_time(breez_payment.payment_time as u64),
                    timezone_id: user_preferences.timezone_config.timezone_id.clone(),
                    timezone_utc_offset_secs: user_preferences
                        .timezone_config
                        .timezone_utc_offset_secs,
                };
                let offer = None;
                (exchange_rate, time, offer)
            } // TODO: change interface to accommodate for local payment data being non-existent
            Some(d) => {
                let exchange_rate = Some(d.exchange_rate);
                let time = TzTime {
                    time: unix_timestamp_to_system_time(breez_payment.payment_time as u64),
                    timezone_id: d.user_preferences.timezone_config.timezone_id,
                    timezone_utc_offset_secs: d
                        .user_preferences
                        .timezone_config
                        .timezone_utc_offset_secs,
                };
                let offer = d.offer;
                (exchange_rate, time, offer)
            }
        };

        let (payment_type, amount, network_fees, lsp_fees) = match breez_payment.payment_type {
            breez_sdk_core::PaymentType::Sent => (
                PaymentType::Sending,
                breez_payment
                    .amount_msat
                    .as_msats()
                    .to_amount_up(&exchange_rate),
                Some(
                    breez_payment
                        .fee_msat
                        .as_msats()
                        .to_amount_up(&exchange_rate),
                ),
                None,
            ),
            breez_sdk_core::PaymentType::Received => (
                PaymentType::Receiving,
                breez_payment
                    .amount_msat
                    .as_msats()
                    .to_amount_down(&exchange_rate),
                None,
                Some(
                    breez_payment
                        .fee_msat
                        .as_msats()
                        .to_amount_up(&exchange_rate),
                ),
            ),
            breez_sdk_core::PaymentType::ClosedChannel => {
                return Err(permanent_failure(
                    "Current interface doesn't support PaymentDetails::ClosedChannel",
                ))
            }
        };

        let payment_state = match breez_payment.status {
            PaymentStatus::Pending => PaymentState::Created,
            PaymentStatus::Complete => PaymentState::Succeeded,
            PaymentStatus::Failed => PaymentState::Failed,
        };

        let invoice_details = self
            .decode_invoice(payment_details.bolt11)
            .map_to_permanent_failure("Invalid invoice provided by the Breez SDK")?;

        let description = invoice_details.description.clone();

        Ok(Payment {
            payment_type,
            payment_state,
            fail_reason: None, // TODO: Request SDK to store and provide reason for failed payments
            hash: payment_details.payment_hash,
            amount,
            invoice_details,
            created_at: time.clone(),
            description,
            preimage: Some(payment_details.payment_preimage),
            network_fees,
            lsp_fees,
            offer,
            metadata: String::new(), // TODO: retrieve metadata from local db
        })
    }

    /// Call the method when the app goes to foreground, such that the user can interact with it.
    /// The library starts running the background tasks more frequently to improve user experience.
    pub fn foreground(&self) {
        self.task_manager
            .lock_unwrap()
            .restart(Self::get_foreground_periods());
    }

    /// Call the method when the app goes to background, such that the user can not interact with it.
    /// The library stops running some unnecessary tasks and runs necessary tasks less frequently.
    /// It should save battery and internet traffic.
    pub fn background(&self) {
        self.task_manager.lock_unwrap().restart(BACKGROUND_PERIODS);
    }

    /// List codes of supported fiat currencies.
    /// Please keep in mind that this method doesn't make any network calls. It simply retrieves
    /// previously fetched values that are frequently updated by a background task.
    ///
    /// The fetched list will be persisted across restarts to alleviate the consequences of a
    /// slow or unresponsive exchange rate service.
    /// The method will return an empty list if there is nothing persisted yet and
    /// the values are not yet fetched from the service.
    pub fn list_currency_codes(&self) -> Vec<String> {
        let rates = self.task_manager.lock_unwrap().get_exchange_rates();
        rates.iter().map(|r| r.currency_code.clone()).collect()
    }

    /// Get exchange rate on the BTC/default currency pair
    /// Please keep in mind that this method doesn't make any network calls. It simply retrieves
    /// previously fetched values that are frequently updated by a background task.
    ///
    /// The fetched exchange rates will be persisted across restarts to alleviate the consequences of a
    /// slow or unresponsive exchange rate service.
    ///
    /// The return value is an optional to deal with the possibility
    /// of no exchange rate values being known.
    pub fn get_exchange_rate(&self) -> Option<ExchangeRate> {
        let rates = self.task_manager.lock_unwrap().get_exchange_rates();
        let currency_code = self.user_preferences.lock_unwrap().fiat_currency.clone();
        rates
            .iter()
            .find(|r| r.currency_code == currency_code)
            .cloned()
    }

    /// Change the fiat currency (ISO 4217 currency code) - not all are supported
    /// The method [`LightningNode::list_currency_codes`] can used to list supported codes.
    pub fn change_fiat_currency(&self, fiat_currency: String) {
        self.user_preferences.lock_unwrap().fiat_currency = fiat_currency;
    }

    /// Change the timezone config.
    ///
    /// Parameters:
    /// * `timezone_config` - the user's current timezone
    pub fn change_timezone_config(&self, timezone_config: TzConfig) {
        self.user_preferences.lock_unwrap().timezone_config = timezone_config;
    }

    pub fn accept_pocket_terms_and_conditions(&self) -> Result<()> {
        self.auth
            .accept_custom_terms_and_conditions(CustomTermsAndConditions::Pocket)
            .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)
    }

    /// Register for fiat topups. Returns information that can be used by the user to transfer fiat
    /// to the 3rd party exchange service. Once the 3rd party exchange receives funds, the user will
    /// be able to withdraw sats using LNURL-w.
    ///
    /// Parameters:
    /// * `email` - this email will be used to send status information about different topups
    /// * `user_iban` - the user will send fiat from this iban
    /// * `user_currency` - the fiat currency that will be sent for exchange
    pub fn register_fiat_topup(
        &self,
        email: Option<String>,
        user_iban: String,
        user_currency: TopupCurrency,
    ) -> Result<FiatTopupInfo> {
        trace!("register_fiat_topup() - called with - email: {email:?} - user_iban: {user_iban} - user_currency: {user_currency:?}");
        user_iban
            .parse::<Iban>()
            .map_to_invalid_input("Invalid user_iban")?;

        if let Some(email) = email.as_ref() {
            EmailAddress::from_str(email).map_to_invalid_input("Invalid email")?;
        }

        let topup_info = self
            .fiat_topup_client
            .register_pocket_fiat_topup(&user_iban, user_currency)?;

        self.offer_manager
            .register_topup(topup_info.order_id.clone(), email)
            .map_runtime_error_to(RuntimeErrorCode::OfferServiceUnavailable)?;

        Ok(topup_info)
    }

    /// Hides the topup with the given id. Can be called on expired topups so that they stop being returned
    /// by [`LightningNode::query_uncompleted_offers`].
    ///
    /// Topup id can be obtained from [`OfferKind::Pocket`].
    pub fn hide_topup(&self, id: String) -> Result<()> {
        self.offer_manager
            .hide_topup(id)
            .map_runtime_error_to(RuntimeErrorCode::OfferServiceUnavailable)
    }

    /// Get a list of unclaimed fund offers
    pub fn query_uncompleted_offers(&self) -> Result<Vec<OfferInfo>> {
        let topup_infos = self
            .offer_manager
            .query_uncompleted_topups()
            .map_runtime_error_to(RuntimeErrorCode::OfferServiceUnavailable)?;
        let rate = self.get_exchange_rate();
        Ok(topup_infos
            .into_iter()
            .map(|o| to_offer(o, &rate))
            .collect())
    }

    /// Request to collect the offer (e.g. a Pocket topup).
    /// A payment hash will be returned to track incoming payment.
    /// The offer collection might be considered successful once
    /// [`EventsCallback::payment_received`] is called,
    /// or the [`PaymentState`] of the respective payment becomes [`PaymentState::Succeeded`].
    ///
    /// Parameters:
    /// * `offer` - An offer that is still valid for collection. Must have its `lnurlw` field
    /// filled in.
    pub fn request_offer_collection(&self, offer: OfferInfo) -> Result<String> {
        let lnurlw_data = match self.rt.handle().block_on(parse(
            &offer
                .lnurlw
                .ok_or_invalid_input("The provided offer didn't include an lnurlw")?,
        )) {
            Ok(InputType::LnUrlWithdraw { data }) => data,
            _ => return Err(permanent_failure("Invalid LNURLw in offer")),
        };
        let hash = match self
            .rt
            .handle()
            .block_on(
                self.sdk
                    .lnurl_withdraw(lnurlw_data, offer.amount.sats, None),
            )
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to withdraw offer",
            )? {
            LnUrlWithdrawResult::Ok { data } => data.invoice.payment_hash,
            LnUrlWithdrawResult::ErrorStatus { data } => {
                return Err(runtime_error(
                    RuntimeErrorCode::OfferServiceUnavailable,
                    format!("Failed to withdraw offer due to: {}", data.reason),
                ))
            }
        };

        self.store_payment_info(&hash, Some(offer.offer_kind))?;

        Ok(hash)
    }

    /// Registers a new notification token. If a token has already been registered, it will be updated.
    pub fn register_notification_token(
        &self,
        notification_token: String,
        language_iso_639_1: String,
        country_iso_3166_1_alpha_2: String,
    ) -> Result<()> {
        let language = LanguageCode::from_str(&language_iso_639_1.to_lowercase())
            .map_to_invalid_input("Invalid language code")?;
        let country = CountryCode::for_alpha2(&country_iso_3166_1_alpha_2.to_uppercase())
            .map_to_invalid_input("Invalid country code")?;

        self.offer_manager
            .register_notification_token(notification_token, language, country)
            .map_runtime_error_to(RuntimeErrorCode::OfferServiceUnavailable)
    }

    /// Get the wallet UUID v5 from the wallet pubkey
    ///
    /// Returns an optional value. If the auth flow has never succeeded in this Auth instance,
    /// the wallet UUID v5 is unknown and None is returned. Otherwise, this method will always
    /// return the wallet UUID v5.
    ///
    /// This method does not require network access
    pub fn get_wallet_pubkey_id(&self) -> Option<String> {
        self.auth.get_wallet_pubkey_id()
    }

    /// Get the payment UUID v5 from the payment hash
    ///
    /// Returns a UUID v5 derived from the payment hash. This will always return the same output
    /// given the same input.
    ///
    /// Parameters:
    /// * `payment_hash` - a payment hash represented in hex
    pub fn get_payment_uuid(&self, payment_hash: String) -> Result<String> {
        get_payment_uuid(payment_hash)
    }

    fn store_payment_info(&self, hash: &str, offer: Option<OfferKind>) -> Result<()> {
        let user_preferences = self.user_preferences.lock_unwrap().clone();
        let exchange_rates = self.task_manager.lock_unwrap().get_exchange_rates();
        self.data_store
            .lock_unwrap()
            .store_payment_info(hash, user_preferences, exchange_rates, offer)
            .map_to_permanent_failure("Failed to persist payment info")
    }

    /// Query the current recommended on-chain fee rate.
    ///
    /// This is useful to obtain a fee rate to be used for [`LightningNode::sweep`]
    pub fn query_onchain_fee_rate(&self) -> Result<u32> {
        let recommended_fees = self
            .rt
            .handle()
            .block_on(self.sdk.recommended_fees())
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Couldn't fetch recommended fees",
            )?;

        Ok(recommended_fees.half_hour_fee as u32)
    }

    /// Sweeps all available on-chain funds to the specified on-chain address.
    ///
    /// Parameters:
    /// * `address` - the funds will be sweeped to this address
    /// * `onchain_fee_rate` - the fees that should be applied for the transaction.
    /// The recommended on-chain fee can be queried using [`LightningNode::query_onchain_fee_rate`]
    ///
    /// Returns the txid of the sweeping transaction.
    pub fn sweep(&self, address: String, onchain_fee_rate: u32) -> Result<String> {
        Ok(self
            .rt
            .handle()
            .block_on(self.sdk.sweep(SweepRequest {
                to_address: address,
                fee_rate_sats_per_vbyte: onchain_fee_rate,
            }))
            .map_to_runtime_error(RuntimeErrorCode::NodeUnavailable, "Failed to drain funds")?
            .txid
            .to_hex())
    }

    /// Generates a Bitcoin on-chain address that can be used to topup the local LN wallet from an
    /// external on-chain wallet.
    ///
    /// Funds sent to this address should conform to the min and max values provided within
    /// [`SwapAddressInfo`].
    ///
    /// If a swap is in progress, this method will return an error.
    ///
    /// Parameters:
    /// * `lsp_fee_params` - the lsp fee parameters to be used if a new channel needs to
    /// be opened. Can be obtained using [`LightningNode::calculate_lsp_fee`].
    pub fn generate_swap_address(
        &self,
        lsp_fee_params: Option<OpeningFeeParams>,
    ) -> Result<SwapAddressInfo> {
        let swap_info = self
            .rt
            .handle()
            .block_on(self.sdk.receive_onchain(ReceiveOnchainRequest {
                opening_fee_params: lsp_fee_params,
            }))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to generate swap address. Could one be in progress?",
            )?;
        let rate = self.get_exchange_rate();

        Ok(SwapAddressInfo {
            address: swap_info.bitcoin_address,
            min_deposit: ((swap_info.min_allowed_deposit as u64).as_sats()).to_amount_up(&rate),
            max_deposit: ((swap_info.max_allowed_deposit as u64).as_sats()).to_amount_down(&rate),
        })
    }

    /// Lists all unresolved failed swaps. Each individual failed swap can be refunded
    /// using [`LightningNode::resolve_failed_swap`].
    pub fn get_unresolved_failed_swaps(&self) -> Result<Vec<FailedSwapInfo>> {
        Ok(self
            .rt
            .handle()
            .block_on(self.sdk.list_refundables())
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to list refundable failed swaps",
            )?
            .into_iter()
            .map(|s| FailedSwapInfo {
                address: s.bitcoin_address.clone(),
                amount: s
                    .confirmed_sats
                    .as_sats()
                    .to_amount_down(&self.get_exchange_rate()),
                created_at: unix_timestamp_to_system_time(s.created_at as u64),
            })
            .collect())
    }

    /// Creates and broadcasts a refund transaction to recover funds from a failed swap. Existing
    /// failed swaps can be listed using [`LightningNode::get_unresolved_failed_swaps`].
    ///
    /// Parameters:
    /// * `failed_swap_address` - the address of the failed swap (can be obtained from [`FailedSwapInfo`])
    /// * `to_address` - the destination address to which funds will be sent
    /// * `onchain_fee_rate` - the fee rate that will be applied. The recommeded one can be fetched
    /// using [`LightningNode::query_onchain_fee_rate`]
    ///
    /// Returns the txid of the refund transaction.
    pub fn resolve_failed_swap(
        &self,
        failed_swap_address: String,
        to_address: String,
        onchain_fee_rate: u32,
    ) -> Result<String> {
        self.rt
            .handle()
            .block_on(
                self.sdk
                    .refund(failed_swap_address, to_address, onchain_fee_rate),
            )
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to create and broadcast failed swap refund transaction",
            )
    }

    /// Prints additional debug information to the logs.
    ///
    /// Throws an error in case that the necessary information can't be retrieved.
    pub fn log_debug_info(&self) -> Result<()> {
        let peers = self
            .rt
            .handle()
            .block_on(self.sdk.execute_dev_command("listpeers".to_string()))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Couldn't execute `listpeers` command",
            )?;

        let peer_channels = self
            .rt
            .handle()
            .block_on(self.sdk.execute_dev_command("listpeerchannels".to_string()))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Couldn't execute `listpeerchannels` command",
            )?;

        info!("List of peers:\n{}", peers);
        info!("List of peer channels:\n{}", peer_channels);

        Ok(())
    }
}

fn to_offer(topup_info: TopupInfo, current_rate: &Option<ExchangeRate>) -> OfferInfo {
    let exchange_rate = ExchangeRate {
        currency_code: topup_info.exchange_rate.currency_code,
        rate: topup_info.exchange_rate.sats_per_unit,
        updated_at: topup_info.exchange_rate.updated_at,
    };

    let status = match topup_info.status {
        TopupStatus::READY => OfferStatus::READY,
        TopupStatus::FAILED => OfferStatus::FAILED,
        TopupStatus::REFUNDED => OfferStatus::REFUNDED,
        TopupStatus::SETTLED => OfferStatus::SETTLED,
    };

    OfferInfo {
        offer_kind: OfferKind::Pocket {
            id: topup_info.id,
            exchange_rate,
            topup_value_minor_units: topup_info.topup_value_minor_units,
            exchange_fee_minor_units: topup_info.exchange_fee_minor_units,
            exchange_fee_rate_permyriad: topup_info.exchange_fee_rate_permyriad,
            error: topup_info.error,
        },
        amount: topup_info.amount_sat.as_sats().to_amount_down(current_rate),
        lnurlw: topup_info.lnurlw,
        created_at: topup_info.exchange_rate.updated_at,
        expires_at: topup_info.expires_at,
        status,
    }
}

/// Accept lipa's terms and conditions. Should be called before instantiating a [`LightningNode`]
/// for the first time.
pub fn accept_terms_and_conditions(environment: EnvironmentCode, seed: Vec<u8>) -> Result<()> {
    enable_backtrace();
    let environment = Environment::load(environment);
    let seed = sanitize_input::strong_type_seed(&seed)?;
    let auth = build_auth(&seed, environment.backend_url)?;
    auth.accept_terms_and_conditions()
        .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)
}

fn derive_secret_from_mnemonic(mnemonic: Mnemonic, passphrase: String) -> Secret {
    let seed = mnemonic.to_seed(&passphrase);
    let mnemonic_string: Vec<String> = mnemonic.word_iter().map(String::from).collect();

    Secret {
        mnemonic: mnemonic_string,
        passphrase,
        seed: seed.to_vec(),
    }
}

/// Generate a new mnemonic with an optional passphrase. Provide an empty string to use no passphrase.
pub fn generate_secret(passphrase: String) -> std::result::Result<Secret, SimpleError> {
    let entropy = random::generate_random_bytes::<U32>().map_err(|e| SimpleError::Simple {
        msg: format!("Failed to generate random bytes: {e}"),
    })?;
    let mnemonic = Mnemonic::from_entropy(&entropy).map_err(|e| SimpleError::Simple {
        msg: format!("Failed to generate mnemonic: {e}"),
    })?;

    Ok(derive_secret_from_mnemonic(mnemonic, passphrase))
}

/// Generate a Secret object (containing the seed). Provide an empty string to use no passphrase.
pub fn mnemonic_to_secret(
    mnemonic_string: Vec<String>,
    passphrase: String,
) -> std::result::Result<Secret, MnemonicError> {
    let mnemonic = Mnemonic::from_str(&mnemonic_string.join(" ")).map_err(to_mnemonic_error)?;
    Ok(derive_secret_from_mnemonic(mnemonic, passphrase))
}

/// Return a list of valid BIP-39 English words starting with the prefix.
/// Calling this function with empty prefix will return the full list of BIP-39 words.
pub fn words_by_prefix(prefix: String) -> Vec<String> {
    Language::English
        .words_by_prefix(&prefix)
        .iter()
        .map(|w| w.to_string())
        .collect()
}

fn build_auth(seed: &[u8; 64], graphql_url: String) -> Result<Auth> {
    let auth_keys = derive_key_pair_hex(seed, BACKEND_AUTH_DERIVATION_PATH)
        .lift_invalid_input()
        .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)?;
    let auth_keys = KeyPair {
        secret_key: auth_keys.secret_key,
        public_key: auth_keys.public_key,
    };
    Auth::new(
        graphql_url,
        AuthLevel::Pseudonymous,
        auth_keys,
        generate_keypair(),
    )
    .map_to_permanent_failure("Failed to build auth client")
}

fn derive_key_pair_hex(seed: &[u8; 64], derivation_path: &str) -> Result<KeyPair> {
    let master_xpriv = ExtendedPrivKey::new_master(Network::Bitcoin, seed)
        .map_to_invalid_input("Failed to get xpriv from from seed")?;

    let derivation_path = DerivationPath::from_str(derivation_path)
        .map_to_invalid_input("Invalid derivation path")?;

    let derived_xpriv = master_xpriv
        .derive_priv(SECP256K1, &derivation_path)
        .map_to_permanent_failure("Failed to derive keys")?;

    let secret_key = derived_xpriv.private_key.secret_bytes();
    let public_key = PublicKey::from_secret_key(SECP256K1, &derived_xpriv.private_key).serialize();

    Ok(KeyPair {
        secret_key: secret_key.to_vec().to_hex(),
        public_key: public_key.to_hex(),
    })
}

fn get_payment_uuid(payment_hash: String) -> Result<String> {
    let hash = hex::decode(payment_hash).map_to_invalid_input("Invalid payment hash encoding")?;

    Ok(Uuid::new_v5(&Uuid::NAMESPACE_OID, &hash)
        .hyphenated()
        .to_string())
}

pub(crate) fn enable_backtrace() {
    env::set_var("RUST_BACKTRACE", "1");
}

fn get_payment_max_routing_fee_mode(
    amount_sat: u64,
    exchange_rate: &Option<ExchangeRate>,
) -> MaxRoutingFeeMode {
    if amount_sat * (MAX_FEE_PERMYRIAD as u64) / 10 < EXEMPT_FEE.msats {
        MaxRoutingFeeMode::Absolute {
            max_fee_amount: EXEMPT_FEE.to_amount_up(exchange_rate),
        }
    } else {
        MaxRoutingFeeMode::Relative {
            max_fee_permyriad: MAX_FEE_PERMYRIAD,
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/lipalightninglib.uniffi.rs"));

#[cfg(test)]
mod tests {
    use super::*;
    use bip39::Mnemonic;
    use perro::Error;
    use std::str::FromStr;

    const PAYMENT_HASH: &str = "0b78877a596f18d5f6effde3dda1df25a5cf20439ff1ac91478d7e518211040f";
    const PAYMENT_UUID: &str = "c6e597bd-0a98-5b46-8e74-f6098f5d16a3";
    const BACKEND_AUTH_DERIVATION_PATH: &str = "m/76738065'/0'/0";
    // Values used for testing were obtained from https://iancoleman.io/bip39
    const MNEMONIC_STR: &str = "between angry ketchup hill admit attitude echo wisdom still barrel coral obscure home museum trick grow magic eagle school tilt loop actress equal law";
    const SEED_HEX: &str = "781bfd3b2c6a5cfa9ed1551303fa20edf12baa5864521e7782d42a1bb15c2a444f7b81785f537bec6e38a533d0dc88e2a7effad7b975dd7c9bca1f9e7117966d";
    const DERIVED_AUTH_SECRET_KEY_HEX: &str =
        "1b64f7c3f7462e3815eacef53ddf18e5623bf8945d065761b05b022f19e60251";
    const DERIVED_AUTH_PUBLIC_KEY_HEX: &str =
        "02549b15801b155d32ca3931665361b1d2997ee531859b2d48cebbc2ccf21aac96";

    #[test]
    pub fn test_payment_uuid() {
        let payment_uuid = get_payment_uuid(PAYMENT_HASH.to_string());

        assert_eq!(payment_uuid, Ok(PAYMENT_UUID.to_string()));
    }

    #[test]
    pub fn test_payment_uuid_invalid_input() {
        let invalid_hash_encoding = get_payment_uuid("INVALID_HEX_STRING".to_string());

        assert!(matches!(
            invalid_hash_encoding,
            Err(Error::InvalidInput { .. })
        ));

        assert_eq!(
            &invalid_hash_encoding.unwrap_err().to_string()[0..43],
            "InvalidInput: Invalid payment hash encoding"
        );
    }

    fn mnemonic_to_seed(mnemonic: &str) -> [u8; 64] {
        let mnemonic = Mnemonic::from_str(mnemonic).unwrap();
        let mut seed = [0u8; 64];
        seed.copy_from_slice(&mnemonic.to_seed("")[0..64]);

        seed
    }

    #[test]
    fn test_derive_auth_key_pair() {
        let seed = mnemonic_to_seed(MNEMONIC_STR);
        assert_eq!(seed.to_hex(), SEED_HEX.to_string());

        let key_pair = derive_key_pair_hex(&seed, BACKEND_AUTH_DERIVATION_PATH).unwrap();

        assert_eq!(key_pair.secret_key, DERIVED_AUTH_SECRET_KEY_HEX.to_string());
        assert_eq!(key_pair.public_key, DERIVED_AUTH_PUBLIC_KEY_HEX.to_string());
    }

    #[test]
    fn test_get_payment_max_routing_fee_mode_absolute() {
        let max_routing_mode = get_payment_max_routing_fee_mode(3_900, &None);

        match max_routing_mode {
            MaxRoutingFeeMode::Absolute { max_fee_amount } => {
                assert_eq!(max_fee_amount.sats, EXEMPT_FEE.sats);
            }
            _ => {
                panic!("Unexpected variant");
            }
        }
    }

    #[test]
    fn test_get_payment_max_routing_fee_mode_relative() {
        let max_routing_mode = get_payment_max_routing_fee_mode(
            EXEMPT_FEE.msats / ((MAX_FEE_PERMYRIAD as u64) / 10),
            &None,
        );

        match max_routing_mode {
            MaxRoutingFeeMode::Relative { max_fee_permyriad } => {
                assert_eq!(max_fee_permyriad, MAX_FEE_PERMYRIAD);
            }
            _ => {
                panic!("Unexpected variant");
            }
        }
    }
}
