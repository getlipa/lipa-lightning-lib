use crate::amount::{Amount, ToAmount};

use crate::ExchangeRate;
use std::time::{Duration, SystemTime};

pub struct InvoiceDetails {
    pub invoice: String,
    pub amount: Option<Amount>,
    pub description: String,
    pub payment_hash: String,
    pub payee_pub_key: String,
    pub creation_timestamp: SystemTime,
    pub expiry_interval: Duration,
    pub expiry_timestamp: SystemTime,
}

impl InvoiceDetails {
    pub(crate) fn from_local_invoice(invoice: Bolt11Invoice, rate: &Option<ExchangeRate>) -> Self {
        let amount = invoice
            .amount_milli_satoshis()
            .map(|a| a.to_amount_down(rate));
        to_invoice_details(invoice, amount)
    }

    pub(crate) fn from_remote_invoice(invoice: Bolt11Invoice, rate: &Option<ExchangeRate>) -> Self {
        let amount = invoice
            .amount_milli_satoshis()
            .map(|a| a.to_amount_up(rate));
        to_invoice_details(invoice, amount)
    }
}

fn to_invoice_details(invoice: Bolt11Invoice, amount: Option<Amount>) -> InvoiceDetails {
    let description = match invoice.description() {
        Bolt11InvoiceDescription::Direct(d) => d.to_string(),
        Bolt11InvoiceDescription::Hash(_) => String::new(),
    };

    let payee_pub_key = match invoice.payee_pub_key() {
        None => invoice.recover_payee_pub_key().to_string(),
        Some(p) => p.to_string(),
    };

    InvoiceDetails {
        invoice: invoice.to_string(),
        amount,
        description,
        payment_hash: invoice.payment_hash().to_string(),
        payee_pub_key,
        creation_timestamp: invoice.timestamp(),
        expiry_interval: invoice.expiry_time(),
        expiry_timestamp: invoice.timestamp() + invoice.expiry_time(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{str::FromStr, time::UNIX_EPOCH};

    const THOUSAND_SATS: u64 = 1_000_000;

    const SECONDS_IN_AN_HOUR: u64 = 3600;

    const REGTEST_INVOICE: &str = "lnbcrt10u1p36gm69pp56haryefc0cvsdc7ucwgnm4j3kul5pjkdlc94vwkju5xktwsvtv6sdpyf36kkefvypyjqstdypvk7atjyprxzargv4eqcqzpgxqrrsssp5e777z0f2g05l99yw8cuvhnq68e7xstulcan5tzvdh4f6642f836q9qyyssqw2g88srqdqrqngzcrzq877hz64sf320kgh5yjwwg7negxeuq909kac33tgheq7re5k7luh6q3xam6jk46p0cepkx89hfdl9g0mx24csqgxhk8x";
    const REGTEST_INVOICE_HASH: &str =
        "d5fa3265387e1906e3dcc3913dd651b73f40cacdfe0b563ad2e50d65ba0c5b35";
    const REGTEST_INVOICE_PAYEE_PUB_KEY: &str =
        "02cc8a9ee52470a08bfe54194cb9b25021bed4c05db6c08118f6d92f97c070b234";
    const REGTEST_INVOICE_DURATION_FROM_UNIX_EPOCH: Duration = Duration::from_secs(1671720773);
    const REGTEST_INVOICE_EXPIRY: Duration = Duration::from_secs(SECONDS_IN_AN_HOUR);
    const REGTEST_INVOICE_DESCRIPTION: &str = "Luke, I Am Your Father";

    const MAINNET_INVOICE: &str = "lnbc10u1p36gu9hpp5dmg5up4kyhkefpxue5smrgucg889esu6zc9vyntnzmqyyr4dycaqdqqcqpjsp5h0n3nc53t2tcm8a9kjpsdgql7ex2c7qrpc0dn4ja9c64adycxx7s9q7sqqqqqqqqqqqqqqqqqqqsqqqqqysgqmqz9gxqyjw5qrzjqwryaup9lh50kkranzgcdnn2fgvx390wgj5jd07rwr3vxeje0glcll63swcqxvlas5qqqqlgqqqqqeqqjqmstdrvfcyq9as46xuu63dfgstehmthqlxg8ljuyqk2z9mxvhjfzh0a6jm53rrgscyd7v0y7dj4zckq69tlsdex0352y89wmvvv0j3gspnku4sz";
    const MAINNET_INVOICE_HASH: &str =
        "6ed14e06b625ed9484dccd21b1a39841ce5cc39a160ac24d7316c0420ead263a";
    const MAINNET_INVOICE_PAYEE_PUB_KEY: &str =
        "025f73fbb3fe0c6d07e0df48dd1addb82bb2d27400183881214f5183b00333fd85";
    const MAINNET_INVOICE_DURATION_FROM_UNIX_EPOCH: Duration = Duration::from_secs(1671721143);
    const MAINNET_INVOICE_EXPIRY: Duration = Duration::from_secs(604800);
    const MAINNET_INVOICE_DESCRIPTION: &str = "";

    fn parse_invoice(invoice: &str) -> Bolt11Invoice {
        Bolt11Invoice::from_str(invoice).unwrap()
    }

    #[test]
    fn test_invoice_parsing() {
        // Test valid hardcoded regtest invoice
        let invoice = parse_invoice(REGTEST_INVOICE);
        let invoice_details = InvoiceDetails::from_remote_invoice(invoice, &None);
        assert_eq!(invoice_details.payment_hash, REGTEST_INVOICE_HASH);
        assert_eq!(
            invoice_details
                .creation_timestamp
                .duration_since(UNIX_EPOCH)
                .unwrap(),
            REGTEST_INVOICE_DURATION_FROM_UNIX_EPOCH
        );
        assert_eq!(
            invoice_details.amount.as_ref().unwrap().sats,
            THOUSAND_SATS / 1000
        );
        assert!(invoice_details.amount.as_ref().unwrap().fiat.is_none());
        assert_invoice_details(
            invoice_details,
            REGTEST_INVOICE_DESCRIPTION,
            SystemTime::UNIX_EPOCH + REGTEST_INVOICE_DURATION_FROM_UNIX_EPOCH,
            REGTEST_INVOICE_EXPIRY,
            REGTEST_INVOICE_PAYEE_PUB_KEY,
            REGTEST_INVOICE_HASH,
        );

        // Test valid hardcoded mainnet invoice
        let invoice = parse_invoice(MAINNET_INVOICE);
        let now = SystemTime::now();
        let rate = Some(ExchangeRate {
            currency_code: "EUR".to_string(),
            rate: 1234,
            updated_at: now,
        });
        let invoice_details = InvoiceDetails::from_remote_invoice(invoice.clone(), &rate);
        assert_eq!(invoice_details.payment_hash, MAINNET_INVOICE_HASH);
        assert_eq!(
            invoice_details
                .creation_timestamp
                .duration_since(UNIX_EPOCH)
                .unwrap(),
            MAINNET_INVOICE_DURATION_FROM_UNIX_EPOCH
        );
        assert_eq!(
            invoice_details.amount.as_ref().unwrap().sats,
            THOUSAND_SATS / 1000
        );
        assert_eq!(
            invoice_details
                .amount
                .as_ref()
                .unwrap()
                .fiat
                .as_ref()
                .unwrap()
                .minor_units,
            82
        );
        assert_invoice_details(
            invoice_details,
            MAINNET_INVOICE_DESCRIPTION,
            SystemTime::UNIX_EPOCH + MAINNET_INVOICE_DURATION_FROM_UNIX_EPOCH,
            MAINNET_INVOICE_EXPIRY,
            MAINNET_INVOICE_PAYEE_PUB_KEY,
            MAINNET_INVOICE_HASH,
        );

        let invoice_details = InvoiceDetails::from_local_invoice(invoice, &rate);
        assert_eq!(
            invoice_details
                .amount
                .as_ref()
                .unwrap()
                .fiat
                .as_ref()
                .unwrap()
                .minor_units,
            81
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn assert_invoice_details(
        invoice_details: InvoiceDetails,
        description: &str,
        creation_timestamp: SystemTime,
        expiry: Duration,
        payee_pub_key: &str,
        payment_hash: &str,
    ) {
        assert_eq!(invoice_details.description, description);
        assert_eq!(invoice_details.creation_timestamp, creation_timestamp);
        assert_eq!(invoice_details.expiry_interval, expiry);
        assert_eq!(invoice_details.payee_pub_key, payee_pub_key);
        assert_eq!(invoice_details.payment_hash, payment_hash);
    }
}
