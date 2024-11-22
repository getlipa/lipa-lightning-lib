//! # lipa-lightning-lib (aka 3L)
//!
//! This crate implements all the main business logic of the lipa wallet.
//!
//! Most functionality can be accessed by creating an instance of [`LightningNode`] and using its methods.

#![allow(clippy::let_unit_value)]
#![allow(deprecated)]

extern crate core;

mod actions_required;
mod activities;
mod activity;
mod amount;
mod analytics;
mod async_runtime;
mod auth;
mod backup;
mod callbacks;
mod config;
mod data_store;
mod errors;
mod event;
mod exchange_rate_provider;
mod fiat_topup;
mod invoice_details;
mod key_derivation;
mod lightning;
mod lightning_address;
mod limits;
mod locker;
mod logger;
mod migrations;
mod node_config;
mod notification_handling;
mod offer;
mod onchain;
mod payment;
mod phone_number;
mod random;
mod recovery;
mod reverse_swap;
mod sanitize_input;
mod secret;
mod support;
mod swap;
mod symmetric_encryption;
mod task_manager;
mod util;

pub use crate::activity::{Activity, ChannelCloseInfo, ChannelCloseState, ListActivitiesResponse};
pub use crate::amount::{Amount, FiatValue};
use crate::amount::{AsSats, Msats, Permyriad, ToAmount};
use crate::analytics::{derive_analytics_keys, AnalyticsInterceptor};
pub use crate::analytics::{AnalyticsConfig, InvoiceCreationMetadata, PaymentMetadata};
use crate::async_runtime::AsyncRuntime;
use crate::auth::{build_async_auth, build_auth};
use crate::backup::BackupManager;
pub use crate::callbacks::EventsCallback;
pub use crate::errors::{
    DecodeDataError, Error as LnError, LnUrlPayError, LnUrlPayErrorCode, LnUrlPayResult,
    MnemonicError, NotificationHandlingError, NotificationHandlingErrorCode, ParseError,
    ParsePhoneNumberError, ParsePhoneNumberPrefixError, PayError, PayErrorCode, PayResult, Result,
    RuntimeErrorCode, SimpleError, UnsupportedDataType,
};
use crate::errors::{LnUrlWithdrawError, LnUrlWithdrawErrorCode, LnUrlWithdrawResult};
use crate::event::LipaEventListener;
pub use crate::exchange_rate_provider::ExchangeRate;
use crate::exchange_rate_provider::ExchangeRateProviderImpl;
pub use crate::invoice_details::InvoiceDetails;
use crate::key_derivation::derive_persistence_encryption_key;
pub use crate::lightning::bolt11::Bolt11;
pub use crate::lightning::lnurl::{LnUrlPayDetails, LnUrlWithdrawDetails, Lnurl};
pub use crate::lightning::receive_limits::{LiquidityLimit, ReceiveAmountLimits};
pub use crate::limits::PaymentAmountLimits;
use crate::locker::Locker;
pub use crate::node_config::{
    BreezSdkConfig, LightningNodeConfig, MaxRoutingFeeConfig, ReceiveLimitsConfig,
    RemoteServicesConfig, TzConfig, TzTime,
};
pub use crate::notification_handling::{handle_notification, Notification, NotificationToggles};
pub use crate::offer::{OfferInfo, OfferKind, OfferStatus};
pub use crate::payment::{
    IncomingPaymentInfo, OutgoingPaymentInfo, PaymentInfo, PaymentState, Recipient,
};
use crate::phone_number::PhoneNumberPrefixParser;
pub use crate::phone_number::{PhoneNumber, PhoneNumberRecipient};
pub use crate::recovery::recover_lightning_node;
pub use crate::reverse_swap::ReverseSwapInfo;
pub use crate::secret::{generate_secret, mnemonic_to_secret, words_by_prefix, Secret};
pub use crate::swap::{
    FailedSwapInfo, ResolveFailedSwapInfo, SwapAddressInfo, SwapInfo, SwapToLightningFees,
};
use crate::symmetric_encryption::deterministic_encrypt;
use crate::task_manager::TaskManager;
use crate::util::unix_timestamp_to_system_time;
pub use crate::util::Util;

#[cfg(not(feature = "mock-deps"))]
#[allow(clippy::single_component_path_imports)]
use pocketclient;
#[cfg(feature = "mock-deps")]
use pocketclient_mock as pocketclient;

pub use crate::pocketclient::FiatTopupInfo;
use crate::pocketclient::PocketClient;

pub use crate::actions_required::ActionsRequired;
pub use crate::activities::Activities;
pub use crate::config::Config;
pub use crate::fiat_topup::FiatTopup;
pub use crate::lightning::{Lightning, PaymentAffordability};
pub use crate::lightning_address::LightningAddress;
pub use crate::onchain::channel_closes::{ChannelClose, SweepChannelCloseInfo};
pub use crate::onchain::reverse_swap::ReverseSwap;
pub use crate::onchain::swap::{Swap, SweepFailedSwapInfo};
pub use crate::onchain::Onchain;
use crate::support::Support;
pub use breez_sdk_core::error::ReceiveOnchainError as SwapError;
pub use breez_sdk_core::error::RedeemOnchainError as SweepError;
use breez_sdk_core::error::{ReceiveOnchainError, RedeemOnchainError};
pub use breez_sdk_core::HealthCheckStatus as BreezHealthCheckStatus;
pub use breez_sdk_core::ReverseSwapStatus;
use breez_sdk_core::{
    BitcoinAddressData, BreezServices, ConnectRequest, EnvironmentType, EventListener,
    GreenlightCredentials, GreenlightNodeConfig, ListPaymentsRequest, LnUrlPayRequestData,
    LnUrlWithdrawRequestData, Network, NodeConfig, OpeningFeeParams, PaymentDetails,
    PaymentTypeFilter, PrepareOnchainPaymentResponse,
};
use crow::{OfferManager, TopupError};
pub use crow::{PermanentFailureCode, TemporaryFailureCode};
use data_store::DataStore;
use hex::FromHex;
use honeybadger::Auth;
pub use honeybadger::{TermsAndConditions, TermsAndConditionsStatus};
use log::{debug, error, info, Level};
use logger::init_logger_once;
use num_enum::TryFromPrimitive;
use parrot::AnalyticsClient;
pub use parrot::PaymentSource;
use perro::{
    ensure, invalid_input, permanent_failure, runtime_error, MapToError, OptionToError, ResultTrait,
};
use squirrel::RemoteBackupClient;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::{env, fs};

const LOGS_DIR: &str = "logs";

const CLN_DUST_LIMIT_SAT: u64 = 546;

pub(crate) const DB_FILENAME: &str = "db2.db3";

/// Represent the result of comparision of a value with a given range.
pub enum RangeHit {
    /// The value is below the left side of the range.
    Below { min: Amount },
    /// The value is whithin the range.
    In,
    /// The value is above the right side of the range.
    Above { max: Amount },
}

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
    /// The max amount that can be received in a single payment.
    /// Can be lower than `total_inbound_capacity` because MPP isn't allowed.
    pub max_receivable_single_payment: Amount,
    /// Capacity the node can actually receive.
    /// It excludes non usable channels, pending HTLCs, channels reserves, etc.
    pub total_inbound_capacity: Amount,
    /// Capacity the node can actually send.
    /// It excludes non usable channels, pending HTLCs, channels reserves, etc.
    pub outbound_capacity: Amount,
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

/// An error associated with a specific PocketOffer. Can be temporary, indicating there was an issue
/// with a previous withdrawal attempt and it can be retried, or it can be permanent.
///
/// More information on each specific error can be found on
/// [Pocket's Documentation Page](<https://pocketbitcoin.com/developers/docs/rest/v1/webhooks>).
pub type PocketOfferError = TopupError;

#[derive(Clone)]
pub struct SweepInfo {
    pub address: String,
    pub onchain_fee_rate: u32,
    pub onchain_fee_amount: Amount,
    pub amount: Amount,
}

