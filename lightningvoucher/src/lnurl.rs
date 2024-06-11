use crate::voucher::{Voucher, VoucherKey};

use serde_json::json;

pub fn encode(voucher: &Voucher, lnurl_prefix: &str) -> String {
    let hrp = bech32::Hrp::parse("lnurl").expect("valid hrp");

    let key = encode_key(&voucher.key);
    let lnurl_raw = format!("{lnurl_prefix}{key}");
    bech32::encode::<bech32::Bech32>(hrp, lnurl_raw.as_bytes()).expect("bech32 encoding")
}

pub fn decode(lnurl: &str) -> VoucherKey {
    let (_, bytes) = bech32::decode(lnurl).expect("Invalid lnurl");
    let url = String::from_utf8(bytes).expect("Invalid lnurl");
    let (_url, key) = url.rsplit_once('/').expect("Missing / in url");
    let key = data_encoding::BASE64URL_NOPAD
        .decode(key.as_bytes())
        .unwrap();
    *VoucherKey::from_slice(&key)
}

pub fn to_lnurl_response(voucher: &Voucher, lnurl_prefix: &str) -> String {
    let m = &voucher.metadata;
    json!({
        "tag": "withdrawRequest",
        "callback": lnurl_prefix,
        "k1": encode_key(&voucher.key),
        "minWithdrawable": m.amount_range_sat.0 * 1000,
        "maxWithdrawable": m.amount_range_sat.1 * 1000,
        "defaultDescription": m.description,
    })
    .to_string()
}

fn encode_key(key: &VoucherKey) -> String {
    data_encoding::BASE64URL_NOPAD.encode(key)
}
