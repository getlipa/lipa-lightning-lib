//! # lipa-lightning-lib (aka 3L)
//!
//! This crate implements all the main business logic of the lipa wallet.
//!
//! Most functionality can be accessed by creating an instance of [`LightningNode`] and using its methods.

#![allow(clippy::let_unit_value)]

extern crate core;

mod activity;
mod amount;
mod analytics;
mod async_runtime;
mod auth;
mod backup;
mod callbacks;
mod config;
mod data_store;
mod environment;
mod errors;
mod event;
mod exchange_rate_provider;
mod fund_migration;
mod invoice_details;
mod key_derivation;
mod limits;
mod lnurl;
mod locker;
mod logger;
mod migrations;
mod notification_handling;
mod offer;
mod payment;
mod phone_number;
mod random;
mod recovery;
mod reverse_swap;
mod sanitize_input;
mod secret;
mod swap;
mod symmetric_encryption;
mod task_manager;
mod util;

pub use crate::activity::{Activity, ChannelCloseInfo, ChannelCloseState, ListActivitiesResponse};
pub use crate::amount::{Amount, FiatValue};
use crate::amount::{AsSats, Msats, Permyriad, Sats, ToAmount};
use crate::analytics::{derive_analytics_keys, AnalyticsConfig, AnalyticsInterceptor};
pub use crate::analytics::{InvoiceCreationMetadata, PaymentMetadata};
use crate::async_runtime::AsyncRuntime;
use crate::auth::{build_async_auth, build_auth};
use crate::backup::BackupManager;
pub use crate::callbacks::EventsCallback;
use crate::config::WithTimezone;
pub use crate::config::{Config, TzConfig, TzTime};
use crate::data_store::CreatedInvoice;
use crate::environment::Environment;
pub use crate::environment::EnvironmentCode;
use crate::errors::{
    map_lnurl_pay_error, map_lnurl_withdraw_error, map_send_payment_error, LnUrlWithdrawError,
    LnUrlWithdrawErrorCode, LnUrlWithdrawResult,
};
pub use crate::errors::{
    DecodeDataError, Error as LnError, LnUrlPayError, LnUrlPayErrorCode, LnUrlPayResult,
    MnemonicError, NotificationHandlingError, NotificationHandlingErrorCode, ParseError,
    ParsePhoneNumberError, ParsePhoneNumberPrefixError, PayError, PayErrorCode, PayResult, Result,
    RuntimeErrorCode, SimpleError, UnsupportedDataType,
};
use crate::event::LipaEventListener;
pub use crate::exchange_rate_provider::ExchangeRate;
use crate::exchange_rate_provider::ExchangeRateProviderImpl;
pub use crate::invoice_details::InvoiceDetails;
use crate::key_derivation::derive_persistence_encryption_key;
pub use crate::limits::{LiquidityLimit, PaymentAmountLimits};
pub use crate::lnurl::{LnUrlPayDetails, LnUrlWithdrawDetails};
use crate::locker::Locker;
pub use crate::notification_handling::{handle_notification, Notification, NotificationToggles};
pub use crate::offer::{OfferInfo, OfferKind, OfferStatus};
pub use crate::payment::{
    IncomingPaymentInfo, OutgoingPaymentInfo, PaymentInfo, PaymentState, Recipient,
};
pub use crate::phone_number::PhoneNumber;
use crate::phone_number::PhoneNumberPrefixParser;
pub use crate::recovery::recover_lightning_node;
pub use crate::reverse_swap::ReverseSwapInfo;
pub use crate::secret::{generate_secret, mnemonic_to_secret, words_by_prefix, Secret};
pub use crate::swap::{
    FailedSwapInfo, ResolveFailedSwapInfo, SwapAddressInfo, SwapInfo, SwapToLightningFees,
};
use crate::symmetric_encryption::deterministic_encrypt;
use crate::task_manager::TaskManager;
use crate::util::{
    replace_byte_arrays_by_hex_string, unix_timestamp_to_system_time, LogIgnoreError,
};
pub use pocketclient::FiatTopupInfo;
use pocketclient::PocketClient;

pub use breez_sdk_core::error::ReceiveOnchainError as SwapError;
use breez_sdk_core::error::{ReceiveOnchainError, SendPaymentError};
pub use breez_sdk_core::HealthCheckStatus as BreezHealthCheckStatus;
pub use breez_sdk_core::ReverseSwapStatus;
use breez_sdk_core::{
    parse, parse_invoice, BitcoinAddressData, BreezServices, ClosedChannelPaymentDetails,
    ConnectRequest, EventListener, GreenlightCredentials, GreenlightNodeConfig, InputType,
    ListPaymentsRequest, LnUrlPayRequest, LnUrlPayRequestData, LnUrlWithdrawRequest,
    LnUrlWithdrawRequestData, Network, NodeConfig, OpenChannelFeeRequest, OpeningFeeParams,
    PayOnchainRequest, PaymentDetails, PaymentStatus, PaymentTypeFilter,
    PrepareOnchainPaymentRequest, PrepareOnchainPaymentResponse, PrepareRedeemOnchainFundsRequest,
    PrepareRefundRequest, ReceiveOnchainRequest, RedeemOnchainFundsRequest, RefundRequest,
    ReportIssueRequest, ReportPaymentFailureDetails, ReverseSwapFeesRequest, SendPaymentRequest,
    SignMessageRequest, UnspentTransactionOutput,
};
use crow::{CountryCode, LanguageCode, OfferManager, TopupError, TopupInfo};
pub use crow::{PermanentFailureCode, TemporaryFailureCode};
use data_store::DataStore;
use email_address::EmailAddress;
use honeybadger::Auth;
pub use honeybadger::{TermsAndConditions, TermsAndConditionsStatus};
use iban::Iban;
use log::{debug, error, info, warn, Level};
use logger::init_logger_once;
use parrot::AnalyticsClient;
pub use parrot::PaymentSource;
use perro::{
    ensure, invalid_input, permanent_failure, runtime_error, MapToError, OptionToError, ResultTrait,
};
use squirrel::RemoteBackupClient;
use std::cmp::Reverse;
use std::collections::HashSet;
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use std::{env, fs};
use uuid::Uuid;

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
    /// Capacity the node can actually receive.
    /// It excludes non usable channels, pending HTLCs, channels reserves, etc.
    pub inbound_capacity: Amount,
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

const MAX_FEE_PERMYRIAD: Permyriad = Permyriad(150);
const EXEMPT_FEE: Sats = Sats::new(21);

/// The main class/struct of this library. Constructing an instance will initiate the Lightning node and
/// run it in the background. As long as an instance of `LightningNode` is held, the node will continue to run
/// in the background. Dropping the instance will start a deinit process.  
pub struct LightningNode {
    user_preferences: Arc<Mutex<UserPreferences>>,
    sdk: Arc<BreezServices>,
    auth: Arc<Auth>,
    async_auth: Arc<honeybadger::asynchronous::Auth>,
    fiat_topup_client: PocketClient,
    offer_manager: OfferManager,
    rt: AsyncRuntime,
    data_store: Arc<Mutex<DataStore>>,
    task_manager: Arc<Mutex<TaskManager>>,
    analytics_interceptor: Arc<AnalyticsInterceptor>,
    environment: Environment,
    allowed_countries_country_iso_3166_1_alpha_2: Vec<String>,
    phone_number_prefix_parser: PhoneNumberPrefixParser,
}

