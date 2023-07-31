use crate::amount::ToAmount;
use crate::errors::Result;
use crate::{ExchangeRate, FiatValue, OfferInfo, OfferKind};
use honey_badger::graphql::schema::list_available_topups::ListAvailableTopupsTopup;
use honey_badger::graphql::schema::{
    list_available_topups, register_email, ListAvailableTopups, RegisterEmail,
};
use honey_badger::graphql::{build_client, post_blocking};
use honey_badger::{graphql, Auth};
use perro::{permanent_failure, MapToError, OptionToError};
use std::sync::Arc;
use std::time::SystemTime;

pub(crate) struct BackendClient {
    backend_url: String,
    auth: Arc<Auth>,
}

impl BackendClient {
    pub fn new(backend_url: String, auth: Arc<Auth>) -> Result<Self> {
        Ok(Self { backend_url, auth })
    }

    pub fn register_email(&self, email: String) -> graphql::Result<()> {
        let variables = register_email::Variables { email };
        let access_token = self.auth.query_token()?;
        let client = build_client(Some(&access_token))?;
        let data = post_blocking::<RegisterEmail>(&client, &self.backend_url, variables)?;
        if !matches!(
            data.register_email,
            Some(register_email::RegisterEmailRegisterEmail { .. })
        ) {
            return Err(permanent_failure("Backend rejected email registration"));
        }
        Ok(())
    }

    pub fn query_available_topups(&self) -> graphql::Result<Vec<OfferInfo>> {
        let access_token = self.auth.query_token()?;
        let client = build_client(Some(&access_token))?;
        let data = post_blocking::<ListAvailableTopups>(
            &client,
            &self.backend_url,
            list_available_topups::Variables {},
        )?;
        data.topup.iter().map(topup_to_offer_info).collect()
    }
}

fn topup_to_offer_info(topup: &ListAvailableTopupsTopup) -> graphql::Result<OfferInfo> {
    let created_at = SystemTime::from(
        chrono::DateTime::parse_from_rfc3339(&topup.created_at).map_to_runtime_error(
            graphql::GraphQlRuntimeErrorCode::CorruptData,
            "The backend returned an invalid timestamp",
        )?,
    );
    let expires_at = SystemTime::from(
        chrono::DateTime::parse_from_rfc3339(topup.expires_at.as_ref().ok_or_runtime_error(
            graphql::GraphQlRuntimeErrorCode::CorruptData,
            "The backend returned an incomplete topup - missing expires_at",
        )?)
        .map_to_runtime_error(
            graphql::GraphQlRuntimeErrorCode::CorruptData,
            "The backend returned an invalid timestamp",
        )?,
    );
    let exchange_rate = (100_000_000_f64 / topup.exchange_rate).round() as u32;
    let topup_fiat_value_minor_units = (topup.amount_user_currency * 100_f64).round() as u64;
    let exchange_fee_fiat_value_minor_units =
        (topup.exchange_fee_user_currency * 100_f64).round() as u64;
    let currency_code = topup.user_currency.to_string().to_uppercase();
    let lnurlw = topup
        .lnurl
        .as_ref()
        .ok_or_runtime_error(
            graphql::GraphQlRuntimeErrorCode::CorruptData,
            "The backend returned an incomplete topup - missing lnurlw",
        )?
        .clone();

    Ok(OfferInfo {
        offer_kind: OfferKind::Pocket {
            topup_value: FiatValue {
                minor_units: topup_fiat_value_minor_units,
                currency_code: currency_code.clone(),
                rate: exchange_rate,
                converted_at: created_at,
            },
            exchange_fee: FiatValue {
                minor_units: exchange_fee_fiat_value_minor_units,
                currency_code: currency_code.clone(),
                rate: exchange_rate,
                converted_at: created_at,
            },
            exchange_fee_rate_permyriad: (topup.exchange_fee_rate * 10000_f64).round() as u16,
        },
        amount: (topup.amount_sat * 1000).to_amount_down(&Some(ExchangeRate {
            currency_code,
            rate: exchange_rate,
            updated_at: created_at,
        })),
        lnurlw,
        created_at,
        expires_at,
    })
}

#[cfg(test)]
mod tests {
    use crate::backend_client::topup_to_offer_info;
    use crate::OfferKind;
    use honey_badger::graphql;
    use honey_badger::graphql::schema::list_available_topups::{
        topup_status_enum, ListAvailableTopupsTopup,
    };

    const LNURL: &str = "LNURL1DP68GURN8GHJ7UR0VD4K2ARPWPCZ6EMFWSKHXARPVA5KUEEDWPHKX6M9W3SHQUPWWEJHYCM9DSHXZURS9ASHQ6F0D3H82UNV9AMKJARGV3EXZAE0XVUNQDNYVDJRGTF4XGEKXTF5X56NXTTZX3NRWTT9XDJRJEP4VE3XGD3KXVXTX4LS";

    #[test]
    fn test_topup_to_offer_info() {
        let amount_user_currency = 8.0;
        let mut topup = ListAvailableTopupsTopup {
            additional_info: None,
            amount_sat: 42578,
            amount_user_currency,
            created_at: "2023-07-21T16:39:21.271+00:00".to_string(),
            exchange_fee_rate: 0.014999999664723873,
            exchange_fee_user_currency: 0.11999999731779099,
            exchange_rate: 18507.0,
            expires_at: Some("2023-09-21T16:39:21.919+00:00".to_string()),
            id: "1707e09e-ebe1-4004-abd7-7a64604501b3".to_string(),
            lightning_fee_user_currency: 0.0,
            lnurl: Some(LNURL.to_string()),
            node_pub_key: "0233786a3f5c79d25508ed973e7a37506ddab49d41a07fcb3d341ab638000d69cf"
                .to_string(),
            status: topup_status_enum::READY,
            user_currency: "eur".to_string(),
        };

        let offer_info = topup_to_offer_info(&topup).unwrap();

        let OfferKind::Pocket {
            topup_value,
            exchange_fee,
            exchange_fee_rate_permyriad,
        } = offer_info.offer_kind;
        assert_eq!(exchange_fee.minor_units, 12);
        assert_eq!(exchange_fee.currency_code, "EUR");
        assert_eq!(exchange_fee.rate, 5403);
        assert_eq!(exchange_fee_rate_permyriad, 150);
        assert_eq!(offer_info.amount.sats, 42578);
        assert_eq!(offer_info.lnurlw, String::from(LNURL));
        assert_eq!(
            offer_info.amount.fiat.unwrap().minor_units + exchange_fee.minor_units,
            topup_value.minor_units
        );

        topup.lnurl = None;
        assert!(matches!(
            topup_to_offer_info(&topup),
            Err(graphql::Error::RuntimeError {
                code: graphql::GraphQlRuntimeErrorCode::CorruptData,
                ..
            })
        ));

        topup.lnurl = Some(String::from(LNURL));
        topup.expires_at = None;
        assert!(matches!(
            topup_to_offer_info(&topup),
            Err(graphql::Error::RuntimeError {
                code: graphql::GraphQlRuntimeErrorCode::CorruptData,
                ..
            })
        ));
    }
}
