use crate::async_runtime::Handle;
use crate::errors::{Result, RuntimeErrorCode};
use crate::runtime_error;

use breez_sdk_core::{BreezServices, SignMessageRequest};
use chrono::{DateTime, Utc};
use log::error;
use perro::Error::RuntimeError;
use perro::MapToError;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

/// Information about a fiat top-up registration
#[derive(Debug)]
pub struct FiatTopupInfo {
    pub order_id: String,
    /// The user should transfer fiat from this IBAN
    pub debitor_iban: String,
    /// This reference should be included in the fiat transfer reference
    pub creditor_reference: String,
    /// The user should transfer fiat to this IBAN
    pub creditor_iban: String,
    pub creditor_bank_name: String,
    pub creditor_bank_street: String,
    pub creditor_bank_postal_code: String,
    pub creditor_bank_town: String,
    pub creditor_bank_country: String,
    pub creditor_bank_bic: String,
    pub creditor_name: String,
    pub creditor_street: String,
    pub creditor_postal_code: String,
    pub creditor_town: String,
    pub creditor_country: String,
}

impl FiatTopupInfo {
    fn from_pocket_create_order_response(create_order_response: CreateOrderResponse) -> Self {
        FiatTopupInfo {
            order_id: create_order_response.id,
            debitor_iban: create_order_response.payment_method.debitor_iban,
            creditor_reference: create_order_response.payment_method.creditor_reference,
            creditor_iban: create_order_response.payment_method.creditor_iban,
            creditor_bank_name: create_order_response.payment_method.creditor_bank_name,
            creditor_bank_street: create_order_response.payment_method.creditor_bank_street,
            creditor_bank_postal_code: create_order_response
                .payment_method
                .creditor_bank_postal_code,
            creditor_bank_town: create_order_response.payment_method.creditor_bank_town,
            creditor_bank_country: create_order_response.payment_method.creditor_bank_country,
            creditor_bank_bic: create_order_response.payment_method.creditor_bank_bic,
            creditor_name: create_order_response.payment_method.creditor_name,
            creditor_street: create_order_response.payment_method.creditor_street,
            creditor_postal_code: create_order_response.payment_method.creditor_postal_code,
            creditor_town: create_order_response.payment_method.creditor_town,
            creditor_country: create_order_response.payment_method.creditor_country,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ChallengeResponse {
    id: String,
    token: String,
    expires_on: Option<DateTime<Utc>>,
    completed_on: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
struct PaymentMethodRequest {
    currency: String,
    debitor_iban: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PayoutMethod {
    node_pubkey: String,
    message: String,
    signature: String,
}

#[derive(Debug, Serialize)]
struct CreateOrderRequest {
    active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    affiliate_id: Option<String>,
    payment_method: PaymentMethodRequest,
    payout_method: PayoutMethod,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct PaymentMethodResponse {
    currency: String,
    debitor_iban: String,
    creditor_reference: String,
    creditor_iban: String,
    creditor_bank_name: String,
    creditor_bank_street: String,
    creditor_bank_postal_code: String,
    creditor_bank_town: String,
    creditor_bank_country: String,
    creditor_bank_bic: String,
    creditor_name: String,
    creditor_street: String,
    creditor_postal_code: String,
    creditor_town: String,
    creditor_country: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct CreateOrderResponse {
    id: String,
    active: bool,
    created_on: Option<DateTime<Utc>>,
    affiliate_id: Option<String>,
    fee_rate: f64,
    payment_method: PaymentMethodResponse,
    payout_method: PayoutMethod,
}

pub(crate) struct PocketClient {
    pocket_url: String,
    client: reqwest::blocking::Client,
    sdk: Arc<BreezServices>,
    rt_handle: Handle,
}

impl PocketClient {
    pub fn new(pocket_url: String, sdk: Arc<BreezServices>, rt_handle: Handle) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_to_permanent_failure("Failed to build reqwest client for PocketClient")?;
        Ok(Self {
            pocket_url,
            client,
            sdk,
            rt_handle,
        })
    }

    pub fn register_pocket_fiat_topup(
        &self,
        user_iban: &str,
        user_currency: String,
    ) -> Result<FiatTopupInfo> {
        let challenge_response = self.request_challenge()?;

        let create_order_response =
            self.create_order(challenge_response, user_iban, user_currency)?;

        Ok(FiatTopupInfo::from_pocket_create_order_response(
            create_order_response,
        ))
    }

    fn request_challenge(&self) -> Result<ChallengeResponse> {
        let raw_response = self
            .client
            .post(format!("{}/v1/challenges", self.pocket_url))
            .send()
            .map_to_runtime_error(
                RuntimeErrorCode::OfferServiceUnavailable,
                "Failed to get a response from the Pocket API",
            )?;

        if raw_response.status() != StatusCode::CREATED {
            error!(
                "Got unexpected response to Pocket challenge request: Pocket API returned status {}", raw_response.status()
            );
            runtime_error!(
                RuntimeErrorCode::OfferServiceUnavailable,
                format!("Got unexpected response to Pocket challenge request: Pocket API returned status {}", raw_response.status()));
        }

        raw_response
            .json::<ChallengeResponse>()
            .map_to_runtime_error(
                RuntimeErrorCode::OfferServiceUnavailable,
                "Failed to parse ChallengeResponse",
            )
    }

    fn create_order(
        &self,
        challenge_response: ChallengeResponse,
        user_iban: &str,
        user_currency: String,
    ) -> Result<CreateOrderResponse> {
        let message = format!(
            "I confirm my bitcoin wallet. [{}]",
            challenge_response.token
        );

        let signature = self.sign_message(message.clone())?;
        let node_pubkey = self
            .sdk
            .node_info()
            .map_err(|e| RuntimeError {
                code: RuntimeErrorCode::NodeUnavailable,
                msg: e.to_string(),
            })?
            .id;

        let create_order_request = CreateOrderRequest {
            active: true,
            affiliate_id: None,
            payment_method: PaymentMethodRequest {
                currency: user_currency.to_lowercase(),
                debitor_iban: user_iban.to_string(),
            },
            payout_method: PayoutMethod {
                node_pubkey,
                message,
                signature,
            },
        };

        let raw_response = self
            .client
            .post(format!("{}/v1/orders", self.pocket_url))
            .json(&create_order_request)
            .send()
            .map_to_runtime_error(
                RuntimeErrorCode::OfferServiceUnavailable,
                "Failed to get a response from the Pocket API",
            )?;

        if raw_response.status() != StatusCode::CREATED {
            error!(
                "Got unexpected response to Pocket order creation request: Pocket API returned status {}", raw_response.status()
            );
            runtime_error!(
                RuntimeErrorCode::OfferServiceUnavailable,
                format!("Got unexpected response to Pocket order creation request: Pocket API returned status {}", raw_response.status()));
        }

        raw_response
            .json::<CreateOrderResponse>()
            .map_to_runtime_error(
                RuntimeErrorCode::OfferServiceUnavailable,
                "Failed to parse CreateOrderResponse",
            )
    }

    fn sign_message(&self, message: String) -> Result<String> {
        Ok(self
            .rt_handle
            .block_on(self.sdk.sign_message(SignMessageRequest { message }))
            .map_to_runtime_error(RuntimeErrorCode::NodeUnavailable, "Couldn't sign message")?
            .signature)
    }
}
