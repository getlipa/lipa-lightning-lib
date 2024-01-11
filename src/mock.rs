use crate::amount::{AsSats, ToAmount};
use crate::async_runtime::AsyncRuntime;
use crate::errors::LnUrlWithdrawResult;
use crate::{
    Amount, BreezHealthCheckStatus, CalculateLspFeeResponse, ChannelsInfo, ClearWalletInfo,
    DecodeDataError, DecodedData, EventsCallback, ExchangeRate, FailedSwapInfo, FiatTopupInfo,
    InvoiceAffordability, InvoiceCreationMetadata, InvoiceDetails, LightningNode,
    ListPaymentsResponse, LnUrlPayResult, LspFee, MaxRoutingFeeMode, NodeInfo, OfferInfo,
    PayResult, Payment, PaymentAmountLimits, PaymentMetadata, PaymentState, PaymentType,
    ResolveFailedSwapInfo, RuntimeErrorCode, SwapAddressInfo, SweepInfo, TzConfig, TzTime,
    UnsupportedDataType,
};
use breez_sdk_core::error::ReceiveOnchainError;
use breez_sdk_core::{
    BitcoinAddressData, LnUrlPayRequestData, LnUrlWithdrawRequestData, OpeningFeeParams,
};
use honey_badger::{TermsAndConditions, TermsAndConditionsStatus};
use perro::{invalid_input, runtime_error};
use std::ops::Add;
use std::string::ToString;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

pub struct MockLightningNode {
    rt: AsyncRuntime,
    events_callback: Arc<Box<dyn EventsCallback>>,
    payments: Vec<Payment>,
}

impl MockLightningNode {
    pub fn new(events_callback: Box<dyn EventsCallback>) -> crate::Result<Self> {
        let rt = AsyncRuntime::new()?;

        Ok(Self {
            rt,
            events_callback: Arc::new(events_callback),
            payments: vec![Payment {
                payment_type: PaymentType::Receiving,
                payment_state: PaymentState::Succeeded,
                fail_reason: None,
                hash: "".to_string(),
                amount: mock_amount(25421),
                requested_amount: mock_amount(25421),
                invoice_details: InvoiceDetails {
                    invoice: "".to_string(),
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
                preimage: Some("".to_string()),
                network_fees: None,
                lsp_fees: Some(mock_amount(0)),
                offer: None,
                swap: None,
                lightning_address: None,
            }],
        })
    }
}

impl LightningNode for MockLightningNode {
    fn get_node_info(&self) -> crate::Result<NodeInfo> {
        Ok(NodeInfo {
            node_pubkey: "020333076e35e398a0c14c8a0211563bbcdce5087cb300342cba09414e9b5f3605"
                .to_string(),
            peers: vec![
                "0264a62a4307d701c04a46994ce5f5323b1ca28c80c66b73c631dbcb0990d6e835".to_string(),
            ],
            onchain_balance: mock_amount(43_000),
            channels_info: ChannelsInfo {
                local_balance: mock_amount(132_321),
                inbound_capacity: mock_amount(900_000),
                outbound_capacity: mock_amount(132_321),
            },
        })
    }

    fn query_lsp_fee(&self) -> crate::Result<LspFee> {
        todo!()
    }

    fn calculate_lsp_fee(&self, amount_sat: u64) -> crate::Result<CalculateLspFeeResponse> {
        todo!()
    }

    fn get_payment_amount_limits(&self) -> crate::Result<PaymentAmountLimits> {
        todo!()
    }

    fn create_invoice(
        &self,
        amount_sat: u64,
        _lsp_fee_params: Option<OpeningFeeParams>,
        description: String,
        _metadata: InvoiceCreationMetadata,
    ) -> crate::Result<InvoiceDetails> {
        // TODO how to mock both successful and failed invoices

        let events_callback = Arc::clone(&self.events_callback);
        let _handle = self.rt.handle().spawn(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            events_callback.payment_received(
                "4b103110b896f7ab72df521dad8bfbe95d84868381a5471ca4ab2a2dd2d6d22e".to_string(),
            );
        });

