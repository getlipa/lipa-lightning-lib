mod print_events_handler;
mod setup;

use crate::setup::start_alice;

use serial_test::file_serial;
use std::time::{Duration, SystemTime};
use uniffi_lipalightninglib::{DecodeDataError, DecodedData, InvoiceDetails, UnsupportedDataType};

#[test]
#[file_serial(key, "/tmp/3l-int-tests-lock")]
fn test_decoding() {
    let node = start_alice().unwrap();

    let invalid_invoice = "invalid".to_string();
    let result = node.decode_data(invalid_invoice);
    assert!(matches!(result, Err(DecodeDataError::Unrecognized { .. })));

    let bitcoin_address = "1DTHjgRiPnCYhgy7PcKxEEWAyFi4VoJpqi".to_string();
    let result = node.decode_data(bitcoin_address);
    assert!(matches!(
        result,
        Err(DecodeDataError::Unsupported {
            typ: UnsupportedDataType::BitcoinAddress
        })
    ));

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

    // LNURL-pay from https://lnurl.fiatjaf.com/
    let valid_lnurlp = "lightning:LNURL1DP68GURN8GHJ7MRWW4EXCTNXD9SHG6NPVCHXXMMD9AKXUATJDSKHQCTE8AEK2UMND9HKU0FJ89JXXCT989JRGVE3XVMK2ERZXPJX2DECXP3KXV33XQCKVE3C8QMXXD3CVVUXXEPNV3NRWE3HXVUKZWP3XSEX2V3CXEJXGCNRXGUKGUQ0868".to_string();
    let data = node.decode_data(valid_lnurlp).unwrap();
    assert!(matches!(data, DecodedData::LnUrlPay { .. }));
}

fn unix_timestamp_to_system_time(timestamp: u64) -> SystemTime {
    let duration = Duration::from_secs(timestamp);
    SystemTime::UNIX_EPOCH + duration
}