#[derive(Clone, PartialEq, Debug)]
pub(crate) struct UserPreferences {
    fiat_currency: String,
    timezone_config: TzConfig,
}

/// Decoded data that can be obtained using [`LightningNode::decode_data`].
pub enum DecodedData {
    Bolt11Invoice {
        invoice_details: InvoiceDetails,
    },
    LnUrlPay {
        lnurl_pay_details: LnUrlPayDetails,
    },
    LnUrlWithdraw {
        lnurl_withdraw_details: LnUrlWithdrawDetails,
    },
    OnchainAddress {
        onchain_address_details: BitcoinAddressData,
    },
}

/// Invoice affordability returned by [`LightningNode::get_invoice_affordability`].
#[derive(Debug)]
pub enum InvoiceAffordability {
    /// Not enough funds available to pay the requested amount.
    NotEnoughFunds,
    /// Not enough funds available to pay the requested amount and the max routing fees.
    /// There might be a route that is affordable enough but it is unknown until tried.
    UnaffordableFees,
    /// Enough funds for the invoice and routing fees are available.
    Affordable,
}

impl From<PaymentAffordability> for InvoiceAffordability {
    fn from(value: PaymentAffordability) -> Self {
        match value {
            PaymentAffordability::NotEnoughFunds => InvoiceAffordability::NotEnoughFunds,
            PaymentAffordability::UnaffordableFees => InvoiceAffordability::UnaffordableFees,
            PaymentAffordability::Affordable => InvoiceAffordability::Affordable,
        }
    }
}

/// Information about a wallet clearance operation as returned by
/// [`LightningNode::prepare_clear_wallet`].
pub struct ClearWalletInfo {
    /// The total amount available to be cleared. The amount sent will be smaller due to fees.
    pub clear_amount: Amount,
    /// Total fee estimate. Can differ from that fees that are charged when clearing the wallet.
    pub total_estimated_fees: Amount,
    /// Estimate for the total that will be paid in on-chain fees (lockup + claim txs).
    pub onchain_fee: Amount,
    /// Estimate for the fee paid to the swap service.
    pub swap_fee: Amount,
    prepare_response: PrepareOnchainPaymentResponse,
}

#[derive(PartialEq, Eq, Debug, TryFromPrimitive, Clone, Copy)]
#[repr(u8)]
pub(crate) enum EnableStatus {
    Enabled,
    FeatureDisabled,
}

pub enum FeatureFlag {
    LightningAddress,
    PhoneNumber,
}

/// The main class/struct of this library. Constructing an instance will initiate the Lightning node and
/// run it in the background. As long as an instance of `LightningNode` is held, the node will continue to run
/// in the background. Dropping the instance will start a deinit process.  
pub struct LightningNode {
    sdk: Arc<BreezServices>,
    auth: Arc<Auth>,
    offer_manager: Arc<OfferManager>,
    rt: Arc<AsyncRuntime>,
    node_config: LightningNodeConfig,
    activities: Arc<Activities>,
    lightning: Arc<Lightning>,
    config: Arc<Config>,
    fiat_topup: Arc<FiatTopup>,
    actions_required: Arc<ActionsRequired>,
    onchain: Arc<Onchain>,
    lightning_address: Arc<LightningAddress>,
    phone_number: Arc<PhoneNumber>,
    util: Arc<Util>,
}

/// Contains the fee information for the options to resolve funds that have moved on-chain.
///
/// This can occur due to channel closes, or swaps that failed to resolve in the available period.
pub struct OnchainResolvingFees {
    /// Fees to swap the funds back to lightning using [`LightningNode::swap_channel_close_funds_to_lightning`]
    /// or [`LightningNode::swap_failed_swap_funds_to_lightning`].
    /// Only available if enough funds are there to swap.
    pub swap_fees: Option<SwapToLightningFees>,
    /// Estimate of the fees for sending the funds on-chain using [`LightningNode::sweep_funds_from_channel_closes`]
    /// or [`LightningNode::resolve_failed_swap`].
    /// The exact fees will be known when calling [`LightningNode::prepare_sweep_funds_from_channel_closes`]
    /// or [`LightningNode::prepare_resolve_failed_swap`].
    pub sweep_onchain_fee_estimate: Amount,
    /// The fee rate used to compute `swaps_fees` and `sweep_onchain_fee_estimate`.
    /// It should be provided when swapping funds back to lightning or when sweeping funds
    /// to on-chain to ensure the same fee rate is used.
    pub sats_per_vbyte: u32,
}

#[allow(clippy::large_enum_variant)]
pub enum ActionRequiredItem {
    UncompletedOffer { offer: OfferInfo },
    UnresolvedFailedSwap { failed_swap: FailedSwapInfo },
    ChannelClosesFundsAvailable { available_funds: Amount },
}

impl From<OfferInfo> for ActionRequiredItem {
    fn from(value: OfferInfo) -> Self {
        ActionRequiredItem::UncompletedOffer { offer: value }
    }
}

impl From<FailedSwapInfo> for ActionRequiredItem {
    fn from(value: FailedSwapInfo) -> Self {
        ActionRequiredItem::UnresolvedFailedSwap { failed_swap: value }
    }
}

