use anyhow::{anyhow, bail, Result};
use bitcoin::hashes::{sha256, Hash};
use lightning::ln::PaymentSecret;
use lightning_invoice::{Currency, InvoiceBuilder};
use rand::{Rng, RngCore};
use secp256k1::{Secp256k1, SecretKey};
use std::cmp::{max, min};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const PAYEE_PUBKEY_DUMMY: &str =
    "020333076e35e398a0c14c8a0211563bbcdce5087cb300342cba09414e9b5f3605";
const SAMPLE_PAYMENT_SECRET: &str =
    "91f65d26832cb762a96c455d253f3cb4c3005ad9089d2df8612ffdf7a6b7f92f";

const LSP_ID: &str = "c0ff3e11-2222-3333-4444-555555555555";
const LSP_NAME: &str = "notdiem.lsp.mock";
const LSP_PUBKEY: &str = "0314c2aac9c7e9064773616e89daeb71be1d26966fd0e2dfbb8bfbc62d885bb5ab";
const LSP_HOST: &str = "97.35.97.53:9735";
const LSP_BASE_FEE_MSAT: i64 = 1000;
const LSP_FEE_RATE: f64 = 0.00001;
const LSP_TIMELOCK_DELTA: u32 = 42;
const LSP_MIN_HTLC_MSAT: i64 = 600;
const LSP_ADDED_LIQUIDITY_ON_NEW_CHANNELS_MSAT: u64 = 50_000_000;
const OPENING_FEE_PARAMS_MIN_MSAT: u64 = 5_000_000;
const OPENING_FEE_PARAMS_MIN_MSAT_MORE_EXPENSIVE: u64 = 6_000_000;
const OPENING_FEE_PARAMS_PROPORTIONAL: u32 = 50;
const OPENING_FEE_PARAMS_PROPORTIONAL_MORE_EXPENSIVE: u32 = 100;
const OPENING_FEE_PARAMS_MAX_IDLE_TIME: u32 = 10000;
const OPENING_FEE_PARAMS_MAX_CLIENT_TO_SELF_DELAY: u32 = 256;
const OPENING_FEE_PARAMS_PROMISE: &str = "promite";

const SWAP_MIN_AMOUNT_SAT: u64 = 1_000;
const SWAP_MAX_AMOUNT_SAT: u64 = 1_000_000;
const SWAPPER_ROUTING_FEE_SAT: u64 = 150;
const SWAP_TX_WEIGHT: u64 = 800;
const SWAP_FEE_PERCENTAGE: f64 = 0.5;
const SWAP_ADDRESS_DUMMY: &str = "bc1qftnnghhyhyegmzmh0t7uczysr05e3vx75t96y2";
const TX_ID_DUMMY: &str = "f4184fc596403b9d638783cf57adfe4c75c605f6356fbc91338530e9831e9e16";

const LNURL_PAY_FEE_MSAT: u64 = 8_000;

use breez_sdk_core::error::{
    ReceiveOnchainError, ReceivePaymentError, RedeemOnchainError, SdkError, SdkResult,
    SendOnchainError, SendPaymentError,
};
use breez_sdk_core::lnurl::pay::{LnUrlPayResult, LnUrlPaySuccessData};
use breez_sdk_core::InputType::Bolt11;
use breez_sdk_core::PaymentDetails::Ln;
pub use breez_sdk_core::{
    parse_invoice, BitcoinAddressData, BreezEvent, ClosedChannelPaymentDetails, ConnectRequest,
    EnvironmentType, EventListener, GreenlightCredentials, GreenlightNodeConfig, HealthCheckStatus,
    InputType, InvoicePaidDetails, LNInvoice, ListPaymentsRequest, LnPaymentDetails, LnUrlPayError,
    LnUrlPayRequest, LnUrlPayRequestData, LnUrlWithdrawError, LnUrlWithdrawRequest,
    LnUrlWithdrawRequestData, LnUrlWithdrawResult, MetadataItem, Network, NodeConfig,
    OnchainPaymentLimitsResponse, OpenChannelFeeRequest, OpeningFeeParams, OpeningFeeParamsMenu,
    PayOnchainRequest, Payment, PaymentDetails, PaymentFailedData, PaymentStatus, PaymentType,
    PaymentTypeFilter, PrepareOnchainPaymentRequest, PrepareOnchainPaymentResponse,
    PrepareRedeemOnchainFundsRequest, PrepareRefundRequest, ReceiveOnchainRequest,
    ReceivePaymentRequest, ReceivePaymentResponse, RedeemOnchainFundsRequest, RefundRequest,
    ReportIssueRequest, ReportPaymentFailureDetails, ReverseSwapFeesRequest, ReverseSwapStatus,
    SendPaymentRequest, SignMessageRequest, SwapAmountType, SwapStatus, UnspentTransactionOutput,
};
use breez_sdk_core::{
    ChannelState, Config, LspInformation, NodeState, OpenChannelFeeResponse, PayOnchainResponse,
    PrepareRedeemOnchainFundsResponse, PrepareRefundResponse, RecommendedFees,
    RedeemOnchainFundsResponse, RefundResponse, ReverseSwapInfo, ReverseSwapPairInfo,
    SendPaymentResponse, ServiceHealthCheckResponse, SignMessageResponse, SwapInfo,
};
use chrono::{DateTime, Utc};
use hex::FromHex;
use lazy_static::lazy_static;
use tokio::runtime::Handle;
use tokio::sync::Mutex;

pub mod error {
    pub use breez_sdk_core::error::*;
}

pub mod lnurl {
    pub mod pay {
        pub use breez_sdk_core::lnurl::pay::*;
    }
}

#[derive(Clone)]
struct Channel {
    pub capacity_msat: u64,
    pub local_balance_msat: u64,
}

impl Channel {
    fn get_inbound_capacity_msat(&self) -> u64 {
        self.capacity_msat - self.local_balance_msat
    }
}

lazy_static! {
    static ref HEALTH_STATUS: Mutex<HealthCheckStatus> = Mutex::new(HealthCheckStatus::Operational);
    static ref PAYMENT_DELAY: Mutex<PaymentDelay> = Mutex::new(PaymentDelay::Immediate);
    static ref PAYMENT_OUTCOME: Mutex<PaymentOutcome> = Mutex::new(PaymentOutcome::Success);
    static ref PAYMENTS: std::sync::Mutex<Vec<Payment>> = std::sync::Mutex::new(Vec::new());
    static ref SWAPS: Mutex<Vec<SwapInfo>> = Mutex::new(Vec::new());
    static ref CHANNELS: std::sync::Mutex<Vec<Channel>> = std::sync::Mutex::new(Vec::new());
    static ref CHANNELS_PENDING_CLOSE: std::sync::Mutex<Vec<Channel>> =
        std::sync::Mutex::new(Vec::new());
    static ref CHANNELS_CLOSED: std::sync::Mutex<Vec<Channel>> = std::sync::Mutex::new(Vec::new());
}

#[derive(Debug)]
enum PaymentOutcome {
    Success,
    AlreadyPaid,
    GenericError,
    InvalidNetwork,
    InvoiceExpired,
    Failed,
    Timeout,
    RouteNotFound,
    RouteTooExpensive,
    ServiceConnectivity,
}

enum PaymentDelay {
    Immediate,
    Short,
    Long,
}

pub struct BreezServices {
    priv_key: SecretKey,
    pub_key: String,
    event_listener: Box<dyn EventListener>,
}

