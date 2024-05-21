use chrono::{DateTime, Utc};
use perro::{ensure, runtime_error, MapToError, OptionToError};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::time::Duration;

/// Information about a fiat top-up registration
#[derive(Debug, Clone, PartialEq)]
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
    pub currency: String,
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
            currency: create_order_response.payment_method.currency,
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

pub struct PocketClient {
    pocket_url: String,
    client: reqwest::Client,
}

/// A code that specifies the Pocket client error that occurred
#[derive(Debug, PartialEq, Eq)]
pub enum PocketClientErrorCode {
    ServiceUnavailable,
    UnexpectedResponse,
    SignerFailure,
}

impl Display for PocketClientErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

pub type PocketClientError = perro::Error<PocketClientErrorCode>;
pub type Result<T> = std::result::Result<T, PocketClientError>;

impl PocketClient {
    pub fn new(pocket_url: String) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_to_permanent_failure("Failed to build a Pocket Client instance")?;
        Ok(Self { pocket_url, client })
    }

    pub async fn register_pocket_fiat_topup<S, Fut>(
        &self,
        user_iban: &str,
        user_currency: String,
        node_pubkey: String,
        sign_message: S,
    ) -> Result<FiatTopupInfo>
    where
        S: FnOnce(String) -> Fut,
        Fut: Future<Output = Option<String>>,
    {
        let challenge_response = self.request_challenge().await?;

        let create_order_response = self
            .create_order(
                challenge_response,
                user_iban,
                user_currency,
                node_pubkey,
                sign_message,
            )
            .await?;

        Ok(FiatTopupInfo::from_pocket_create_order_response(
            create_order_response,
        ))
    }

    async fn request_challenge(&self) -> Result<ChallengeResponse> {
        let raw_response = self
            .client
            .post(format!("{}/v1/challenges", self.pocket_url))
            .send()
            .await
            .map_to_runtime_error(
                PocketClientErrorCode::ServiceUnavailable,
                "Failed to get a response from the Pocket API",
            )?;

        ensure!(
            raw_response.status() == StatusCode::CREATED,
            runtime_error(
                PocketClientErrorCode::UnexpectedResponse,
                "Got unexpected response to Pocket challenge request"
            )
        );

        raw_response
            .json::<ChallengeResponse>()
            .await
            .map_to_runtime_error(
                PocketClientErrorCode::UnexpectedResponse,
                "Failed to parse ChallengeResponse",
            )
    }

    async fn create_order<S, Fut>(
        &self,
        challenge_response: ChallengeResponse,
        user_iban: &str,
        user_currency: String,
        node_pubkey: String,
        sign_message: S,
    ) -> Result<CreateOrderResponse>
    where
        S: FnOnce(String) -> Fut,
        Fut: Future<Output = Option<String>>,
    {
        let message = format!(
            "I confirm my bitcoin wallet. [{}]",
            challenge_response.token
        );

        let signature = sign_message(message.clone()).await.ok_or_runtime_error(
            PocketClientErrorCode::SignerFailure,
            "Failed to create signature",
        )?;

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
            .await
            .map_to_runtime_error(
                PocketClientErrorCode::ServiceUnavailable,
                "Failed to get a response from the Pocket API",
            )?;

        ensure!(
            raw_response.status() == StatusCode::CREATED,
            runtime_error(
                PocketClientErrorCode::UnexpectedResponse,
                "Got unexpected response to Pocket order creation request"
            )
        );

        raw_response
            .json::<CreateOrderResponse>()
            .await
            .map_to_runtime_error(
                PocketClientErrorCode::UnexpectedResponse,
                "Failed to parse CreateOrderResponse",
            )
    }
}