impl LightningNode {
    /// Create a new instance of [`LightningNode`].
    ///
    /// Parameters:
    /// * `config` - configuration parameters
    /// * `events_callback` - a callbacks interface for the consumer of this library to be notified
    ///   of certain events.
    ///
    /// Requires network: **yes**
    pub fn new(
        node_config: LightningNodeConfig,
        events_callback: Box<dyn EventsCallback>,
    ) -> Result<Self> {
        enable_backtrace();
        fs::create_dir_all(&node_config.local_persistence_path).map_to_permanent_failure(
            format!(
                "Failed to create directory: {}",
                &node_config.local_persistence_path,
            ),
        )?;
        if let Some(level) = node_config.file_logging_level {
            init_logger_once(
                level,
                &Path::new(&node_config.local_persistence_path).join(LOGS_DIR),
            )?;
        }
        info!("3L version: {}", env!("GITHUB_REF"));

        let rt = Arc::new(AsyncRuntime::new()?);

        let strong_typed_seed = sanitize_input::strong_type_seed(&node_config.seed)?;
        let auth = Arc::new(build_auth(
            &strong_typed_seed,
            &node_config.remote_services_config.backend_url,
        )?);
        let async_auth = Arc::new(build_async_auth(
            &strong_typed_seed,
            &node_config.remote_services_config.backend_url,
        )?);

        let db_path = format!("{}/{DB_FILENAME}", node_config.local_persistence_path);
        let mut data_store = DataStore::new(&db_path)?;

        let fiat_currency = match data_store.retrieve_last_set_fiat_currency()? {
            None => {
                data_store.store_selected_fiat_currency(&node_config.default_fiat_currency)?;
                node_config.default_fiat_currency.clone()
            }
            Some(c) => c,
        };

        let data_store = Arc::new(Mutex::new(data_store));

        let user_preferences = Arc::new(Mutex::new(UserPreferences {
            fiat_currency,
            timezone_config: node_config.timezone_config.clone(),
        }));

        let analytics_client = AnalyticsClient::new(
            node_config.remote_services_config.backend_url.clone(),
            derive_analytics_keys(&strong_typed_seed)?,
            Arc::clone(&async_auth),
        );

        let analytics_config = data_store.lock_unwrap().retrieve_analytics_config()?;
        let analytics_interceptor = Arc::new(AnalyticsInterceptor::new(
            analytics_client,
            Arc::clone(&user_preferences),
            rt.handle(),
            analytics_config,
        ));

        let events_callback = Arc::new(events_callback);
        let event_listener = Box::new(LipaEventListener::new(
            Arc::clone(&events_callback),
            Arc::clone(&analytics_interceptor),
        ));

        let sdk = rt.handle().block_on(async {
            let sdk = start_sdk(&node_config, event_listener).await?;
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
            Ok(sdk)
        })?;

        let exchange_rate_provider = Box::new(ExchangeRateProviderImpl::new(
            node_config.remote_services_config.backend_url.clone(),
            Arc::clone(&auth),
        ));

        let offer_manager = Arc::new(OfferManager::new(
            node_config.remote_services_config.backend_url.clone(),
            Arc::clone(&auth),
        ));

        let fiat_topup_client =
            PocketClient::new(node_config.remote_services_config.pocket_url.clone())
                .map_to_runtime_error(
                    RuntimeErrorCode::OfferServiceUnavailable,
                    "Couldn't create a fiat topup client",
                )?;

        let persistence_encryption_key = derive_persistence_encryption_key(&strong_typed_seed)?;
        let backup_client = RemoteBackupClient::new(
            node_config.remote_services_config.backend_url.clone(),
            Arc::clone(&async_auth),
        );
        let backup_manager = BackupManager::new(backup_client, db_path, persistence_encryption_key);

        let task_manager = Arc::new(Mutex::new(TaskManager::new(
            rt.handle(),
            exchange_rate_provider,
            Arc::clone(&data_store),
            Arc::clone(&sdk),
            backup_manager,
            events_callback,
            node_config.breez_sdk_config.breez_sdk_api_key.clone(),
        )?));
        task_manager.lock_unwrap().foreground();

        register_webhook_url(&rt, &sdk, &auth, &node_config)?;

        let phone_number_prefix_parser = PhoneNumberPrefixParser::new(
            &node_config.phone_number_allowed_countries_iso_3166_1_alpha_2,
        );

        let support = Arc::new(Support {
            user_preferences: Arc::clone(&user_preferences),
            sdk: Arc::clone(&sdk),
            auth: Arc::clone(&auth),
            async_auth: Arc::clone(&async_auth),
            fiat_topup_client,
            offer_manager: Arc::clone(&offer_manager),
            rt: Arc::clone(&rt),
            data_store: Arc::clone(&data_store),
            task_manager: Arc::clone(&task_manager),
            allowed_countries_country_iso_3166_1_alpha_2: node_config
                .phone_number_allowed_countries_iso_3166_1_alpha_2
                .clone(),
            phone_number_prefix_parser: phone_number_prefix_parser.clone(),
            persistence_encryption_key,
            node_config: node_config.clone(),
            analytics_interceptor,
        });

        let activities = Arc::new(Activities::new(Arc::clone(&support)));

        let lightning = Arc::new(Lightning::new(Arc::clone(&support)));

        let config = Arc::new(Config::new(Arc::clone(&support)));

        let fiat_topup = Arc::new(FiatTopup::new(
            Arc::clone(&support),
            Arc::clone(&activities),
        ));

        let onchain = Arc::new(Onchain::new(Arc::clone(&support)));

        let actions_required = Arc::new(ActionsRequired::new(
            Arc::clone(&support),
            Arc::clone(&fiat_topup),
            Arc::clone(&onchain),
        ));

        let lightning_address = Arc::new(LightningAddress::new(Arc::clone(&support)));

        let phone_number = Arc::new(PhoneNumber::new(Arc::clone(&support)));

        let util = Arc::new(Util::new(Arc::clone(&support)));

        Ok(LightningNode {
            sdk,
            auth,
            offer_manager,
            rt,
            node_config,
            activities,
            lightning,
            config,
            fiat_topup,
            actions_required,
            onchain,
            lightning_address,
            phone_number,
            util,
        })
    }

    pub fn activities(&self) -> Arc<Activities> {
        Arc::clone(&self.activities)
    }

    pub fn lightning(&self) -> Arc<Lightning> {
        Arc::clone(&self.lightning)
    }

    pub fn config(&self) -> Arc<Config> {
        Arc::clone(&self.config)
    }

    pub fn fiat_topup(&self) -> Arc<FiatTopup> {
        Arc::clone(&self.fiat_topup)
    }

    pub fn actions_required(&self) -> Arc<ActionsRequired> {
        Arc::clone(&self.actions_required)
    }

    pub fn onchain(&self) -> Arc<Onchain> {
        Arc::clone(&self.onchain)
    }

    pub fn lightning_address(&self) -> Arc<LightningAddress> {
        Arc::clone(&self.lightning_address)
    }

    pub fn phone_number(&self) -> Arc<PhoneNumber> {
        Arc::clone(&self.phone_number)
    }

    pub fn util(&self) -> Arc<Util> {
        Arc::clone(&self.util)
    }

    /// Request some basic info about the node
    ///
    /// Requires network: **no**
    #[deprecated = "util().get_node_info() should be used instead"]
    pub fn get_node_info(&self) -> Result<NodeInfo> {
        self.util.get_node_info()
    }

    /// When *receiving* payments, a new channel MAY be required. A fee will be charged to the user.
    /// This does NOT impact *sending* payments.
    /// Get information about the fee charged by the LSP for opening new channels
    ///
    /// Requires network: **no**
    #[deprecated = "lightning().get_lsp_fee() should be used instead"]
    pub fn query_lsp_fee(&self) -> Result<LspFee> {
        self.lightning.get_lsp_fee()
    }

    /// Calculate the actual LSP fee for the given amount of an incoming payment.
    /// If the already existing inbound capacity is enough, no new channel is required.
    ///
    /// Parameters:
    /// * `amount_sat` - amount in sats to compute LSP fee for
    ///
    /// For the returned fees to be guaranteed to be accurate, the returned `lsp_fee_params` must be
    /// provided to [`LightningNode::create_invoice`]
    ///
    /// Requires network: **yes**
    #[deprecated = "lightning().calculate_lsp_fee_for_amount() should be used instead"]
    pub fn calculate_lsp_fee(&self, amount_sat: u64) -> Result<CalculateLspFeeResponse> {
        self.lightning.calculate_lsp_fee_for_amount(amount_sat)
    }

    /// Get the current limits for the amount that can be transferred in a single payment.
    /// Currently there are only limits for receiving payments.
    /// The limits (partly) depend on the channel situation of the node, so it should be called
    /// again every time the user is about to receive a payment.
    /// The limits stay the same regardless of what amount wants to receive (= no changes while
    /// he's typing the amount)
    ///
    /// Requires network: **no**
    #[deprecated = "lightning().determine_receive_amount_limits() should be used instead"]
    pub fn get_payment_amount_limits(&self) -> Result<PaymentAmountLimits> {
        self.lightning
            .determine_receive_amount_limits()
            .map(PaymentAmountLimits::from)
    }

    /// Create an invoice to receive a payment with.
    ///
    /// Parameters:
    /// * `amount_sat` - the smallest amount of sats required for the node to accept the incoming
    ///   payment (sender will have to pay fees on top of that amount)
    /// * `lsp_fee_params` - the params that will be used to determine the lsp fee.
    ///    Can be obtained from [`LightningNode::calculate_lsp_fee`] to guarantee predicted fees
    ///    are the ones charged.
    /// * `description` - a description to be embedded into the created invoice
    /// * `metadata` - additional data about the invoice creation used for analytics purposes,
    ///    used to improve the user experience
    ///
    /// Requires network: **yes**
    #[deprecated = "lightning().bolt11().create() should be used instead"]
    pub fn create_invoice(
        &self,
        amount_sat: u64,
        lsp_fee_params: Option<OpeningFeeParams>,
        description: String,
        metadata: InvoiceCreationMetadata,
    ) -> Result<InvoiceDetails> {
        self.lightning
            .bolt11()
            .create(amount_sat, lsp_fee_params, description, metadata)
    }

    /// Parse a phone number prefix, check against the list of allowed countries
    /// (set in [`LightningNodeConfig::phone_number_allowed_countries_iso_3166_1_alpha_2`]).
    /// The parser is not strict, it parses some invalid prefixes as valid.
    ///
    /// Requires network: **no**
    #[deprecated = "phone_number().parse_prefix() should be used instead"]
    pub fn parse_phone_number_prefix(
        &self,
        phone_number_prefix: String,
    ) -> std::result::Result<(), ParsePhoneNumberPrefixError> {
        self.phone_number.parse_prefix(phone_number_prefix)
    }

