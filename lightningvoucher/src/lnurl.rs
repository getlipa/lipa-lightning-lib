use crate::key::RedeemerKey;
use crate::voucher::{Voucher, VoucherMetadata};

use serde::{Deserialize, Serialize};

pub fn encode(redeemer_key: &RedeemerKey, lnurl_prefix: &str) -> String {
    // TODO: lnurl_prefix must end with `/` or `=`.
    let hrp = bech32::Hrp::parse("lnurl").expect("valid hrp");

    let key = redeemer_key.encode();
    let lnurl_raw = format!("{lnurl_prefix}{key}");
    bech32::encode::<bech32::Bech32>(hrp, lnurl_raw.as_bytes()).expect("bech32 encoding")
}

pub fn decode(lnurl: &str) -> RedeemerKey {
    let (_, bytes) = bech32::decode(lnurl).expect("Invalid lnurl");
    let url = String::from_utf8(bytes).expect("Invalid lnurl");
    // TODO: Split by '/' or '='.
    let (_url, redeemer_key) = url.rsplit_once('/').expect("Missing / in url");
    RedeemerKey::decode(redeemer_key)
}

pub fn to_lnurl_response(voucher: &Voucher, lnurl_prefix: String) -> String {
    let signature = voucher.signature.serialize_compact();
    let signature = data_encoding::HEXLOWER.encode(&signature);
    let response = Response {
        tag: "withdrawRequest".to_string(),
        callback: lnurl_prefix,
        k1: voucher.redeemer_key.encode(),
        signature,
        metadata: voucher.metadata.clone(),
    };
    serde_json::to_string(&response).unwrap()
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub tag: String,
    pub callback: String,
    pub k1: String,
    pub signature: String,
    #[serde(flatten)]
    pub metadata: VoucherMetadata,
}