impl BreezServices {
    pub async fn connect(
        _req: ConnectRequest,
        event_listener: Box<dyn EventListener>,
    ) -> SdkResult<Arc<BreezServices>> {
        let priv_key = SecretKey::from_slice(&generate_32_random_bytes()).unwrap();
        let pub_key = priv_key.public_key(&Secp256k1::new()).to_string();

        let sdk = Arc::new(BreezServices {
            priv_key,
            pub_key,
            event_listener,
        });
        let sdk_task = Arc::clone(&sdk);
        Handle::current().spawn(async move {
            loop {
                let _ = sdk_task.sync().await;
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
        Ok(sdk)
    }
    pub async fn send_payment(
        &self,
        req: SendPaymentRequest,
    ) -> Result<SendPaymentResponse, SendPaymentError> {
        match &*PAYMENT_DELAY.lock().await {
            PaymentDelay::Immediate => {}
            PaymentDelay::Short => {
                thread::sleep(Duration::from_secs(3));
            }
            PaymentDelay::Long => {
                thread::sleep(Duration::from_secs(10));
            }
        }

        let (_, payment_preimage) = generate_2_hashes();

        match &*PAYMENT_OUTCOME.lock().await {
            PaymentOutcome::Success => {
                let parsed_invoice = parse_invoice(req.bolt11.as_str())?;
                let invoice_amount_msat = parsed_invoice.amount_msat.unwrap_or_default();
                let provided_amount_msat = req.amount_msat.unwrap_or_default();

                // Ensure amount is provided for zero invoice
                if provided_amount_msat == 0 && invoice_amount_msat == 0 {
                    return Err(SendPaymentError::InvalidAmount {
                        err: "Amount must be provided when paying a zero invoice".into(),
                    });
                }

                // Ensure amount is not provided for invoice that contains amount
                if provided_amount_msat > 0 && invoice_amount_msat > 0 {
                    return Err(SendPaymentError::InvalidAmount {
                        err: "Amount should not be provided when paying a non zero invoice".into(),
                    });
                }

                let amount_msat = max(provided_amount_msat, invoice_amount_msat);

                let routing_fee_msat = rand::thread_rng().gen_range(1000..4000);
                if get_balance_msat() < amount_msat {
                    return Err(SendPaymentError::RouteNotFound {
                        err: "Ran out of routes".into(),
                    });
                } else {
                    send_payment_mock_channels(amount_msat + routing_fee_msat).await;
                }

                let payment = create_payment(MockPayment {
                    payment_type: PaymentType::Sent,
                    amount_msat,
                    fee_msat: routing_fee_msat,
                    description: None,
                    payment_hash: parsed_invoice.payment_hash,
                    payment_preimage,
                    destination_pubkey: parsed_invoice.payee_pubkey,
                    bolt11: req.bolt11,
                    lnurl_pay_domain: None,
                    lnurl_pay_comment: None,
                    ln_address: None,
                    lnurl_metadata: None,
                    lnurl_withdraw_endpoint: None,
                    swap_info: None,
                    reverse_swap_info: None,
                });

                PAYMENTS.lock().unwrap().push(payment.clone());

                self.event_listener.on_event(BreezEvent::PaymentSucceed {
                    details: payment.clone(),
                });

                Ok(SendPaymentResponse { payment })
            }
            PaymentOutcome::AlreadyPaid => {
                self.event_listener.on_event(BreezEvent::PaymentFailed {
                    details: PaymentFailedData {
                        error: "Already paid".to_string(),
                        node_id: self.pub_key.clone(),
                        invoice: None,
                        label: None,
                    },
                });
                Err(SendPaymentError::AlreadyPaid)
            }
            PaymentOutcome::GenericError => {
                self.event_listener.on_event(BreezEvent::PaymentFailed {
                    details: PaymentFailedData {
                        error: "Generic error".to_string(),
                        node_id: self.pub_key.clone(),
                        invoice: None,
                        label: None,
                    },
                });
                Err(SendPaymentError::Generic {
                    err: "Generic error".into(),
                })
            }
            PaymentOutcome::InvalidNetwork => {
                self.event_listener.on_event(BreezEvent::PaymentFailed {
                    details: PaymentFailedData {
                        error: "Invalid network".to_string(),
                        node_id: self.pub_key.clone(),
                        invoice: None,
                        label: None,
                    },
                });
                Err(SendPaymentError::InvalidNetwork {
                    err: "Invalid network".into(),
                })
            }
            PaymentOutcome::InvoiceExpired => {
                self.event_listener.on_event(BreezEvent::PaymentFailed {
                    details: PaymentFailedData {
                        error: "Invoice expired".to_string(),
                        node_id: self.pub_key.clone(),
                        invoice: None,
                        label: None,
                    },
                });
                Err(SendPaymentError::InvoiceExpired {
                    err: "Invoice expired".into(),
                })
            }
            PaymentOutcome::Failed => {
                self.event_listener.on_event(BreezEvent::PaymentFailed {
                    details: PaymentFailedData {
                        error: "Payment failed".to_string(),
                        node_id: self.pub_key.clone(),
                        invoice: None,
                        label: None,
                    },
                });
                Err(SendPaymentError::PaymentFailed {
                    err: "Payment Failed".into(),
                })
            }
            PaymentOutcome::Timeout => {
                self.event_listener.on_event(BreezEvent::PaymentFailed {
                    details: PaymentFailedData {
                        error: "Payment timed out".to_string(),
                        node_id: self.pub_key.clone(),
                        invoice: None,
                        label: None,
                    },
                });
                Err(SendPaymentError::PaymentTimeout {
                    err: "Payment timed out".into(),
                })
            }
            PaymentOutcome::RouteNotFound => {
                self.event_listener.on_event(BreezEvent::PaymentFailed {
                    details: PaymentFailedData {
                        error: "Route not found".to_string(),
                        node_id: self.pub_key.clone(),
                        invoice: None,
                        label: None,
                    },
                });
                Err(SendPaymentError::RouteNotFound {
                    err: "Route not found".into(),
                })
            }
            PaymentOutcome::RouteTooExpensive => {
                self.event_listener.on_event(BreezEvent::PaymentFailed {
                    details: PaymentFailedData {
                        error: "Route too expensive".to_string(),
                        node_id: self.pub_key.clone(),
                        invoice: None,
                        label: None,
                    },
                });
                Err(SendPaymentError::RouteTooExpensive {
                    err: "Route too expensive".into(),
                })
            }
            PaymentOutcome::ServiceConnectivity => {
                self.event_listener.on_event(BreezEvent::PaymentFailed {
                    details: PaymentFailedData {
                        error: "Service connectivity error".to_string(),
                        node_id: self.pub_key.clone(),
                        invoice: None,
                        label: None,
                    },
                });
                Err(SendPaymentError::ServiceConnectivity {
                    err: "Service connectivity error".into(),
                })
            }
        }
    }

    pub async fn lnurl_pay(&self, req: LnUrlPayRequest) -> Result<LnUrlPayResult, LnUrlPayError> {
        let (invoice, preimage, payment_hash) = self.create_invoice(req.amount_msat, "");

        if get_balance_msat() < req.amount_msat {
            return Err(LnUrlPayError::RouteNotFound {
                err: "Ran out of routes".into(),
            });
        } else {
            send_payment_mock_channels(req.amount_msat + LNURL_PAY_FEE_MSAT).await;
        }

        let payment = create_payment(MockPayment {
            payment_type: PaymentType::Sent,
            amount_msat: req.amount_msat,
            fee_msat: LNURL_PAY_FEE_MSAT,
            description: None,
            payment_hash: payment_hash.clone(),
            payment_preimage: preimage,
            destination_pubkey: PAYEE_PUBKEY_DUMMY.to_string(),
            bolt11: invoice,
            lnurl_pay_domain: Some(req.data.domain.clone()),
            lnurl_pay_comment: req.comment.clone(),
            ln_address: req.data.ln_address.clone(),
            lnurl_metadata: Some(req.data.metadata_str.clone()),
            lnurl_withdraw_endpoint: None,
            swap_info: None,
            reverse_swap_info: None,
        });

        PAYMENTS.lock().unwrap().push(payment.clone());

        self.event_listener.on_event(BreezEvent::PaymentSucceed {
            details: payment.clone(),
        });

        Ok(LnUrlPayResult::EndpointSuccess {
            data: LnUrlPaySuccessData {
                payment,
                success_action: None,
            },
        })
    }

    pub async fn lnurl_withdraw(
        &self,
        req: LnUrlWithdrawRequest,
    ) -> Result<LnUrlWithdrawResult, LnUrlWithdrawError> {
        let lsp_fee_msat = receive_payment_mock_channels(req.amount_msat)
            .await
            .map_err(|e| LnUrlWithdrawError::InvalidAmount { err: e.to_string() })?;

        let (invoice, preimage, payment_hash) =
            self.create_invoice(req.amount_msat, &req.description.unwrap_or_default());

        let payment = create_payment(MockPayment {
            payment_type: PaymentType::Received,
            amount_msat: req.amount_msat - lsp_fee_msat,
            fee_msat: lsp_fee_msat,
            description: None,
            payment_hash: payment_hash.clone(),
            payment_preimage: preimage,
            destination_pubkey: self.pub_key.clone(),
            bolt11: invoice.clone(),
            lnurl_pay_domain: None,
            lnurl_pay_comment: None,
            ln_address: None,
            lnurl_metadata: None,
            lnurl_withdraw_endpoint: Some("https://lnurl.dummy.com/lnurl-withdraw".to_string()),
            swap_info: None,
            reverse_swap_info: None,
        });

        PAYMENTS.lock().unwrap().push(payment.clone());

        self.event_listener.on_event(BreezEvent::InvoicePaid {
            details: InvoicePaidDetails {
                payment_hash: payment_hash.clone(),
                bolt11: invoice.clone(),
                payment: Some(payment),
            },
        });

        Ok(LnUrlWithdrawResult::Ok {
            data: breez_sdk_core::LnUrlWithdrawSuccessData {
                invoice: LNInvoice {
                    bolt11: invoice,
                    network: Network::Bitcoin,
                    payee_pubkey: self.pub_key.clone(),
                    payment_hash,
                    description: None,
                    description_hash: None,
                    amount_msat: Some(req.amount_msat),
                    timestamp: 0,
                    expiry: 0,
                    routing_hints: vec![],
                    payment_secret: vec![],
                    min_final_cltv_expiry_delta: 0,
                },
            },
        })
    }

    pub async fn receive_payment(
        &self,
        req: ReceivePaymentRequest,
    ) -> Result<ReceivePaymentResponse, ReceivePaymentError> {
        // Has nothing to do with receiving a payment, but is a mechanism to control the mock
        match req.description.trim().to_lowercase().as_str() {
            "health.operational" | "ho" => {
                *HEALTH_STATUS.lock().await = HealthCheckStatus::Operational
            }
            "health.maintenance" | "hm" => {
                *HEALTH_STATUS.lock().await = HealthCheckStatus::Maintenance
            }
            "health.disruption" | "hd" => {
                *HEALTH_STATUS.lock().await = HealthCheckStatus::ServiceDisruption
            }
            "pay.delay.immediate" | "pd.immediate" => {
                *PAYMENT_DELAY.lock().await = PaymentDelay::Immediate
            }
            "pay.delay.short" | "pd.short" => *PAYMENT_DELAY.lock().await = PaymentDelay::Short,
            "pay.delay.long" | "pay.delay" | "pd" => {
                *PAYMENT_DELAY.lock().await = PaymentDelay::Long
            }
            "pay.success" | "ps" => *PAYMENT_OUTCOME.lock().await = PaymentOutcome::Success,
            "pay.err.already_paid" | "pe.already_paid" => {
                *PAYMENT_OUTCOME.lock().await = PaymentOutcome::AlreadyPaid
            }
            "pay.err.generic" | "pay.err" | "pe" => {
                *PAYMENT_OUTCOME.lock().await = PaymentOutcome::GenericError
            }
            "pay.err.network" | "pe.network" => {
                *PAYMENT_OUTCOME.lock().await = PaymentOutcome::InvalidNetwork
            }
            "pay.err.expired" | "pe.expired" => {
                *PAYMENT_OUTCOME.lock().await = PaymentOutcome::InvoiceExpired
            }
            "pay.err.failed" | "pe.failed" => {
                *PAYMENT_OUTCOME.lock().await = PaymentOutcome::Failed
            }
            "pay.err.timeout" | "pe.timeout" => {
                *PAYMENT_OUTCOME.lock().await = PaymentOutcome::Timeout
            }
            "pay.err.route" | "pe.route" => {
                *PAYMENT_OUTCOME.lock().await = PaymentOutcome::RouteNotFound
            }
            "pay.err.route_too_expensive" | "pe.rte" => {
                *PAYMENT_OUTCOME.lock().await = PaymentOutcome::RouteTooExpensive
            }
            "pay.err.connectivity" | "pe.connectivity" => {
                *PAYMENT_OUTCOME.lock().await = PaymentOutcome::ServiceConnectivity
            }
            "mimic.activities" | "ma" => self.simulate_activities(req.amount_msat),
            "mimic.pay2addr" | "mp" => self.simulate_payments(PaymentType::Sent, 10, true).await,
            "channels.close_largest" | "cclose" => close_channel_with_largest_balance().await,
            "channels.confirm_pending_closes" | "cconf" => confirm_pending_channel_closes(),
            "swaps.start" | "ss" => self.start_swap(req.amount_msat / 1_000).await?,
            "swaps.confirm_onchain" | "sc" => self.confirm_swap_onchain().await?,
            "swaps.redeem" | "sr" => {
                let swaps = SWAPS.lock().await.clone();
                if let Some(swap) = swaps.iter().find(|s| s.status == SwapStatus::Redeemable) {
                    self.redeem_swap(swap.bitcoin_address.clone()).await?;
                }
            }
            "swaps.expire" | "se" => {
                self.expire_swap().await?;
            }
            "clearwallet.issuetx" | "ci" => {
                self.issue_clear_wallet_tx().await;
            }
            "clearwallet.confirmtx" | "cc" => {
                self.confirm_clear_wallet_tx().await;
            }
            "clearwallet.simulate_cancellation" | "cs" => {
                self.simulate_clear_wallet_cancellation().await;
            }
            _ => {}
        }

        let (invoice, preimage, payment_hash) =
            self.create_invoice(req.amount_msat, &req.description);

        let description = Option::from(req.description);

        let mut lsp_fee_msat_optional = None;

        if let PaymentOutcome::Success = &*PAYMENT_OUTCOME.lock().await {
            let lsp_fee_msat = receive_payment_mock_channels(req.amount_msat)
                .await
                .map_err(|e| ReceivePaymentError::InvalidAmount { err: e.to_string() })?;

            if lsp_fee_msat > 0 {
                lsp_fee_msat_optional = Some(lsp_fee_msat);
            }

            let payment = create_payment(MockPayment {
                payment_type: PaymentType::Received,
                amount_msat: req.amount_msat - lsp_fee_msat,
                fee_msat: lsp_fee_msat,
                description: description.clone(),
                payment_hash: payment_hash.clone(),
                payment_preimage: preimage,
                destination_pubkey: self.pub_key.clone(),
                bolt11: invoice.to_string(),
                lnurl_pay_domain: None,
                lnurl_pay_comment: None,
                ln_address: None,
                lnurl_metadata: None,
                lnurl_withdraw_endpoint: None,
                swap_info: None,
                reverse_swap_info: None,
            });

            PAYMENTS.lock().unwrap().push(payment.clone());
            self.event_listener.on_event(BreezEvent::InvoicePaid {
                details: InvoicePaidDetails {
                    payment_hash: payment_hash.clone(),
                    bolt11: invoice.to_string(),
                    payment: Some(payment),
                },
            });
        }

        Ok(ReceivePaymentResponse {
            ln_invoice: LNInvoice {
                bolt11: invoice.to_string(),
                network: Network::Bitcoin,
                payee_pubkey: self.pub_key.clone(),
                payment_hash,
                description,
                description_hash: None,
                amount_msat: Some(req.amount_msat),
                timestamp: Utc::now().timestamp() as u64,
                expiry: 3600,
                routing_hints: vec![],
                payment_secret: Vec::from(SAMPLE_PAYMENT_SECRET.as_bytes()),
                min_final_cltv_expiry_delta: 144,
            },
            opening_fee_params: None,
            opening_fee_msat: lsp_fee_msat_optional,
        })
    }

    pub async fn service_health_check(_api_key: String) -> SdkResult<ServiceHealthCheckResponse> {
        Ok(ServiceHealthCheckResponse {
            status: HEALTH_STATUS.lock().await.clone(),
        })
    }

    // Not useful for the mock, but required to keep same interface
    pub async fn report_issue(&self, _req: ReportIssueRequest) -> SdkResult<()> {
        Ok(())
    }

    pub fn node_info(&self) -> SdkResult<NodeState> {
        let balance = get_balance_msat();

        Ok(NodeState {
            id: self.pub_key.clone(),
            block_height: 1234567,
            channels_balance_msat: balance,
            onchain_balance_msat: get_onchain_balance_msat(),
            pending_onchain_balance_msat: get_pending_onchain_balance_msat(),
            utxos: vec![],
            max_payable_msat: balance,
            max_receivable_msat: get_inbound_liquidity_msat(),
            max_single_payment_amount_msat: balance,
            max_chan_reserve_msats: 0,
            connected_peers: vec![LSP_ID.to_string()],
            max_receivable_single_payment_amount_msat: get_inbound_liquidity_msat(), // TODO: max receivable in single channel
            total_inbound_liquidity_msats: get_inbound_liquidity_msat(),
        })
    }

    pub async fn sign_message(&self, _req: SignMessageRequest) -> SdkResult<SignMessageResponse> {
        Ok(SignMessageResponse {
            signature: "This_dummy_string_represents_a_signature_for_a_given_message".to_string(),
        })
    }

    pub async fn list_payments(&self, req: ListPaymentsRequest) -> SdkResult<Vec<Payment>> {
        let payment_type_filter = req
            .filters
            .as_ref()
            .unwrap_or(&vec![
                PaymentTypeFilter::Sent,
                PaymentTypeFilter::Received,
                PaymentTypeFilter::ClosedChannel,
            ])
            .iter()
            .map(|f| match f {
                PaymentTypeFilter::Sent => PaymentType::Sent,
                PaymentTypeFilter::Received => PaymentType::Received,
                PaymentTypeFilter::ClosedChannel => PaymentType::ClosedChannel,
            })
            .collect::<Vec<PaymentType>>();

        PAYMENTS
            .lock()
            .unwrap()
            .sort_by(|a, b| a.payment_time.cmp(&b.payment_time));

        let payments = PAYMENTS
            .lock()
            .unwrap()
            .clone()
            .into_iter()
            .filter(|p| {
                payment_type_filter.is_empty() || payment_type_filter.contains(&p.payment_type)
            })
            .filter(|p| {
                p.payment_time >= req.from_timestamp.unwrap_or(0)
                    && p.payment_time <= req.to_timestamp.unwrap_or(Utc::now().timestamp())
            })
            .skip(req.offset.unwrap_or(0) as usize)
            .take(req.limit.unwrap_or(1_000_000) as usize)
            .collect::<Vec<Payment>>();

        Ok(payments)
    }

    pub async fn payment_by_hash(&self, hash: String) -> SdkResult<Option<Payment>> {
        Ok(PAYMENTS
            .lock()
            .unwrap()
            .iter()
            .find(|p| {
                if let PaymentDetails::Ln { data } = &p.details {
                    data.payment_hash == hash
                } else {
                    false
                }
            })
            .cloned())
    }

    pub async fn prepare_redeem_onchain_funds(
        &self,
        req: PrepareRedeemOnchainFundsRequest,
    ) -> Result<PrepareRedeemOnchainFundsResponse, RedeemOnchainError> {
        let tx_fee_sat = SWAP_TX_WEIGHT / 4 * (req.sat_per_vbyte as u64);
        if get_onchain_balance_msat() <= tx_fee_sat * 1_000 {
            return Err(RedeemOnchainError::InsufficientFunds {
                err: "Insufficient funds to pay fees".to_string(),
            });
        }
        Ok(PrepareRedeemOnchainFundsResponse {
            tx_weight: SWAP_TX_WEIGHT,
            tx_fee_sat,
        })
    }

    pub async fn redeem_onchain_funds(
        &self,
        _req: RedeemOnchainFundsRequest,
    ) -> SdkResult<RedeemOnchainFundsResponse> {
        CHANNELS_CLOSED.lock().unwrap().clear();

        Ok(RedeemOnchainFundsResponse {
            txid: Vec::from_hex(TX_ID_DUMMY).unwrap(),
        })
    }

    pub async fn redeem_swap(&self, _swap_address: String) -> SdkResult<()> {
        let swap = {
            let swaps = SWAPS.lock().await.clone();
            swaps
                .into_iter()
                .find(|s| s.status == SwapStatus::Redeemable)
        };

        if let Some(mut swap) = swap {
            SWAPS
                .lock()
                .await
                .retain(|s| s.status != SwapStatus::Redeemable);

            swap.paid_msat = swap.confirmed_sats * 1_000;
            swap.status = SwapStatus::Completed;

            let lsp_fee_msat = receive_payment_mock_channels(swap.confirmed_sats)
                .await
                .map_err(|e| SdkError::Generic { err: e.to_string() })?;

            let (invoice, payment_preimage, payment_hash) =
                self.create_invoice(swap.confirmed_sats, "Swap invoice");
            let payment = create_payment(MockPayment {
                payment_type: PaymentType::Received,
                amount_msat: swap.confirmed_sats * 1_000 - lsp_fee_msat,
                fee_msat: lsp_fee_msat,
                description: Some("swapped in".to_string()),
                payment_hash,
                payment_preimage,
                destination_pubkey: self.pub_key.clone(),
                bolt11: invoice.clone(),
                lnurl_pay_domain: None,
                lnurl_pay_comment: None,
                ln_address: None,
                lnurl_metadata: None,
                lnurl_withdraw_endpoint: None,
                swap_info: Some(swap.clone()),
                reverse_swap_info: None,
            });
            PAYMENTS.lock().unwrap().push(payment.clone());

            self.event_listener.on_event(BreezEvent::InvoicePaid {
                details: InvoicePaidDetails {
                    payment_hash: "".to_string(),
                    bolt11: invoice,
                    payment: Some(payment),
                },
            });
            self.event_listener.on_event(BreezEvent::SwapUpdated {
                details: swap.clone(),
            });
        } else {
            return Err(SdkError::Generic {
                err: "Swap not found or not redeemable".into(),
            });
        }

        Ok(())
    }

    /// List available LSPs that can be selected by the user
    pub async fn list_lsps(&self) -> SdkResult<Vec<LspInformation>> {
        Ok(vec![LspInformation {
            id: LSP_ID.to_string(),
            name: LSP_NAME.to_string(),
            widget_url: "".to_string(),
            pubkey: LSP_PUBKEY.to_string(),
            host: LSP_HOST.to_string(),
            base_fee_msat: LSP_BASE_FEE_MSAT,
            fee_rate: LSP_FEE_RATE,
            time_lock_delta: LSP_TIMELOCK_DELTA,
            min_htlc_msat: LSP_MIN_HTLC_MSAT,
            lsp_pubkey: vec![],
            opening_fee_params_list: OpeningFeeParamsMenu { values: vec![] },
        }])
    }

    pub async fn connect_lsp(&self, _lsp_id: String) -> SdkResult<()> {
        Ok(())
    }

    pub async fn lsp_id(&self) -> SdkResult<Option<String>> {
        Ok(Some(LSP_ID.to_string()))
    }

    pub async fn open_channel_fee(
        &self,
        req: OpenChannelFeeRequest,
    ) -> SdkResult<OpenChannelFeeResponse> {
        let fee_msat = match req.amount_msat {
            None => None,
            Some(amount_msat) => {
                if get_inbound_liquidity_msat() < amount_msat {
                    let mut opening_fee = max(
                        OPENING_FEE_PARAMS_MIN_MSAT,
                        amount_msat * OPENING_FEE_PARAMS_PROPORTIONAL as u64 / 10_000,
                    );
                    // simulate the lsp offering a higher expiry time for a higher fee
                    if req.expiry.is_some() {
                        opening_fee += 1_000;
                    }
                    Some(opening_fee)
                } else {
                    Some(0)
                }
            }
        };

        let (in_two_hours, in_three_days) = get_lsp_fee_params_expiry_dates();
        let valid_until = if req.expiry.is_some() {
            in_two_hours.to_rfc3339()
        } else {
            in_three_days.to_rfc3339()
        };

        Ok(OpenChannelFeeResponse {
            fee_msat,
            fee_params: OpeningFeeParams {
                min_msat: OPENING_FEE_PARAMS_MIN_MSAT,
                proportional: OPENING_FEE_PARAMS_PROPORTIONAL,
                valid_until,
                max_idle_time: OPENING_FEE_PARAMS_MAX_IDLE_TIME,
                max_client_to_self_delay: OPENING_FEE_PARAMS_MAX_CLIENT_TO_SELF_DELAY,
                promise: OPENING_FEE_PARAMS_PROMISE.to_string(),
            },
        })
    }

    pub async fn receive_onchain(
        &self,
        _req: ReceiveOnchainRequest,
    ) -> Result<SwapInfo, ReceiveOnchainError> {
        let now = Utc::now().timestamp();

        if self.in_progress_swap().await?.is_some() {
            return Err(ReceiveOnchainError::SwapInProgress {
                err: SWAP_ADDRESS_DUMMY.to_string(),
            });
        }

        let swap = SwapInfo {
            bitcoin_address: SWAP_ADDRESS_DUMMY.to_string(),
            created_at: now,
            lock_height: 10,
            payment_hash: vec![],
            preimage: vec![],
            private_key: vec![],
            public_key: vec![],
            swapper_public_key: vec![],
            script: vec![],
            bolt11: None,
            paid_msat: 0,
            total_incoming_txs: 0,
            confirmed_sats: 0,
            unconfirmed_sats: 0,
            status: SwapStatus::Initial,
            refund_tx_ids: vec![],
            unconfirmed_tx_ids: vec![],
            confirmed_tx_ids: vec![],
            min_allowed_deposit: 1_000,
            max_allowed_deposit: 100_000_000,
            max_swapper_payable: 100_000_000,
            last_redeem_error: None,
            channel_opening_fees: None,
            confirmed_at: None,
        };

        Ok(swap)
    }

    pub async fn in_progress_swap(&self) -> SdkResult<Option<SwapInfo>> {
        Ok(SWAPS
            .lock()
            .await
            .iter()
            .find(|swap| {
                swap.status != SwapStatus::Initial
                    && swap.status != SwapStatus::Completed
                    && swap.status != SwapStatus::Refundable
            })
            .cloned())
    }

    pub async fn fetch_reverse_swap_fees(
        &self,
        req: ReverseSwapFeesRequest,
    ) -> SdkResult<ReverseSwapPairInfo> {
        let total_fees = req
            .send_amount_sat
            .map(|amount| (amount as f64) / 100.0 * SWAP_FEE_PERCENTAGE)
            .map(|fees| fees as u64);
        Ok(ReverseSwapPairInfo {
            min: SWAP_MIN_AMOUNT_SAT,
            max: SWAP_MAX_AMOUNT_SAT,
            fees_hash: "this-should-be-a-hash.dummy".to_string(),
            fees_percentage: SWAP_FEE_PERCENTAGE,
            fees_lockup: 500,
            fees_claim: 500,
            total_fees,
        })
    }

    pub async fn onchain_payment_limits(&self) -> SdkResult<OnchainPaymentLimitsResponse> {
        let balance_sat = get_balance_msat() / 1000;
        let max_payable_sat = balance_sat.saturating_sub(SWAPPER_ROUTING_FEE_SAT);

        Ok(OnchainPaymentLimitsResponse {
            min_sat: SWAP_MIN_AMOUNT_SAT,
            max_sat: SWAP_MAX_AMOUNT_SAT,
            max_payable_sat,
        })
    }

    pub async fn prepare_onchain_payment(
        &self,
        req: PrepareOnchainPaymentRequest,
    ) -> Result<PrepareOnchainPaymentResponse, SendOnchainError> {
        let fees_lockup = 500;
        let fees_claim = 500;
        let total_fees = ((req.amount_sat as f64) / 100.0 * SWAP_FEE_PERCENTAGE) as u64
            + fees_lockup
            + fees_claim;
        let recipient_amount_sat = req.amount_sat - total_fees;
        Ok(PrepareOnchainPaymentResponse {
            fees_hash: "this-should-be-a-hash.dummy".to_string(),
            fees_percentage: SWAP_FEE_PERCENTAGE,
            fees_lockup,
            fees_claim,
            sender_amount_sat: req.amount_sat,
            recipient_amount_sat,
            total_fees,
        })
    }

    pub async fn pay_onchain(
        &self,
        req: PayOnchainRequest,
    ) -> Result<PayOnchainResponse, SendOnchainError> {
        if req.prepare_res.sender_amount_sat < SWAP_MIN_AMOUNT_SAT {
            return Err(SendOnchainError::PaymentFailed {
                err: "Insufficient funds".to_string(),
            });
        }

        let now = Utc::now().timestamp();

        let amount_msat = req.prepare_res.sender_amount_sat * 1_000;

        let routing_fee_msat = SWAPPER_ROUTING_FEE_SAT * 1000;
        send_payment_mock_channels(amount_msat + routing_fee_msat).await;

        let reverse_swap_info = ReverseSwapInfo {
            id: now.to_string(),
            claim_pubkey: req.recipient_address,
            lockup_txid: Some("LOCKUP-TXID-DUMMY".to_string()),
            claim_txid: Some("CLAIM-TXID-DUMMY".to_string()),
            onchain_amount_sat: req.prepare_res.sender_amount_sat,
            status: ReverseSwapStatus::InProgress,
        };

        let (invoice, preimage, payment_hash) = self.create_invoice(amount_msat, "");
        let payment = create_payment(MockPayment {
            payment_type: PaymentType::Sent,
            amount_msat,
            fee_msat: routing_fee_msat,
            description: None,
            payment_hash,
            payment_preimage: preimage,
            destination_pubkey: PAYEE_PUBKEY_DUMMY.to_string(),
            bolt11: invoice,
            lnurl_pay_domain: None,
            lnurl_pay_comment: None,
            ln_address: None,
            lnurl_metadata: None,
            lnurl_withdraw_endpoint: None,
            swap_info: None,
            reverse_swap_info: Some(reverse_swap_info.clone()),
        });
        PAYMENTS.lock().unwrap().push(payment.clone());

        self.event_listener
            .on_event(BreezEvent::ReverseSwapUpdated {
                details: reverse_swap_info.clone(),
            });

        Ok(PayOnchainResponse { reverse_swap_info })
    }

    pub async fn list_refundables(&self) -> SdkResult<Vec<SwapInfo>> {
        let swaps = SWAPS.lock().await.clone();
        Ok(swaps
            .into_iter()
            .filter(|swap| swap.status == SwapStatus::Refundable)
            .collect())
    }
    pub async fn prepare_refund(
        &self,
        req: PrepareRefundRequest,
    ) -> SdkResult<PrepareRefundResponse> {
        Ok(PrepareRefundResponse {
            refund_tx_weight: SWAP_TX_WEIGHT as u32,
            refund_tx_fee_sat: SWAP_TX_WEIGHT / 4 * req.sat_per_vbyte as u64,
        })
    }

    pub async fn refund(&self, req: RefundRequest) -> SdkResult<RefundResponse> {
        let refundable_swap = {
            let swaps = SWAPS.lock().await;
            swaps
                .clone()
                .into_iter()
                .find(|s| s.status == SwapStatus::Refundable)
        };

        if let Some(refundable_swap) = refundable_swap {
            let onchain_fees = self
                .prepare_refund(PrepareRefundRequest {
                    swap_address: refundable_swap.bitcoin_address,
                    to_address: SWAP_ADDRESS_DUMMY.to_string(),
                    sat_per_vbyte: req.sat_per_vbyte,
                })
                .await?
                .refund_tx_fee_sat;

            if req.to_address == SWAP_ADDRESS_DUMMY {
                self.start_swap(refundable_swap.confirmed_sats - onchain_fees)
                    .await?;
            }
        } else {
            return Err(SdkError::Generic {
                err: "Refundable swap not found".to_string(),
            });
        }

        SWAPS
            .lock()
            .await
            .retain(|swap| swap.status != SwapStatus::Refundable);

        Ok(RefundResponse {
            refund_tx_id: TX_ID_DUMMY.to_string(),
        })
    }

    pub async fn execute_dev_command(&self, command: String) -> SdkResult<String> {
        match command.as_str() {
            "listpeerchannels" => Ok("This is a mock response for listpeerchannels".to_string()),
            "listpayments" => Ok(format!("{:?}", PAYMENTS.lock().unwrap().clone())),
            _ => panic!("Command {command} not implemented in mock yet"),
        }
    }

    pub async fn sync(&self) -> SdkResult<()> {
        self.event_listener.on_event(BreezEvent::Synced);
        Ok(())
    }

    pub async fn lsp_info(&self) -> SdkResult<LspInformation> {
        let (in_two_hours, in_three_days) = get_lsp_fee_params_expiry_dates();

        Ok(LspInformation {
            id: LSP_ID.to_string(),
            name: LSP_NAME.to_string(),
            widget_url: "".to_string(),
            pubkey: LSP_PUBKEY.to_string().to_string(),
            host: LSP_HOST.to_string(),
            base_fee_msat: LSP_BASE_FEE_MSAT,
            fee_rate: LSP_FEE_RATE,
            time_lock_delta: LSP_TIMELOCK_DELTA,
            min_htlc_msat: LSP_MIN_HTLC_MSAT,
            lsp_pubkey: vec![],
            opening_fee_params_list: OpeningFeeParamsMenu {
                values: vec![
                    OpeningFeeParams {
                        min_msat: OPENING_FEE_PARAMS_MIN_MSAT,
                        proportional: OPENING_FEE_PARAMS_PROPORTIONAL,
                        valid_until: in_two_hours.to_rfc3339(),
                        max_idle_time: OPENING_FEE_PARAMS_MAX_IDLE_TIME,
                        max_client_to_self_delay: OPENING_FEE_PARAMS_MAX_CLIENT_TO_SELF_DELAY,
                        promise: OPENING_FEE_PARAMS_PROMISE.to_string(),
                    },
                    OpeningFeeParams {
                        min_msat: OPENING_FEE_PARAMS_MIN_MSAT_MORE_EXPENSIVE,
                        proportional: OPENING_FEE_PARAMS_PROPORTIONAL_MORE_EXPENSIVE,
                        valid_until: in_three_days.to_rfc3339(),
                        max_idle_time: OPENING_FEE_PARAMS_MAX_IDLE_TIME,
                        max_client_to_self_delay: OPENING_FEE_PARAMS_MAX_CLIENT_TO_SELF_DELAY,
                        promise: OPENING_FEE_PARAMS_PROMISE.to_string(),
                    },
                ],
            },
        })
    }

    pub async fn recommended_fees(&self) -> SdkResult<RecommendedFees> {
        Ok(RecommendedFees {
            fastest_fee: 20,
            half_hour_fee: 15,
            hour_fee: 12,
            economy_fee: 10,
            minimum_fee: 5,
        })
    }

    pub fn default_config(
        env_type: EnvironmentType,
        api_key: String,
        node_config: NodeConfig,
    ) -> Config {
        match env_type {
            EnvironmentType::Production => Config::production(api_key, node_config),
            EnvironmentType::Staging => Config::staging(api_key, node_config),
        }
    }

    pub async fn register_webhook(&self, _webhook_url: String) -> SdkResult<()> {
        Ok(())
    }

    async fn start_swap(&self, amount_sat: u64) -> Result<()> {
        if self.in_progress_swap().await?.is_some() {
            bail!("A swap is already in progress");
        }

        let mut swaps = SWAPS.lock().await;

        let swap = SwapInfo {
            bitcoin_address: SWAP_ADDRESS_DUMMY.to_string(),
            created_at: Utc::now().timestamp(),
            lock_height: 10,
            payment_hash: vec![],
            preimage: vec![],
            private_key: vec![],
            public_key: vec![],
            swapper_public_key: vec![],
            script: vec![],
            bolt11: None,
            paid_msat: 0,
            total_incoming_txs: 1,
            confirmed_sats: 0,
            unconfirmed_sats: amount_sat,
            status: SwapStatus::WaitingConfirmation,
            refund_tx_ids: vec![],
            unconfirmed_tx_ids: vec![TX_ID_DUMMY.to_string()],
            confirmed_tx_ids: vec![],
            min_allowed_deposit: 1_000,
            max_allowed_deposit: 100_000_000,
            max_swapper_payable: 100_000_000,
            last_redeem_error: None,
            channel_opening_fees: None,
            confirmed_at: None,
        };

        swaps.push(swap.clone());

        self.event_listener
            .on_event(BreezEvent::SwapUpdated { details: swap });

        Ok(())
    }

    async fn confirm_swap_onchain(&self) -> Result<()> {
        let mut swaps = SWAPS.lock().await;

        if let Some(in_progress_swap) = swaps
            .iter_mut()
            .find(|s| s.status == SwapStatus::WaitingConfirmation)
        {
            in_progress_swap.confirmed_at = Some(830_000);
            in_progress_swap.confirmed_sats = in_progress_swap.unconfirmed_sats;
            in_progress_swap.unconfirmed_sats = 0;
            in_progress_swap.confirmed_tx_ids = in_progress_swap.unconfirmed_tx_ids.clone();
            in_progress_swap.unconfirmed_tx_ids = vec![];
            in_progress_swap.status = SwapStatus::Redeemable;
            self.event_listener.on_event(BreezEvent::SwapUpdated {
                details: in_progress_swap.clone(),
            });
        } else {
            bail!("No swap is waiting confirmation")
        }

        Ok(())
    }
    async fn issue_clear_wallet_tx(&self) {
        Self::change_status_of_all_reverse_swaps(ReverseSwapStatus::CompletedSeen).await;
    }
    async fn confirm_clear_wallet_tx(&self) {
        Self::change_status_of_all_reverse_swaps(ReverseSwapStatus::CompletedConfirmed).await;
    }
    async fn simulate_clear_wallet_cancellation(&self) {
        Self::change_status_of_all_reverse_swaps(ReverseSwapStatus::Cancelled).await;
    }

    async fn change_status_of_all_reverse_swaps(status: ReverseSwapStatus) {
        PAYMENTS.lock().unwrap().iter_mut().for_each(|payment| {
            if let Ln { ref mut data } = payment.details {
                if let Some(reverse_swap_info) = &mut data.reverse_swap_info {
                    reverse_swap_info.status = status;
                }
            }
        });
    }

    async fn expire_swap(&self) -> Result<()> {
        let mut swaps = SWAPS.lock().await;

        if let Some(redeemaable_swap) = swaps
            .iter_mut()
            .find(|s| s.status == SwapStatus::Redeemable)
        {
            redeemaable_swap.status = SwapStatus::Refundable;
            self.event_listener.on_event(BreezEvent::SwapUpdated {
                details: redeemaable_swap.clone(),
            });
        } else {
            bail!("No swap is redeemable")
        }

        Ok(())
    }

    fn simulate_activities(&self, amount_msat: u64) {
        self.simulate_channle_closes(amount_msat);
    }

    fn simulate_channle_closes(&self, amount_msat: u64) {
        close_channel(Channel {
            capacity_msat: 10_000_000,
            local_balance_msat: 0,
        });
        close_channel(Channel {
            capacity_msat: 10_000_000,
            local_balance_msat: amount_msat,
        });
        confirm_pending_channel_closes();

        close_channel(Channel {
            capacity_msat: 20_000_000,
            local_balance_msat: 0,
        });
        close_channel(Channel {
            capacity_msat: 20_000_000,
            local_balance_msat: 7_000_000,
        });
    }

    async fn simulate_payments(
        &self,
        payment_type: PaymentType,
        number_of_payments: u32,
        ln_address: bool,
    ) {
        for _ in 0..number_of_payments {
            let amount_msat = rand::thread_rng().gen_range(1000..1_000_000_000);
            let (invoice, preimage, payment_hash) = self.create_invoice(amount_msat, "");

            let ln_address = if ln_address {
                let mut rng = rand::thread_rng();
                let prefix: Vec<u8> = (0..10).map(|_| rng.gen_range(b'a'..=b'z')).collect();
                Some(format!(
                    "{}@wallet.lipa.swiss",
                    String::from_utf8(prefix).unwrap()
                ))
            } else {
                None
            };

            let payment = create_payment(MockPayment {
                payment_type: payment_type.clone(),
                amount_msat,
                fee_msat: rand::thread_rng().gen_range(1000..4000),
                description: None,
                payment_hash,
                payment_preimage: preimage,
                destination_pubkey: PAYEE_PUBKEY_DUMMY.to_string(),
                bolt11: invoice,
                lnurl_pay_domain: None,
                lnurl_pay_comment: None,
                ln_address,
                lnurl_metadata: None,
                lnurl_withdraw_endpoint: None,
                swap_info: None,
                reverse_swap_info: None,
            });

            PAYMENTS.lock().unwrap().push(payment.clone());
        }
    }

    pub async fn generate_diagnostic_data(&self) -> SdkResult<String> {
        Ok("Dummy diagnostics".to_string())
    }

    pub async fn close_lsp_channels(&self) -> SdkResult<Vec<String>> {
        // No need to implement this in the mock
        Ok(Vec::new())
    }

    fn create_invoice(&self, amount_msat: u64, description: &str) -> (String, String, String) {
        let (preimage, payment_hash) = generate_2_hashes_raw();
        let preimage = format!("{:x}", preimage);
        let payment_secret = PaymentSecret([42u8; 32]);

        let invoice = InvoiceBuilder::new(Currency::Bitcoin)
            .amount_milli_satoshis(amount_msat)
            .description(description.to_string())
            .payment_hash(payment_hash)
            .payment_secret(payment_secret)
            .current_timestamp()
            .min_final_cltv_expiry_delta(144)
            .build_signed(|hash| Secp256k1::new().sign_ecdsa_recoverable(hash, &self.priv_key))
            .unwrap();

        (invoice.to_string(), preimage, payment_hash.to_string())
    }
}

pub async fn parse(input: &str) -> Result<InputType> {
    let input = input.strip_prefix("lightning:").unwrap_or(input).trim();

    if let Ok(invoice) = parse_invoice(input) {
        return Ok(Bolt11 { invoice });
    }

    if input.starts_with("bc1q") || input.starts_with("bc1p") {
        return Ok(InputType::BitcoinAddress {
            address: BitcoinAddressData {
                address: input.to_string(),
                network: Network::Bitcoin,
                amount_sat: None,
                label: None,
                message: None,
            },
        });
    }

    if input.contains('@') {
        let domain = input.split('@').last().unwrap();
        return Ok(InputType::LnUrlPay {
            data: LnUrlPayRequestData {
                callback: format!("https://{domain}/lnurl-pay/callback/e9a0f330f34ac16d297094f568060d267bac6319a7f0d06eaf89d7fc1512f39a"),
                min_sendable: 1,
                max_sendable: 1_000_000_000,
                metadata_str: "[[\"text/plain\",\"dummy\"],[\"text/long-desc\",\"dummy description\"]]".to_string(),
                comment_allowed: 100,
                domain: domain.to_string(),
                allows_nostr: false,
                nostr_pubkey: None,
                ln_address: Some(input.to_string()),
            },
        });
    }

    if input.starts_with("02") || input.starts_with("03") {
        return Ok(InputType::NodeId {
            node_id: input.to_string(),
        });
    }

    if input.starts_with("http") {
        return Ok(InputType::Url {
            url: input.to_string(),
        });
    }

    if input.to_lowercase().starts_with("lnurl") {
        let (_hrp, data) = bech32::decode(input).unwrap();

        let decoded_url = std::str::from_utf8(&data).unwrap();

        if decoded_url.contains("lnurl-pay") {
            return Ok(InputType::LnUrlPay {
                data: LnUrlPayRequestData {
                    callback: "https://lnurl.dummy.com/lnurl-pay/callback/e9a0f330f34ac16d297094f568060d267bac6319a7f0d06eaf89d7fc1512f39a".to_string(),
                    min_sendable: 1,
                    max_sendable: 1_000_000_000,
                    metadata_str: "[[\"text/plain\",\"dummy\"],[\"text/long-desc\",\"dummy description\"]]".to_string(),
                    comment_allowed: 100,
                    domain: "lnurl.dummy.com".to_string(),
                    allows_nostr: false,
                    nostr_pubkey: None,
                    ln_address: None,
                },
            });
        } else {
            return Ok(InputType::LnUrlWithdraw {
                data: LnUrlWithdrawRequestData {
                    callback: "https://lnurl.dummy.com/lnurl-withdraw/callback/e9a0f330f34ac16d297094f568060d267bac6319a7f0d06eaf89d7fc1512f39a".to_string(),
                    k1: "".to_string(),
                    default_description: "dummy default description".to_string(),
                    min_withdrawable: 0,
                    max_withdrawable: 30_000_000,
                },
            });
        }
    }

    anyhow::bail!("Invalid input");
}

/// Returns channel opening fees in msats.
/// Fails if a channel is needed and amount_msat is not enough to cover it.
async fn receive_payment_mock_channels(amount_msat: u64) -> Result<u64> {
    if amount_msat <= get_inbound_liquidity_msat() {
        let mut amount_left = amount_msat;
        for c in CHANNELS.lock().unwrap().iter_mut() {
            if c.get_inbound_capacity_msat() > 0 {
                let amount_to_receive = min(amount_left, c.get_inbound_capacity_msat());
                amount_left -= amount_to_receive;
                c.local_balance_msat += amount_to_receive;
            }
            if amount_left == 0 {
                break;
            }
        }
        Ok(0)
    } else {
        let mut channels = CHANNELS.lock().unwrap();
        let lsp_fee = max(
            OPENING_FEE_PARAMS_MIN_MSAT,
            amount_msat * OPENING_FEE_PARAMS_PROPORTIONAL as u64 / 10_000,
        );
        if amount_msat < lsp_fee {
            return Err(anyhow!(
                "Invalid amount_msat ({amount_msat}) - not enough to cover channel opening fees ({lsp_fee})"
            ));
        }
        channels.push(Channel {
            capacity_msat: amount_msat + LSP_ADDED_LIQUIDITY_ON_NEW_CHANNELS_MSAT,
            local_balance_msat: amount_msat - lsp_fee,
        });
        Ok(lsp_fee)
    }
}

async fn send_payment_mock_channels(amount_with_fees_msat: u64) {
    if amount_with_fees_msat > get_balance_msat() {
        panic!("Not enough balance");
    }

    let mut amount_left = amount_with_fees_msat;
    for c in CHANNELS.lock().unwrap().iter_mut() {
        if c.local_balance_msat < amount_left {
            amount_left -= c.local_balance_msat;
            c.local_balance_msat = 0;
        } else {
            c.local_balance_msat -= amount_left;
            break;
        }
    }
}

fn get_balance_msat() -> u64 {
    CHANNELS
        .lock()
        .unwrap()
        .iter()
        .map(|c| c.local_balance_msat)
        .sum()
}

fn get_inbound_liquidity_msat() -> u64 {
    CHANNELS
        .lock()
        .unwrap()
        .iter()
        .map(|c| c.get_inbound_capacity_msat())
        .sum()
}

async fn close_channel_with_largest_balance() {
    let mut channels = CHANNELS.lock().unwrap();
    let max_index = channels
        .iter()
        .enumerate()
        .max_by_key(|(_, c)| c.local_balance_msat)
        .map(|(i, _)| i);

    if let Some(i) = max_index {
        let channel = channels.remove(i);
        close_channel(channel);
    }
}

fn close_channel(channel: Channel) {
    let now = Utc::now().timestamp();
    PAYMENTS.lock().unwrap().push(Payment {
        id: now.to_string(),
        payment_type: PaymentType::ClosedChannel,
        payment_time: now,
        amount_msat: channel.local_balance_msat,
        fee_msat: 0,
        status: PaymentStatus::Pending,
        error: None,
        description: None,
        details: PaymentDetails::ClosedChannel {
            data: ClosedChannelPaymentDetails {
                state: ChannelState::PendingClose,
                funding_txid: TX_ID_DUMMY.to_string(),
                short_channel_id: Some("mock_short_channel_id".to_string()),
                closing_txid: Some(TX_ID_DUMMY.to_string()),
            },
        },
        metadata: None,
    });
    CHANNELS_PENDING_CLOSE.lock().unwrap().push(channel);
}

fn get_onchain_balance_msat() -> u64 {
    CHANNELS_CLOSED
        .lock()
        .unwrap()
        .iter()
        .map(|c| c.local_balance_msat)
        .sum()
}

fn get_pending_onchain_balance_msat() -> u64 {
    CHANNELS_PENDING_CLOSE
        .lock()
        .unwrap()
        .iter()
        .map(|c| c.local_balance_msat)
        .sum()
}

fn confirm_pending_channel_closes() {
    CHANNELS_CLOSED
        .lock()
        .unwrap()
        .append(CHANNELS_PENDING_CLOSE.lock().unwrap().as_mut());

    PAYMENTS
        .lock()
        .unwrap()
        .iter_mut()
        .filter(|p| p.payment_type == PaymentType::ClosedChannel)
        .for_each(|p| {
            p.status = PaymentStatus::Complete;
            if let PaymentDetails::ClosedChannel { ref mut data } = &mut p.details {
                data.state = ChannelState::Closed
            }
        });
}

fn generate_2_hashes() -> (String, String) {
    let hashes = generate_2_hashes_raw();

    (format!("{:x}", hashes.0), format!("{:x}", hashes.1))
}
fn generate_2_hashes_raw() -> (sha256::Hash, sha256::Hash) {
    let hash1 = sha256::Hash::hash(&generate_32_random_bytes());

    (hash1, Hash::hash(hash1.as_byte_array()))
}

fn get_lsp_fee_params_expiry_dates() -> (DateTime<Utc>, DateTime<Utc>) {
    let now = Utc::now();
    let in_two_hours = now + Duration::from_secs(60 * 60 * 2);
    let in_three_days = now + Duration::from_secs(60 * 60 * 24 * 3);

    (in_two_hours, in_three_days)
}

struct MockPayment {
    payment_type: PaymentType,
    amount_msat: u64,
    fee_msat: u64,
    description: Option<String>,
    payment_hash: String,
    payment_preimage: String,
    destination_pubkey: String,
    bolt11: String,
    lnurl_pay_domain: Option<String>,
    lnurl_pay_comment: Option<String>,
    ln_address: Option<String>,
    lnurl_metadata: Option<String>,
    lnurl_withdraw_endpoint: Option<String>,
    swap_info: Option<SwapInfo>,
    reverse_swap_info: Option<ReverseSwapInfo>,
}

fn create_payment(p: MockPayment) -> Payment {
    let now = Utc::now().timestamp();

    Payment {
        id: now.to_string(), // Placeholder. ID is probably never used
        payment_type: p.payment_type,
        payment_time: now,
        amount_msat: p.amount_msat,
        fee_msat: p.fee_msat,
        status: PaymentStatus::Complete,
        error: None,
        description: p.description,
        details: PaymentDetails::Ln {
            data: LnPaymentDetails {
                payment_hash: p.payment_hash,
                label: "".to_string(),
                destination_pubkey: p.destination_pubkey,
                payment_preimage: p.payment_preimage,
                keysend: false,
                bolt11: p.bolt11,
                open_channel_bolt11: None,
                lnurl_success_action: None,
                lnurl_pay_domain: p.lnurl_pay_domain,
                lnurl_pay_comment: p.lnurl_pay_comment,
                ln_address: p.ln_address,
                lnurl_metadata: p.lnurl_metadata,
                lnurl_withdraw_endpoint: p.lnurl_withdraw_endpoint,
                swap_info: p.swap_info,
                reverse_swap_info: p.reverse_swap_info,
                pending_expiration_block: None,
            },
        },
        metadata: None,
    }
}

fn generate_32_random_bytes() -> Vec<u8> {
    let mut bytes = vec![0; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes
}