    /// Parse a phone number, check against the list of allowed countries
    /// (set in [`LightningNodeConfig::phone_number_allowed_countries_iso_3166_1_alpha_2`]).
    ///
    /// Returns a possible lightning address, which can be checked for existence
    /// with [`LightningNode::decode_data`].
    ///
    /// Requires network: **no**
    #[deprecated = "phone_number().parse_to_lightning_address() should be used instead"]
    pub fn parse_phone_number_to_lightning_address(
        &self,
        phone_number: String,
    ) -> std::result::Result<String, ParsePhoneNumberError> {
        self.phone_number.parse_to_lightning_address(phone_number)
    }

    /// Decode a user-provided string (usually obtained from QR-code or pasted).
    ///
    /// Requires network: **yes**
    #[deprecated = "util().decode_data() should be used instead"]
    pub fn decode_data(&self, data: String) -> std::result::Result<DecodedData, DecodeDataError> {
        self.util.decode_data(data)
    }

    /// Get the max routing fee mode that will be employed to restrict the fees for paying a given amount in sats
    ///
    /// Requires network: **no**
    #[deprecated = "lightning().determine_max_routing_fee_mode() should be used instead"]
    pub fn get_payment_max_routing_fee_mode(&self, amount_sat: u64) -> MaxRoutingFeeMode {
        self.lightning.determine_max_routing_fee_mode(amount_sat)
    }

    /// Checks if the given amount could be spent on an invoice.
    ///
    /// Parameters:
    /// * `amount` - The to be spent amount.
    ///
    /// Requires network: **no**
    #[deprecated = "lightning().determine_payment_affordability() should be used instead"]
    pub fn get_invoice_affordability(&self, amount_sat: u64) -> Result<InvoiceAffordability> {
        self.lightning
            .determine_payment_affordability(amount_sat)
            .map(InvoiceAffordability::from)
    }

    /// Start an attempt to pay an invoice. Can immediately fail, meaning that the payment couldn't be started.
    /// If successful, it doesn't mean that the payment itself was successful (funds received by the payee).
    /// After this method returns, the consumer of this library will learn about a successful/failed payment through the
    /// callbacks [`EventsCallback::payment_sent`] and [`EventsCallback::payment_failed`].
    ///
    /// Parameters:
    /// * `invoice_details` - details of an invoice decode by [`LightningNode::decode_data`]
    /// * `metadata` - additional meta information about the payment, used by analytics to improve the user experience.
    ///
    /// Requires network: **yes**
    #[deprecated = "lightning().bolt11().pay() should be used instead"]
    pub fn pay_invoice(
        &self,
        invoice_details: InvoiceDetails,
        metadata: PaymentMetadata,
    ) -> PayResult<()> {
        self.lightning.bolt11().pay(invoice_details, metadata)
    }

    /// Similar to [`LightningNode::pay_invoice`] with the difference that the passed in invoice
    /// does not have any payment amount specified, and allows the caller of the method to
    /// specify an amount instead.
    ///
    /// Additional Parameters:
    /// * `amount_sat` - amount in sats to be paid
    ///
    /// Requires network: **yes**
    #[deprecated = "lightning().bolt11().pay_open_amount() should be used instead"]
    pub fn pay_open_invoice(
        &self,
        invoice_details: InvoiceDetails,
        amount_sat: u64,
        metadata: PaymentMetadata,
    ) -> PayResult<()> {
        self.lightning
            .bolt11()
            .pay_open_amount(invoice_details, amount_sat, metadata)
    }

    /// Pay an LNURL-pay the provided amount.
    ///
    /// Parameters:
    /// * `lnurl_pay_request_data` - LNURL-pay request data as obtained from [`LightningNode::decode_data`]
    /// * `amount_sat` - amount to be paid
    /// * `comment` - optional comment to be sent to payee (`max_comment_length` in
    ///   [`LnUrlPayDetails`] must be respected)
    ///
    /// Returns the payment hash of the payment.
    ///
    /// Requires network: **yes**
    #[deprecated = "lightning().lnurl().pay() should be used instead"]
    pub fn pay_lnurlp(
        &self,
        lnurl_pay_request_data: LnUrlPayRequestData,
        amount_sat: u64,
        comment: Option<String>,
    ) -> LnUrlPayResult<String> {
        self.lightning
            .lnurl()
            .pay(lnurl_pay_request_data, amount_sat, comment)
    }

    /// List recipients from the most recent used.
    ///
    /// Returns a list of recipients (lightning addresses or phone numbers for now).
    ///
    /// Requires network: **no**
    pub fn list_recipients(&self) -> Result<Vec<Recipient>> {
        let list_payments_request = ListPaymentsRequest {
            filters: Some(vec![PaymentTypeFilter::Sent]),
            metadata_filters: None,
            from_timestamp: None,
            to_timestamp: None,
            include_failures: Some(true),
            limit: None,
            offset: None,
        };
        let to_lightning_address = |p: breez_sdk_core::Payment| match p.details {
            PaymentDetails::Ln { data } => match data.ln_address {
                Some(lightning_address) => Some((lightning_address, -p.payment_time)),
                None => None,
            },
            _ => None,
        };
        let mut lightning_addresses = self
            .rt
            .handle()
            .block_on(self.sdk.list_payments(list_payments_request))
            .map_to_runtime_error(RuntimeErrorCode::NodeUnavailable, "Failed to list payments")?
            .into_iter()
            .flat_map(to_lightning_address)
            .collect::<Vec<_>>();
        lightning_addresses.sort();
        lightning_addresses.dedup_by_key(|p| p.0.clone());
        lightning_addresses.sort_by_key(|p| p.1);

        let recipients = lightning_addresses
            .into_iter()
            .map(|p| {
                Recipient::from_lightning_address(
                    &p.0,
                    &self
                        .node_config
                        .remote_services_config
                        .lipa_lightning_domain,
                )
            })
            .collect();
        Ok(recipients)
    }

    /// Withdraw an LNURL-withdraw the provided amount.
    ///
    /// A successful return means the LNURL-withdraw service has started a payment.
    /// Only after the event [`EventsCallback::payment_received`] can the payment be considered
    /// received.
    ///
    /// Parameters:
    /// * `lnurl_withdraw_request_data` - LNURL-withdraw request data as obtained from [`LightningNode::decode_data`]
    /// * `amount_sat` - amount to be withdraw
    ///
    /// Returns the payment hash of the payment.
    ///
    /// Requires network: **yes**
    #[deprecated = "lightning().lnurl().withdraw() should be used instead"]
    pub fn withdraw_lnurlw(
        &self,
        lnurl_withdraw_request_data: LnUrlWithdrawRequestData,
        amount_sat: u64,
    ) -> LnUrlWithdrawResult<String> {
        self.lightning
            .lnurl()
            .withdraw(lnurl_withdraw_request_data, amount_sat)
    }

    /// Get a list of the latest activities
    ///
    /// Parameters:
    /// * `number_of_completed_activities` - the maximum number of completed activities that will be returned
    ///
    /// Requires network: **no**
    #[deprecated = "activities().list() should be used instead"]
    pub fn get_latest_activities(
        &self,
        number_of_completed_activities: u32,
    ) -> Result<ListActivitiesResponse> {
        self.activities.list(number_of_completed_activities)
    }

    /// Get an incoming payment by its payment hash.
    ///
    /// Parameters:
    /// * `hash` - hex representation of payment hash
    ///
    /// Requires network: **no**
    #[deprecated = "activities().get_incoming_payment() should be used instead"]
    pub fn get_incoming_payment(&self, hash: String) -> Result<IncomingPaymentInfo> {
        self.activities.get_incoming_payment(hash)
    }

