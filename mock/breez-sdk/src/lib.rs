use anyhow::Result;
use bitcoin::hashes::{sha256, Hash};
use lightning::ln::PaymentSecret;
use lightning_invoice::{Currency, InvoiceBuilder};
use rand::{Rng, RngCore};
use secp256k1::{Secp256k1, SecretKey};
use std::cmp::max;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const NODE_PRIVKEY: &[u8] = &[
    0xe1, 0x26, 0xf6, 0x8f, 0x7e, 0xaf, 0xcc, 0x8b, 0x74, 0xf5, 0x4d, 0x26, 0x9f, 0xe2, 0x06, 0xbe,
    0x71, 0x50, 0x00, 0xf9, 0x4d, 0xac, 0x06, 0x7d, 0x1c, 0x04, 0xa8, 0xca, 0x3b, 0x2d, 0xb7, 0x34,
];
const NODE_PUBKEY: &str = "03e7156ae33b0a208d0744199163177e909e80176e55d97a2f221ede0f934dd9ad";
const MAX_RECEIVABLE_MSAT: u64 = 1_000_000_000;

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
const OPENING_FEE_PARAMS_MIN_MSAT: u64 = 5_000_000;
const OPENING_FEE_PARAMS_PROPORTIONAL: u32 = 50;
const OPENING_FEE_PARAMS_VALID_UNTIL: &str = "2030-02-16T11:46:49Z";
const OPENING_FEE_PARAMS_MAX_IDLE_TIME: u32 = 10000;
const OPENING_FEE_PARAMS_MAX_CLIENT_TO_SELF_DELAY: u32 = 256;
const OPENING_FEE_PARAMS_PROMISE: &str = "promite";

const SWAP_MIN_AMOUNT_SAT: u64 = 1_000;
const SWAP_MAX_AMOUNT_SAT: u64 = 1_000_000;
const SWAP_FEE_SAT: u64 = 1_500;
const SWAP_TX_WEIGHT: u64 = 800;
const SAT_PER_VBYTE: u64 = 12;
const SWAP_FEE_PERCENTAGE: f64 = 0.5;
const SWAP_ADDRESS_DUMMY: &str = "1BitcoinEaterAddressDontSendf59kuE";
const SWAP_RECEIVED_SATS_ON_CHAIN: u64 = 100_000;
const TX_ID_DUMMY: &str = "f4184fc596403b9d638783cf57adfe4c75c605f6356fbc91338530e9831e9e16";

use breez_sdk_core::error::{
    LnUrlPayError, LnUrlWithdrawError, ReceiveOnchainError, ReceivePaymentError, SdkResult,
    SendOnchainError, SendPaymentError,
};
use breez_sdk_core::InputType::Bolt11;
use breez_sdk_core::PaymentDetails::Ln;
pub use breez_sdk_core::{
    parse_invoice, BitcoinAddressData, BreezEvent, ClosedChannelPaymentDetails, EnvironmentType,
    EventListener, GreenlightCredentials, GreenlightNodeConfig, HealthCheckStatus, InputType,
    InvoicePaidDetails, LNInvoice, ListPaymentsRequest, LnPaymentDetails, LnUrlPayRequest,
    LnUrlPayRequestData, LnUrlPayResult, LnUrlWithdrawRequest, LnUrlWithdrawRequestData,
    LnUrlWithdrawResult, MetadataItem, Network, NodeConfig, OpenChannelFeeRequest,
    OpeningFeeParams, OpeningFeeParamsMenu, Payment, PaymentDetails, PaymentFailedData,
    PaymentStatus, PaymentType, PaymentTypeFilter, PrepareRedeemOnchainFundsRequest,
    PrepareRefundRequest, ReceiveOnchainRequest, ReceivePaymentRequest, ReceivePaymentResponse,
    RedeemOnchainFundsRequest, RefundRequest, ReportIssueRequest, ReportPaymentFailureDetails,
    ReverseSwapFeesRequest, SendOnchainRequest, SendPaymentRequest, SignMessageRequest,
};
use breez_sdk_core::{
    Config, LspInformation, MaxReverseSwapAmountResponse, NodeState, OpenChannelFeeResponse,
    PrepareRedeemOnchainFundsResponse, PrepareRefundResponse, RecommendedFees,
    RedeemOnchainFundsResponse, RefundResponse, ReverseSwapInfo, ReverseSwapPairInfo,
    ReverseSwapStatus, SendOnchainResponse, SendPaymentResponse, ServiceHealthCheckResponse,
    SignMessageResponse, SwapInfo, SwapStatus,
};
use chrono::Utc;
use hex::FromHex;
use lazy_static::lazy_static;

