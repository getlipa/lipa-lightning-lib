use crate::Result;

use bitcoin::hashes::{sha256, Hash};
use perro::{ensure, permanent_failure, MapToError};
use rand::RngCore;
use reqwest::StatusCode;
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug)]
pub struct Voucher {
    hash: String,
    preimage: String,
    amount_sat: u32,
    passcode: Option<String>,
    lnurl: String,
    pub fallback: String,
}

#[derive(Debug, Deserialize)]
struct PostVoucherResponse {
    lnurl_prefix: String,
    url_prefix: String,
}

pub struct VoucherServer {}

impl VoucherServer {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn issue_voucher(
        &self,
        amount_sat: u32,
        passcode: Option<String>,
    ) -> Result<Voucher> {
        const URL: &str = "https://voucher.zzd.es";
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_to_permanent_failure("Failed to build a Pocket Client instance")?;

        let preimage = generate_32_random_bytes();
        let preimage = hex::encode(preimage);
        let hash = sha256::Hash::hash(preimage.as_bytes());
        let hash = hex::encode(hash);

        let url = format!("{URL}/{hash}/{amount_sat}");
        let response = client
            .post(url)
            .send()
            .await
            .map_to_permanent_failure("Failed to post voucher")?;
        ensure!(
            response.status() == StatusCode::OK,
            permanent_failure("Failed to pose voucher, status code")
        );
        let response = response
            .json::<PostVoucherResponse>()
            .await
            .map_to_permanent_failure("Failed to parse PostVoucherResponse")?;

        let lnurl_raw = format!("{}{preimage}", response.lnurl_prefix);
        let hrp = bech32::Hrp::parse("lnurl").expect("valid hrp");
        let lnurl =
            bech32::encode::<bech32::Bech32>(hrp, lnurl_raw.as_bytes()).expect("bech32 encoding");
        let fallback = format!("{}{}", response.url_prefix, lnurl.to_uppercase());

        let voucher = Voucher {
            hash,
            preimage,
            amount_sat,
            passcode,
            lnurl,
            fallback,
        };

        // TODO: Store voucher.
        println!("Voucher generated: {voucher:?}");
        Ok(voucher)
    }
}

fn generate_32_random_bytes() -> Vec<u8> {
    let mut bytes = vec![0; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes
}