    /// Get an outgoing payment by its payment hash.
    ///
    /// Parameters:
    /// * `hash` - hex representation of payment hash
    ///
    /// Requires network: **no**
    #[deprecated = "activities().get_outgoing_payment() should be used instead"]
    pub fn get_outgoing_payment(&self, hash: String) -> Result<OutgoingPaymentInfo> {
        self.activities.get_outgoing_payment(hash)
    }

    /// Get an activity by its payment hash.
    ///
    /// Parameters:
    /// * `hash` - hex representation of payment hash
    ///
    /// Requires network: **no**
    #[deprecated = "activities().get() should be used instead"]
    pub fn get_activity(&self, hash: String) -> Result<Activity> {
        self.activities.get(hash)
    }

    /// Set a personal note on a specific payment.
    ///
    /// Parameters:
    /// * `payment_hash` - The hash of the payment for which a personal note will be set.
    /// * `note` - The personal note.
    ///
    /// Requires network: **no**
    #[deprecated = "activities().set_personal_note() should be used instead"]
    pub fn set_payment_personal_note(&self, payment_hash: String, note: String) -> Result<()> {
        self.activities.set_personal_note(payment_hash, note)
    }

    /// Call the method when the app goes to foreground, such that the user can interact with it.
    /// The library starts running the background tasks more frequently to improve user experience.
    ///
    /// Requires network: **no**
    #[deprecated = "config().foreground() should be used instead"]
    pub fn foreground(&self) {
        self.config.foreground()
    }

    /// Call the method when the app goes to background, such that the user can not interact with it.
    /// The library stops running some unnecessary tasks and runs necessary tasks less frequently.
    /// It should save battery and internet traffic.
    ///
    /// Requires network: **no**
    #[deprecated = "config().background() should be used instead"]
    pub fn background(&self) {
        self.config.background()
    }

