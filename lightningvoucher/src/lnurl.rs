use crate::voucher::Voucher;

use secp256k1::PublicKey;
use serde_json::Value;

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

pub fn to_lnurl_response(voucher: &Voucher, lnurl_prefix: String) -> String {
    let signature = data_encoding::HEXLOWER.encode(&voucher.signature.serialize_compact());
    let mut json = serde_json::to_value(&voucher.metadata).unwrap();
    let map = json.as_object_mut().unwrap();
    map.insert(
        "tag".to_string(),
        Value::String("withdrawRequest".to_string()),
    );
    map.insert("callback".to_string(), Value::String(lnurl_prefix));
    map.insert(
        "k1".to_string(),
        Value::String(encode_key(&voucher.redeemer_key)),
    );
    map.insert("signature".to_string(), Value::String(signature));
    json.to_string()
}

fn encode_key(key: &PublicKey) -> String {
    data_encoding::BASE64URL_NOPAD.encode(&key.serialize())
}
