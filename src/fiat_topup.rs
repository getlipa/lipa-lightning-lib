use crate::errors::{Result, RuntimeErrorCode};
use chrono::{DateTime, Utc};
use log::error;
use perro::{runtime_error, MapToError, ResultTrait};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug)]
pub enum TopupCurrency {
    EUR,
    CHF,
    GBP,
}

#[derive(Debug)]
pub struct FiatTopupInfo {
    pub debitor_iban: String,
    pub creditor_reference: String,
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
    core_node: Arc<LightningNode>,
}

impl PocketClient {
    pub fn new(pocket_url: String, core_node: Arc<LightningNode>) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_to_permanent_failure("Failed to build reqwest client for PocketClient")?;
        Ok(Self {
            pocket_url,
            client,
            core_node,
        })
    }

    pub fn register_pocket_fiat_topup(
        &self,
        user_iban: &str,
        user_currency: TopupCurrency,
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
            return Err(runtime_error(
                RuntimeErrorCode::OfferServiceUnavailable,
                format!("Got unexpected response to Pocket challenge request: Pocket API returned status {}", raw_response.status()),
            ));
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
        user_currency: TopupCurrency,
    ) -> Result<CreateOrderResponse> {
        let message = format!(
            "I confirm my bitcoin wallet. [{}]",
            challenge_response.token
        );
        let signature = self
            .core_node
            .sign_message(&message)
            .map_runtime_error_using(RuntimeErrorCode::from_eel_runtime_error_code)?;
        let node_pubkey = self.core_node.get_node_info().node_pubkey.to_string();

        let user_currency = match user_currency {
            TopupCurrency::EUR => "eur",
            TopupCurrency::CHF => "chf",
            TopupCurrency::GBP => "gbp",
        };
        let create_order_request = CreateOrderRequest {
            active: true,
            affiliate_id: None,
            payment_method: PaymentMethodRequest {
                currency: user_currency.to_string(),
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
            return Err(runtime_error(
                RuntimeErrorCode::OfferServiceUnavailable,
                format!("Got unexpected response to Pocket order creation request: Pocket API returned status {}", raw_response.status()),
            ));
        }

        raw_response
            .json::<CreateOrderResponse>()
            .map_to_runtime_error(
                RuntimeErrorCode::OfferServiceUnavailable,
                "Failed to parse CreateOrderResponse",
            )
    }
}