    /// List codes of supported fiat currencies.
    /// Please keep in mind that this method doesn't make any network calls. It simply retrieves
    /// previously fetched values that are frequently updated by a background task.
    ///
    /// The fetched list will be persisted across restarts to alleviate the consequences of a
    /// slow or unresponsive exchange rate service.
    /// The method will return an empty list if there is nothing persisted yet and
    /// the values are not yet fetched from the service.
    ///
    /// Requires network: **no**
    #[deprecated = "config().list_currencies() should be used instead"]
    pub fn list_currency_codes(&self) -> Vec<String> {
        self.config.list_currencies()
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
    ///
    /// Requires network: **no**
    #[deprecated = "util().get_exchange_rate() should be used instead"]
    pub fn get_exchange_rate(&self) -> Option<ExchangeRate> {
        self.util.get_exchange_rate()
    }

    /// Change the fiat currency (ISO 4217 currency code) - not all are supported
    /// The method [`LightningNode::list_currency_codes`] can used to list supported codes.
    ///
    /// Requires network: **no**
    #[deprecated = "config().set_fiat_currency() should be used instead"]
    pub fn change_fiat_currency(&self, fiat_currency: String) -> Result<()> {
        self.config.set_fiat_currency(fiat_currency)
    }

    /// Change the timezone config.
    ///
    /// Parameters:
    /// * `timezone_config` - the user's current timezone
    ///
    /// Requires network: **no**
    #[deprecated = "config().set_timezone_config() should be used instead"]
    pub fn change_timezone_config(&self, timezone_config: TzConfig) {
        self.config.set_timezone_config(timezone_config)
    }

    /// Accepts Pocket's T&C.
    ///
    /// Parameters:
    /// * `version` - the version number being accepted.
    /// * `fingerprint` - the fingerprint of the version being accepted.
    ///
    /// Requires network: **yes**
    #[deprecated = "fiat_topup().accept_tc() should be used instead"]
    pub fn accept_pocket_terms_and_conditions(
        &self,
        version: i64,
        fingerprint: String,
    ) -> Result<()> {
        self.fiat_topup.accept_tc(version, fingerprint)
    }

    /// Similar to [`get_terms_and_conditions_status`] with the difference that this method is pre-filling
    /// the environment and seed based on the node configuration.
    ///
    /// Requires network: **yes**
    #[deprecated = "fiat_topup().query_tc_status() should be used instead"]
    pub fn get_terms_and_conditions_status(
        &self,
        terms_and_conditions: TermsAndConditions,
    ) -> Result<TermsAndConditionsStatus> {
        self.auth
            .get_terms_and_conditions_status(terms_and_conditions)
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
    ///    exchange. Not all are supported. A consumer of this library should find out about available
    ///    ones using other sources.
    ///
    /// Requires network: **yes**
    #[deprecated = "fiat_topup().register() should be used instead"]
    pub fn register_fiat_topup(
        &self,
        email: Option<String>,
        user_iban: String,
        user_currency: String,
    ) -> Result<FiatTopupInfo> {
        self.fiat_topup.register(email, user_iban, user_currency)
    }

    /// Resets a previous fiat topups registration.
    ///
    /// Requires network: **no**
    #[deprecated = "fiat_topup().reset() should be used instead"]
    pub fn reset_fiat_topup(&self) -> Result<()> {
        self.fiat_topup.reset()
    }

    /// Hides the topup with the given id. Can be called on expired topups so that they stop being returned
    /// by [`LightningNode::query_uncompleted_offers`].
    ///
    /// Topup id can be obtained from [`OfferKind::Pocket`].
    ///
    /// Requires network: **yes**
    #[deprecated = "fiat_topup().dismiss_topup() should be used instead"]
    pub fn hide_topup(&self, id: String) -> Result<()> {
        self.offer_manager
            .hide_topup(id)
            .map_runtime_error_to(RuntimeErrorCode::OfferServiceUnavailable)
    }

    /// List action required items.
    ///
    /// Returns a list of actionable items. They can be:
    /// * Uncompleted offers (either available for collection or failed).
    /// * Unresolved failed swaps.
    /// * Available funds resulting from channel closes.
    ///
    /// Requires network: **yes**
    #[deprecated = "actions_required().list() should be used instead"]
    pub fn list_action_required_items(&self) -> Result<Vec<ActionRequiredItem>> {
        self.actions_required.list()
    }

    /// Hides the channel close action required item in case the amount cannot be recovered due
    /// to it being too small. The item will reappear once the amount of funds changes or
    /// onchain-fees go down enough to make the amount recoverable.
    ///
    /// Requires network: **no**
    #[deprecated = "actions_required().hide_unrecoverable_channel_close_funds_item() should be used instead"]
    pub fn hide_channel_closes_funds_available_action_required_item(&self) -> Result<()> {
        self.actions_required()
            .hide_unrecoverable_channel_close_funds_item()
    }

    /// Hides the unresolved failed swap action required item in case the amount cannot be
    /// recovered due to it being too small. The item will reappear once the onchain-fees go
    /// down enough to make the amount recoverable.
    ///
    /// Requires network: **no**
    #[deprecated = "actions_required().hide_unrecoverable_failed_swap_item() should be used instead"]
    pub fn hide_unresolved_failed_swap_action_required_item(
        &self,
        failed_swap_info: FailedSwapInfo,
    ) -> Result<()> {
        self.actions_required()
            .hide_unrecoverable_failed_swap_item(failed_swap_info)
    }

    /// Get a list of unclaimed fund offers
    ///
    /// Requires network: **yes**
    #[deprecated = "actions_required().list() should be used instead"]
    pub fn query_uncompleted_offers(&self) -> Result<Vec<OfferInfo>> {
        self.fiat_topup.query_uncompleted_offers()
    }

    /// Calculates the lightning payout fee for an uncompleted offer.
    ///
    /// Parameters:
    /// * `offer` - An uncompleted offer for which the lightning payout fee should get calculated.
    ///
    /// Requires network: **yes**
    #[deprecated = "fiat_topup().calculate_payout_fee() should be used instead"]
    pub fn calculate_lightning_payout_fee(&self, offer: OfferInfo) -> Result<Amount> {
        self.fiat_topup().calculate_payout_fee(offer)
    }

    /// Request to collect the offer (e.g. a Pocket topup).
    /// A payment hash will be returned to track incoming payment.
    /// The offer collection might be considered successful once
    /// [`EventsCallback::payment_received`] is called,
    /// or the [`PaymentState`] of the respective payment becomes [`PaymentState::Succeeded`].
    ///
    /// Parameters:
    /// * `offer` - An offer that is still valid for collection. Must have its `lnurlw` field
    ///   filled in.
    ///
    /// Requires network: **yes**
    #[deprecated = "fiat_topup().request_collection() should be used instead"]
    pub fn request_offer_collection(&self, offer: OfferInfo) -> Result<String> {
        self.fiat_topup().request_collection(offer)
    }

    /// Registers a new notification token. If a token has already been registered, it will be updated.
    ///
    /// Requires network: **yes**
    #[deprecated = "config().register_notification_token() should be used instead"]
    pub fn register_notification_token(
        &self,
        notification_token: String,
        language_iso_639_1: String,
        country_iso_3166_1_alpha_2: String,
    ) -> Result<()> {
        self.config.register_notification_token(
            notification_token,
            language_iso_639_1,
            country_iso_3166_1_alpha_2,
        )
    }

    /// Get the wallet UUID v5 from the wallet pubkey
    ///
    /// If the auth flow has never succeeded in this Auth instance, this method will require network
    /// access.
    ///
    /// Requires network: **yes**
    #[deprecated = "util().query_wallet_pubkey_id() should be used instead"]
    pub fn get_wallet_pubkey_id(&self) -> Result<String> {
        self.util.query_wallet_pubkey_id()
    }

    /// Get the payment UUID v5 from the payment hash
    ///
    /// Returns a UUID v5 derived from the payment hash. This will always return the same output
    /// given the same input.
    ///
    /// Parameters:
    /// * `payment_hash` - a payment hash represented in hex
    ///
    /// Requires network: **no**
    #[deprecated = "util().derive_payment_uuid() should be used instead"]
    pub fn get_payment_uuid(&self, payment_hash: String) -> Result<String> {
        self.util.derive_payment_uuid(payment_hash)
    }

    /// Query the current recommended on-chain fee rate.
    ///
    /// This is useful to obtain a fee rate to be used for [`LightningNode::sweep_funds_from_channel_closes`].
    ///
    /// Requires network: **yes**
    #[deprecated = "New onchain interface automatically chooses fee rate"]
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

    /// Prepares a sweep of all available on-chain funds to the provided on-chain address.
    ///
    /// Parameters:
    /// * `address` - the funds will be sweeped to this address
    /// * `onchain_fee_rate` - ignored
    ///
    /// Returns information on the prepared sweep, including the exact fee that results from
    /// using the provided fee rate. The method [`LightningNode::sweep_funds_from_channel_closes`] can be used to broadcast
    /// the sweep transaction.
    ///
    /// Requires network: **yes**
    #[deprecated = "onchain().channel_close().prepare_sweep() should be used instead"]
    pub fn prepare_sweep_funds_from_channel_closes(
        &self,
        address: String,
        _onchain_fee_rate: u32,
    ) -> std::result::Result<SweepInfo, RedeemOnchainError> {
        self.onchain
            .channel_close()
            .prepare_sweep(BitcoinAddressData {
                address,
                network: Network::Bitcoin,
                amount_sat: None,
                label: None,
                message: None,
            })
            .map(SweepInfo::from)
    }

    /// Sweeps all available on-chain funds to the specified on-chain address.
    ///
    /// Parameters:
    /// * `sweep_info` - a prepared sweep info that can be obtained using
    ///     [`LightningNode::prepare_sweep_funds_from_channel_closes`]
    ///
    /// Returns the txid of the sweep transaction.
    ///
    /// Requires network: **yes**
    #[deprecated = "onchain().channel_close().sweep() should be used instead"]
    pub fn sweep_funds_from_channel_closes(&self, sweep_info: SweepInfo) -> Result<String> {
        self.onchain.channel_close().sweep(sweep_info.into())
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
    ///   be opened. Can be obtained using [`LightningNode::calculate_lsp_fee`].
    ///
    /// Requires network: **yes**
    #[deprecated = "onchain().swaps().create() should be used instead"]
    pub fn generate_swap_address(
        &self,
        lsp_fee_params: Option<OpeningFeeParams>,
    ) -> std::result::Result<SwapAddressInfo, ReceiveOnchainError> {
        self.onchain.swap().create(lsp_fee_params)
    }

    /// Lists all unresolved failed swaps. Each individual failed swap can be recovered
    /// using [`LightningNode::resolve_failed_swap`].
    ///
    /// Requires network: **yes**
    #[deprecated = "actions_required().list() should be used instead"]
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
            .filter(|s| s.refund_tx_ids.is_empty())
            .map(|s| FailedSwapInfo {
                address: s.bitcoin_address,
                amount: s
                    .confirmed_sats
                    .as_sats()
                    .to_amount_down(&self.get_exchange_rate()),
                created_at: unix_timestamp_to_system_time(s.created_at as u64),
            })
            .collect())
    }

    /// Returns the fees for resolving a failed swap if there are enough funds to pay for fees.
    ///
    /// Must only be called when the failed swap is unresolved.
    ///
    /// Returns the fee information for the available resolving options.
    ///
    /// Requires network: *yes*
    #[deprecated = "onchain().swaps().determine_resolving_fees() should be used instead"]
    pub fn get_failed_swap_resolving_fees(
        &self,
        failed_swap_info: FailedSwapInfo,
    ) -> Result<Option<OnchainResolvingFees>> {
        self.onchain
            .swap()
            .determine_resolving_fees(failed_swap_info)
    }

    /// Prepares the resolution of a failed swap in order to know how much will be recovered and how much
    /// will be paid in on-chain fees.
    ///
    /// Parameters:
    /// * `failed_swap_info` - the failed swap that will be prepared
    /// * `to_address` - the destination address to which funds will be sent
    /// * `onchain_fee_rate` - ignored
    ///
    /// Requires network: **yes**
    #[deprecated = "onchain().swaps().prepare_sweep() should be used instead"]
    pub fn prepare_resolve_failed_swap(
        &self,
        failed_swap_info: FailedSwapInfo,
        to_address: String,
        _onchain_fee_rate: u32,
    ) -> Result<ResolveFailedSwapInfo> {
        self.onchain
            .swap()
            .prepare_sweep(
                failed_swap_info,
                BitcoinAddressData {
                    address: to_address,
                    network: Network::Bitcoin,
                    amount_sat: None,
                    label: None,
                    message: None,
                },
            )
            .map(ResolveFailedSwapInfo::from)
    }

    /// Creates and broadcasts a resolving transaction to recover funds from a failed swap. Existing
    /// failed swaps can be listed using [`LightningNode::get_unresolved_failed_swaps`] and preparing
    /// the resolution of a failed swap can be done using [`LightningNode::prepare_resolve_failed_swap`].
    ///
    /// Parameters:
    /// * `resolve_failed_swap_info` - Information needed to resolve the failed swap. Can be obtained
    ///   using [`LightningNode::prepare_resolve_failed_swap`].
    ///
    /// Returns the txid of the resolving transaction.
    ///
    /// Paid on-chain fees can be known in advance using [`LightningNode::prepare_resolve_failed_swap`].
    ///
    /// Requires network: **yes**
    #[deprecated = "onchain().swaps().sweep() should be used instead"]
    pub fn resolve_failed_swap(
        &self,
        resolve_failed_swap_info: ResolveFailedSwapInfo,
    ) -> Result<String> {
        self.onchain.swap().sweep(resolve_failed_swap_info.into())
    }

    /// Automatically swaps failed swap funds back to lightning.
    ///
    /// If a swap is in progress, this method will return an error.
    ///
    /// If the current balance doesn't fulfill the limits, this method will return an error.
    /// Before using this method use [`LightningNode::get_failed_swap_resolving_fees`] to validate a swap is available.
    ///
    /// Parameters:
    /// * `sats_per_vbyte` - the fee rate to use for the on-chain transaction.
    ///   Can be obtained with [`LightningNode::get_failed_swap_resolving_fees`].
    /// * `lsp_fee_params` - the lsp fee params for opening a new channel if necessary.
    ///   Can be obtained with [`LightningNode::get_failed_swap_resolving_fees`].
    ///
    /// Returns the txid of the sweeping tx.
    ///
    /// Requires network: **yes**
    #[deprecated = "onchain().swaps().swap() should be used instead"]
    pub fn swap_failed_swap_funds_to_lightning(
        &self,
        failed_swap_info: FailedSwapInfo,
        sats_per_vbyte: u32,
        lsp_fee_param: Option<OpeningFeeParams>,
    ) -> Result<String> {
        self.onchain
            .swap()
            .swap(failed_swap_info, sats_per_vbyte, lsp_fee_param)
    }

    /// Returns the fees for resolving channel closes if there are enough funds to pay for fees.
    ///
    /// Must only be called when there are onchain funds to resolve.
    ///
    /// Returns the fee information for the available resolving options.
    ///
    /// Requires network: **yes**
    #[deprecated = "onchain().channel_close().determine_resolving_fees() should be used instead"]
    pub fn get_channel_close_resolving_fees(&self) -> Result<Option<OnchainResolvingFees>> {
        self.onchain.channel_close().determine_resolving_fees()
    }

    /// Automatically swaps on-chain funds back to lightning.
    ///
    /// If a swap is in progress, this method will return an error.
    ///
    /// If the current balance doesn't fulfill the limits, this method will return an error.
    /// Before using this method use [`LightningNode::get_channel_close_resolving_fees`] to validate a swap is available.
    ///
    /// Parameters:
    /// * `sats_per_vbyte` - the fee rate to use for the on-chain transaction.
    ///   Can be obtained with [`LightningNode::get_channel_close_resolving_fees`].
    /// * `lsp_fee_params` - the lsp fee params for opening a new channel if necessary.
    ///   Can be obtained with [`LightningNode::get_channel_close_resolving_fees`].
    ///
    /// Returns the txid of the sweeping tx.
    ///
    /// Requires network: **yes**
    #[deprecated = "onchain().channel_close().swap() should be used instead"]
    pub fn swap_channel_close_funds_to_lightning(
        &self,
        sats_per_vbyte: u32,
        lsp_fee_params: Option<OpeningFeeParams>,
    ) -> std::result::Result<String, RedeemOnchainError> {
        self.onchain
            .channel_close()
            .swap(sats_per_vbyte, lsp_fee_params)
    }

    /// Prints additional debug information to the logs.
    ///
    /// Throws an error in case that the necessary information can't be retrieved.
    ///
    /// Requires network: **yes**
    #[deprecated = "util().log_debug_info() should be used instead"]
    pub fn log_debug_info(&self) -> Result<()> {
        self.util.log_debug_info()
    }

    /// Returns the latest [`FiatTopupInfo`] if the user has registered for the fiat topup.
    ///
    /// Requires network: **no**
    #[deprecated = "fiat_topup().get_info() should be used instead"]
    pub fn retrieve_latest_fiat_topup_info(&self) -> Result<Option<FiatTopupInfo>> {
        self.fiat_topup.get_info()
    }

    /// Returns the health check status of Breez and Greenlight services.
    ///
    /// Requires network: **yes**
    #[deprecated = "util().query_health_status() should be used instead"]
    pub fn get_health_status(&self) -> Result<BreezHealthCheckStatus> {
        self.util.query_health_status()
    }

    /// Check if clearing the wallet is feasible.
    ///
    /// Meaning that the balance is within the range of what can be reverse-swapped.
    ///
    /// Requires network: **yes**
    #[deprecated = "onchain().reverse_swap().determine_clear_wallet_feasibility() should be used instead"]
    pub fn check_clear_wallet_feasibility(&self) -> Result<RangeHit> {
        self.onchain
            .reverse_swap()
            .determine_clear_wallet_feasibility()
    }

    /// Prepares a reverse swap that sends all funds in LN channels. This is possible because the
    /// route to the swap service is known, so fees can be known in advance.
    ///
    /// This can fail if the balance is either too low or too high for it to be reverse-swapped.
    /// The method [`LightningNode::check_clear_wallet_feasibility`] can be used to check if the balance
    /// is within the required range.
    ///
    /// Requires network: **yes**
    #[deprecated = "onchain().reverse_swap().prepare_clear_wallet() should be used instead"]
    pub fn prepare_clear_wallet(&self) -> Result<ClearWalletInfo> {
        self.onchain.reverse_swap().prepare_clear_wallet()
    }

    /// Starts a reverse swap that sends all funds in LN channels to the provided on-chain address.
    ///
    /// Parameters:
    /// * `clear_wallet_info` - An instance of [`ClearWalletInfo`] obtained using
    ///   [`LightningNode::prepare_clear_wallet`].
    /// * `destination_onchain_address_data` - An on-chain address data instance. Can be obtained
    ///   using [`LightningNode::decode_data`].
    ///
    /// Requires network: **yes**
    #[deprecated = "onchain().reverse_swap().clear_wallet() should be used instead"]
    pub fn clear_wallet(
        &self,
        clear_wallet_info: ClearWalletInfo,
        destination_onchain_address_data: BitcoinAddressData,
    ) -> Result<()> {
        self.onchain
            .reverse_swap()
            .clear_wallet(clear_wallet_info, destination_onchain_address_data)
    }

    /// Set the analytics configuration.
    ///
    /// This can be used to completely prevent any analytics data from being reported.
    ///
    /// Requires network: **no**
    #[deprecated = "config().set_analytics_config() should be used instead"]
    pub fn set_analytics_config(&self, config: AnalyticsConfig) -> Result<()> {
        self.config.set_analytics_config(config)
    }

    /// Get the currently configured analytics configuration.
    ///
    /// Requires network: **no**
    #[deprecated = "config().get_analytics_config() should be used instead"]
    pub fn get_analytics_config(&self) -> Result<AnalyticsConfig> {
        self.config.get_analytics_config()
    }

    /// Register a human-readable lightning address or return the previously
    /// registered one.
    ///
    /// Requires network: **yes**
    #[deprecated = "lightning_address().register() should be used instead"]
    pub fn register_lightning_address(&self) -> Result<String> {
        self.lightning_address.register()
    }

    /// Query the registered lightning address.
    ///
    /// Requires network: **no**
    #[deprecated = "lightning_address().get() should be used instead"]
    pub fn query_lightning_address(&self) -> Result<Option<String>> {
        self.lightning_address.get()
    }

    /// Query for a previously verified phone number.
    ///
    /// Requires network: **no**
    #[deprecated = "phone_number().get() should be used instead"]
    pub fn query_verified_phone_number(&self) -> Result<Option<String>> {
        self.phone_number.get()
    }

    /// Start the verification process for a new phone number. This will trigger an SMS containing
    /// an OTP to be sent to the provided `phone_number`. To conclude the verification process,
    /// the method [`LightningNode::verify_phone_number`] should be called next.
    ///
    /// Parameters:
    /// * `phone_number` - the phone number to be registered. Needs to be checked for validity using
    ///   [LightningNode::parse_phone_number_to_lightning_address].
    ///
    /// Requires network: **yes**
    #[deprecated = "phone_number().register() should be used instead"]
    pub fn request_phone_number_verification(&self, phone_number: String) -> Result<()> {
        self.phone_number.register(phone_number)
    }

    /// Finish the verification process for a new phone number.
    ///
    /// Parameters:
    /// * `phone_number` - the phone number to be verified.
    /// * `otp` - the OTP code sent as an SMS to the phone number.
    ///
    /// Requires network: **yes**
    #[deprecated = "phone_number().verify() should be used instead"]
    pub fn verify_phone_number(&self, phone_number: String, otp: String) -> Result<()> {
        self.phone_number.verify(phone_number, otp)
    }

    /// Set value of a feature flag.
    /// The method will report the change to the backend and update the local database.
    ///
    /// Parameters:
    /// * `feature` - feature flag to be set.
    /// * `enable` - enable or disable the feature.
    ///
    /// Requires network: **yes**
    #[deprecated = "config().set_feature_flag() should be used instead"]
    pub fn set_feature_flag(&self, feature: FeatureFlag, flag_enabled: bool) -> Result<()> {
        self.config.set_feature_flag(feature, flag_enabled)
    }

    // Only meant for example CLI use
    #[doc(hidden)]
    pub fn close_all_channels_with_current_lsp(&self) -> Result<()> {
        self.rt
            .handle()
            .block_on(self.sdk.close_lsp_channels())
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to close channels",
            )?;
        Ok(())
    }
}

