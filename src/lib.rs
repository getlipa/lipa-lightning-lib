//! # lipa-lightning-lib (aka 3L)
//!
//! This crate implements all the main business logic of the lipa wallet.
//!
//! Most functionality can be accessed by creating an instance of [`LightningNode`] and using its methods.

#![allow(clippy::let_unit_value)]

extern crate core;

mod amount;
mod analytics;
mod async_runtime;
mod backup;
mod callbacks;
mod config;
mod data_store;
mod environment;
mod errors;
mod exchange_rate_provider;
mod fiat_topup;
mod fund_migration;
mod invoice_details;
mod key_derivation;
mod limits;
mod locker;
mod logger;
mod macros;
mod migrations;
mod random;
mod recovery;
mod sanitize_input;
mod secret;
mod symmetric_encryption;
mod task_manager;
mod util;

use crate::amount::ToAmount;
pub use crate::amount::{Amount, FiatValue};
use crate::amount::{AsSats, Sats};
use crate::async_runtime::AsyncRuntime;
pub use crate::callbacks::EventsCallback;
pub use crate::config::{Config, TzConfig, TzTime};
use crate::environment::Environment;
pub use crate::environment::EnvironmentCode;
use crate::errors::{to_mnemonic_error, Error, SimpleError};
pub use crate::errors::{
    DecodeDataError, Error as LnError, LnUrlPayError, LnUrlPayErrorCode, LnUrlPayResult,
    MnemonicError, PayError, PayErrorCode, PayResult, Result, RuntimeErrorCode,
    UnsupportedDataType,
};
pub use crate::exchange_rate_provider::ExchangeRate;
use crate::exchange_rate_provider::ExchangeRateProviderImpl;
pub use crate::fiat_topup::FiatTopupInfo;
use crate::fiat_topup::PocketClient;
pub use crate::invoice_details::InvoiceDetails;
pub use crate::limits::{LiquidityLimit, PaymentAmountLimits};
use crate::locker::Locker;
pub use crate::recovery::recover_lightning_node;
use crate::secret::Secret;
use crate::task_manager::{TaskManager, TaskPeriods};
use crate::util::unix_timestamp_to_system_time;

pub use breez_sdk_core::error::ReceiveOnchainError as SwapError;
pub use parrot::PaymentSource;

use crate::analytics::{derive_analytics_keys, AnalyticsInterceptor};
pub use crate::analytics::{InvoiceCreationMetadata, PaymentMetadata};
use crate::backup::BackupManager;
use crate::key_derivation::derive_persistence_encryption_key;
use bip39::{Language, Mnemonic};
use bitcoin::bip32::{DerivationPath, ExtendedPrivKey};
use bitcoin::secp256k1::{PublicKey, SECP256K1};
use bitcoin::Network;
use breez_sdk_core::error::{LnUrlWithdrawError, ReceiveOnchainError, SendPaymentError};
use breez_sdk_core::{
    parse, parse_invoice, BreezEvent, BreezServices, EventListener, GreenlightCredentials,
    GreenlightNodeConfig, InputType, ListPaymentsRequest, LnUrlPayRequest, LnUrlPayRequestData,
    LnUrlWithdrawRequest, LnUrlWithdrawResult, NodeConfig, OpenChannelFeeRequest, OpeningFeeParams,
    PaymentDetails, PaymentStatus, PaymentTypeFilter, PrepareRefundRequest, ReceiveOnchainRequest,
    RefundRequest, SendPaymentRequest, SweepRequest,
};
use cipher::generic_array::typenum::U32;
use crow::{CountryCode, LanguageCode, OfferManager, TopupError, TopupInfo, TopupStatus};
pub use crow::{PermanentFailureCode, TemporaryFailureCode};
use data_store::DataStore;
use email_address::EmailAddress;
use honey_badger::secrets::{generate_keypair, KeyPair};
use honey_badger::{Auth, AuthLevel, CustomTermsAndConditions};
use iban::Iban;
use log::{info, trace};
use logger::init_logger_once;
use parrot::AnalyticsClient;
use perro::{
    invalid_input, permanent_failure, runtime_error, MapToError, OptionToError, ResultTrait,
};
use squirrel::RemoteBackupClient;
use std::cmp::Reverse;
use std::collections::HashSet;
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use std::{env, fs};
use uuid::Uuid;

const LOG_LEVEL: log::Level = log::Level::Trace;
const LOGS_DIR: &str = "logs";

pub(crate) const DB_FILENAME: &str = "db2.db3";

const BACKEND_AUTH_DERIVATION_PATH: &str = "m/76738065'/0'/0";

/// The fee charged by the Lightning Service Provider (LSP) for opening a channel with the node.
/// This fee is being charged at the time of the channel creation.
/// The LSP simply subtracts this fee from an incoming payment (if this incoming payment leads to a channel creation).
pub struct LspFee {
    pub channel_minimum_fee: Amount,
    /// Parts per myriad (aka basis points) -> 100 is 1%
    pub channel_fee_permyriad: u64,
}

