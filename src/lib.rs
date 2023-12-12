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
mod auth;
mod backup;
mod callbacks;
mod config;
mod data_store;
mod environment;
mod errors;
mod event;
mod exchange_rate_provider;
mod fiat_topup;
mod fund_migration;
mod invoice_details;
mod key_derivation;
mod limits;
mod lnurl;
mod locker;
mod logger;
mod migrations;
mod offer;
mod payment;
mod random;
mod recovery;
mod sanitize_input;
mod secret;
mod swap;
mod symmetric_encryption;
mod task_manager;
mod util;

pub use crate::amount::{Amount, FiatValue};
use crate::amount::{AsSats, Sats, ToAmount};
use crate::analytics::{derive_analytics_keys, AnalyticsInterceptor};
pub use crate::analytics::{InvoiceCreationMetadata, PaymentMetadata};
use crate::async_runtime::AsyncRuntime;
use crate::auth::{build_async_auth, build_auth};
use crate::backup::BackupManager;
pub use crate::callbacks::EventsCallback;
pub use crate::config::{Config, TzConfig, TzTime};
use crate::data_store::CreatedInvoice;
use crate::environment::Environment;
pub use crate::environment::EnvironmentCode;
use crate::errors::{
    map_lnurl_pay_error, map_lnurl_withdraw_error, map_send_payment_error, LnUrlWithdrawErrorCode,
    LnUrlWithdrawResult,
};
pub use crate::errors::{
    DecodeDataError, Error as LnError, LnUrlPayError, LnUrlPayErrorCode, LnUrlPayResult,
    MnemonicError, PayError, PayErrorCode, PayResult, Result, RuntimeErrorCode, SimpleError,
    UnsupportedDataType,
};
use crate::event::LipaEventListener;
pub use crate::exchange_rate_provider::ExchangeRate;
use crate::exchange_rate_provider::ExchangeRateProviderImpl;
pub use crate::fiat_topup::FiatTopupInfo;
use crate::fiat_topup::PocketClient;
pub use crate::invoice_details::InvoiceDetails;
use crate::key_derivation::derive_persistence_encryption_key;
pub use crate::limits::{LiquidityLimit, PaymentAmountLimits};
pub use crate::lnurl::{LnUrlPayDetails, LnUrlWithdrawDetails};
use crate::locker::Locker;
pub use crate::offer::{OfferInfo, OfferKind, OfferStatus};
pub use crate::payment::{Payment, PaymentState, PaymentType};
pub use crate::recovery::recover_lightning_node;
pub use crate::secret::{generate_secret, mnemonic_to_secret, words_by_prefix, Secret};
pub use crate::swap::{FailedSwapInfo, ResolveFailedSwapInfo, SwapAddressInfo, SwapInfo};
use crate::task_manager::TaskManager;
use crate::util::unix_timestamp_to_system_time;
use crate::util::LogIgnoreError;

pub use breez_sdk_core::error::ReceiveOnchainError as SwapError;
use breez_sdk_core::error::{LnUrlWithdrawError, ReceiveOnchainError, SendPaymentError};
use breez_sdk_core::{
    parse, parse_invoice, BreezServices, GreenlightCredentials, GreenlightNodeConfig, InputType,
    ListPaymentsRequest, LnUrlPayRequest, LnUrlPayRequestData, LnUrlWithdrawRequest,
    LnUrlWithdrawRequestData, NodeConfig, OpenChannelFeeRequest, OpeningFeeParams, PaymentDetails,
    PaymentTypeFilter, PrepareRefundRequest, PrepareSweepRequest, ReceiveOnchainRequest,
    RefundRequest, ReportIssueRequest, ReportPaymentFailureDetails, SendPaymentRequest,
    SweepRequest,
};
use crow::{CountryCode, LanguageCode, OfferManager, TopupError, TopupInfo};
pub use crow::{PermanentFailureCode, TemporaryFailureCode};
use data_store::DataStore;
use email_address::EmailAddress;
use honey_badger::{Auth, TermsAndConditions, TermsAndConditionsStatus};
use iban::Iban;
use log::{debug, info, Level};
use logger::init_logger_once;
use parrot::AnalyticsClient;
pub use parrot::PaymentSource;
use perro::{
    ensure, invalid_input, permanent_failure, runtime_error, MapToError, OptionToError, ResultTrait,
};
use squirrel::RemoteBackupClient;
use std::cmp::Reverse;
use std::collections::HashSet;
use std::ops::Not;
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use std::{env, fs};
use uuid::Uuid;