        Ok(InvoiceDetails {
            invoice: "lnbc250u1pjea89asp5xsq3f4rqv9my2h0wasjz6tjqx09rhs7gu7gtencl9zzdf8ex6grspp5fvgrzy9cjmm6kukl2gw6mzlma9wcfp5rsxj5w89y4v4zm5kk6ghqdq5f4hkx6eqd9h8vmmfvdjscqzysrzjqtypret4hcklglvtfrdt85l3exc0dctdp4qttmtcy5es3lpt6utsmren8w7cwh8zesqqqqlgqqqqqzsqyg9qxpqysgqwq0at669s9t56j5r5m3nj4gj2pm7p7xpexsvwl58scnld3krfcshnawtgvmng7la2ngluqns3g3ysmwgc53ruhqlzkkglq7shy30q6gq64se8y".to_string(),
            amount: Some(mock_amount(amount_sat)),
            description,
            payment_hash: "4b103110b896f7ab72df521dad8bfbe95d84868381a5471ca4ab2a2dd2d6d22e".to_string(),
            payee_pub_key: "020333076e35e398a0c14c8a0211563bbcdce5087cb300342cba09414e9b5f3605".to_string(),
            creation_timestamp: SystemTime::now(),
            expiry_interval: Default::default(),
            expiry_timestamp: SystemTime::now().add(Duration::from_secs(60 * 60 * 2)),
        })
    }

    fn decode_data(&self, data: String) -> Result<DecodedData, DecodeDataError> {
        match data.as_str() {
            "Bolt11Invoice" => Ok(DecodedData::Bolt11Invoice {
                invoice_details: InvoiceDetails {
                    invoice: "lnbc50u1pjeag4nsp53qs709cxm4t3e5dlygngl5hhk49fjewt598q3ffhmpj055xjxd9qpp53544rx97gv8t7qd05rqmnv3c7da9tm2rm8h806hsrf8mxll49wnqdqqcqzysrzjqtypret4hcklglvtfrdt85l3exc0dctdp4qttmtcy5es3lpt6uts6yggsxjpeup35qqqqqqqqqqqqqqqyg9qxpqysgq06ygs3xampvmwwpf4e25trau52pl74nqm6pqkwdw49cn670a8vqyrg8kmskumlrzwudn85e2paadzdz3lgwz8jgfddrk3wxfpe3utlgptn0sv8".to_string(),
                    amount: Some(mock_amount(5_000)),
                    description: "Mock Bolt 11 Invoice".to_string(),
                    payment_hash: "8d2b5198be430ebf01afa0c1b9b238f37a55ed43d9ee77eaf01a4fb37ff52ba6".to_string(),
                    payee_pub_key: "03079ed23b76b4f89d93ac35606669e5d7b7a73f07ccd0fbd6b4f8a0512dcbb9a4".to_string(),
                    creation_timestamp: SystemTime::now(),
                    expiry_interval: Default::default(),
                    expiry_timestamp: SystemTime::now().add(Duration::from_secs(60 * 60 * 2)),
                },
            }),
            "LnUrlPay" => Err(DecodeDataError::Unsupported { typ: UnsupportedDataType::Url }),
            "LnUrlWithdraw" => Err(DecodeDataError::Unsupported { typ: UnsupportedDataType::Url }),
            "OnchainAddress" => Err(DecodeDataError::Unsupported { typ: UnsupportedDataType::BitcoinAddress }),
            _ => Err(DecodeDataError::Unrecognized { msg: format!("Unrecognized data: {}", data) })
        }
    }

    fn get_payment_max_routing_fee_mode(&self, amount_sat: u64) -> MaxRoutingFeeMode {
        todo!()
    }

    fn get_invoice_affordability(&self, amount_sat: u64) -> crate::Result<InvoiceAffordability> {
        todo!()
    }

    fn pay_invoice(
        &self,
        invoice_details: InvoiceDetails,
        _metadata: PaymentMetadata,
    ) -> PayResult<()> {
        if invoice_details.payment_hash
            == *"8d2b5198be430ebf01afa0c1b9b238f37a55ed43d9ee77eaf01a4fb37ff52ba6".to_string()
        {
            return Ok(());
        }

        invalid_input!("Invalid invoice")
    }

    fn pay_open_invoice(
        &self,
        invoice_details: InvoiceDetails,
        amount_sat: u64,
        metadata: PaymentMetadata,
    ) -> PayResult<()> {
        todo!()
    }

    fn pay_lnurlp(
        &self,
        lnurl_pay_request_data: LnUrlPayRequestData,
        amount_sat: u64,
    ) -> LnUrlPayResult<String> {
        todo!()
    }

    fn list_lightning_addresses(&self) -> crate::Result<Vec<String>> {
        todo!()
    }

    fn withdraw_lnurlw(
        &self,
        lnurl_withdraw_request_data: LnUrlWithdrawRequestData,
        amount_sat: u64,
    ) -> LnUrlWithdrawResult<String> {
        todo!()
    }

