pub use pocketclient::{FiatTopupInfo, Result};
use std::future::Future;
use uuid::Uuid;

pub struct PocketClient {}

impl PocketClient {
    pub fn new(_pocket_url: String) -> Result<Self> {
        Ok(PocketClient {})
    }

    pub async fn register_pocket_fiat_topup<S, Fut>(
        &self,
        _user_iban: &str,
        _user_currency: String,
        _node_pubkey: String,
        _sign_message: S,
    ) -> Result<FiatTopupInfo>
    where
        S: FnOnce(String) -> Fut,
        Fut: Future<Output = Option<String>>,
    {
        Ok(FiatTopupInfo {
            order_id: Uuid::new_v4().to_string(),
            debitor_iban: "Mock debitor_iban".to_string(),
            creditor_reference: "Mock creditor_reference".to_string(),
            creditor_iban: "Mock creditor_iban".to_string(),
            creditor_bank_name: "Mock creditor_bank_name".to_string(),
            creditor_bank_street: "Mock creditor_bank_street".to_string(),
            creditor_bank_postal_code: "Mock creditor_bank_postal_code".to_string(),
            creditor_bank_town: "Mock creditor_bank_town".to_string(),
            creditor_bank_country: "Mock creditor_bank_country".to_string(),
            creditor_bank_bic: "Mock creditor_bank_bic".to_string(),
            creditor_name: "Mock creditor_name".to_string(),
            creditor_street: "Mock creditor_street".to_string(),
            creditor_postal_code: "Mock creditor_postal_code".to_string(),
            creditor_town: "Mock creditor_town".to_string(),
            creditor_country: "Mock creditor_country".to_string(),
            currency: "Mock currency".to_string(),
        })
    }
}