const LOG_LEVEL: Level = Level::Debug;
const LOGS_DIR: &str = "logs";

pub(crate) const DB_FILENAME: &str = "db2.db3";

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

pub struct SweepInfo {
    pub address: String,
    pub onchain_fee_rate: u32,
    pub onchain_fee_sat: Amount,
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
}

const MAX_FEE_PERMYRIAD: u16 = 50;
const EXEMPT_FEE: Sats = Sats::new(21);

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
    environment: Environment,
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
        info!("3L version: {}", env!("GITHUB_REF"));

        let rt = AsyncRuntime::new()?;

        let environment = Environment::load(config.environment);

        let strong_typed_seed = sanitize_input::strong_type_seed(&config.seed)?;
        let auth = Arc::new(build_auth(
            &strong_typed_seed,
            environment.backend_url.clone(),
        )?);
        let async_auth = Arc::new(build_async_auth(
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
            environment.environment_type.clone(),
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

        let analytics_client = AnalyticsClient::new(
            environment.backend_url.clone(),
            derive_analytics_keys(&strong_typed_seed)?,
            Arc::clone(&async_auth),
        );

        let analytics_interceptor = Arc::new(AnalyticsInterceptor::new(
            analytics_client,
            Arc::clone(&user_preferences),
            rt.handle(),
        ));

        let event_listener = Box::new(LipaEventListener::new(
            events_callback,
            Arc::clone(&analytics_interceptor),
        ));

        let sdk = rt
            .handle()
            .block_on(BreezServices::connect(
                breez_config,
                config.seed.clone(),
                event_listener,
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
            Ok::<(), LnError>(())
        })?;

        let exchange_rate_provider = Box::new(ExchangeRateProviderImpl::new(
            environment.backend_url.clone(),
            Arc::clone(&auth),
        ));

        let offer_manager = OfferManager::new(environment.backend_url.clone(), Arc::clone(&auth));

        let db_path = format!("{}/{DB_FILENAME}", config.local_persistence_path);

        let data_store = Arc::new(Mutex::new(DataStore::new(&db_path)?));

        let fiat_topup_client = PocketClient::new(
            environment.pocket_url.clone(),
            Arc::clone(&sdk),
            rt.handle(),
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
            environment,
        })
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

        self.store_payment_info(&response.ln_invoice.payment_hash, None);
        self.data_store
            .lock_unwrap()
            .store_created_invoice(
                &response.ln_invoice.payment_hash,
                &response.ln_invoice.bolt11,
                &response.opening_fee_msat,
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
            breez_sdk_core::LnUrlPayResult::PayError { data } => {
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

    /// List lightning addresses from the most recent used.
    ///
    /// Returns a list of lightning addresses.
    pub fn list_lightning_addresses(&self) -> Result<Vec<String>> {
        let list_payments_request = ListPaymentsRequest {
            filters: Some(vec![PaymentTypeFilter::Sent]),
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
        Ok(lightning_addresses.into_iter().map(|p| p.0).collect())
    }

    /// Withdraw an LNURL-withdraw the provided amount.
    ///
    /// Parameters:
    /// * `lnurl_withdraw_request_data` - LNURL-withdraw request data as obtained from [`LightningNode::decode_data`]
    /// * `amount_sat` - amount to be withdraw
    ///
    /// Returns the payment hash of the payment.
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
                description: Some("LNURL Withdrawal".into()),
            }))
            .map_err(map_lnurl_withdraw_error)?
        {
            breez_sdk_core::LnUrlWithdrawResult::Ok { data } => Ok(data.invoice.payment_hash),
            breez_sdk_core::LnUrlWithdrawResult::ErrorStatus { data } => runtime_error!(
                LnUrlWithdrawErrorCode::LnUrlServerError,
                "LNURL server returned error: {}",
                data.reason
            ),
        }?;
        self.store_payment_info(&payment_hash, None);
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

        let breez_payment_hashes: HashSet<String> = breez_payments
            .iter()
            .map(|p| p.invoice_details.payment_hash.clone())
            .collect();
        let created_invoices = self
            .data_store
            .lock_unwrap()
            .retrieve_created_invoices(number_of_payments)?;
        let mut pending_inbound_payments = created_invoices
            .into_iter()
            .filter(|i| !breez_payment_hashes.contains(i.hash.as_str()))
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
        let optional_invoice = self
            .data_store
            .lock_unwrap()
            .retrieve_created_invoice_by_hash(&hash)?;
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
        } else if let Some(invoice) = optional_invoice {
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

                let offer = d
                    .offer
                    .clone()
                    .as_mut()
                    .map(|o| match o {
                        OfferKind::Pocket {
                            ref mut lightning_payout_fee,
                            topup_value_sats,
                            ..
                        } => {
                            if let Some(a) = invoice_details.amount {
                                *lightning_payout_fee = Some(
                                    (*topup_value_sats - a.sats)
                                        .as_sats()
                                        .to_amount_up(&exchange_rate),
                                );
                            }
                            o
                        }
                    })
                    .cloned();

                (exchange_rate, time, offer)
            }
        };

        let (payment_type, amount, requested_amount, network_fees, lsp_fees) =
            match breez_payment.payment_type {
                breez_sdk_core::PaymentType::Sent => (
                    PaymentType::Sending,
                    (breez_payment.amount_msat + breez_payment.fee_msat)
                        .as_msats()
                        .to_amount_up(&exchange_rate),
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
                    (breez_payment.amount_msat - breez_payment.fee_msat)
                        .as_msats()
                        .to_amount_down(&exchange_rate),
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
            payment_state: breez_payment.status.into(),
            fail_reason: None, // TODO: Request SDK to store and provide reason for failed payments - issue: https://github.com/breez/breez-sdk/issues/626
            hash: payment_details.payment_hash,
            amount,
            requested_amount,
            invoice_details,
            created_at: time,
            description,
            preimage: payment_details
                .payment_preimage
                .is_empty()
                .not()
                .then_some(payment_details.payment_preimage),
            network_fees,
            lsp_fees,
            offer,
            swap,
            lightning_address: payment_details.ln_address,
        })
    }

    fn payment_from_created_invoice(&self, created_invoice: &CreatedInvoice) -> Result<Payment> {
        let invoice = parse_invoice(created_invoice.invoice.as_str())
            .map_to_permanent_failure("Invalid invoice provided by the Breez SDK")?;
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
        let lsp_fees = created_invoice
            .channel_opening_fees
            .map(|f| f.as_msats().to_amount_up(&exchange_rate));
        let amount = invoice_details
            .amount
            .clone()
            .ok_or_permanent_failure("Locally created invoice doesn't include an amount")?
            .sats
            .as_sats()
            .to_amount_down(&exchange_rate);

        Ok(Payment {
            payment_type: PaymentType::Receiving,
            payment_state,
            fail_reason: None,
            hash: invoice_details.payment_hash.clone(),
            amount: amount.clone(),
            requested_amount: amount,
            invoice_details: invoice_details.clone(),
            created_at: time,
            description: invoice_details.description,
            preimage: None,
            network_fees: None,
            lsp_fees,
            offer: None,
            swap: None,
            lightning_address: None,
        })
    }

    /// Call the method when the app goes to foreground, such that the user can interact with it.
    /// The library starts running the background tasks more frequently to improve user experience.
    pub fn foreground(&self) {
        self.task_manager.lock_unwrap().foreground();
    }

    /// Call the method when the app goes to background, such that the user can not interact with it.
    /// The library stops running some unnecessary tasks and runs necessary tasks less frequently.
    /// It should save battery and internet traffic.
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
            .accept_terms_and_conditions(TermsAndConditions::Pocket)
            .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)
    }

    /// Similar to [`get_terms_and_conditions_status`] with the difference that this method is pre-filling
    /// the environment and seed based on the node configuration.
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

    pub fn reset_fiat_topup(&self) -> Result<()> {
        self.data_store.lock_unwrap().clear_fiat_topup_info()
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
                .map(|topup_info| OfferInfo::from(topup_info, &rate))
                .collect(),
        )
    }

    /// Calculates the lightning payout fee for an uncompleted offer.
    ///
    /// Parameters:
    /// * `offer` - An uncompleted offer for which the lightning payout fee should get calculated.
    pub fn calculate_lightning_payout_fee(&self, offer: OfferInfo) -> Result<Amount> {
        ensure!(
            offer.status == OfferStatus::REFUNDED || offer.status == OfferStatus::SETTLED,
            invalid_input(format!("Provided offer is already completed: {:?}", offer))
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
            Ok(breez_sdk_core::LnUrlWithdrawResult::Ok { data }) => data.invoice.payment_hash,
            Ok(breez_sdk_core::LnUrlWithdrawResult::ErrorStatus { data }) => runtime_error!(
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

        self.store_payment_info(&hash, Some(offer.offer_kind));

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

    fn store_payment_info(&self, hash: &str, offer: Option<OfferKind>) {
        let user_preferences = self.user_preferences.lock_unwrap().clone();
        let exchange_rates = self.task_manager.lock_unwrap().get_exchange_rates();
        self.data_store
            .lock_unwrap()
            .store_payment_info(hash, user_preferences, exchange_rates, offer)
            .log_ignore_error(Level::Error, "Failed to persist payment info")
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
    pub fn prepare_sweep(&self, address: String, onchain_fee_rate: u32) -> Result<SweepInfo> {
        let res = self
            .rt
            .handle()
            .block_on(self.sdk.prepare_sweep(PrepareSweepRequest {
                to_address: address.clone(),
                sat_per_vbyte: onchain_fee_rate as u64,
            }))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to prepare sweep transaction",
            )?;
        Ok(SweepInfo {
            address,
            onchain_fee_rate,
            onchain_fee_sat: res
                .sweep_tx_fee_sat
                .as_sats()
                .to_amount_up(&self.get_exchange_rate()),
        })
    }

    /// Sweeps all available on-chain funds to the specified on-chain address.
    ///
    /// Parameters:
    /// * `sweep_info` - a prepared sweep info that can be obtained using [`LightningNode::prepare_sweep`]
    ///
    /// Returns the txid of the sweep transaction.
    pub fn sweep(&self, sweep_info: SweepInfo) -> Result<String> {
        let txid = self
            .rt
            .handle()
            .block_on(self.sdk.sweep(SweepRequest {
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

        info!("3L version: {}", env!("GITHUB_REF"));
        info!("Wallet pubkey id: {:?}", self.get_wallet_pubkey_id());
        // Print connected peers, balances, inbound/outbound capacities, on-chain funds.
        info!("Node state:\n{node_state:?}");
        info!("List of available lsps:\n{available_lsps:?}");
        info!("Connected lsp id: {connected_lsp}");
        info!("List of peer channels:\n{channels}");
        Ok(())
    }

    /// Returns the latest [`FiatTopupInfo`] if the user has registered for the fiat topup.
    pub fn retrieve_latest_fiat_topup_info(&self) -> Result<Option<FiatTopupInfo>> {
        self.data_store
            .lock_unwrap()
            .retrieve_latest_fiat_topup_info()
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
}

/// Accept lipa's terms and conditions. Should be called before instantiating a [`LightningNode`]
/// for the first time.
pub fn accept_terms_and_conditions(environment: EnvironmentCode, seed: Vec<u8>) -> Result<()> {
    enable_backtrace();
    let environment = Environment::load(environment);
    let seed = sanitize_input::strong_type_seed(&seed)?;
    let auth = build_auth(&seed, environment.backend_url)?;
    auth.accept_terms_and_conditions(TermsAndConditions::Lipa)
        .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)
}

/// Allows checking if certain terms and conditions have been accepted by the user.
///
/// Parameters:
/// * `environment` - Which environment should be used.
/// * `seed` - The seed of the wallet.
/// * `terms_and_conditions` - [`TermsAndConditions`] for which the status should be requested.
///
/// Returns the status of the requested [`TermsAndConditions`].
pub fn get_terms_and_conditions_status(
    environment: EnvironmentCode,
    seed: Vec<u8>,
    terms_and_conditions: TermsAndConditions,
) -> Result<TermsAndConditionsStatus> {
    enable_backtrace();
    let environment = Environment::load(environment);
    let seed = sanitize_input::strong_type_seed(&seed)?;
    let auth = build_auth(&seed, environment.backend_url)?;
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
    use crow::TopupStatus;
    use perro::Error;

    const PAYMENT_HASH: &str = "0b78877a596f18d5f6effde3dda1df25a5cf20439ff1ac91478d7e518211040f";
    const PAYMENT_UUID: &str = "c6e597bd-0a98-5b46-8e74-f6098f5d16a3";

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
                requested_amount: Amount {
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
                lightning_address: None,
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
                requested_amount: Amount {
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
                    topup_value_sats: 0,
                    exchange_fee_minor_units: 0,
                    exchange_fee_rate_permyriad: 0,
                    lightning_payout_fee: None,
                    error: None,
                }),
                swap: None,
                lightning_address: None,
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
                requested_amount: Amount {
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
                    topup_value_sats: 0,
                    exchange_fee_minor_units: 0,
                    exchange_fee_rate_permyriad: 0,
                    lightning_payout_fee: None,
                    error: None,
                }),
                swap: None,
                lightning_address: None,
            },
        ];

        let filtered_topups = filter_out_recently_claimed_topups(topups, latest_payments);

        assert_eq!(filtered_topups.len(), 1);
        assert_eq!(filtered_topups.get(0).unwrap().id, "234");
    }
}