    fn get_latest_payments(
        &self,
        number_of_completed_payments: u32,
    ) -> crate::Result<ListPaymentsResponse> {
        let mut completed_payments = self.payments.to_vec();
        completed_payments.truncate(number_of_completed_payments as usize);
        Ok(ListPaymentsResponse {
            pending_payments: vec![],
            completed_payments,
        })
    }

    fn get_payment(&self, hash: String) -> crate::Result<Payment> {
        self.payments
            .iter()
            .find(|p| p.hash == hash)
            .cloned()
            .ok_or(runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Couldn't find payment with hash",
            ))
    }

    fn foreground(&self) {}

    fn background(&self) {}

    fn list_currency_codes(&self) -> Vec<String> {
        todo!()
    }

    fn get_exchange_rate(&self) -> Option<ExchangeRate> {
        todo!()
    }

    fn change_fiat_currency(&self, fiat_currency: String) {
        todo!()
    }

    fn change_timezone_config(&self, timezone_config: TzConfig) {
        todo!()
    }

    fn accept_pocket_terms_and_conditions(&self) -> crate::Result<()> {
        todo!()
    }

    fn get_terms_and_conditions_status(
        &self,
        terms_and_conditions: TermsAndConditions,
    ) -> crate::Result<TermsAndConditionsStatus> {
        todo!()
    }

    fn register_fiat_topup(
        &self,
        email: Option<String>,
        user_iban: String,
        user_currency: String,
    ) -> crate::Result<FiatTopupInfo> {
        todo!()
    }

    fn reset_fiat_topup(&self) -> crate::Result<()> {
        todo!()
    }

    fn hide_topup(&self, id: String) -> crate::Result<()> {
        todo!()
    }

    fn query_uncompleted_offers(&self) -> crate::Result<Vec<OfferInfo>> {
        todo!()
    }

    fn calculate_lightning_payout_fee(&self, offer: OfferInfo) -> crate::Result<Amount> {
        todo!()
    }

    fn request_offer_collection(&self, offer: OfferInfo) -> crate::Result<String> {
        todo!()
    }

    fn register_notification_token(
        &self,
        notification_token: String,
        language_iso_639_1: String,
        country_iso_3166_1_alpha_2: String,
    ) -> crate::Result<()> {
        todo!()
    }

    fn get_wallet_pubkey_id(&self) -> Option<String> {
        todo!()
    }

    fn get_payment_uuid(&self, payment_hash: String) -> crate::Result<String> {
        todo!()
    }

    fn query_onchain_fee_rate(&self) -> crate::Result<u32> {
        todo!()
    }

    fn prepare_sweep(&self, address: String, onchain_fee_rate: u32) -> crate::Result<SweepInfo> {
        todo!()
    }

    fn sweep(&self, sweep_info: SweepInfo) -> crate::Result<String> {
        todo!()
    }

    fn generate_swap_address(
        &self,
        lsp_fee_params: Option<OpeningFeeParams>,
    ) -> Result<SwapAddressInfo, ReceiveOnchainError> {
        todo!()
    }

    fn get_unresolved_failed_swaps(&self) -> crate::Result<Vec<FailedSwapInfo>> {
        todo!()
    }

    fn prepare_resolve_failed_swap(
        &self,
        failed_swap_info: FailedSwapInfo,
        to_address: String,
        onchain_fee_rate: u32,
    ) -> crate::Result<ResolveFailedSwapInfo> {
        todo!()
    }

    fn resolve_failed_swap(
        &self,
        resolve_failed_swap_info: ResolveFailedSwapInfo,
    ) -> crate::Result<String> {
        todo!()
    }

    fn log_debug_info(&self) -> crate::Result<()> {
        todo!()
    }

    fn retrieve_latest_fiat_topup_info(&self) -> crate::Result<Option<FiatTopupInfo>> {
        todo!()
    }

    fn get_health_status(&self) -> crate::Result<BreezHealthCheckStatus> {
        todo!()
    }

    fn is_clear_wallet_feasible(&self) -> crate::Result<bool> {
        todo!()
    }

    fn prepare_clear_wallet(&self) -> crate::Result<ClearWalletInfo> {
        todo!()
    }

    fn clear_wallet(
        &self,
        clear_wallet_info: ClearWalletInfo,
        destination_onchain_address_data: BitcoinAddressData,
    ) -> crate::Result<()> {
        todo!()
    }
}

fn mock_amount(amount_sats: u64) -> Amount {
    amount_sats.as_sats().to_amount_up(&Some(ExchangeRate {
        currency_code: "CHF".to_string(),
        rate: 2342,
        updated_at: SystemTime::now(),
    }))
}