pub(crate) async fn start_sdk(
    config: &LightningNodeConfig,
    event_listener: Box<dyn EventListener>,
) -> Result<Arc<BreezServices>> {
    let developer_cert = config
        .breez_sdk_config
        .breez_sdk_partner_certificate
        .as_bytes()
        .to_vec();
    let developer_key = config
        .breez_sdk_config
        .breez_sdk_partner_key
        .as_bytes()
        .to_vec();
    let partner_credentials = GreenlightCredentials {
        developer_cert,
        developer_key,
    };

    let mut breez_config = BreezServices::default_config(
        EnvironmentType::Production,
        config.breez_sdk_config.breez_sdk_api_key.clone(),
        NodeConfig::Greenlight {
            config: GreenlightNodeConfig {
                partner_credentials: Some(partner_credentials),
                invite_code: None,
            },
        },
    );

    breez_config
        .working_dir
        .clone_from(&config.local_persistence_path);
    breez_config.exemptfee_msat = config
        .max_routing_fee_config
        .max_routing_fee_exempt_fee_sats
        .as_sats()
        .msats;
    breez_config.maxfee_percent =
        Permyriad(config.max_routing_fee_config.max_routing_fee_permyriad).to_percentage();
    let connect_request = ConnectRequest {
        config: breez_config,
        seed: config.seed.clone(),
        restore_only: None,
    };
    BreezServices::connect(connect_request, event_listener)
        .await
        .map_to_runtime_error(
            RuntimeErrorCode::NodeUnavailable,
            "Failed to initialize a breez sdk instance",
        )
}

