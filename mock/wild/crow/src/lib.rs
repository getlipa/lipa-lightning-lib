use graphql::ExchangeRate;
use honey_badger::Auth;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

pub use isocountry::CountryCode;
pub use isolanguage_1::LanguageCode;

pub use crow::PermanentFailureCode;
pub use crow::TemporaryFailureCode;
pub use crow::TopupError;
pub use crow::TopupInfo;
pub use crow::TopupStatus;
use lazy_static::lazy_static;

lazy_static! {
    static ref TOPUPS: Mutex<Vec<TopupInfo>> = Mutex::new(Vec::new());
}

const REFUND_EMAIL: &str = "refund@top.up";
const SETTLE_EMAIL: &str = "settle@top.up";
const FAIL_EMAIL: &str = "fail@top.up";

pub struct OfferManager {}

impl OfferManager {
    pub fn new(_backend_url: String, _auth: Arc<Auth>) -> Self {
        Self {}
    }

    pub fn register_topup(&self, order_id: String, email: Option<String>) -> graphql::Result<()> {
        let mut status = TopupStatus::READY;

        if let Some(email) = email {
            match email.as_str() {
                REFUND_EMAIL => status = TopupStatus::REFUNDED,
                SETTLE_EMAIL => status = TopupStatus::SETTLED,
                FAIL_EMAIL => status = TopupStatus::FAILED,
                _ => {}
            }
        }

        TOPUPS.lock().unwrap().push(TopupInfo {
            id: order_id,
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

        Ok(())
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
            .filter(|t| t.status != TopupStatus::SETTLED && t.status != TopupStatus::REFUNDED)
            .cloned()
            .collect())
    }
}
