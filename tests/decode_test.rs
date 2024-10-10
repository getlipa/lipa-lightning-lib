mod print_events_handler;
mod setup;

use crate::setup::start_node;

use breez_sdk_core::Network;
use serial_test::file_serial;
use std::time::{Duration, SystemTime};
use uniffi_lipalightninglib::{DecodeDataError, DecodedData, InvoiceDetails, UnsupportedDataType};

#[test]
#[file_serial(key, path => "/tmp/3l-int-tests-lock")]
fn test_decoding() {
    let node = start_node().unwrap();

    let invalid_invoice = "invalid".to_string();
    let result = node.decode_data(invalid_invoice);
    assert!(matches!(result, Err(DecodeDataError::Unrecognized { .. })));

    let bitcoin_address = "bc1qftnnghhyhyegmzmh0t7uczysr05e3vx75t96y2".to_string();
    let data = node.decode_data(bitcoin_address.clone()).unwrap();
    assert!(matches!(data, DecodedData::OnchainAddress { .. }));
    if let DecodedData::OnchainAddress {
        onchain_address_details,
    } = data
    {
        assert_eq!(onchain_address_details.address, bitcoin_address);
        assert_eq!(onchain_address_details.network, Network::Bitcoin);
    }

    let node_id = "03864ef025fde8fb587d989186ce6a4a186895ee44a926bfc370e2c366597a3f8f".to_string();
    let result = node.decode_data(node_id);
    assert!(matches!(
        result,
        Err(DecodeDataError::Unsupported {
            typ: UnsupportedDataType::NodeId
        })
    ));

    let url = "https://lipa.swiss".to_string();
    let result = node.decode_data(url);
    assert!(matches!(
        result,
        Err(DecodeDataError::Unsupported {
            typ: UnsupportedDataType::Url
        })
    ));

    let valid_invoice = "lnbc1pjs6m8ppp5krf0wqz805p6v2f2ducge75lxg5v9dk34t3vdamz4j0h9ycstp6sdqu2askcmr9wssx7e3q2dshgmmndp5scqzzsxqyz5vqsp5hymglgtm35e7hy6w7c4wswmcs77xg0hu8ns83wmkfskq9p34w8ds9qyyssq389370f0wm48ecajj9nz5vnx2nuru2cwmkdz93qywy45uvf5f7sjp9wjuv3gyvtr8emm6w56s7x94fpxqkgfpgeqq38xz85k9clnkqcq3rw49n".to_string();
    let data = node.decode_data(valid_invoice.clone()).unwrap();
    assert!(matches!(data, DecodedData::Bolt11Invoice { .. }));
    if let DecodedData::Bolt11Invoice { invoice_details } = data {
        let expected_invoice_details = InvoiceDetails {
            invoice: valid_invoice,
            amount: None,
            description: "Wallet of Satoshi".into(),
            payment_hash: "b0d2f700477d03a6292a6f308cfa9f3228c2b6d1aae2c6f762ac9f7293105875".into(),
            payee_pub_key: "035e4ff418fc8b5554c5d9eea66396c227bd429a3251c8cbc711002ba215bfc226"
                .into(),
            creation_timestamp: unix_timestamp_to_system_time(1695378657),
            expiry_interval: Duration::from_secs(86400),
            expiry_timestamp: unix_timestamp_to_system_time(1695465057),
        };
        assert_eq!(invoice_details, expected_invoice_details);
    }

    let testnet_invoice = "lntb10u1pjkvq6mpp5zszjfrehd5y8sq4w47jegjy5xglw3smcfelfkqud56vtq9c48kmsdqqcqzzsxqyz5vqsp5kgjy259sn4t24er4hawcsr9zl9u7vrkdk7a9kcs9ffury0kf50cq9qyyssqept74lw02kkng3cpzqhyrwt542ct6dtfcz7mtesfggt57r5j7djyz7z5de4cyaupehhwyv7ql6yatqe3e4hvnp2lvpvdwxstpy2rnwqq89p90d".to_string();
    let result = node.decode_data(testnet_invoice);
    assert!(matches!(
        result,
        Err(DecodeDataError::Unsupported {
            typ: UnsupportedDataType::Network { .. }
        })
    ));

    let lightning_address = "danielgranhao@walletofsatoshi.com".to_string();
    let data = node.decode_data(lightning_address).unwrap();
    assert!(matches!(data, DecodedData::LnUrlPay { .. }));
    if let DecodedData::LnUrlPay { lnurl_pay_details } = data {
        assert_eq!(
            lnurl_pay_details.request_data.ln_address,
            Some("danielgranhao@walletofsatoshi.com".to_string())
        );
    }

    // LNURL-pay from https://lnurl.fiatjaf.com/
    let valid_lnurlp = "lightning:LNURL1DP68GURN8GHJ7MRWW4EXCTNXD9SHG6NPVCHXXMMD9AKXUATJDSKHQCTE8AEK2UMND9HKU0FJ89JXXCT989JRGVE3XVMK2ERZXPJX2DECXP3KXV33XQCKVE3C8QMXXD3CVVUXXEPNV3NRWE3HXVUKZWP3XSEX2V3CXEJXGCNRXGUKGUQ0868".to_string();
    let data = node.decode_data(valid_lnurlp).unwrap();
    assert!(matches!(data, DecodedData::LnUrlPay { .. }));

    // LNURL-withdraw from https://lnurl.fiatjaf.com/
    let valid_lnurlw = "lightning:LNURL1DP68GURN8GHJ7MRWW4EXCTNXD9SHG6NPVCHXXMMD9AKXUATJDSKHW6T5DPJ8YCTH8AEK2UMND9HKU0TZVFNXYDFSXP3RQWRYXP3XYCNYXSUXYD3S893XYVPC8YCX2CN9VYMRYWT9X3JX2WT9V5MNXCFSV4NXGWRYV5EN2ERRVE3XGCFCXSMNYRWM42Q".to_string();
    let data = node.decode_data(valid_lnurlw).unwrap();
    assert!(matches!(data, DecodedData::LnUrlWithdraw { .. }));
}

fn unix_timestamp_to_system_time(timestamp: u64) -> SystemTime {
    let duration = Duration::from_secs(timestamp);
    SystemTime::UNIX_EPOCH + duration
}