/// Accept lipa's terms and conditions. Should be called before instantiating a [`LightningNode`]
/// for the first time.
///
/// Parameters:
/// * `backend_url`
/// * `seed` - the seed from the wallet for which the T&C will be accepted.
/// * `version` - the version number being accepted.
/// * `fingerprint` - the fingerprint of the version being accepted.
///
/// Requires network: **yes**
pub fn accept_terms_and_conditions(
    backend_url: String,
    seed: Vec<u8>,
    version: i64,
    fingerprint: String,
) -> Result<()> {
    enable_backtrace();
    let seed = sanitize_input::strong_type_seed(&seed)?;
    let auth = build_auth(&seed, &backend_url)?;
    auth.accept_terms_and_conditions(TermsAndConditions::Lipa, version, fingerprint)
        .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)
}

/// Try to parse the provided string as a lightning address, return [`ParseError`]
/// precisely indicating why parsing failed.
///
/// Requires network: **no**
pub fn parse_lightning_address(address: &str) -> std::result::Result<(), ParseError> {
    parser::parse_lightning_address(address).map_err(ParseError::from)
}

/// Allows checking if certain terms and conditions have been accepted by the user.
///
/// Parameters:
/// * `environment` - Which environment should be used.
/// * `seed` - The seed of the wallet.
/// * `terms_and_conditions` - [`TermsAndConditions`] for which the status should be requested.
///
/// Returns the status of the requested [`TermsAndConditions`].
///
/// Requires network: **yes**
pub fn get_terms_and_conditions_status(
    backend_url: String,
    seed: Vec<u8>,
    terms_and_conditions: TermsAndConditions,
) -> Result<TermsAndConditionsStatus> {
    enable_backtrace();
    let seed = sanitize_input::strong_type_seed(&seed)?;
    let auth = build_auth(&seed, &backend_url)?;
    auth.get_terms_and_conditions_status(terms_and_conditions)
        .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)
}

pub(crate) fn enable_backtrace() {
    env::set_var("RUST_BACKTRACE", "1");
}

fn fill_payout_fee(
    offer: OfferKind,
    requested_amount: Msats,
    rate: &Option<ExchangeRate>,
) -> OfferKind {
    match offer {
        OfferKind::Pocket {
            id,
            exchange_rate,
            topup_value_minor_units,
            topup_value_sats,
            exchange_fee_minor_units,
            exchange_fee_rate_permyriad,
            lightning_payout_fee: _,
            error,
        } => {
            let lightning_payout_fee = topup_value_sats.map(|v| {
                (v.as_sats().msats - requested_amount.msats)
                    .as_msats()
                    .to_amount_up(rate)
            });

            OfferKind::Pocket {
                id,
                exchange_rate,
                topup_value_minor_units,
                topup_value_sats,
                exchange_fee_minor_units,
                exchange_fee_rate_permyriad,
                lightning_payout_fee,
                error,
            }
        }
    }
}

// TODO provide corrupted acticity information partially instead of hiding it
fn filter_out_and_log_corrupted_activities(r: Result<Activity>) -> Option<Activity> {
    if r.is_ok() {
        r.ok()
    } else {
        error!(
            "Corrupted activity data, ignoring activity: {}",
            r.expect_err("Expected error, received ok")
        );
        None
    }
}

// TODO provide corrupted payment information partially instead of hiding it
fn filter_out_and_log_corrupted_payments(
    r: Result<IncomingPaymentInfo>,
) -> Option<IncomingPaymentInfo> {
    if r.is_ok() {
        r.ok()
    } else {
        error!(
            "Corrupted payment data, ignoring payment: {}",
            r.expect_err("Expected error, received ok")
        );
        None
    }
}

pub(crate) fn register_webhook_url(
    rt: &AsyncRuntime,
    sdk: &BreezServices,
    auth: &Auth,
    config: &LightningNodeConfig,
) -> Result<()> {
    let id = auth.get_wallet_pubkey_id().map_to_runtime_error(
        RuntimeErrorCode::AuthServiceUnavailable,
        "Failed to authenticate in order to get wallet pubkey id",
    )?;
    let encrypted_id = deterministic_encrypt(
        id.as_bytes(),
        &<[u8; 32]>::from_hex(
            &config
                .remote_services_config
                .notification_webhook_secret_hex,
        )
        .map_to_invalid_input("Invalid notification_webhook_secret_hex")?,
    )
    .map_to_permanent_failure("Failed to encrypt wallet pubkey id")?;
    let encrypted_id = hex::encode(encrypted_id);
    let webhook_url = config
        .remote_services_config
        .notification_webhook_base_url
        .replacen("{id}", &encrypted_id, 1);
    rt.handle()
        .block_on(sdk.register_webhook(webhook_url.clone()))
        .map_to_runtime_error(
            RuntimeErrorCode::NodeUnavailable,
            "Failed to register notification webhook",
        )?;
    debug!("Successfully registered notification webhook with Breez SDK. URL: {webhook_url}");
    Ok(())
}

fn with_status(status: EnableStatus) -> impl Fn((String, EnableStatus)) -> Option<String> {
    move |(v, s)| if s == status { Some(v) } else { None }
}

include!(concat!(env!("OUT_DIR"), "/lipalightninglib.uniffi.rs"));