/// Contains the fee information for the options to resolve on-chain funds from channel closes.
pub struct ChannelCloseResolvingFees {
    /// Fees to swap the funds back to lightning using [`LightningNode::swap_onchain_to_lightning`]
    /// Only available if enough funds are there to swap.
    pub swap_fees: Option<SwapToLightningFees>,
    /// Estimate of the fees for sending the funds on-chain using [`LightningNode::sweep`].
    /// The exact fees will be known when calling [`LightningNode::prepare_sweep`].
    pub sweep_onchain_fee_estimate: Amount,
    /// Used internally to sweep with the given on-chain fees.
    /// See [`LightningNode::prepare_sweep`] and [`LightningNode::swap_onchain_to_lightning`]
    pub sat_per_vbyte: u32,
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
    /// of certain events.
    ///
    /// Requires network: **yes**
    pub fn new(config: Config, events_callback: Box<dyn EventsCallback>) -> Result<Self> {
        enable_backtrace();
        fs::create_dir_all(&config.local_persistence_path).map_to_permanent_failure(format!(
            "Failed to create directory: {}",
            &config.local_persistence_path,
        ))?;
        if let Some(level) = config.file_logging_level {
            init_logger_once(
                level,
                &Path::new(&config.local_persistence_path).join(LOGS_DIR),
            )?;
        }
        info!("3L version: {}", env!("GITHUB_REF"));

        let rt = AsyncRuntime::new()?;

        let environment = Environment::load(config.environment)?;

        let strong_typed_seed = sanitize_input::strong_type_seed(&config.seed)?;
        let auth = Arc::new(build_auth(&strong_typed_seed, &environment.backend_url)?);
        let async_auth = Arc::new(build_async_auth(
            &strong_typed_seed,
            &environment.backend_url,
        )?);

        let user_preferences = Arc::new(Mutex::new(UserPreferences {
            fiat_currency: config.fiat_currency.clone(),
            timezone_config: config.timezone_config.clone(),
        }));

        let analytics_client = AnalyticsClient::new(
            environment.backend_url.clone(),
            derive_analytics_keys(&strong_typed_seed)?,
            Arc::clone(&async_auth),
        );

        let db_path = format!("{}/{DB_FILENAME}", config.local_persistence_path);
        let data_store = Arc::new(Mutex::new(DataStore::new(&db_path)?));

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
            let sdk = start_sdk(&config, &environment, event_listener).await?;
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
            environment.backend_url.clone(),
            Arc::clone(&auth),
        ));

        let offer_manager = OfferManager::new(environment.backend_url.clone(), Arc::clone(&auth));

        let fiat_topup_client = PocketClient::new(environment.pocket_url.clone())
            .map_to_runtime_error(
                RuntimeErrorCode::OfferServiceUnavailable,
                "Couldn't create a fiat topup client",
            )?;

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
            events_callback,
        )?));
        task_manager.lock_unwrap().foreground();

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

        fund_migration::log_fund_migration_data(&strong_typed_seed)?;

        register_webhook_url(&rt, &sdk, &auth, &environment)?;

        let phone_number_prefix_parser =
            PhoneNumberPrefixParser::new(&config.phone_number_allowed_countries_iso_3166_1_alpha_2);

        Ok(LightningNode {
            user_preferences,
            sdk,
            auth,
            async_auth,
            fiat_topup_client,
            offer_manager,
            rt,
            data_store,
            task_manager,
            analytics_interceptor,
            environment,
            allowed_countries_country_iso_3166_1_alpha_2: config
                .phone_number_allowed_countries_iso_3166_1_alpha_2,
            phone_number_prefix_parser,
        })
    }

    /// Request some basic info about the node
    ///
    /// Requires network: **no**
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
    ///
    /// Requires network: **no**
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
    ///
    /// Requires network: **yes**
    pub fn calculate_lsp_fee(&self, amount_sat: u64) -> Result<CalculateLspFeeResponse> {
        let req = OpenChannelFeeRequest {
            amount_msat: Some(amount_sat.as_sats().msats),
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
                .ok_or_permanent_failure("Breez SDK open_channel_fee returned None lsp fee when provided with Some(amount_msat)")?
                .as_msats()
                .to_amount_up(&self.get_exchange_rate()),
            lsp_fee_params: Some(res.fee_params),
        })
    }

    /// Get the current limits for the amount that can be transferred in a single payment.
    /// Currently there are only limits for receiving payments.
    /// The limits (partly) depend on the channel situation of the node, so it should be called
    /// again every time the user is about to receive a payment.
    /// The limits stay the same regardless of what amount wants to receive (= no changes while
    /// he's typing the amount)
    ///
    /// Requires network: **no**
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
    ///
    /// Requires network: **yes**
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

        self.store_payment_info(&response.ln_invoice.payment_hash, None);
        self.data_store
            .lock_unwrap()
            .store_created_invoice(
                &response.ln_invoice.payment_hash,
                &response.ln_invoice.bolt11,
                &response.opening_fee_msat,
                response.ln_invoice.timestamp + response.ln_invoice.expiry,
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

    /// Parse a phone number prefix, check against the list of allowed countries
    /// (set in [`Config::phone_number_allowed_countries_iso_3166_1_alpha_2`]).
    /// The parser is not strict, it parses some invalid prefixes as valid.
    ///
    /// Requires network: **no**
    pub fn parse_phone_number_prefix(
        &self,
        phone_number_prefix: String,
    ) -> std::result::Result<(), ParsePhoneNumberPrefixError> {
        self.phone_number_prefix_parser.parse(&phone_number_prefix)
    }

    /// Parse a phone number, check against the list of allowed countries
    /// (set in [`Config::phone_number_allowed_countries_iso_3166_1_alpha_2`]).
    ///
    /// Returns a possible lightning address, which can be checked for existence
    /// with [`LightningNode::decode_data`].
    ///
    /// Requires network: **no**
    pub fn parse_phone_number_to_lightning_address(
        &self,
        phone_number: String,
    ) -> std::result::Result<String, ParsePhoneNumberError> {
        let phone_number = self.parse_phone_number(phone_number)?;
        Ok(phone_number.to_lightning_address(&self.environment.lipa_lightning_domain))
    }

    fn parse_phone_number(
        &self,
        phone_number: String,
    ) -> std::result::Result<PhoneNumber, ParsePhoneNumberError> {
        let phone_number = PhoneNumber::parse(&phone_number)?;
        ensure!(
            self.allowed_countries_country_iso_3166_1_alpha_2
                .contains(&phone_number.country_code.as_ref().to_string()),
            ParsePhoneNumberError::UnsupportedCountry
        );
        Ok(phone_number)
    }

    /// Decode a user-provided string (usually obtained from QR-code or pasted).
    ///
    /// Requires network: **yes**
    pub fn decode_data(&self, data: String) -> std::result::Result<DecodedData, DecodeDataError> {
        match self.rt.handle().block_on(parse(&data)) {
            Ok(InputType::Bolt11 { invoice }) => {
                ensure!(
                    invoice.network == self.environment.network,
                    DecodeDataError::Unsupported {
                        typ: UnsupportedDataType::Network {
                            network: invoice.network.to_string(),
                        },
                    }
                );

                Ok(DecodedData::Bolt11Invoice {
                    invoice_details: InvoiceDetails::from_ln_invoice(
                        invoice,
                        &self.get_exchange_rate(),
                    ),
                })
            }
            Ok(InputType::LnUrlPay { data }) => Ok(DecodedData::LnUrlPay {
                lnurl_pay_details: LnUrlPayDetails::from_lnurl_pay_request_data(
                    data,
                    &self.get_exchange_rate(),
                )?,
            }),
            Ok(InputType::BitcoinAddress { address }) => Ok(DecodedData::OnchainAddress {
                onchain_address_details: address,
            }),
            Ok(InputType::LnUrlAuth { .. }) => Err(DecodeDataError::Unsupported {
                typ: UnsupportedDataType::LnUrlAuth,
            }),
            Ok(InputType::LnUrlError { data }) => {
                Err(DecodeDataError::LnUrlError { msg: data.reason })
            }
            Ok(InputType::LnUrlWithdraw { data }) => Ok(DecodedData::LnUrlWithdraw {
                lnurl_withdraw_details: LnUrlWithdrawDetails::from_lnurl_withdraw_request_data(
                    data,
                    &self.get_exchange_rate(),
                ),
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
    ///
    /// Requires network: **no**
    pub fn get_payment_max_routing_fee_mode(&self, amount_sat: u64) -> MaxRoutingFeeMode {
        get_payment_max_routing_fee_mode(amount_sat, &self.get_exchange_rate())
    }

    /// Checks if the given amount could be spent on an invoice.
    ///
    /// Parameters:
    /// * `amount` - The to be spent amount.
    ///
    /// Requires network: **no**
    pub fn get_invoice_affordability(&self, amount_sat: u64) -> Result<InvoiceAffordability> {
        let amount = amount_sat.as_sats();

        let routing_fee_mode = self.get_payment_max_routing_fee_mode(amount_sat);

        let max_fee_msats = match routing_fee_mode {
            MaxRoutingFeeMode::Relative { max_fee_permyriad } => {
                Permyriad(max_fee_permyriad).of(&amount).msats
            }
            MaxRoutingFeeMode::Absolute { max_fee_amount } => max_fee_amount.sats.as_sats().msats,
        };

        let node_state = self.sdk.node_info().map_to_runtime_error(
            RuntimeErrorCode::NodeUnavailable,
            "Failed to read node info",
        )?;

        if amount.msats > node_state.max_payable_msat {
            return Ok(InvoiceAffordability::NotEnoughFunds);
        }

        if amount.msats + max_fee_msats > node_state.max_payable_msat {
            return Ok(InvoiceAffordability::UnaffordableFees);
        }

        Ok(InvoiceAffordability::Affordable)
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
    ///
    /// Requires network: **yes**
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
        self.store_payment_info(&invoice_details.payment_hash, None);
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

        let result = self
            .rt
            .handle()
            .block_on(self.sdk.send_payment(SendPaymentRequest {
                bolt11: invoice_details.invoice,
                amount_msat,
                label: None,
            }));

        if matches!(
            result,
            Err(SendPaymentError::Generic { .. }
                | SendPaymentError::PaymentFailed { .. }
                | SendPaymentError::PaymentTimeout { .. }
                | SendPaymentError::RouteNotFound { .. }
                | SendPaymentError::RouteTooExpensive { .. }
                | SendPaymentError::ServiceConnectivity { .. })
        ) {
            self.report_send_payment_issue(invoice_details.payment_hash);
        }

        result.map_err(map_send_payment_error)?;
        Ok(())
    }

    /// Pay an LNURL-pay the provided amount.
    ///
    /// Parameters:
    /// * `lnurl_pay_request_data` - LNURL-pay request data as obtained from [`LightningNode::decode_data`]
    /// * `amount_sat` - amount to be paid
    /// * `comment` - optional comment to be sent to payee (`max_comment_length` in
    /// [`LnUrlPayDetails`] must be respected)
    ///
    /// Returns the payment hash of the payment.
    ///
    /// Requires network: **yes**
    pub fn pay_lnurlp(
        &self,
        lnurl_pay_request_data: LnUrlPayRequestData,
        amount_sat: u64,
        comment: Option<String>,
    ) -> LnUrlPayResult<String> {
        let comment_allowed = lnurl_pay_request_data.comment_allowed;
        ensure!(
            !matches!(comment, Some(ref comment) if comment.len() > comment_allowed as usize),
            invalid_input(format!(
                "The provided comment is longer than the allowed {comment_allowed} characters"
            ))
        );

        let payment_hash = match self
            .rt
            .handle()
            .block_on(self.sdk.lnurl_pay(LnUrlPayRequest {
                data: lnurl_pay_request_data,
                amount_msat: amount_sat.as_sats().msats,
                comment,
                payment_label: None,
            }))
            .map_err(map_lnurl_pay_error)?
        {
            breez_sdk_core::lnurl::pay::LnUrlPayResult::EndpointSuccess { data } => {
                Ok(data.payment.id)
            }
            breez_sdk_core::lnurl::pay::LnUrlPayResult::EndpointError { data } => runtime_error!(
                LnUrlPayErrorCode::LnUrlServerError,
                "LNURL server returned error: {}",
                data.reason
            ),
            breez_sdk_core::lnurl::pay::LnUrlPayResult::PayError { data } => {
                self.report_send_payment_issue(data.payment_hash);
                runtime_error!(
                    LnUrlPayErrorCode::PaymentFailed,
                    "Paying invoice for LNURL pay failed: {}",
                    data.reason
                )
            }
        }?;
        self.store_payment_info(&payment_hash, None);
        Ok(payment_hash)
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
                Recipient::from_lightning_address(&p.0, &self.environment.lipa_lightning_domain)
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
    pub fn withdraw_lnurlw(
        &self,
        lnurl_withdraw_request_data: LnUrlWithdrawRequestData,
        amount_sat: u64,
    ) -> LnUrlWithdrawResult<String> {
        let payment_hash = match self
            .rt
            .handle()
            .block_on(self.sdk.lnurl_withdraw(LnUrlWithdrawRequest {
                data: lnurl_withdraw_request_data,
                amount_msat: amount_sat.as_sats().msats,
                description: None,
            }))
            .map_err(map_lnurl_withdraw_error)?
        {
            breez_sdk_core::LnUrlWithdrawResult::Ok { data } => Ok(data.invoice.payment_hash),
            breez_sdk_core::LnUrlWithdrawResult::Timeout { data } => {
                warn!("Tolerating LNURL-w timeout");
                Ok(data.invoice.payment_hash)
            }
            breez_sdk_core::LnUrlWithdrawResult::ErrorStatus { data } => runtime_error!(
                LnUrlWithdrawErrorCode::LnUrlServerError,
                "LNURL server returned error: {}",
                data.reason
            ),
        }?;
        self.store_payment_info(&payment_hash, None);
        Ok(payment_hash)
    }

    /// Get a list of the latest activities
    ///
    /// Parameters:
    /// * `number_of_completed_activities` - the maximum number of completed activities that will be returned
    ///
    /// Requires network: **no**
    pub fn get_latest_activities(
        &self,
        number_of_completed_activities: u32,
    ) -> Result<ListActivitiesResponse> {
        const LEEWAY_FOR_PENDING_PAYMENTS: u32 = 10;
        let list_payments_request = ListPaymentsRequest {
            filters: Some(vec![
                PaymentTypeFilter::Sent,
                PaymentTypeFilter::Received,
                PaymentTypeFilter::ClosedChannel,
            ]),
            metadata_filters: None,
            from_timestamp: None,
            to_timestamp: None,
            include_failures: Some(true),
            limit: Some(number_of_completed_activities + LEEWAY_FOR_PENDING_PAYMENTS),
            offset: None,
        };
        let breez_activities = self
            .rt
            .handle()
            .block_on(self.sdk.list_payments(list_payments_request))
            .map_to_runtime_error(RuntimeErrorCode::NodeUnavailable, "Failed to list payments")?
            .into_iter()
            .map(|p| self.activity_from_breez_payment(p))
            .filter_map(filter_out_and_log_corrupted_activities)
            .collect::<Vec<_>>();

        // Query created invoices, filter out ones which are in the breez db.
        let created_invoices = self
            .data_store
            .lock_unwrap()
            .retrieve_created_invoices(number_of_completed_activities)?;

        let activities = self.multiplex_activities(breez_activities, created_invoices);

        // Split by state.
        let mut pending_activities = Vec::new();
        let mut completed_activities = Vec::new();
        activities.into_iter().for_each(|m| {
            if m.is_pending() {
                pending_activities.push(m)
            } else {
                completed_activities.push(m)
            }
        });

        if let Some(in_progress_swap) = self
            .rt
            .handle()
            .block_on(self.sdk.in_progress_swap())
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to get in-progress swap",
            )?
        {
            let created_at = unix_timestamp_to_system_time(in_progress_swap.created_at as u64)
                .with_timezone(self.user_preferences.lock_unwrap().clone().timezone_config);

            pending_activities.push(Activity::Swap {
                incoming_payment_info: None,
                swap_info: SwapInfo {
                    bitcoin_address: in_progress_swap.bitcoin_address,
                    created_at,
                    // Multiple txs can be sent to swap address and they aren't guaranteed to
                    // confirm all at the same time. Our best guess of the amount that will be
                    // received once the entire swap confirms is given by confirmed sats added to
                    // any unconfirmed sats waiting to be confirmed.
                    paid_amount: (in_progress_swap.unconfirmed_sats
                        + in_progress_swap.confirmed_sats)
                        .as_sats()
                        .to_amount_down(&self.get_exchange_rate()),
                },
            })
        }

        pending_activities.sort_by_key(|m| Reverse(m.get_time()));
        completed_activities.sort_by_key(|m| Reverse(m.get_time()));
        completed_activities.truncate(number_of_completed_activities as usize);
        Ok(ListActivitiesResponse {
            pending_activities,
            completed_activities,
        })
    }

    /// Combines a list of activities with a list of locally created invoices
    /// into a single activity list.
    ///
    /// Duplicates are removed.
    fn multiplex_activities(
        &self,
        breez_activities: Vec<Activity>,
        local_created_invoices: Vec<CreatedInvoice>,
    ) -> Vec<Activity> {
        let breez_payment_hashes: HashSet<_> = breez_activities
            .iter()
            .filter_map(|m| m.get_payment_info().map(|p| p.hash.clone()))
            .collect();
        let mut activities = local_created_invoices
            .into_iter()
            .filter(|i| !breez_payment_hashes.contains(i.hash.as_str()))
            .map(|i| self.payment_from_created_invoice(&i))
            .filter_map(filter_out_and_log_corrupted_payments)
            .map(|p| Activity::IncomingPayment {
                incoming_payment_info: p,
            })
            .collect::<Vec<_>>();
        activities.extend(breez_activities);
        activities
    }

    /// Get an incoming payment by its payment hash.
    ///
    /// Parameters:
    /// * `hash` - hex representation of payment hash
    ///
    /// Requires network: **no**
    pub fn get_incoming_payment(&self, hash: String) -> Result<IncomingPaymentInfo> {
        if let Some(breez_payment) = self
            .rt
            .handle()
            .block_on(self.sdk.payment_by_hash(hash.clone()))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to get payment by hash",
            )?
        {
            return match self.activity_from_breez_ln_payment(breez_payment)? {
                Activity::IncomingPayment {
                    incoming_payment_info,
                } => Ok(incoming_payment_info),
                Activity::OutgoingPayment { .. } => invalid_input!("OutgoingPayment was found"),
                Activity::OfferClaim {
                    incoming_payment_info,
                    ..
                } => Ok(incoming_payment_info),
                Activity::Swap {
                    incoming_payment_info: Some(incoming_payment_info),
                    ..
                } => Ok(incoming_payment_info),
                Activity::Swap {
                    incoming_payment_info: None,
                    ..
                } => invalid_input!("Pending swap was found"),
                Activity::ReverseSwap { .. } => invalid_input!("ReverseSwap was found"),
                Activity::ChannelClose { .. } => invalid_input!("ChannelClose was found"),
            };
        }
        let invoice = self
            .data_store
            .lock_unwrap()
            .retrieve_created_invoice_by_hash(&hash)?
            .ok_or_invalid_input("No payment with provided hash was found")?;
        self.payment_from_created_invoice(&invoice)
    }

    /// Get an outgoing payment by its payment hash.
    ///
    /// Parameters:
    /// * `hash` - hex representation of payment hash
    ///
    /// Requires network: **no**
    pub fn get_outgoing_payment(&self, hash: String) -> Result<OutgoingPaymentInfo> {
        let breez_payment = self
            .rt
            .handle()
            .block_on(self.sdk.payment_by_hash(hash))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to get payment by hash",
            )?
            .ok_or_invalid_input("No payment with provided hash was found")?;

        match self.activity_from_breez_ln_payment(breez_payment)? {
            Activity::IncomingPayment { .. } => invalid_input!("IncomingPayment was found"),
            Activity::OutgoingPayment {
                outgoing_payment_info,
            } => Ok(outgoing_payment_info),
            Activity::OfferClaim { .. } => invalid_input!("OfferClaim was found"),
            Activity::Swap { .. } => invalid_input!("Swap was found"),
            Activity::ReverseSwap {
                outgoing_payment_info,
                ..
            } => Ok(outgoing_payment_info),
            Activity::ChannelClose { .. } => invalid_input!("ChannelClose was found"),
        }
    }

    /// Get an activity by its payment hash.
    ///
    /// Parameters:
    /// * `hash` - hex representation of payment hash
    ///
    /// Requires network: **no**
    pub fn get_activity(&self, hash: String) -> Result<Activity> {
        let payment = self
            .rt
            .handle()
            .block_on(self.sdk.payment_by_hash(hash))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to get payment by hash",
            )?
            .ok_or_invalid_input("No activity with provided hash was found")?;

        self.activity_from_breez_ln_payment(payment)
    }

    /// Set a personal note on a specific payment.
    ///
    /// Parameters:
    /// * `payment_hash` - The hash of the payment for which a personal note will be set.
    /// * `note` - The personal note.
    ///
    /// Requires network: **no**
    pub fn set_payment_personal_note(&self, payment_hash: String, note: String) -> Result<()> {
        let note = Some(note.trim().to_string()).filter(|s| !s.is_empty());

        self.data_store
            .lock_unwrap()
            .update_personal_note(&payment_hash, note.as_deref())
    }

    fn activity_from_breez_payment(
        &self,
        breez_payment: breez_sdk_core::Payment,
    ) -> Result<Activity> {
        match &breez_payment.details {
            PaymentDetails::Ln { .. } => self.activity_from_breez_ln_payment(breez_payment),
            PaymentDetails::ClosedChannel { data } => {
                self.activity_from_breez_closed_channel_payment(&breez_payment, data)
            }
        }
    }

    fn activity_from_breez_ln_payment(
        &self,
        breez_payment: breez_sdk_core::Payment,
    ) -> Result<Activity> {
        let payment_details = match breez_payment.details {
            PaymentDetails::Ln { ref data } => data,
            PaymentDetails::ClosedChannel { .. } => {
                invalid_input!("PaymentInfo cannot be created from channel close")
            }
        };
        let local_payment_data = self
            .data_store
            .lock_unwrap()
            .retrieve_payment_info(&payment_details.payment_hash)?;
        let (exchange_rate, tz_config, personal_note, offer, received_on, received_lnurl_comment) =
            match local_payment_data {
                Some(data) => (
                    Some(data.exchange_rate),
                    data.user_preferences.timezone_config,
                    data.personal_note,
                    data.offer,
                    data.received_on,
                    data.received_lnurl_comment,
                ),
                None => (
                    self.get_exchange_rate(),
                    self.user_preferences.lock_unwrap().timezone_config.clone(),
                    None,
                    None,
                    None,
                    None,
                ),
            };

        if let Some(offer) = offer {
            let incoming_payment_info = IncomingPaymentInfo::new(
                breez_payment,
                &exchange_rate,
                tz_config,
                personal_note,
                received_on,
                received_lnurl_comment,
                &self.environment.lipa_lightning_domain,
            )?;
            let offer_kind = fill_payout_fee(
                offer,
                incoming_payment_info.requested_amount.sats.as_msats(),
                &exchange_rate,
            );
            Ok(Activity::OfferClaim {
                incoming_payment_info,
                offer_kind,
            })
        } else if let Some(ref s) = payment_details.swap_info {
            let swap_info = SwapInfo {
                bitcoin_address: s.bitcoin_address.clone(),
                // TODO: Persist SwapInfo in local db on state change, requires https://github.com/breez/breez-sdk/issues/518
                created_at: unix_timestamp_to_system_time(s.created_at as u64)
                    .with_timezone(tz_config.clone()),
                paid_amount: s.paid_msat.as_msats().to_amount_down(&exchange_rate),
            };
            let incoming_payment_info = IncomingPaymentInfo::new(
                breez_payment,
                &exchange_rate,
                tz_config,
                personal_note,
                received_on,
                received_lnurl_comment,
                &self.environment.lipa_lightning_domain,
            )?;
            Ok(Activity::Swap {
                incoming_payment_info: Some(incoming_payment_info),
                swap_info,
            })
        } else if let Some(ref s) = payment_details.reverse_swap_info {
            let reverse_swap_info = ReverseSwapInfo {
                paid_onchain_amount: s.onchain_amount_sat.as_sats().to_amount_up(&exchange_rate),
                claim_txid: s.claim_txid.clone(),
                status: s.status,
            };
            let outgoing_payment_info = OutgoingPaymentInfo::new(
                breez_payment,
                &exchange_rate,
                tz_config,
                personal_note,
                &self.environment.lipa_lightning_domain,
            )?;
            Ok(Activity::ReverseSwap {
                outgoing_payment_info,
                reverse_swap_info,
            })
        } else if breez_payment.payment_type == breez_sdk_core::PaymentType::Received {
            let incoming_payment_info = IncomingPaymentInfo::new(
                breez_payment,
                &exchange_rate,
                tz_config,
                personal_note,
                received_on,
                received_lnurl_comment,
                &self.environment.lipa_lightning_domain,
            )?;
            Ok(Activity::IncomingPayment {
                incoming_payment_info,
            })
        } else if breez_payment.payment_type == breez_sdk_core::PaymentType::Sent {
            let outgoing_payment_info = OutgoingPaymentInfo::new(
                breez_payment,
                &exchange_rate,
                tz_config,
                personal_note,
                &self.environment.lipa_lightning_domain,
            )?;
            Ok(Activity::OutgoingPayment {
                outgoing_payment_info,
            })
        } else {
            permanent_failure!("Unreachable code")
        }
    }

    fn activity_from_breez_closed_channel_payment(
        &self,
        breez_payment: &breez_sdk_core::Payment,
        details: &ClosedChannelPaymentDetails,
    ) -> Result<Activity> {
        let amount = breez_payment
            .amount_msat
            .as_msats()
            .to_amount_up(&self.get_exchange_rate());

        let user_preferences = self.user_preferences.lock_unwrap();

        let time = unix_timestamp_to_system_time(breez_payment.payment_time as u64)
            .with_timezone(user_preferences.timezone_config.clone());

        let (closed_at, state) = match breez_payment.status {
            PaymentStatus::Pending => (None, ChannelCloseState::Pending),
            PaymentStatus::Complete => (Some(time), ChannelCloseState::Confirmed),
            PaymentStatus::Failed => {
                permanent_failure!("A channel close Breez Payment has status *Failed*");
            }
        };

        // According to the docs, it can only be empty for older closed channels.
        let closing_tx_id = details.closing_txid.clone().unwrap_or_default();

        Ok(Activity::ChannelClose {
            channel_close_info: ChannelCloseInfo {
                amount,
                state,
                closed_at,
                closing_tx_id,
            },
        })
    }

    fn payment_from_created_invoice(
        &self,
        created_invoice: &CreatedInvoice,
    ) -> Result<IncomingPaymentInfo> {
        let invoice =
            parse_invoice(created_invoice.invoice.as_str()).map_to_permanent_failure(format!(
                "Invalid invoice obtained from local db: {}",
                created_invoice.invoice
            ))?;
        let invoice_details = InvoiceDetails::from_ln_invoice(invoice.clone(), &None);

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
        let invoice_details = InvoiceDetails::from_ln_invoice(invoice, &exchange_rate);
        // For receiving payments, we use the invoice timestamp.
        let time = invoice_details
            .creation_timestamp
            .with_timezone(local_payment_data.user_preferences.timezone_config);
        let lsp_fees = created_invoice
            .channel_opening_fees
            .unwrap_or_default()
            .as_msats()
            .to_amount_up(&exchange_rate);
        let requested_amount = invoice_details
            .amount
            .clone()
            .ok_or_permanent_failure("Locally created invoice doesn't include an amount")?
            .sats
            .as_sats()
            .to_amount_down(&exchange_rate);

        let amount = requested_amount.clone().sats - lsp_fees.sats;
        let amount = amount.as_sats().to_amount_down(&exchange_rate);

        let personal_note = local_payment_data.personal_note;

        let payment_info = PaymentInfo {
            payment_state,
            hash: invoice_details.payment_hash.clone(),
            amount,
            invoice_details: invoice_details.clone(),
            created_at: time,
            description: invoice_details.description,
            preimage: None,
            personal_note,
        };
        let incoming_payment_info = IncomingPaymentInfo {
            payment_info,
            requested_amount,
            lsp_fees,
            received_on: None,
            received_lnurl_comment: None,
        };
        Ok(incoming_payment_info)
    }

    /// Call the method when the app goes to foreground, such that the user can interact with it.
    /// The library starts running the background tasks more frequently to improve user experience.
    ///
    /// Requires network: **no**
    pub fn foreground(&self) {
        self.task_manager.lock_unwrap().foreground();
    }

    /// Call the method when the app goes to background, such that the user can not interact with it.
    /// The library stops running some unnecessary tasks and runs necessary tasks less frequently.
    /// It should save battery and internet traffic.
    ///
    /// Requires network: **no**
    pub fn background(&self) {
        self.task_manager.lock_unwrap().background();
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
    ///
    /// Requires network: **no**
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
    ///
    /// Requires network: **no**
    pub fn change_fiat_currency(&self, fiat_currency: String) {
        self.user_preferences.lock_unwrap().fiat_currency = fiat_currency;
    }

    /// Change the timezone config.
    ///
    /// Parameters:
    /// * `timezone_config` - the user's current timezone
    ///
    /// Requires network: **no**
    pub fn change_timezone_config(&self, timezone_config: TzConfig) {
        self.user_preferences.lock_unwrap().timezone_config = timezone_config;
    }

    /// Accepts Pocket's T&C.
    ///
    /// Parameters:
    /// * `version` - the version number being accepted.
    ///
    /// Requires network: **yes**
    pub fn accept_pocket_terms_and_conditions(&self, version: i64) -> Result<()> {
        self.auth
            .accept_terms_and_conditions(TermsAndConditions::Pocket, version)
            .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)
    }

    /// Similar to [`get_terms_and_conditions_status`] with the difference that this method is pre-filling
    /// the environment and seed based on the node configuration.
    ///
    /// Requires network: **yes**
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
    /// exchange. Not all are supported. A consumer of this library should find out about available
    /// ones using other sources.
    ///
    /// Requires network: **yes**
    pub fn register_fiat_topup(
        &self,
        email: Option<String>,
        user_iban: String,
        user_currency: String,
    ) -> Result<FiatTopupInfo> {
        debug!("register_fiat_topup() - called with - email: {email:?} - user_iban: {user_iban} - user_currency: {user_currency:?}");
        user_iban
            .parse::<Iban>()
            .map_to_invalid_input("Invalid user_iban")?;

        if let Some(email) = email.as_ref() {
            EmailAddress::from_str(email).map_to_invalid_input("Invalid email")?;
        }

        let sdk = Arc::clone(&self.sdk);
        let sign_message = |message| async move {
            sdk.sign_message(SignMessageRequest { message })
                .await
                .ok()
                .map(|r| r.signature)
        };
        let topup_info = self
            .rt
            .handle()
            .block_on(self.fiat_topup_client.register_pocket_fiat_topup(
                &user_iban,
                user_currency,
                self.get_node_info()?.node_pubkey,
                sign_message,
            ))
            .map_to_runtime_error(
                RuntimeErrorCode::OfferServiceUnavailable,
                "Failed to register pocket fiat topup",
            )?;

        self.data_store
            .lock_unwrap()
            .store_fiat_topup_info(topup_info.clone())?;

        self.offer_manager
            .register_topup(topup_info.order_id.clone(), email)
            .map_runtime_error_to(RuntimeErrorCode::OfferServiceUnavailable)?;

        Ok(topup_info)
    }

    /// Resets a previous fiat topups registration.
    ///
    /// Requires network: **no**
    pub fn reset_fiat_topup(&self) -> Result<()> {
        self.data_store.lock_unwrap().clear_fiat_topup_info()
    }

    /// Hides the topup with the given id. Can be called on expired topups so that they stop being returned
    /// by [`LightningNode::query_uncompleted_offers`].
    ///
    /// Topup id can be obtained from [`OfferKind::Pocket`].
    ///
    /// Requires network: **yes**
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
    pub fn list_action_required_items(&self) -> Result<Vec<ActionRequiredItem>> {
        let uncompleted_offers = self.query_uncompleted_offers()?;

        let failed_swaps = self.get_unresolved_failed_swaps()?;

        let available_channel_closes_funds = self.get_node_info()?.onchain_balance;

        let mut action_required_items: Vec<ActionRequiredItem> = uncompleted_offers
            .into_iter()
            .map(|o| o.into())
            .chain(failed_swaps.into_iter().map(|s| s.into()))
            .collect();

        // CLN currently forces a min-emergency onchain balance of 546 (the dust limit)
        // TODO: Replace CLN_DUST_LIMIT_SAT with 0 if/when
        //      https://github.com/ElementsProject/lightning/issues/7131 is addressed
        if available_channel_closes_funds.sats > CLN_DUST_LIMIT_SAT {
            let utxos = self.get_node_utxos()?;

            // If we already have a 546 sat UTXO, then we hide from the total amount available
            let available_funds_sats = if utxos
                .iter()
                .any(|u| u.amount_millisatoshi == CLN_DUST_LIMIT_SAT * 1_000)
            {
                available_channel_closes_funds.sats
            } else {
                available_channel_closes_funds.sats - CLN_DUST_LIMIT_SAT
            };

            action_required_items.push(ActionRequiredItem::ChannelClosesFundsAvailable {
                available_funds: available_funds_sats
                    .as_sats()
                    .to_amount_down(&self.get_exchange_rate()),
            })
        }

        // TODO: improve ordering of items in the returned vec
        Ok(action_required_items)
    }

    /// Get a list of unclaimed fund offers
    ///
    /// Requires network: **yes**
    pub fn query_uncompleted_offers(&self) -> Result<Vec<OfferInfo>> {
        let topup_infos = self
            .offer_manager
            .query_uncompleted_topups()
            .map_runtime_error_to(RuntimeErrorCode::OfferServiceUnavailable)?;
        let rate = self.get_exchange_rate();

        let list_payments_request = ListPaymentsRequest {
            filters: Some(vec![PaymentTypeFilter::Received]),
            metadata_filters: None,
            from_timestamp: None,
            to_timestamp: None,
            include_failures: Some(false),
            limit: Some(5),
            offset: None,
        };
        let latest_activities = self
            .rt
            .handle()
            .block_on(self.sdk.list_payments(list_payments_request))
            .map_to_runtime_error(RuntimeErrorCode::NodeUnavailable, "Failed to list payments")?
            .into_iter()
            .filter(|p| p.status == PaymentStatus::Complete)
            .map(|p| self.activity_from_breez_payment(p))
            .filter_map(filter_out_and_log_corrupted_activities)
            .collect::<Vec<_>>();

        Ok(
            filter_out_recently_claimed_topups(topup_infos, latest_activities)
                .into_iter()
                .map(|topup_info| OfferInfo::from(topup_info, &rate))
                .collect(),
        )
    }

    /// Calculates the lightning payout fee for an uncompleted offer.
    ///
    /// Parameters:
    /// * `offer` - An uncompleted offer for which the lightning payout fee should get calculated.
    ///
    /// Requires network: **yes**
    pub fn calculate_lightning_payout_fee(&self, offer: OfferInfo) -> Result<Amount> {
        ensure!(
            offer.status != OfferStatus::REFUNDED && offer.status != OfferStatus::SETTLED,
            invalid_input(format!("Provided offer is already completed: {offer:?}"))
        );

        let max_withdrawable_msats = match self.rt.handle().block_on(parse(
            &offer
                .lnurlw
                .ok_or_permanent_failure("Uncompleted offer didn't include an lnurlw")?,
        )) {
            Ok(InputType::LnUrlWithdraw { data }) => data,
            Ok(input_type) => {
                permanent_failure!("Invalid input type LNURLw in uncompleted offer: {input_type:?}")
            }
            Err(err) => {
                permanent_failure!("Invalid LNURLw in uncompleted offer: {err}")
            }
        }
        .max_withdrawable;

        ensure!(
            max_withdrawable_msats <= offer.amount.sats.as_sats().msats,
            permanent_failure("LNURLw provides more")
        );

        let exchange_rate = self.get_exchange_rate();

        Ok((offer.amount.sats.as_sats().msats - max_withdrawable_msats)
            .as_msats()
            .to_amount_up(&exchange_rate))
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
    ///
    /// Requires network: **yes**
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
        let collectable_amount = lnurlw_data.max_withdrawable;
        let hash = match self
            .rt
            .handle()
            .block_on(self.sdk.lnurl_withdraw(LnUrlWithdrawRequest {
                data: lnurlw_data,
                amount_msat: collectable_amount,
                description: None,
            })) {
            Ok(breez_sdk_core::LnUrlWithdrawResult::Ok { data }) => data.invoice.payment_hash,
            Ok(breez_sdk_core::LnUrlWithdrawResult::Timeout { .. }) => runtime_error!(
                RuntimeErrorCode::OfferServiceUnavailable,
                "Failed to withdraw offer due to timeout",
            ),
            Ok(breez_sdk_core::LnUrlWithdrawResult::ErrorStatus { data }) => runtime_error!(
                RuntimeErrorCode::OfferServiceUnavailable,
                "Failed to withdraw offer due to: {}",
                data.reason
            ),
            Err(breez_sdk_core::LnUrlWithdrawError::Generic { err }) => runtime_error!(
                RuntimeErrorCode::OfferServiceUnavailable,
                "Failed to withdraw offer due to: {err}"
            ),
            Err(breez_sdk_core::LnUrlWithdrawError::InvalidAmount { err }) => {
                permanent_failure!("Invalid amount in invoice for LNURL withdraw: {err}")
            }
            Err(breez_sdk_core::LnUrlWithdrawError::InvalidInvoice { err }) => {
                permanent_failure!("Invalid invoice for LNURL withdraw: {err}")
            }
            Err(breez_sdk_core::LnUrlWithdrawError::InvalidUri { err }) => {
                permanent_failure!("Invalid URL in LNURL withdraw: {err}")
            }
            Err(breez_sdk_core::LnUrlWithdrawError::ServiceConnectivity { err }) => {
                runtime_error!(
                    RuntimeErrorCode::OfferServiceUnavailable,
                    "Failed to withdraw offer due to: {err}"
                )
            }
            Err(breez_sdk_core::LnUrlWithdrawError::InvoiceNoRoutingHints { err }) => {
                permanent_failure!(
                    "A locally created invoice doesn't have any routing hints: {err}"
                )
            }
        };

        self.store_payment_info(&hash, Some(offer.offer_kind));

        Ok(hash)
    }

    /// Registers a new notification token. If a token has already been registered, it will be updated.
    ///
    /// Requires network: **yes**
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
    /// If the auth flow has never succeeded in this Auth instance, this method will require network
    /// access.
    ///
    /// Requires network: **yes**
    pub fn get_wallet_pubkey_id(&self) -> Result<String> {
        self.auth.get_wallet_pubkey_id().map_to_runtime_error(
            RuntimeErrorCode::AuthServiceUnavailable,
            "Failed to authenticate in order to get the wallet pubkey id",
        )
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
    pub fn get_payment_uuid(&self, payment_hash: String) -> Result<String> {
        get_payment_uuid(payment_hash)
    }

    fn store_payment_info(&self, hash: &str, offer: Option<OfferKind>) {
        let user_preferences = self.user_preferences.lock_unwrap().clone();
        let exchange_rates = self.task_manager.lock_unwrap().get_exchange_rates();
        self.data_store
            .lock_unwrap()
            .store_payment_info(hash, user_preferences, exchange_rates, offer, None, None)
            .log_ignore_error(Level::Error, "Failed to persist payment info")
    }

    /// Query the current recommended on-chain fee rate.
    ///
    /// This is useful to obtain a fee rate to be used for [`LightningNode::sweep`].
    ///
    /// Requires network: **yes**
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
    /// * `onchain_fee_rate` - the fee rate that should be applied for the transaction.
    /// The recommended on-chain fee rate can be queried using [`LightningNode::query_onchain_fee_rate`]
    ///
    /// Returns information on the prepared sweep, including the exact fee that results from
    /// using the provided fee rate. The method [`LightningNode::sweep`] can be used to broadcast
    /// the sweep transaction.
    ///
    /// Requires network: **yes**
    pub fn prepare_sweep(&self, address: String, onchain_fee_rate: u32) -> Result<SweepInfo> {
        let res = self
            .rt
            .handle()
            .block_on(
                self.sdk
                    .prepare_redeem_onchain_funds(PrepareRedeemOnchainFundsRequest {
                        to_address: address.clone(),
                        sat_per_vbyte: onchain_fee_rate,
                    }),
            )
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to prepare sweep transaction",
            )?;

        let onchain_balance_sat = self
            .sdk
            .node_info()
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to fetch on-chain balance",
            )?
            .onchain_balance_msat
            .as_msats()
            .to_amount_down(&None)
            .sats;

        let rate = self.get_exchange_rate();

        // Add the amount that won't be possible to be swept due to CLN's min-emergency limit (546 sats)
        // TODO: remove CLN_DUST_LIMIT_SAT addition if/when
        //      https://github.com/ElementsProject/lightning/issues/7131 is addressed
        let utxos = self.get_node_utxos()?;
        let onchain_fee_sat = if utxos
            .iter()
            .any(|u| u.amount_millisatoshi == CLN_DUST_LIMIT_SAT * 1_000)
        {
            res.tx_fee_sat
        } else {
            res.tx_fee_sat + CLN_DUST_LIMIT_SAT
        };

        let onchain_fee_amount = onchain_fee_sat.as_sats().to_amount_up(&rate);

        Ok(SweepInfo {
            address,
            onchain_fee_rate,
            onchain_fee_amount,
            amount: (onchain_balance_sat - res.tx_fee_sat)
                .as_sats()
                .to_amount_up(&rate),
        })
    }

    /// Sweeps all available on-chain funds to the specified on-chain address.
    ///
    /// Parameters:
    /// * `sweep_info` - a prepared sweep info that can be obtained using [`LightningNode::prepare_sweep`]
    ///
    /// Returns the txid of the sweep transaction.
    ///
    /// Requires network: **yes**
    pub fn sweep(&self, sweep_info: SweepInfo) -> Result<String> {
        let txid = self
            .rt
            .handle()
            .block_on(self.sdk.redeem_onchain_funds(RedeemOnchainFundsRequest {
                to_address: sweep_info.address,
                sat_per_vbyte: sweep_info.onchain_fee_rate,
            }))
            .map_to_runtime_error(RuntimeErrorCode::NodeUnavailable, "Failed to sweep funds")?
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
    ///
    /// Requires network: **yes**
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
            min_deposit: (swap_info.min_allowed_deposit as u64)
                .as_sats()
                .to_amount_up(&rate),
            max_deposit: (swap_info.max_allowed_deposit as u64)
                .as_sats()
                .to_amount_down(&rate),
            swap_fee: 0_u64.as_sats().to_amount_up(&rate),
        })
    }

    /// Lists all unresolved failed swaps. Each individual failed swap can be recovered
    /// using [`LightningNode::resolve_failed_swap`].
    ///
    /// Requires network: **yes**
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
                address: s.bitcoin_address,
                amount: s
                    .confirmed_sats
                    .as_sats()
                    .to_amount_down(&self.get_exchange_rate()),
                created_at: unix_timestamp_to_system_time(s.created_at as u64),
            })
            .collect())
    }

    /// Prepares the resolution of a failed swap in order to know how much will be recovered and how much
    /// will be paid in on-chain fees.
    ///
    /// Parameters:
    /// * `failed_swap_info` - the failed swap that will be prepared
    /// * `to_address` - the destination address to which funds will be sent
    /// * `onchain_fee_rate` - the fee rate that will be applied. The recommended one can be fetched
    /// using [`LightningNode::query_onchain_fee_rate`]
    ///
    /// Requires network: **yes**
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
    ///
    /// Requires network: **yes**
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

    /// Returns the fees for resolving channel closes.
    ///
    /// Throws an [`RuntimeErrorCode::NoOnChainFundsToResolve`] error if no on-chain funds are available to resolve.
    ///
    /// Returns the fee information for the available resolving options.
    ///
    /// Requires network: **yes**
    pub fn get_channel_close_resolving_fees(&self) -> Result<ChannelCloseResolvingFees> {
        let rate = self.get_exchange_rate();
        let onchain_balance = self
            .sdk
            .node_info()
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Couldn't fetch on-chain balance",
            )?
            .onchain_balance_msat
            .as_msats();

        if onchain_balance.msats == 0 {
            runtime_error!(
                RuntimeErrorCode::NoOnChainFundsToResolve,
                "No on-chain funds to resolve"
            )
        }

        let lsp_fees =
            self.calculate_lsp_fee(onchain_balance.msats.as_msats().to_amount_up(&None).sats)?;

        let swap_info = self
            .rt
            .handle()
            .block_on(self.sdk.receive_onchain(ReceiveOnchainRequest {
                opening_fee_params: lsp_fees.lsp_fee_params,
            }))
            .ok();

        let sat_per_vbyte = self.query_onchain_fee_rate()?;

        let prepared_sweep = self.prepare_sweep(
            swap_info
                .clone()
                .map(|s| s.bitcoin_address)
                .unwrap_or("1BitcoinEaterAddressDontSendf59kuE".to_string()),
            sat_per_vbyte,
        )?;

        if swap_info.is_none()
            || prepared_sweep.amount.sats < (swap_info.clone().unwrap().min_allowed_deposit as u64)
            || prepared_sweep.amount.sats > (swap_info.clone().unwrap().max_allowed_deposit as u64)
        {
            return Ok(ChannelCloseResolvingFees {
                swap_fees: None,
                sweep_onchain_fee_estimate: prepared_sweep.onchain_fee_amount,
                sat_per_vbyte,
            });
        }

        let lsp_fees = self.calculate_lsp_fee(prepared_sweep.amount.sats)?;

        let swap_fee = 0_u64.as_sats();
        let swap_to_lightning_fees = SwapToLightningFees {
            swap_fee: swap_fee.sats.as_sats().to_amount_up(&rate),
            onchain_fee: prepared_sweep.clone().onchain_fee_amount,
            channel_opening_fee: lsp_fees.lsp_fee.clone(),
            total_fees: (swap_fee.sats
                + prepared_sweep.onchain_fee_amount.sats
                + lsp_fees.lsp_fee.sats)
                .as_sats()
                .to_amount_up(&rate),
            lsp_fee_params: lsp_fees.lsp_fee_params,
        };

        Ok(ChannelCloseResolvingFees {
            swap_fees: Some(swap_to_lightning_fees),
            sweep_onchain_fee_estimate: prepared_sweep.onchain_fee_amount,
            sat_per_vbyte,
        })
    }

    /// Automatically swaps on-chain funds back to lightning.
    ///
    /// If a swap is in progress, this method will return an error.
    ///
    /// If the current balance doesn't fulfill the limits, this method will return an error.
    /// Before using this method use [`LightningNode::get_channel_close_resolving_fees`] to validate a swap is available.
    ///
    /// Parameters:
    /// * `sat_per_vbyte` - the fee rate to use for the on-chain transaction.
    /// Can be obtained with [`LightningNode::get_channel_close_resolving_fees`].
    /// * `lsp_fee_params` - the lsp fee params for opening a new channel if necessary.
    /// Can be obtained with [`LightningNode::get_channel_close_resolving_fees`].
    ///
    /// Returns the txid of the sweeping tx.
    ///
    /// Requires network: **yes**
    pub fn swap_onchain_to_lightning(
        &self,
        sat_per_vbyte: u32,
        lsp_fee_params: Option<OpeningFeeParams>,
    ) -> std::result::Result<String, ReceiveOnchainError> {
        let onchain_balance = self.sdk.node_info()?.onchain_balance_msat.as_msats();

        let swap_address_info = self.generate_swap_address(lsp_fee_params)?;

        if swap_address_info.min_deposit.sats.as_sats().msats > onchain_balance.msats {
            return Err(ReceiveOnchainError::Generic {
                err: format!(
                    "Not enough funds ({} msats) available for swap ({} msats)",
                    onchain_balance.msats,
                    swap_address_info.min_deposit.sats.as_sats().msats,
                ),
            });
        }

        if swap_address_info.max_deposit.sats.as_sats().msats < onchain_balance.msats {
            return Err(ReceiveOnchainError::Generic {
                err: format!(
                    "Available funds ({} msats) exceed limit for swap ({} msats)",
                    onchain_balance.msats,
                    swap_address_info.max_deposit.sats.as_sats().msats,
                ),
            });
        }

        let sweep_result = self.rt.handle().block_on(self.sdk.redeem_onchain_funds(
            RedeemOnchainFundsRequest {
                to_address: swap_address_info.address,
                sat_per_vbyte,
            },
        ))?;

        Ok(hex::encode(sweep_result.txid))
    }

    /// Prints additional debug information to the logs.
    ///
    /// Throws an error in case that the necessary information can't be retrieved.
    ///
    /// Requires network: **yes**
    pub fn log_debug_info(&self) -> Result<()> {
        self.rt
            .handle()
            .block_on(self.sdk.sync())
            .log_ignore_error(Level::Error, "Failed to sync node");

        let available_lsps = self
            .rt
            .handle()
            .block_on(self.sdk.list_lsps())
            .map_to_runtime_error(RuntimeErrorCode::NodeUnavailable, "Couldn't list lsps")?;

        let connected_lsp = self
            .rt
            .handle()
            .block_on(self.sdk.lsp_id())
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to get current lsp id",
            )?
            .unwrap_or("<no connection>".to_string());

        let node_state = self.sdk.node_info().map_to_runtime_error(
            RuntimeErrorCode::NodeUnavailable,
            "Failed to read node info",
        )?;

        let channels = self
            .rt
            .handle()
            .block_on(self.sdk.execute_dev_command("listpeerchannels".to_string()))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Couldn't execute `listpeerchannels` command",
            )?;

        let payments = self
            .rt
            .handle()
            .block_on(self.sdk.execute_dev_command("listpayments".to_string()))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Couldn't execute `listpayments` command",
            )?;

        let diagnostics = self
            .rt
            .handle()
            .block_on(self.sdk.generate_diagnostic_data())
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Couldn't call generate_diagnostic_data",
            )?;

        info!("3L version: {}", env!("GITHUB_REF"));
        info!("Wallet pubkey id: {:?}", self.get_wallet_pubkey_id());
        // Print connected peers, balances, inbound/outbound capacities, on-chain funds.
        info!("Node state:\n{node_state:?}");
        info!(
            "List of available lsps:\n{}",
            replace_byte_arrays_by_hex_string(&format!("{available_lsps:?}"))
        );
        info!("Connected lsp id: {connected_lsp}");
        info!(
            "List of peer channels:\n{}",
            replace_byte_arrays_by_hex_string(&channels)
        );
        info!(
            "List of payments:\n{}",
            replace_byte_arrays_by_hex_string(&payments)
        );
        info!("Diagnostic data:\n{diagnostics}");
        Ok(())
    }

    /// Returns the latest [`FiatTopupInfo`] if the user has registered for the fiat topup.
    ///
    /// Requires network: **no**
    pub fn retrieve_latest_fiat_topup_info(&self) -> Result<Option<FiatTopupInfo>> {
        self.data_store
            .lock_unwrap()
            .retrieve_latest_fiat_topup_info()
    }

    /// Returns the health check status of Breez and Greenlight services.
    ///
    /// Requires network: **yes**
    pub fn get_health_status(&self) -> Result<BreezHealthCheckStatus> {
        Ok(self
            .rt
            .handle()
            .block_on(BreezServices::service_health_check(
                env!("BREEZ_SDK_API_KEY").to_string(),
            ))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to get health status",
            )?
            .status)
    }

    /// Check if clearing the wallet is feasible.
    ///
    /// Meaning that the balance is within the range of what can be reverse-swapped.
    ///
    /// Requires network: **yes**
    pub fn check_clear_wallet_feasibility(&self) -> Result<RangeHit> {
        let limits = self
            .rt
            .handle()
            .block_on(
                self.sdk
                    .fetch_reverse_swap_fees(ReverseSwapFeesRequest::default()),
            )
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to fetch reverse swap fees",
            )?;
        let balance_sat = self
            .sdk
            .node_info()
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to read node info",
            )?
            .channels_balance_msat
            .as_msats()
            .sats_round_down()
            .sats;
        let exchange_rate = self.get_exchange_rate();

        // Accomodating lightning network routing fees.
        let routing_fee = MAX_FEE_PERMYRIAD.of(&limits.min.as_sats()).sats_round_up();
        let min = limits.min + routing_fee.sats;
        let range_hit = match balance_sat {
            balance_sat if balance_sat < min => RangeHit::Below {
                min: min.as_sats().to_amount_up(&exchange_rate),
            },
            balance_sat if balance_sat <= limits.max => RangeHit::In,
            balance_sat if limits.max < balance_sat => RangeHit::Above {
                max: limits.max.as_sats().to_amount_down(&exchange_rate),
            },
            _ => permanent_failure!("Unreachable code in check_clear_wallet_feasibility()"),
        };
        Ok(range_hit)
    }

    /// Prepares a reverse swap that sends all funds in LN channels. This is possible because the
    /// route to the swap service is known, so fees can be known in advance.
    ///
    /// This can fail if the balance is either too low or too high for it to be reverse-swapped.
    /// The method [`LightningNode::check_clear_wallet_feasibility`] can be used to check if the balance
    /// is within the required range.
    ///
    /// Requires network: **yes**
    pub fn prepare_clear_wallet(&self) -> Result<ClearWalletInfo> {
        let claim_tx_feerate = self.query_onchain_fee_rate()?;
        let limits = self
            .rt
            .handle()
            .block_on(self.sdk.onchain_payment_limits())
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to get on-chain payment limits",
            )?;
        let prepare_response = self
            .rt
            .handle()
            .block_on(
                self.sdk
                    .prepare_onchain_payment(PrepareOnchainPaymentRequest {
                        amount_sat: limits.max_sat,
                        amount_type: breez_sdk_core::SwapAmountType::Send,
                        claim_tx_feerate,
                    }),
            )
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to prepare on-chain payment",
            )?;

        let total_fees_sat = prepare_response.total_fees;
        let onchain_fee_sat = prepare_response.fees_claim + prepare_response.fees_lockup;
        let swap_fee_sat = total_fees_sat - onchain_fee_sat;
        let exchange_rate = self.get_exchange_rate();

        Ok(ClearWalletInfo {
            clear_amount: prepare_response
                .sender_amount_sat
                .as_sats()
                .to_amount_up(&exchange_rate),
            total_estimated_fees: total_fees_sat.as_sats().to_amount_up(&exchange_rate),
            onchain_fee: onchain_fee_sat.as_sats().to_amount_up(&exchange_rate),
            swap_fee: swap_fee_sat.as_sats().to_amount_up(&exchange_rate),
            prepare_response,
        })
    }

    /// Starts a reverse swap that sends all funds in LN channels to the provided on-chain address.
    ///
    /// Parameters:
    /// * `clear_wallet_info` - An instance of [`ClearWalletInfo`] obtained using
    /// [`LightningNode::prepare_clear_wallet`].
    /// * `destination_onchain_address_data` - An on-chain address data instance. Can be obtained
    /// using [`LightningNode::decode_data`].
    ///
    /// Requires network: **yes**
    pub fn clear_wallet(
        &self,
        clear_wallet_info: ClearWalletInfo,
        destination_onchain_address_data: BitcoinAddressData,
    ) -> Result<()> {
        self.rt
            .handle()
            .block_on(self.sdk.pay_onchain(PayOnchainRequest {
                recipient_address: destination_onchain_address_data.address,
                prepare_res: clear_wallet_info.prepare_response,
            }))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to start reverse swap",
            )?;
        Ok(())
    }

    /// Set the analytics configuration.
    ///
    /// This can be used to completely prevent any analytics data from being reported.
    ///
    /// Requires network: **no**
    pub fn set_analytics_config(&self, config: AnalyticsConfig) -> Result<()> {
        *self.analytics_interceptor.config.lock_unwrap() = config.clone();
        self.data_store
            .lock_unwrap()
            .append_analytics_config(config)
    }

    /// Get the currently configured analytics configuration.
    ///
    /// Requires network: **no**
    pub fn get_analytics_config(&self) -> Result<AnalyticsConfig> {
        self.data_store.lock_unwrap().retrieve_analytics_config()
    }

    /// Register a human-readable lightning address or return the previously
    /// registered one.
    ///
    /// Requires network: **yes**
    pub fn register_lightning_address(&self) -> Result<String> {
        let address = self
            .rt
            .handle()
            .block_on(pigeon::assign_lightning_address(
                &self.environment.backend_url,
                &self.async_auth,
            ))
            .map_to_runtime_error(
                RuntimeErrorCode::AuthServiceUnavailable,
                "Failed to register a lightning address",
            )?;
        self.data_store
            .lock_unwrap()
            .store_lightning_address(&address)?;
        Ok(address)
    }

    /// Query the registered lightning address.
    ///
    /// Requires network: **no**
    pub fn query_lightning_address(&self) -> Result<Option<String>> {
        let addresses = self
            .data_store
            .lock_unwrap()
            .retrieve_lightning_addresses()?;
        Ok(addresses.into_iter().next())
    }

    /// Query for a previously verified phone number.
    ///
    /// Requires network: **yes**
    pub fn query_verified_phone_number(&self) -> Result<Option<String>> {
        self.rt
            .handle()
            .block_on(pigeon::query_verified_phone_number(
                &self.environment.backend_url,
                &self.async_auth,
            ))
            .map_to_runtime_error(
                RuntimeErrorCode::AuthServiceUnavailable,
                "Failed to query verified phone number",
            )
    }

    /// Start the verification process for a new phone number. This will trigger an SMS containing
    /// an OTP to be sent to the provided `phone_number`. To conclude the verification process,
    /// the method [`LightningNode::verify_phone_number`] should be called next.
    ///
    /// Parameters:
    /// * `phone_number` - the phone number to be registered. Needs to be checked for validity using
    /// [LightningNode::parse_phone_number_to_lightning_address].
    ///
    /// Requires network: **yes**
    pub fn request_phone_number_verification(&self, phone_number: String) -> Result<()> {
        let phone_number =
            PhoneNumber::parse(&phone_number).map_to_invalid_input("Invalid phone number")?;

        self.rt
            .handle()
            .block_on(pigeon::request_phone_number_verification(
                &self.environment.backend_url,
                &self.async_auth,
                phone_number.e164,
            ))
            .map_to_runtime_error(
                RuntimeErrorCode::AuthServiceUnavailable,
                "Failed to register phone number",
            )
    }

    /// Finish the verification process for a new phone number.
    ///
    /// Parameters:
    /// * `phone_number` - the phone number to be verified.
    /// * `otp` - the OTP code sent as an SMS to the phone number.
    ///
    /// Requires network: **yes**
    pub fn verify_phone_number(&self, phone_number: String, otp: String) -> Result<()> {
        let phone_number =
            PhoneNumber::parse(&phone_number).map_to_invalid_input("Invalid phone number")?;

        self.rt
            .handle()
            .block_on(pigeon::verify_phone_number(
                &self.environment.backend_url,
                &self.async_auth,
                phone_number.e164,
                otp,
            ))
            .map_to_runtime_error(
                RuntimeErrorCode::AuthServiceUnavailable,
                "Failed to submit phone number registration otp",
            )
    }

    fn report_send_payment_issue(&self, payment_hash: String) {
        debug!("Reporting failure of payment: {payment_hash}");
        let data = ReportPaymentFailureDetails {
            payment_hash,
            comment: None,
        };
        let request = ReportIssueRequest::PaymentFailure { data };
        self.rt
            .handle()
            .block_on(self.sdk.report_issue(request))
            .log_ignore_error(Level::Warn, "Failed to report issue");
    }

    fn get_node_utxos(&self) -> Result<Vec<UnspentTransactionOutput>> {
        let node_state = self
            .sdk
            .node_info()
            .map_to_runtime_error(RuntimeErrorCode::NodeUnavailable, "Couldn't get node info")?;

        Ok(node_state.utxos)
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
    config: &Config,
    environment: &Environment,
    event_listener: Box<dyn EventListener>,
) -> Result<Arc<BreezServices>> {
    let developer_cert = env!("BREEZ_SDK_PARTNER_CERTIFICATE").as_bytes().to_vec();
    let developer_key = env!("BREEZ_SDK_PARTNER_KEY").as_bytes().to_vec();
    let partner_credentials = GreenlightCredentials {
        developer_cert,
        developer_key,
    };

    let mut breez_config = BreezServices::default_config(
        environment.environment_type.clone(),
        env!("BREEZ_SDK_API_KEY").to_string(),
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
    breez_config.exemptfee_msat = EXEMPT_FEE.msats;
    breez_config.maxfee_percent = MAX_FEE_PERMYRIAD.to_percentage();
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
/// * `environment` - the [`EnvironmentCode`] of the intended environment.
/// * `seed` - the seed from the wallet for which the T&C will be accepted.
/// * `version` - the version number being accepted.
///
/// Requires network: **yes**
pub fn accept_terms_and_conditions(
    environment: EnvironmentCode,
    seed: Vec<u8>,
    version: i64,
) -> Result<()> {
    enable_backtrace();
    let environment = Environment::load(environment)?;
    let seed = sanitize_input::strong_type_seed(&seed)?;
    let auth = build_auth(&seed, &environment.backend_url)?;
    auth.accept_terms_and_conditions(TermsAndConditions::Lipa, version)
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
    environment: EnvironmentCode,
    seed: Vec<u8>,
    terms_and_conditions: TermsAndConditions,
) -> Result<TermsAndConditionsStatus> {
    enable_backtrace();
    let environment = Environment::load(environment)?;
    let seed = sanitize_input::strong_type_seed(&seed)?;
    let auth = build_auth(&seed, &environment.backend_url)?;
    auth.get_terms_and_conditions_status(terms_and_conditions)
        .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)
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
    let relative_fee = MAX_FEE_PERMYRIAD.of(&amount_sat.as_sats());
    if relative_fee.msats < EXEMPT_FEE.msats {
        MaxRoutingFeeMode::Absolute {
            max_fee_amount: EXEMPT_FEE.to_amount_up(exchange_rate),
        }
    } else {
        MaxRoutingFeeMode::Relative {
            max_fee_permyriad: MAX_FEE_PERMYRIAD.0,
        }
    }
}

fn filter_out_recently_claimed_topups(
    topups: Vec<TopupInfo>,
    latest_activities: Vec<Activity>,
) -> Vec<TopupInfo> {
    let pocket_id = |a: Activity| match a {
        Activity::OfferClaim {
            incoming_payment_info: _,
            offer_kind: OfferKind::Pocket { id, .. },
        } => Some(id),
        _ => None,
    };
    let latest_succeeded_payment_offer_ids: HashSet<String> = latest_activities
        .into_iter()
        .filter(|a| a.get_payment_info().map(|p| p.payment_state) == Some(PaymentState::Succeeded))
        .filter_map(pocket_id)
        .collect();
    topups
        .into_iter()
        .filter(|o| !latest_succeeded_payment_offer_ids.contains(&o.id))
        .collect()
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
    environment: &Environment,
) -> Result<()> {
    let id = auth.get_wallet_pubkey_id().map_to_runtime_error(
        RuntimeErrorCode::AuthServiceUnavailable,
        "Failed to authenticate in order to get wallet pubkey id",
    )?;
    let encrypted_id =
        deterministic_encrypt(id.as_bytes(), &environment.notification_webhook_secret)
            .map_to_permanent_failure("Failed to encrypt wallet pubkey id")?;
    let encrypted_id = hex::encode(encrypted_id);
    let webhook_url = environment
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

include!(concat!(env!("OUT_DIR"), "/lipalightninglib.uniffi.rs"));

#[cfg(test)]
mod tests {
    use super::*;
    use crow::TopupStatus;
    use perro::Error;

    const PAYMENT_HASH: &str = "0b78877a596f18d5f6effde3dda1df25a5cf20439ff1ac91478d7e518211040f";
    const PAYMENT_UUID: &str = "c6e597bd-0a98-5b46-8e74-f6098f5d16a3";

    #[test]
    fn test_payment_uuid() {
        let payment_uuid = get_payment_uuid(PAYMENT_HASH.to_string());

        assert_eq!(payment_uuid, Ok(PAYMENT_UUID.to_string()));
    }

    #[test]
    fn test_payment_uuid_invalid_input() {
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
    fn test_get_payment_max_routing_fee_mode_absolute() {
        let max_routing_mode = get_payment_max_routing_fee_mode(
            EXEMPT_FEE.msats / ((MAX_FEE_PERMYRIAD.0 as u64) / 10) - 1,
            &None,
        );

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
            EXEMPT_FEE.msats / ((MAX_FEE_PERMYRIAD.0 as u64) / 10),
            &None,
        );

        match max_routing_mode {
            MaxRoutingFeeMode::Relative { max_fee_permyriad } => {
                assert_eq!(max_fee_permyriad, MAX_FEE_PERMYRIAD.0);
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

        let mut payment_info = PaymentInfo {
            payment_state: PaymentState::Succeeded,
            hash: "hash".to_string(),
            amount: Amount::default(),
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
            created_at: SystemTime::now().with_timezone(TzConfig::default()),
            description: "".to_string(),
            preimage: None,
            personal_note: None,
        };

        let incoming_payment = Activity::IncomingPayment {
            incoming_payment_info: IncomingPaymentInfo {
                payment_info: payment_info.clone(),
                requested_amount: Amount::default(),
                lsp_fees: Amount::default(),
                received_on: None,
                received_lnurl_comment: None,
            },
        };

        payment_info.hash = "hash2".to_string();
        let topup = Activity::OfferClaim {
            incoming_payment_info: IncomingPaymentInfo {
                payment_info: payment_info.clone(),
                requested_amount: Amount::default(),
                lsp_fees: Amount::default(),
                received_on: None,
                received_lnurl_comment: None,
            },
            offer_kind: OfferKind::Pocket {
                id: "123".to_string(),
                exchange_rate: ExchangeRate {
                    currency_code: "".to_string(),
                    rate: 0,
                    updated_at: SystemTime::now(),
                },
                topup_value_minor_units: 0,
                topup_value_sats: Some(0),
                exchange_fee_minor_units: 0,
                exchange_fee_rate_permyriad: 0,
                lightning_payout_fee: None,
                error: None,
            },
        };

        payment_info.hash = "hash3".to_string();
        payment_info.payment_state = PaymentState::Failed;
        let failed_topup = Activity::OfferClaim {
            incoming_payment_info: IncomingPaymentInfo {
                payment_info,
                requested_amount: Amount::default(),
                lsp_fees: Amount::default(),
                received_on: None,
                received_lnurl_comment: None,
            },
            offer_kind: OfferKind::Pocket {
                id: "234".to_string(),
                exchange_rate: ExchangeRate {
                    currency_code: "".to_string(),
                    rate: 0,
                    updated_at: SystemTime::now(),
                },
                topup_value_minor_units: 0,
                topup_value_sats: Some(0),
                exchange_fee_minor_units: 0,
                exchange_fee_rate_permyriad: 0,
                lightning_payout_fee: None,
                error: None,
            },
        };
        let latest_payments = vec![incoming_payment, topup, failed_topup];

        let filtered_topups = filter_out_recently_claimed_topups(topups, latest_payments);

        assert_eq!(filtered_topups.len(), 1);
        assert_eq!(filtered_topups.first().unwrap().id, "234");
    }
}
