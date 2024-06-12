use crate::voucher::Voucher;

use secp256k1::PublicKey;
use serde_json::json;

pub fn encode(voucher: &Voucher, lnurl_prefix: &str) -> String {
    let hrp = bech32::Hrp::parse("lnurl").expect("valid hrp");

    let key = encode_key(&voucher.redeemer_key);
    let lnurl_raw = format!("{lnurl_prefix}{key}");
    bech32::encode::<bech32::Bech32>(hrp, lnurl_raw.as_bytes()).expect("bech32 encoding")
}

pub fn decode(lnurl: &str) -> PublicKey {
    let (_, bytes) = bech32::decode(lnurl).expect("Invalid lnurl");
    let url = String::from_utf8(bytes).expect("Invalid lnurl");
    let (_url, redeemer_key) = url.rsplit_once('/').expect("Missing / in url");
    let redeemer_key = data_encoding::BASE64URL_NOPAD
        .decode(redeemer_key.as_bytes())
        .unwrap();
    PublicKey::from_slice(&redeemer_key).unwrap()
}

pub fn to_lnurl_response(voucher: &Voucher, lnurl_prefix: &str) -> String {
    let m = &voucher.metadata;
    let signature = data_encoding::HEXLOWER.encode(&voucher.signature.serialize_compact());
    json!({
        "tag": "withdrawRequest",
        "callback": lnurl_prefix,
        "k1": encode_key(&voucher.redeemer_key),
        "minWithdrawable": m.amount_range_sat.0 * 1000,
        "maxWithdrawable": m.amount_range_sat.1 * 1000,
        "defaultDescription": m.description,
        "signature": signature,
    })
    .to_string()
}

fn encode_key(key: &PublicKey) -> String {
    data_encoding::BASE64URL_NOPAD.encode(&key.serialize())
}