pub mod error {
    pub use breez_sdk_core::error::*;
}

lazy_static! {
    static ref HEALTH_STATUS: Mutex<HealthCheckStatus> = Mutex::new(HealthCheckStatus::Operational);
    static ref LN_BALANCE_MSAT: Mutex<u64> = Mutex::new(10_000_000);
    static ref PAYMENT_DELAY: Mutex<PaymentDelay> = Mutex::new(PaymentDelay::Immediate);
    static ref PAYMENT_OUTCOME: Mutex<PaymentOutcome> = Mutex::new(PaymentOutcome::Success);
    static ref PAYMENTS: Mutex<Vec<Payment>> = Mutex::new(Vec::new());
    static ref SWAPS: Mutex<Vec<SwapInfo>> = Mutex::new(Vec::new());
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

pub struct BreezServices {}

impl BreezServices {
    pub async fn connect(
        _config: Config,
        _seed: Vec<u8>,
        _event_listener: Box<dyn EventListener>,
    ) -> SdkResult<Arc<BreezServices>> {
        Ok(Arc::new(BreezServices {}))
    }
    pub async fn send_payment(
        &self,
        req: SendPaymentRequest,
    ) -> Result<SendPaymentResponse, SendPaymentError> {
        match &*PAYMENT_DELAY.lock().unwrap() {
            PaymentDelay::Immediate => {}
            PaymentDelay::Short => {
                thread::sleep(Duration::from_secs(3));
            }
            PaymentDelay::Long => {
                thread::sleep(Duration::from_secs(10));
            }
        }

        match &*PAYMENT_OUTCOME.lock().unwrap() {
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

                if *LN_BALANCE_MSAT.lock().unwrap() < amount_msat {
                    return Err(SendPaymentError::RouteNotFound {
                        err: "Ran out of routes".into(),
                    });
                } else {
                    *LN_BALANCE_MSAT.lock().unwrap() -= amount_msat;
                }

                let payment = Payment {
                    id: "".to_string(),
                    payment_type: PaymentType::Sent,
                    payment_time: Utc::now().timestamp(),
                    amount_msat,
                    fee_msat: 1234,
                    status: PaymentStatus::Complete,
                    error: None,
                    description: None,
                    details: PaymentDetails::Ln {
                        data: LnPaymentDetails {
                            payment_hash: "".to_string(),
                            label: "".to_string(),
                            destination_pubkey: "".to_string(),
                            payment_preimage: "".to_string(),
                            keysend: false,
                            bolt11: req.bolt11,
                            open_channel_bolt11: None,
                            lnurl_success_action: None,
                            lnurl_pay_domain: None,
                            ln_address: None,
                            lnurl_metadata: None,
                            lnurl_withdraw_endpoint: None,
                            swap_info: None,
                            reverse_swap_info: None,
                            pending_expiration_block: None,
                        },
                    },
                    metadata: None,
                };

                PAYMENTS.lock().unwrap().push(payment.clone());

                Ok(SendPaymentResponse { payment })
            }
            PaymentOutcome::AlreadyPaid => Err(SendPaymentError::AlreadyPaid),
            PaymentOutcome::GenericError => Err(SendPaymentError::Generic {
                err: "Generic error".into(),
            }),
            PaymentOutcome::InvalidNetwork => Err(SendPaymentError::InvalidNetwork {
                err: "Invalid network".into(),
            }),
            PaymentOutcome::InvoiceExpired => Err(SendPaymentError::InvoiceExpired {
                err: "Invoice expired".into(),
            }),
            PaymentOutcome::Failed => Err(SendPaymentError::PaymentFailed {
                err: "Payment Failed".into(),
            }),
            PaymentOutcome::Timeout => Err(SendPaymentError::PaymentTimeout {
                err: "Payment timed out".into(),
            }),
            PaymentOutcome::RouteNotFound => Err(SendPaymentError::RouteNotFound {
                err: "Route not found".into(),
            }),
            PaymentOutcome::RouteTooExpensive => Err(SendPaymentError::RouteTooExpensive {
                err: "Route too expensive".into(),
            }),
            PaymentOutcome::ServiceConnectivity => Err(SendPaymentError::ServiceConnectivity {
                err: "Service connectivity error".into(),
            }),
        }
    }

    pub async fn lnurl_pay(&self, req: LnUrlPayRequest) -> Result<LnUrlPayResult, LnUrlPayError> {
        let fee_msat = 8_000;
        let now = Utc::now().timestamp();
        let preimage = sha256::Hash::hash(&now.to_be_bytes());
        let payment_hash = format!("{:x}", sha256::Hash::hash(preimage.as_byte_array()));
        let payment_preimage = format!("{:x}", preimage);
        let bolt11 = "lnbc1486290n1pj74h6psp5tmna0gruf44rx0h7xgl2xsmn5xhjnaxktct40pkfg4m9kssytn0spp5qhpx9s8rvmw6jtzkelslve9zfuhpp2w7hn9s6q7xvdnds5jemr2qdpa2pskjepqw3hjq3r0deshgefqw3hjqjzjgcs8vv3qyq5y7unyv4ezqj2y8gszjxqy9ghlcqpjrzjqvutcqr0g2ltxthh82s8l24gy74xe862kelrywc6ktsx2gejgk26szcqygqqy6qqqyqqqqlgqqqq86qqyg9qxpqysgqzjnfufxw375gpqf9cvzd5jxyqqtm56fuw960wyel2ld3he403r7x6uyw59g5sfsj5rclycd09a8p8r2pnyrcanlg27e2a67nh5g248sp7p7s8z".to_string();
        *LN_BALANCE_MSAT.lock().unwrap() -= req.amount_msat + fee_msat;

        let payment = Payment {
            id: now.to_string(), // Placeholder. ID is probably never used
            payment_type: PaymentType::Sent,
            payment_time: now,
            amount_msat: req.amount_msat,
            fee_msat,
            status: PaymentStatus::Complete,
            error: None,
            description: None,
            details: PaymentDetails::Ln {
                data: LnPaymentDetails {
                    payment_hash: payment_hash.clone(),
                    label: "".to_string(),
                    destination_pubkey:
                        "020333076e35e398a0c14c8a0211563bbcdce5087cb300342cba09414e9b5f3605"
                            .to_string(),
                    payment_preimage,
                    keysend: false,
                    bolt11,
                    open_channel_bolt11: None,
                    lnurl_success_action: None,
                    lnurl_pay_domain: Some(req.data.domain.clone()),
                    ln_address: req.data.ln_address.clone(),
                    lnurl_metadata: Some(req.data.metadata_str.clone()),
                    lnurl_withdraw_endpoint: None,
                    swap_info: None,
                    reverse_swap_info: None,
                    pending_expiration_block: None,
                },
            },
            metadata: None,
        };

        PAYMENTS.lock().unwrap().push(payment.clone());

        Ok(LnUrlPayResult::EndpointSuccess {
            data: breez_sdk_core::LnUrlPaySuccessData {
                payment_hash,
                success_action: None,
            },
        })
    }

    pub async fn lnurl_withdraw(
        &self,
        req: LnUrlWithdrawRequest,
    ) -> Result<LnUrlWithdrawResult, LnUrlWithdrawError> {
        *LN_BALANCE_MSAT.lock().unwrap() += req.amount_msat;

        let now = Utc::now().timestamp();
        let preimage = sha256::Hash::hash(&now.to_be_bytes());
        let payment_hash = format!("{:x}", sha256::Hash::hash(preimage.as_byte_array()));
        let payment_preimage = format!("{:x}", preimage);
        let bolt11 = "lnbc1pjlq2t3pp5e3ef7wmszlwxhfpx9cfnxx34gglg779fwnwx9mfm69pfapmymt0qdqqcqzzsxqyz5vqsp5x7k3pjq5y8vk473l6767fenletzwjeaqqukpg9tspfq584g8qp4q9qyyssq678xw6gf2ywl5seummdy8pc6xd0jpvzdexd4v4d3zjse9u6jf7239va4e4r4hhauqrymxu7dp790lv98dl0qhrt4yqxwll2ufkp304gqn6798s".to_string();
        let payee_pubkey = NODE_PUBKEY.to_string();

        let payment = Payment {
            id: now.to_string(), // Placeholder. ID is probably never used
            payment_type: PaymentType::Received,
            payment_time: now,
            amount_msat: req.amount_msat,
            fee_msat: 0,
            status: PaymentStatus::Complete,
            error: None,
            description: None,
            details: PaymentDetails::Ln {
                data: LnPaymentDetails {
                    payment_hash: payment_hash.clone(),
                    label: "".to_string(),
                    destination_pubkey: payee_pubkey.clone(),
                    payment_preimage,
                    keysend: false,
                    bolt11: bolt11.clone(),
                    open_channel_bolt11: None,
                    lnurl_success_action: None,
                    lnurl_pay_domain: None,
                    ln_address: None,
                    lnurl_metadata: None,
                    lnurl_withdraw_endpoint: Some(
                        "https://lnurl.dummy.com/lnurl-withdraw".to_string(),
                    ),
                    swap_info: None,
                    reverse_swap_info: None,
                    pending_expiration_block: None,
                },
            },
            metadata: None,
        };

        PAYMENTS.lock().unwrap().push(payment.clone());

        Ok(LnUrlWithdrawResult::Ok {
            data: breez_sdk_core::LnUrlWithdrawSuccessData {
                invoice: LNInvoice {
                    bolt11,
                    network: Network::Bitcoin,
                    payee_pubkey,
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
        let mut lsp_fee: Option<u64> = None;
        // Has nothing to do with receiving a payment, but is a mechanism to control the mock
        match req.description.as_str() {
            "lsp.channel.required" => {
                lsp_fee = Some(rand::thread_rng().gen_range(2_000_000..=100_000_000));
            }
            "health.operational" => *HEALTH_STATUS.lock().unwrap() = HealthCheckStatus::Operational,
            "health.maintenance" => *HEALTH_STATUS.lock().unwrap() = HealthCheckStatus::Maintenance,
            "health.disruption" => {
                *HEALTH_STATUS.lock().unwrap() = HealthCheckStatus::ServiceDisruption
            }
            "pay.delay.immediate" => *PAYMENT_DELAY.lock().unwrap() = PaymentDelay::Immediate,
            "pay.delay.short" => *PAYMENT_DELAY.lock().unwrap() = PaymentDelay::Short,
            "pay.delay.long" => *PAYMENT_DELAY.lock().unwrap() = PaymentDelay::Long,
            "pay.success" => *PAYMENT_OUTCOME.lock().unwrap() = PaymentOutcome::Success,
            "pay.err.already_paid" => {
                *PAYMENT_OUTCOME.lock().unwrap() = PaymentOutcome::AlreadyPaid
            }
            "pay.err.generic" => *PAYMENT_OUTCOME.lock().unwrap() = PaymentOutcome::GenericError,
            "pay.err.network" => *PAYMENT_OUTCOME.lock().unwrap() = PaymentOutcome::InvalidNetwork,
            "pay.err.expired" => *PAYMENT_OUTCOME.lock().unwrap() = PaymentOutcome::InvoiceExpired,
            "pay.err.failed" => *PAYMENT_OUTCOME.lock().unwrap() = PaymentOutcome::Failed,
            "pay.err.timeout" => *PAYMENT_OUTCOME.lock().unwrap() = PaymentOutcome::Timeout,
            "pay.err.route" => *PAYMENT_OUTCOME.lock().unwrap() = PaymentOutcome::RouteNotFound,
            "pay.err.route_too_expensive" => {
                *PAYMENT_OUTCOME.lock().unwrap() = PaymentOutcome::RouteTooExpensive
            }
            "pay.err.connectivity" => {
                *PAYMENT_OUTCOME.lock().unwrap() = PaymentOutcome::ServiceConnectivity
            }
            _ => {}
        }

        let private_key = SecretKey::from_slice(NODE_PRIVKEY).unwrap();
        let mut preimage: [u8; 32] = [0; 32];
        rand::thread_rng().fill_bytes(&mut preimage);
        let preimage = req.preimage.unwrap_or(preimage.to_vec());
        let payment_hash = sha256::Hash::hash(&preimage);
        let preimage = hex::encode(preimage);
        let payment_secret = PaymentSecret([42u8; 32]);

        let invoice = InvoiceBuilder::new(Currency::Bitcoin)
            .amount_milli_satoshis(req.amount_msat)
            .description(req.description.clone())
            .payment_hash(payment_hash)
            .payment_secret(payment_secret)
            .current_timestamp()
            .min_final_cltv_expiry_delta(144)
            .build_signed(|hash| Secp256k1::new().sign_ecdsa_recoverable(hash, &private_key))
            .unwrap();

        let description = Option::from(req.description);

        if let PaymentOutcome::Success = &*PAYMENT_OUTCOME.lock().unwrap() {
            *LN_BALANCE_MSAT.lock().unwrap() += req.amount_msat - lsp_fee.unwrap_or(0);
            PAYMENTS.lock().unwrap().push(Payment {
                id: Utc::now().timestamp().to_string(), // Placeholder. ID is probably never used
                payment_type: PaymentType::Received,
                payment_time: Utc::now().timestamp(),
                amount_msat: req.amount_msat - lsp_fee.unwrap_or(0),
                fee_msat: lsp_fee.unwrap_or(0),
                status: PaymentStatus::Complete,
                error: None,
                description: description.clone(),
                details: PaymentDetails::Ln {
                    data: LnPaymentDetails {
                        payment_hash: format!("{:x}", payment_hash),
                        label: "".to_string(),
                        destination_pubkey: NODE_PUBKEY.to_string(),
                        payment_preimage: preimage,
                        keysend: false,
                        bolt11: invoice.to_string(),
                        open_channel_bolt11: None,
                        lnurl_success_action: None,
                        lnurl_pay_domain: None,
                        ln_address: None,
                        lnurl_metadata: None,
                        lnurl_withdraw_endpoint: None,
                        swap_info: None,
                        reverse_swap_info: None,
                        pending_expiration_block: None,
                    },
                },
                metadata: None,
            });
        }

        Ok(ReceivePaymentResponse {
            ln_invoice: LNInvoice {
                bolt11: invoice.to_string(),
                network: Network::Bitcoin,
                payee_pubkey: NODE_PUBKEY.to_string(),
                payment_hash: format!("{:x}", payment_hash),
                description,
                description_hash: None,
                amount_msat: Some(req.amount_msat),
                timestamp: Utc::now().timestamp() as u64,
                expiry: 0,
                routing_hints: vec![],
                payment_secret: Vec::from(SAMPLE_PAYMENT_SECRET.as_bytes()),
                min_final_cltv_expiry_delta: 144,
            },
            opening_fee_params: None,
            opening_fee_msat: lsp_fee,
        })
    }

    pub async fn service_health_check(&self) -> SdkResult<ServiceHealthCheckResponse> {
        Ok(ServiceHealthCheckResponse {
            status: HEALTH_STATUS.lock().unwrap().clone(),
        })
    }

    // Not useful for the mock, but required to keep same interface
    pub async fn report_issue(&self, _req: ReportIssueRequest) -> SdkResult<()> {
        Ok(())
    }

    pub fn node_info(&self) -> SdkResult<NodeState> {
        let balance = *LN_BALANCE_MSAT.lock().unwrap();

        Ok(NodeState {
            id: NODE_PUBKEY.to_string(),
            block_height: 1234567,
            channels_balance_msat: balance,
            onchain_balance_msat: 0,
            pending_onchain_balance_msat: 0,
            utxos: vec![],
            max_payable_msat: balance,
            max_receivable_msat: MAX_RECEIVABLE_MSAT,
            max_single_payment_amount_msat: balance,
            max_chan_reserve_msats: 0,
            connected_peers: vec![LSP_ID.to_string()],
            inbound_liquidity_msats: MAX_RECEIVABLE_MSAT,
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
                if let Ln { data } = &p.details {
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
    ) -> SdkResult<PrepareRedeemOnchainFundsResponse> {
        Ok(PrepareRedeemOnchainFundsResponse {
            tx_weight: SWAP_TX_WEIGHT,
            tx_fee_sat: SWAP_TX_WEIGHT / 4 * (req.sat_per_vbyte as u64),
        })
    }

    pub async fn redeem_onchain_funds(
        &self,
        _req: RedeemOnchainFundsRequest,
    ) -> SdkResult<RedeemOnchainFundsResponse> {
        Ok(RedeemOnchainFundsResponse {
            txid: Vec::from_hex(TX_ID_DUMMY).unwrap(),
        })
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
        _req: OpenChannelFeeRequest,
    ) -> SdkResult<OpenChannelFeeResponse> {
        Ok(OpenChannelFeeResponse {
            fee_msat: Some(0),
            fee_params: OpeningFeeParams {
                min_msat: OPENING_FEE_PARAMS_MIN_MSAT,
                proportional: OPENING_FEE_PARAMS_PROPORTIONAL,
                valid_until: OPENING_FEE_PARAMS_VALID_UNTIL.to_string(),
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
            confirmed_sats: 0,
            unconfirmed_sats: 0,
            status: SwapStatus::Initial,
            refund_tx_ids: vec![],
            unconfirmed_tx_ids: vec![],
            confirmed_tx_ids: vec![],
            min_allowed_deposit: 1_000,
            max_allowed_deposit: 1_000_000,
            last_redeem_error: None,
            channel_opening_fees: None,
        };

        SWAPS.lock().unwrap().push(swap.clone());

        Ok(swap)
    }

    pub async fn in_progress_swap(&self) -> SdkResult<Option<SwapInfo>> {
        self.sync().await?;

        Ok(SWAPS
            .lock()
            .unwrap()
            .iter()
            .find(|swap| {
                (swap.confirmed_sats > 0 || swap.unconfirmed_sats > 0)
                    && swap.paid_msat == 0
                    && swap.status != SwapStatus::Expired
            })
            .cloned())
    }

    pub async fn fetch_reverse_swap_fees(
        &self,
        req: ReverseSwapFeesRequest,
    ) -> SdkResult<ReverseSwapPairInfo> {
        let total_estimated_fees = req
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
            total_estimated_fees,
        })
    }

    pub async fn max_reverse_swap_amount(&self) -> SdkResult<MaxReverseSwapAmountResponse> {
        let total_sat = *LN_BALANCE_MSAT.lock().unwrap() / 1000 - SWAP_FEE_SAT;

        Ok(MaxReverseSwapAmountResponse { total_sat })
    }

    pub async fn send_onchain(
        &self,
        req: SendOnchainRequest,
    ) -> Result<SendOnchainResponse, SendOnchainError> {
        let now = Utc::now().timestamp();

        let amount_msat = req.amount_sat * 1_000;
        let fee_msat = *LN_BALANCE_MSAT.lock().unwrap() - amount_msat;
        *LN_BALANCE_MSAT.lock().unwrap() -= amount_msat + fee_msat;

        let reverse_swap_info = ReverseSwapInfo {
            id: now.to_string(),
            claim_pubkey: req.onchain_recipient_address,
            lockup_txid: Some("LOCKUP-TXID-DUMMY".to_string()),
            claim_txid: Some("CLAIM-TXID-DUMMY".to_string()),
            onchain_amount_sat: req.amount_sat,
            status: ReverseSwapStatus::Initial,
        };

        PAYMENTS.lock().unwrap().push(Payment {
            id: format!("random_id_{}", now),
            payment_type: PaymentType::Sent,
            payment_time: now,
            amount_msat,
            fee_msat,
            status: PaymentStatus::Complete,
            error: None,
            description: None,
            details: PaymentDetails::Ln {
                data: LnPaymentDetails {
                    payment_hash: "".to_string(),
                    label: "".to_string(),
                    destination_pubkey: "".to_string(),
                    payment_preimage: "".to_string(),
                    keysend: false,
                    bolt11: "lnbc1486290n1pj74h6psp5tmna0gruf44rx0h7xgl2xsmn5xhjnaxktct40pkfg4m9kssytn0spp5qhpx9s8rvmw6jtzkelslve9zfuhpp2w7hn9s6q7xvdnds5jemr2qdpa2pskjepqw3hjq3r0deshgefqw3hjqjzjgcs8vv3qyq5y7unyv4ezqj2y8gszjxqy9ghlcqpjrzjqvutcqr0g2ltxthh82s8l24gy74xe862kelrywc6ktsx2gejgk26szcqygqqy6qqqyqqqqlgqqqq86qqyg9qxpqysgqzjnfufxw375gpqf9cvzd5jxyqqtm56fuw960wyel2ld3he403r7x6uyw59g5sfsj5rclycd09a8p8r2pnyrcanlg27e2a67nh5g248sp7p7s8z".to_string(),
                    open_channel_bolt11: None,
                    lnurl_success_action: None,
                    lnurl_pay_domain: None,
                    ln_address: None,
                    lnurl_metadata: None,
                    lnurl_withdraw_endpoint: None,
                    swap_info: None,
                    reverse_swap_info: Some(reverse_swap_info.clone()),
                    pending_expiration_block: None,
                },
            },
            metadata: None,
        });

        Ok(SendOnchainResponse {
            reverse_swap_info: reverse_swap_info,
        })
    }

    pub async fn list_refundables(&self) -> SdkResult<Vec<SwapInfo>> {
        self.sync().await?;

        let swaps = SWAPS.lock().unwrap().clone();
        Ok(swaps
            .into_iter()
            .filter(|swap| {
                (swap.confirmed_sats > 0 || swap.unconfirmed_sats > 0)
                    && swap.paid_msat == 0
                    && swap.status == SwapStatus::Expired
            })
            .collect())
    }
    pub async fn prepare_refund(
        &self,
        _req: PrepareRefundRequest,
    ) -> SdkResult<PrepareRefundResponse> {
        Ok(PrepareRefundResponse {
            refund_tx_weight: (SWAP_TX_WEIGHT / 4 * SAT_PER_VBYTE) as u32,
            refund_tx_fee_sat: SWAP_TX_WEIGHT,
        })
    }

    pub async fn refund(&self, _req: RefundRequest) -> SdkResult<RefundResponse> {
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
        Self::simulate_swaps();

        Ok(())
    }

    pub async fn lsp_info(&self) -> SdkResult<LspInformation> {
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
            opening_fee_params_list: OpeningFeeParamsMenu { values: vec![] },
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

    fn simulate_swaps() {
        let now = Utc::now();
        let in_one_hour = now + chrono::Duration::try_hours(1).unwrap();
        let valid_until = in_one_hour.to_rfc3339();
        let now = now.timestamp();

        let mut swaps = SWAPS.lock().unwrap();
        swaps.iter_mut().for_each(|swap| {
            if now - swap.created_at > 30 && swap.created_at <= 60 {
                swap.unconfirmed_sats = SWAP_RECEIVED_SATS_ON_CHAIN;
            } else if now - swap.created_at > 60 {
                swap.unconfirmed_sats = 0;
                swap.confirmed_sats = SWAP_RECEIVED_SATS_ON_CHAIN;
            }

            // swap is redeemable for 60 seconds until it is expired and must be refunded
            if now - swap.created_at < 120 {
                swap.channel_opening_fees = Some(OpeningFeeParams {
                    min_msat: 3_000_000,
                    proportional: 10_000,
                    valid_until: valid_until.clone(),
                    max_idle_time: 0,
                    max_client_to_self_delay: 0,
                    promise: "promise".to_string(),
                });
            } else {
                swap.status = SwapStatus::Expired;
            }
        });
    }
}

pub async fn parse(input: &str) -> Result<InputType> {
    let input = input.trim();

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

    // Without requesting the server, it is not possible to know whether an LNURL string is a pay or withdraw request
    // So instead we interpret the string 'lnurlp' as if it was an LNUrL-pay string
    if input == "lnurlp" {
        return Ok(InputType::LnUrlPay {
            data: LnUrlPayRequestData {
                callback: "https://lnurl.dummy.com/lnurl-pay/callback/e9a0f330f34ac16d297094f568060d267bac6319a7f0d06eaf89d7fc1512f39a".to_string(),
                min_sendable: 1,
                max_sendable: 1_000_000_000,
                metadata_str: "[[\"text/plain\",\"dummy\"],[\"text/long-desc\",\"dummy description\"]]".to_string(),
                comment_allowed: 100,
                domain: "lnurl.dummy.com".to_string(),
                ln_address: None,
            },
        });
    }

    Ok(InputType::LnUrlWithdraw {
        data: LnUrlWithdrawRequestData {
            callback: "https://lnurl.dummy.com/lnurl-withdraw/callback/e9a0f330f34ac16d297094f568060d267bac6319a7f0d06eaf89d7fc1512f39a".to_string(),
            k1: "".to_string(),
            default_description: "dummy default description".to_string(),
            min_withdrawable: 0,
            max_withdrawable: 30_000_000,
        },
    })
}
