use graphql::ExchangeRate;
use honeybadger::Auth;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

pub use isocountry::CountryCode;
pub use isolanguage_1::LanguageCode;

pub use crow::FiatTopupSetupChallenge;
pub use crow::FiatTopupSetupInfo;
pub use crow::PermanentFailureCode;
pub use crow::TemporaryFailureCode;
pub use crow::TopupError;
pub use crow::TopupInfo;
pub use crow::TopupStatus;
use lazy_static::lazy_static;
use uuid::Uuid;

lazy_static! {
    static ref TOPUPS: Mutex<Vec<TopupInfo>> = Mutex::new(Vec::new());
    static ref EMAIL: Mutex<Option<String>> = Mutex::new(None);
}

const REFUND_EMAIL: &str = "refund@top.up";
const FAIL_EMAIL: &str = "fail@top.up";

pub struct OfferManager {}

impl OfferManager {
    pub fn new(_backend_url: String, _auth: Arc<Auth>) -> Self {
        Self {}
    }

    pub fn start_topup_setup(
        &self,
        _node_pubkey: String,
        _provider: String,
        _source_iban: String,
        _user_currency: String,
        email: Option<String>,
        _referral_code: Option<String>,
    ) -> graphql::Result<FiatTopupSetupChallenge> {
        let mut stored_email = EMAIL.lock().unwrap();
        *stored_email = email;

        Ok(FiatTopupSetupChallenge {
            id: "id".to_string(),
            challenge: "challenge".to_string(),
        })
    }

    pub fn complete_topup_setup(
        &self,
        id: String,
        _signed_challenge: String,
        _source_iban: String,
    ) -> graphql::Result<FiatTopupSetupInfo> {
        let mut status = TopupStatus::READY;

        let stored_email = EMAIL.lock().unwrap();
        if let Some(email) = stored_email.as_ref() {
            match email.as_str() {
                REFUND_EMAIL => status = TopupStatus::REFUNDED,
                FAIL_EMAIL => status = TopupStatus::FAILED,
                _ => {}
            }
        }

        TOPUPS.lock().unwrap().push(TopupInfo {
            id,
            status,
            amount_sat: 30_000,
            topup_value_minor_units: 20_00,   // 20 EUR
            exchange_fee_rate_permyriad: 100, // 1%
            exchange_fee_minor_units: 20,     // 20 cents
            exchange_rate: ExchangeRate {
                currency_code: "EUR".to_string(),
                sats_per_unit: 15, // 1 cent = 15 sat
                updated_at: SystemTime::now(),
            },
            expires_at: Some(SystemTime::now() + Duration::from_secs(24 * 60 * 60)),
            lnurlw: Some("LNURL1DP68GURN8GHJ7MRWW4EXCTNXD9SHG6NPVCHXXMMD9AKXUATJDSKHW6T5DPJ8YCTH8AEK2UMND9HKU0TRXQMXXEFJXP3XXVR9VE3NVVTP8PJN2VEEXGCRYWFKVSCRGETY8QMNSVPCVENRWCNZXQ6NVVTPXQMNGER9X43KZCT9V9JRQCF5VEJNQ4W0YKJ".to_string()),
            error: None,
        });

        Ok(FiatTopupSetupInfo {
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

    pub fn register_notification_token(
        &self,
        _notification_token: String,
        _language: LanguageCode,
        _country: CountryCode,
    ) -> graphql::Result<()> {
        Ok(())
    }

    pub fn hide_topup(&self, id: String) -> graphql::Result<()> {
        TOPUPS.lock().unwrap().retain(|topup| topup.id != id);
        Ok(())
    }

    pub fn query_uncompleted_topups(&self) -> graphql::Result<Vec<TopupInfo>> {
        Ok(TOPUPS
            .lock()
            .unwrap()
            .clone()
            .iter()
            .filter(|t| {
                t.status == TopupStatus::READY
                    || t.status == TopupStatus::FAILED
                    || t.status == TopupStatus::REFUNDED
            })
            .cloned()
            .collect())
    }
}