/// The type returned by [`LightningNode::calculate_lsp_fee`].
pub struct CalculateLspFeeResponse {
    /// Indicates the amount that will be charged.
    pub lsp_fee: Amount,
    /// An internal struct is not supposed to be inspected, but only passed to [`LightningNode::create_invoice`].
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

#[derive(PartialEq, Eq, Debug, Clone)]
#[repr(u8)]
pub enum PaymentType {
    Receiving,
    Sending,
}

#[derive(PartialEq, Eq, Debug, Clone)]
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

/// Information about an incoming or outgoing payment.
#[derive(PartialEq)]
pub struct Payment {
    pub payment_type: PaymentType,
    pub payment_state: PaymentState,
    /// For now, will always be empty.
    pub fail_reason: Option<PayErrorCode>,
    /// Hex representation of payment hash.
    pub hash: String,
    /// Nominal amount specified in the invoice.
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
    /// The swap information of a [`PaymentType::Receiving`] payment if triggered by a swap.
    pub swap: Option<SwapInfo>,
}

/// Information about a successful swap.
#[derive(PartialEq)]
pub struct SwapInfo {
    pub bitcoin_address: String,
    pub created_at: TzTime,
    pub paid_sats: u64,
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
    /// The amount that is available to be recovered. The recovery will involve paying some
    /// on-chain fees so it isn't possible to recover the entire amount.
    pub amount: Amount,
    pub created_at: SystemTime,
}

/// Information the resolution of a failed swap.
pub struct ResolveFailedSwapInfo {
    /// The address of the failed swap.
    pub swap_address: String,
    /// The amount that will be sent (swap amount - onchain fee).
    pub recovered_amount: Amount,
    /// The amount that will be paid in onchain fees.
    pub onchain_fee: Amount,
    /// The address to which recovered funds will be sent.
    pub to_address: String,
    /// The onchain fee rate that will be applied. This fee rate results in the `onchain_fee`.
    pub onchain_fee_rate: u32,
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
    analytics_interceptor: Arc<AnalyticsInterceptor>,
}

impl EventListener for LipaEventListener {
    fn on_event(&self, e: BreezEvent) {
        match e {
            BreezEvent::NewBlock { .. } => {}
            BreezEvent::InvoicePaid { details } => {
                self.analytics_interceptor
                    .request_succeeded(details.clone());
                self.events_callback.payment_received(details.payment_hash)
            }
            BreezEvent::Synced => {}
            BreezEvent::PaymentSucceed { details } => {
                if let PaymentDetails::Ln { data } = details.details.clone() {
                    self.analytics_interceptor.pay_succeeded(details);
                    self.events_callback
                        .payment_sent(data.payment_hash, data.payment_preimage)
                }
            }
            BreezEvent::PaymentFailed { details } => {
                if let Some(invoice) = details.invoice.clone() {
                    self.analytics_interceptor.pay_failed(details);
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
    redeem_swaps: Some(Duration::from_secs(10 * 60)),
    backup: Some(Duration::from_secs(30)),
};

const BACKGROUND_PERIODS: TaskPeriods = TaskPeriods {
    update_exchange_rates: None,
    sync_breez: Some(Duration::from_secs(30 * 60)),
    update_lsp_fee: None,
    redeem_swaps: None,
    backup: None,
};

/// The main class/struct of this library. Constructing an instance will initiate the Lightning node and
/// run it in the background. As long as an instance of `LightningNode` is held, the node will continue to run
/// in the background. Dropping the instance will start a deinit process.  
pub struct LightningNode {
    user_preferences: Arc<Mutex<UserPreferences>>,
    sdk: Arc<BreezServices>,
    auth: Arc<Auth>,
    fiat_topup_client: PocketClient,
    offer_manager: OfferManager,
    rt: AsyncRuntime,
    data_store: Arc<Mutex<DataStore>>,
    task_manager: Arc<Mutex<TaskManager>>,
    analytics_interceptor: Arc<AnalyticsInterceptor>,
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

        let user_preferences = Arc::new(Mutex::new(UserPreferences {
            fiat_currency: config.fiat_currency,
            timezone_config: config.timezone_config,
        }));

        let analytics_client = Arc::new(AnalyticsClient::new(
            environment.backend_url.clone(),
            derive_analytics_keys(&strong_typed_seed)?,
            auth.clone(),
        ));

        let analytics_interceptor = Arc::new(AnalyticsInterceptor::new(
            analytics_client.clone(),
            user_preferences.clone(),
            rt.handle(),
        ));

        let sdk = rt
            .handle()
            .block_on(BreezServices::connect(
                breez_config,
                config.seed.clone(),
                Box::new(LipaEventListener {
                    events_callback,
                    analytics_interceptor: analytics_interceptor.clone(),
                }),
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
                    .into_iter()
                    .next()
                    .ok_or_runtime_error(RuntimeErrorCode::NodeUnavailable, "No lsp available")?;
                sdk.connect_lsp(lsp.id).await.map_to_runtime_error(
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

        let offer_manager = OfferManager::new(environment.backend_url.clone(), Arc::clone(&auth));

        let db_path = format!("{}/{DB_FILENAME}", config.local_persistence_path);

        let data_store = Arc::new(Mutex::new(DataStore::new(&db_path)?));

        let fiat_topup_client =
            PocketClient::new(environment.pocket_url, Arc::clone(&sdk), rt.handle())?;

        let async_auth = Arc::new(build_async_auth(
            &strong_typed_seed,
            environment.backend_url.clone(),
        )?);
        let backup_client =
            RemoteBackupClient::new(environment.backend_url.clone(), Arc::clone(&async_auth));
        let backup_manager = BackupManager::new(
            backup_client,
            db_path,
            derive_persistence_encryption_key(&strong_typed_seed)?,
        );

        let task_manager = Arc::new(Mutex::new(TaskManager::new(
            rt.handle(),
            exchange_rate_provider,
            Arc::clone(&data_store),
            Arc::clone(&sdk),
            backup_manager,
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
            analytics_interceptor,
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
                    redeem_swaps: Some(period),
                    backup: Some(period),
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
            amount_msat: amount_sat.as_sats().msats,
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

    /// Create an invoice to receive a payment with.
    ///
    /// Parameters:
    /// * `amount_sat` - the smallest amount of sats required for the node to accept the incoming
    /// payment (sender will have to pay fees on top of that amount)
    /// * `lsp_fee_params` - the params that will be used to determine the lsp fee.
    /// Can be obtained from [`LightningNode::calculate_lsp_fee`] to guarantee predicted fees
    /// are the ones charged.
    /// * `description` - a description to be embedded into the created invoice
    /// * `metadata` - additional data about the invoice creation used for analytics purposes,
    /// used to improve the user experience
    pub fn create_invoice(
        &self,
        amount_sat: u64,
        lsp_fee_params: Option<OpeningFeeParams>,
        description: String,
        metadata: InvoiceCreationMetadata,
    ) -> Result<InvoiceDetails> {
        let response = self
            .rt
            .handle()
            .block_on(
                self.sdk
                    .receive_payment(breez_sdk_core::ReceivePaymentRequest {
                        amount_msat: amount_sat.as_sats().msats,
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
        self.data_store
            .lock_unwrap()
            .store_created_invoice(
                &response.ln_invoice.payment_hash,
                &response.ln_invoice.bolt11,
            )
            .map_to_permanent_failure("Failed to persist created invoice")?;

        self.analytics_interceptor.request_initiated(
            response.clone(),
            self.get_exchange_rate(),
            metadata,
        );
        Ok(InvoiceDetails::from_ln_invoice(
            response.ln_invoice,
            &self.get_exchange_rate(),
        ))
    }

    /// Decode a user-provided string (usually obtained from QR-code or pasted).
    pub fn decode_data(&self, data: String) -> std::result::Result<DecodedData, DecodeDataError> {
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
            Ok(InputType::BitcoinAddress { .. }) => Err(DecodeDataError::Unsupported {
                typ: UnsupportedDataType::BitcoinAddress,
            }),
            Ok(InputType::LnUrlAuth { .. }) => Err(DecodeDataError::Unsupported {
                typ: UnsupportedDataType::LnUrlAuth,
            }),
            Ok(InputType::LnUrlError { data }) => {
                Err(DecodeDataError::LnUrlError { msg: data.reason })
            }
            Ok(InputType::LnUrlWithdraw { .. }) => Err(DecodeDataError::Unsupported {
                typ: UnsupportedDataType::LnUrlWithdraw,
            }),
            Ok(InputType::NodeId { .. }) => Err(DecodeDataError::Unsupported {
                typ: UnsupportedDataType::NodeId,
            }),
            Ok(InputType::Url { .. }) => Err(DecodeDataError::Unsupported {
                typ: UnsupportedDataType::Url,
            }),
            Err(e) => Err(DecodeDataError::Unrecognized { msg: e.to_string() }),
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
    /// * `invoice_details` - details of an invoice decode by [`LightningNode::decode_data`]
    /// * `metadata` - additional meta information about the payment, used by analytics to improve the user experience.
    pub fn pay_invoice(
        &self,
        invoice_details: InvoiceDetails,
        metadata: PaymentMetadata,
    ) -> PayResult<()> {
        self.pay_open_invoice(invoice_details, 0, metadata)
    }

    /// Similar to [`LightningNode::pay_invoice`] with the difference that the passed in invoice
    /// does not have any payment amount specified, and allows the caller of the method to
    /// specify an amount instead.
    ///
    /// Additional Parameters:
    /// * `amount_sat` - amount in sats to be paid
    pub fn pay_open_invoice(
        &self,
        invoice_details: InvoiceDetails,
        amount_sat: u64,
        metadata: PaymentMetadata,
    ) -> PayResult<()> {
        let amount_msat = if amount_sat == 0 {
            None
        } else {
            Some(amount_sat.as_sats().msats)
        };
        self.store_payment_info(&invoice_details.payment_hash, None)
            .map_to_permanent_failure("Failed to persist local payment data")?;
        // TODO: persist metadata
        let node_state = self
            .sdk
            .node_info()
            .map_to_runtime_error(PayErrorCode::NodeUnavailable, "Failed to read node info")?;
        ensure!(
            node_state.id != invoice_details.payee_pub_key,
            runtime_error(
                PayErrorCode::PayingToSelf,
                "A locally issued invoice tried to be paid"
            )
        );

        self.analytics_interceptor.pay_initiated(
            invoice_details.clone(),
            metadata,
            amount_msat,
            self.get_exchange_rate(),
        );

        self.rt
            .handle()
            .block_on(self.sdk.send_payment(SendPaymentRequest {
                bolt11: invoice_details.invoice,
                amount_msat,
            }))
            .map_err(map_send_payment_error)?;
        Ok(())
    }

    /// Pay an LNURL-pay the provided amount.
    ///
    /// Parameters:
    /// * `lnurl_pay_request_data` - LNURL-pay request data as obtained from [`LightningNode::decode_data`]
    /// * `amount_sat` - amount to be paid
    ///
    /// Returns the payment hash of the payment.
    pub fn pay_lnurlp(
        &self,
        lnurl_pay_request_data: LnUrlPayRequestData,
        amount_sat: u64,
    ) -> LnUrlPayResult<String> {
        let payment_hash = match self
            .rt
            .handle()
            .block_on(self.sdk.lnurl_pay(LnUrlPayRequest {
                data: lnurl_pay_request_data,
                amount_msat: amount_sat.as_sats().msats,
                comment: None,
            }))
            .map_err(map_lnurl_pay_error)?
        {
            breez_sdk_core::LnUrlPayResult::EndpointSuccess { data } => Ok(data.payment_hash),
            breez_sdk_core::LnUrlPayResult::EndpointError { data } => runtime_error!(
                LnUrlPayErrorCode::LnUrlServerError,
                "LNURL server returned error: {}",
                data.reason
            ),
            breez_sdk_core::LnUrlPayResult::PayError { data } => runtime_error!(
                LnUrlPayErrorCode::PaymentFailed,
                "Paying invoice for LNURL pay failed: {}",
                data.reason
            ),
        }?;
        if let Err(err) = self.store_payment_info(&payment_hash, None) {
            log::error!("Failed to persist payment info for {payment_hash}: {err}. Ignoring.");
        }
        Ok(payment_hash)
    }

    /// Get a list of the latest payments
    ///
    /// Parameters:
    /// * `number_of_payments` - the maximum number of payments that will be returned
    pub fn get_latest_payments(&self, number_of_payments: u32) -> Result<Vec<Payment>> {
        let list_payments_request = ListPaymentsRequest {
            filters: Some(vec![PaymentTypeFilter::Sent, PaymentTypeFilter::Received]),
            from_timestamp: None,
            to_timestamp: None,
            include_failures: Some(true),
            limit: Some(number_of_payments),
            offset: None,
        };
        let breez_payments = self
            .rt
            .handle()
            .block_on(self.sdk.list_payments(list_payments_request))
            .map_to_runtime_error(RuntimeErrorCode::NodeUnavailable, "Failed to list payments")?
            .into_iter()
            .map(|p| self.payment_from_breez_payment(p))
            .collect::<Result<Vec<Payment>>>()?;

        let breez_payment_invoices: HashSet<String> = breez_payments
            .iter()
            .map(|p| p.invoice_details.invoice.clone())
            .collect();
        let created_invoices = self
            .data_store
            .lock_unwrap()
            .retrieve_created_invoices(number_of_payments)?;
        let mut pending_inbound_payments = created_invoices
            .into_iter()
            .filter(|i| !breez_payment_invoices.contains(i))
            .map(|i| self.payment_from_created_invoice(&i))
            .collect::<Result<Vec<Payment>>>()?;

        let mut payments = breez_payments;
        payments.append(&mut pending_inbound_payments);
        payments.sort_by_key(|p| Reverse(p.created_at.time));
        Ok(payments
            .into_iter()
            .take(number_of_payments as usize)
            .collect())
    }

    /// Get a payment given its payment hash
    ///
    /// Parameters:
    /// * `hash` - hex representation of payment hash
    pub fn get_payment(&self, hash: String) -> Result<Payment> {
        if let Some(breez_payment) = self
            .rt
            .handle()
            .block_on(self.sdk.payment_by_hash(hash.clone()))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to get payment by hash",
            )?
        {
            self.payment_from_breez_payment(breez_payment)
        } else if let Some(invoice) = self
            .data_store
            .lock_unwrap()
            .retrieve_created_invoice_by_hash(&hash)?
        {
            self.payment_from_created_invoice(&invoice)
        } else {
            invalid_input!("No payment with provided hash was found");
        }
    }

    fn payment_from_breez_payment(
        &self,
        breez_payment: breez_sdk_core::Payment,
    ) -> Result<Payment> {
        let payment_details = match breez_payment.details {
            PaymentDetails::Ln { data } => data,
            PaymentDetails::ClosedChannel { .. } => permanent_failure!(
                "Current interface doesn't support PaymentDetails::ClosedChannel"
            ),
        };

        let invoice = parse_invoice(&payment_details.bolt11)
            .map_to_permanent_failure("Invalid invoice provided by the Breez SDK")?;
        let invoice_details = InvoiceDetails::from_ln_invoice(invoice.clone(), &None);

        let local_payment_data = self
            .data_store
            .lock_unwrap()
            .retrieve_payment_info(&payment_details.payment_hash)?;

        // Use invoice timestamp for receiving payments and breez_payment.payment_time for sending ones
        // Reasoning: for receiving payments, Breez returns the time the invoice was paid. Given that
        // now we show pending invoices, this can result in a receiving payment jumping around in the
        // list when it gets paid.
        let time = match breez_payment.payment_type {
            breez_sdk_core::PaymentType::Sent => {
                unix_timestamp_to_system_time(breez_payment.payment_time as u64)
            }
            breez_sdk_core::PaymentType::Received => invoice_details.creation_timestamp,
            breez_sdk_core::PaymentType::ClosedChannel => {
                permanent_failure!(
                    "Current interface doesn't support PaymentDetails::ClosedChannel"
                )
            }
        };
        let (exchange_rate, time, offer) = match local_payment_data {
            None => {
                let exchange_rate = self.get_exchange_rate();
                let user_preferences = self.user_preferences.lock_unwrap();
                let time = TzTime {
                    time,
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
                    time,
                    timezone_id: d.user_preferences.timezone_config.timezone_id,
                    timezone_utc_offset_secs: d
                        .user_preferences
                        .timezone_config
                        .timezone_utc_offset_secs,
                };
                (exchange_rate, time, d.offer)
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
            breez_sdk_core::PaymentType::ClosedChannel => permanent_failure!(
                "Current interface doesn't support PaymentDetails::ClosedChannel"
            ),
        };

        let payment_state = match breez_payment.status {
            PaymentStatus::Pending => PaymentState::Created,
            PaymentStatus::Complete => PaymentState::Succeeded,
            PaymentStatus::Failed => PaymentState::Failed,
        };

        let invoice_details = InvoiceDetails::from_ln_invoice(invoice, &exchange_rate);

        let description = invoice_details.description.clone();

        let user_preferences = self.user_preferences.lock_unwrap();
        let swap = payment_details.swap_info.map(|s| SwapInfo {
            bitcoin_address: s.bitcoin_address,
            created_at: TzTime {
                // TODO: Persist SwapInfo in local db on state change, requires https://github.com/breez/breez-sdk/issues/518
                time: unix_timestamp_to_system_time(s.created_at as u64),
                timezone_id: user_preferences.timezone_config.timezone_id.clone(),
                timezone_utc_offset_secs: user_preferences.timezone_config.timezone_utc_offset_secs,
            },
            paid_sats: s.paid_sats,
        });

        Ok(Payment {
            payment_type,
            payment_state,
            fail_reason: None, // TODO: Request SDK to store and provide reason for failed payments
            hash: payment_details.payment_hash,
            amount,
            invoice_details,
            created_at: time,
            description,
            preimage: Some(payment_details.payment_preimage),
            network_fees,
            lsp_fees,
            offer,
            swap,
        })
    }

    fn payment_from_created_invoice(&self, invoice: &str) -> Result<Payment> {
        let invoice = parse_invoice(invoice)
            .map_to_permanent_failure("Invalid invoice provided by the Breez SDK")?;
        let invoice_details = InvoiceDetails::from_ln_invoice(invoice, &None);

        let payment_state = if SystemTime::now() > invoice_details.expiry_timestamp {
            PaymentState::InvoiceExpired
        } else {
            PaymentState::Created
        };

        let local_payment_data = self
            .data_store
            .lock_unwrap()
            .retrieve_payment_info(&invoice_details.payment_hash)?
            .ok_or_permanent_failure("Locally created invoice doesn't have local payment data")?;
        let exchange_rate = Some(local_payment_data.exchange_rate);
        let time = TzTime {
            time: invoice_details.creation_timestamp, // for receiving payments, we use the invoice timestamp
            timezone_id: local_payment_data
                .user_preferences
                .timezone_config
                .timezone_id,
            timezone_utc_offset_secs: local_payment_data
                .user_preferences
                .timezone_config
                .timezone_utc_offset_secs,
        };

        Ok(Payment {
            payment_type: PaymentType::Receiving,
            payment_state,
            fail_reason: None,
            hash: invoice_details.payment_hash.clone(),
            amount: invoice_details
                .amount
                .clone()
                .ok_or_permanent_failure("Locally created invoice doesn't include an amount")?
                .sats
                .as_sats()
                .to_amount_down(&exchange_rate),
            invoice_details: invoice_details.clone(),
            created_at: time,
            description: invoice_details.description,
            preimage: None,
            network_fees: None,
            lsp_fees: None,
            offer: None,
            swap: None,
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
    /// * `user_currency` - the fiat currency (ISO 4217 currency code) that will be sent for
    /// exchange. Not all are supported. A consumer of this library should find out about available
    /// ones using other sources.
    pub fn register_fiat_topup(
        &self,
        email: Option<String>,
        user_iban: String,
        user_currency: String,
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

        self.data_store
            .lock_unwrap()
            .store_fiat_topup_info(topup_info.clone())?;

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
        let latest_payments = self.get_latest_payments(5)?;
        Ok(
            filter_out_recently_claimed_topups(topup_infos, latest_payments)
                .into_iter()
                .map(|o| to_offer(o, &rate))
                .collect(),
        )
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
            Ok(input_type) => {
                permanent_failure!("Invalid input type LNURLw in offer: {input_type:?}")
            }
            Err(err) => permanent_failure!("Invalid LNURLw in offer: {err}"),
        };
        let hash = match self
            .rt
            .handle()
            .block_on(self.sdk.lnurl_withdraw(LnUrlWithdrawRequest {
                data: lnurlw_data,
                amount_msat: offer.amount.sats.as_sats().msats,
                description: None,
            })) {
            Ok(LnUrlWithdrawResult::Ok { data }) => data.invoice.payment_hash,
            Ok(LnUrlWithdrawResult::ErrorStatus { data }) => runtime_error!(
                RuntimeErrorCode::OfferServiceUnavailable,
                "Failed to withdraw offer due to: {}",
                data.reason
            ),
            Err(LnUrlWithdrawError::Generic { err }) => runtime_error!(
                RuntimeErrorCode::OfferServiceUnavailable,
                "Failed to withdraw offer due to: {err}"
            ),
            Err(LnUrlWithdrawError::InvalidAmount { err }) => {
                permanent_failure!("Invalid amount in invoice for LNURL withdraw: {err}")
            }
            Err(LnUrlWithdrawError::InvalidInvoice { err }) => {
                permanent_failure!("Invalid invoice for LNURL withdraw: {err}")
            }
            Err(LnUrlWithdrawError::InvalidUri { err }) => {
                permanent_failure!("Invalid URL in LNURL withdraw: {err}")
            }
            Err(LnUrlWithdrawError::ServiceConnectivity { err }) => runtime_error!(
                RuntimeErrorCode::OfferServiceUnavailable,
                "Failed to withdraw offer due to: {err}"
            ),
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
        let txid = self
            .rt
            .handle()
            .block_on(self.sdk.sweep(SweepRequest {
                to_address: address,
                fee_rate_sats_per_vbyte: onchain_fee_rate,
            }))
            .map_to_runtime_error(RuntimeErrorCode::NodeUnavailable, "Failed to drain funds")?
            .txid;
        Ok(hex::encode(txid))
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
    ) -> std::result::Result<SwapAddressInfo, ReceiveOnchainError> {
        let swap_info =
            self.rt
                .handle()
                .block_on(self.sdk.receive_onchain(ReceiveOnchainRequest {
                    opening_fee_params: lsp_fee_params,
                }))?;
        let rate = self.get_exchange_rate();

        Ok(SwapAddressInfo {
            address: swap_info.bitcoin_address,
            min_deposit: ((swap_info.min_allowed_deposit as u64).as_sats()).to_amount_up(&rate),
            max_deposit: ((swap_info.max_allowed_deposit as u64).as_sats()).to_amount_down(&rate),
        })
    }

    /// Lists all unresolved failed swaps. Each individual failed swap can be recovered
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

    /// Prepares the resolution of a failed swap in order to know how much will be recovered and how much
    /// will be paid in onchain fees.
    ///
    /// Parameters:
    /// * `failed_swap_info` - the failed swap that will be prepared
    /// * `to_address` - the destination address to which funds will be sent
    /// * `onchain_fee_rate` - the fee rate that will be applied. The recommended one can be fetched
    /// using [`LightningNode::query_onchain_fee_rate`]
    pub fn prepare_resolve_failed_swap(
        &self,
        failed_swap_info: FailedSwapInfo,
        to_address: String,
        onchain_fee_rate: u32,
    ) -> Result<ResolveFailedSwapInfo> {
        let response = self
            .rt
            .handle()
            .block_on(self.sdk.prepare_refund(PrepareRefundRequest {
                swap_address: failed_swap_info.address.clone(),
                to_address: to_address.clone(),
                sat_per_vbyte: onchain_fee_rate,
            }))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to prepare a failed swap refund transaction",
            )?;

        let rate = self.get_exchange_rate();
        let onchain_fee = response.refund_tx_fee_sat.as_sats().to_amount_up(&rate);
        let recovered_amount = (failed_swap_info.amount.sats - onchain_fee.sats)
            .as_sats()
            .to_amount_down(&rate);

        Ok(ResolveFailedSwapInfo {
            swap_address: failed_swap_info.address,
            recovered_amount,
            onchain_fee,
            to_address,
            onchain_fee_rate,
        })
    }

    /// Creates and broadcasts a resolving transaction to recover funds from a failed swap. Existing
    /// failed swaps can be listed using [`LightningNode::get_unresolved_failed_swaps`] and preparing
    /// the resolution of a failed swap can be done using [`LightningNode::prepare_resolve_failed_swap`].
    ///
    /// Parameters:
    /// * `resolve_failed_swap_info` - Information needed to resolve the failed swap. Can be obtained
    /// using [`LightningNode::prepare_resolve_failed_swap`].
    ///
    /// Returns the txid of the resolving transaction.
    ///
    /// Paid on-chain fees can be known in advance using [`LightningNode::prepare_resolve_failed_swap`].
    pub fn resolve_failed_swap(
        &self,
        resolve_failed_swap_info: ResolveFailedSwapInfo,
    ) -> Result<String> {
        Ok(self
            .rt
            .handle()
            .block_on(self.sdk.refund(RefundRequest {
                swap_address: resolve_failed_swap_info.swap_address,
                to_address: resolve_failed_swap_info.to_address,
                sat_per_vbyte: resolve_failed_swap_info.onchain_fee_rate,
            }))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to create and broadcast failed swap refund transaction",
            )?
            .refund_tx_id)
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

    /// Returns the latest [`FiatTopupInfo`] if the user has registered for the fiat topup.
    pub fn retrieve_latest_fiat_topup_info(&self) -> Result<Option<FiatTopupInfo>> {
        self.data_store
            .lock_unwrap()
            .retrieve_latest_fiat_topup_info()
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

fn build_async_auth(
    seed: &[u8; 64],
    graphql_url: String,
) -> Result<honey_badger::asynchronous::Auth> {
    let auth_keys = derive_key_pair_hex(seed, BACKEND_AUTH_DERIVATION_PATH)
        .lift_invalid_input()
        .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)?;
    let auth_keys = KeyPair {
        secret_key: auth_keys.secret_key,
        public_key: auth_keys.public_key,
    };
    honey_badger::asynchronous::Auth::new(
        graphql_url,
        AuthLevel::Pseudonymous,
        auth_keys,
        generate_keypair(),
    )
    .map_to_permanent_failure("Failed to build auth client")
}

fn map_send_payment_error(err: SendPaymentError) -> PayError {
    match err {
        SendPaymentError::AlreadyPaid => {
            runtime_error(PayErrorCode::AlreadyUsedInvoice, String::new())
        }
        SendPaymentError::Generic { err } => runtime_error(PayErrorCode::UnexpectedError, err),
        SendPaymentError::InvalidAmount { err } => invalid_input(format!("Invalid amount: {err}")),
        SendPaymentError::InvalidInvoice { err } => {
            invalid_input(format!("Invalid invoice: {err}"))
        }
        SendPaymentError::InvoiceExpired { err } => {
            runtime_error(PayErrorCode::InvoiceExpired, err)
        }
        SendPaymentError::PaymentFailed { err } => runtime_error(PayErrorCode::PaymentFailed, err),
        SendPaymentError::PaymentTimeout { err } => {
            runtime_error(PayErrorCode::PaymentTimeout, err)
        }
        SendPaymentError::RouteNotFound { err } => runtime_error(PayErrorCode::NoRouteFound, err),
        SendPaymentError::RouteTooExpensive { err } => {
            runtime_error(PayErrorCode::RouteTooExpensive, err)
        }
        SendPaymentError::ServiceConnectivity { err } => {
            runtime_error(PayErrorCode::NodeUnavailable, err)
        }
    }
}

fn map_lnurl_pay_error(
    error: breez_sdk_core::error::LnUrlPayError,
) -> crate::errors::LnUrlPayError {
    use breez_sdk_core::error::LnUrlPayError;
    match error {
        LnUrlPayError::AesDecryptionFailed { .. } => {
            runtime_error(LnUrlPayErrorCode::LnUrlServerError, error)
        }
        LnUrlPayError::InvalidUri { err } => invalid_input(format!("InvalidUri: {err}")),
        LnUrlPayError::AlreadyPaid => permanent_failure("LNURL pay invoice has been already paid"),
        LnUrlPayError::Generic { err } => runtime_error(LnUrlPayErrorCode::UnexpectedError, err),
        LnUrlPayError::InvalidAmount { err } => runtime_error(
            LnUrlPayErrorCode::LnUrlServerError,
            format!("Invalid amount in the invoice from LNURL pay server: {err}"),
        ),
        LnUrlPayError::InvalidInvoice { err } => runtime_error(
            LnUrlPayErrorCode::LnUrlServerError,
            format!("Invalid invoice from LNURL pay server: {err}"),
        ),
        LnUrlPayError::InvoiceExpired { err } => {
            permanent_failure(format!("Invoice for LNURL pay has already expired: {err}"))
        }
        LnUrlPayError::PaymentFailed { err } => {
            runtime_error(LnUrlPayErrorCode::PaymentFailed, err)
        }
        LnUrlPayError::PaymentTimeout { err } => {
            runtime_error(LnUrlPayErrorCode::PaymentTimeout, err)
        }
        LnUrlPayError::RouteNotFound { err } => runtime_error(LnUrlPayErrorCode::NoRouteFound, err),
        LnUrlPayError::RouteTooExpensive { err } => {
            runtime_error(LnUrlPayErrorCode::RouteTooExpensive, err)
        }
        LnUrlPayError::ServiceConnectivity { err } => {
            runtime_error(LnUrlPayErrorCode::ServiceConnectivity, err)
        }
    }
}

pub(crate) fn derive_key_pair_hex(seed: &[u8; 64], derivation_path: &str) -> Result<KeyPair> {
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
        secret_key: hex::encode(secret_key),
        public_key: hex::encode(public_key),
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

fn filter_out_recently_claimed_topups(
    topups: Vec<TopupInfo>,
    latest_payments: Vec<Payment>,
) -> Vec<TopupInfo> {
    let latest_succeeded_payment_offer_ids: HashSet<String> = latest_payments
        .into_iter()
        .filter(|p| p.payment_state == PaymentState::Succeeded)
        .filter_map(|p| p.offer.map(|OfferKind::Pocket { id, .. }| id))
        .collect();
    topups
        .into_iter()
        .filter(|o| !latest_succeeded_payment_offer_ids.contains(&o.id))
        .collect()
}

include!(concat!(env!("OUT_DIR"), "/lipalightninglib.uniffi.rs"));

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key_derivation::tests::mnemonic_to_seed;
    use perro::Error;

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

    #[test]
    fn test_derive_auth_key_pair() {
        let seed = mnemonic_to_seed(MNEMONIC_STR);
        assert_eq!(hex::encode(seed), SEED_HEX.to_string());

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

    #[test]
    fn test_filter_out_recently_claimed_topups() {
        let topups = vec![
            TopupInfo {
                id: "123".to_string(),
                status: TopupStatus::READY,
                amount_sat: 0,
                topup_value_minor_units: 0,
                exchange_fee_rate_permyriad: 0,
                exchange_fee_minor_units: 0,
                exchange_rate: graphql::ExchangeRate {
                    currency_code: "eur".to_string(),
                    sats_per_unit: 0,
                    updated_at: SystemTime::now(),
                },
                expires_at: None,
                lnurlw: None,
                error: None,
            },
            TopupInfo {
                id: "234".to_string(),
                status: TopupStatus::READY,
                amount_sat: 0,
                topup_value_minor_units: 0,
                exchange_fee_rate_permyriad: 0,
                exchange_fee_minor_units: 0,
                exchange_rate: graphql::ExchangeRate {
                    currency_code: "eur".to_string(),
                    sats_per_unit: 0,
                    updated_at: SystemTime::now(),
                },
                expires_at: None,
                lnurlw: None,
                error: None,
            },
        ];

        let latest_payments = vec![
            Payment {
                payment_type: PaymentType::Receiving,
                payment_state: PaymentState::Succeeded,
                fail_reason: None,
                hash: "hash".to_string(),
                amount: Amount {
                    sats: 0,
                    fiat: None,
                },
                invoice_details: InvoiceDetails {
                    invoice: "bca".to_string(),
                    amount: None,
                    description: "".to_string(),
                    payment_hash: "".to_string(),
                    payee_pub_key: "".to_string(),
                    creation_timestamp: SystemTime::now(),
                    expiry_interval: Default::default(),
                    expiry_timestamp: SystemTime::now(),
                },
                created_at: TzTime {
                    time: SystemTime::now(),
                    timezone_id: "".to_string(),
                    timezone_utc_offset_secs: 0,
                },
                description: "".to_string(),
                preimage: None,
                network_fees: None,
                lsp_fees: None,
                offer: None,
                swap: None,
            },
            Payment {
                payment_type: PaymentType::Receiving,
                payment_state: PaymentState::Succeeded,
                fail_reason: None,
                hash: "hash2".to_string(),
                amount: Amount {
                    sats: 0,
                    fiat: None,
                },
                invoice_details: InvoiceDetails {
                    invoice: "abc".to_string(),
                    amount: None,
                    description: "".to_string(),
                    payment_hash: "".to_string(),
                    payee_pub_key: "".to_string(),
                    creation_timestamp: SystemTime::now(),
                    expiry_interval: Default::default(),
                    expiry_timestamp: SystemTime::now(),
                },
                created_at: TzTime {
                    time: SystemTime::now(),
                    timezone_id: "".to_string(),
                    timezone_utc_offset_secs: 0,
                },
                description: "".to_string(),
                preimage: None,
                network_fees: None,
                lsp_fees: None,
                offer: Some(OfferKind::Pocket {
                    id: "123".to_string(),
                    exchange_rate: ExchangeRate {
                        currency_code: "".to_string(),
                        rate: 0,
                        updated_at: SystemTime::now(),
                    },
                    topup_value_minor_units: 0,
                    exchange_fee_minor_units: 0,
                    exchange_fee_rate_permyriad: 0,
                    error: None,
                }),
                swap: None,
            },
            Payment {
                payment_type: PaymentType::Receiving,
                payment_state: PaymentState::Failed,
                fail_reason: None,
                hash: "hash3".to_string(),
                amount: Amount {
                    sats: 0,
                    fiat: None,
                },
                invoice_details: InvoiceDetails {
                    invoice: "cba".to_string(),
                    amount: None,
                    description: "".to_string(),
                    payment_hash: "".to_string(),
                    payee_pub_key: "".to_string(),
                    creation_timestamp: SystemTime::now(),
                    expiry_interval: Default::default(),
                    expiry_timestamp: SystemTime::now(),
                },
                created_at: TzTime {
                    time: SystemTime::now(),
                    timezone_id: "".to_string(),
                    timezone_utc_offset_secs: 0,
                },
                description: "".to_string(),
                preimage: None,
                network_fees: None,
                lsp_fees: None,
                offer: Some(OfferKind::Pocket {
                    id: "234".to_string(),
                    exchange_rate: ExchangeRate {
                        currency_code: "".to_string(),
                        rate: 0,
                        updated_at: SystemTime::now(),
                    },
                    topup_value_minor_units: 0,
                    exchange_fee_minor_units: 0,
                    exchange_fee_rate_permyriad: 0,
                    error: None,
                }),
                swap: None,
            },
        ];

        let filtered_topups = filter_out_recently_claimed_topups(topups, latest_payments);

        assert_eq!(filtered_topups.len(), 1);
        assert_eq!(filtered_topups.get(0).unwrap().id, "234");
    }
}
