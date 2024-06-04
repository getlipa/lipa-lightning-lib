use crate::data_store::DataStore;
use crate::{locker::Locker, Result};

use bitcoin::hashes::{sha256, Hash};
// use breez_sdk_core::parse_invoice;
use perro::{ensure, permanent_failure, MapToError};
use rand::RngCore;
use reqwest::StatusCode;
use serde::Deserialize;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct Voucher {
    pub hash: String,
    pub preimage: String,
    pub amount_sat: u32,
    pub passcode: Option<String>,
    pub lnurl: String,
    pub fallback: String,
}

#[derive(Debug, Deserialize)]
struct PostVoucherResponse {
    lnurl_prefix: String,
    url_prefix: String,
}

#[derive(Debug, Deserialize)]
struct VoucherRedemption {
    preimage: String,
    invoice: String,
    seal: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VoucherRedemptions {
    redemptions: Vec<VoucherRedemption>,
}

pub struct VoucherServer {
    data_store: Arc<Mutex<DataStore>>,
}

impl VoucherServer {
    pub fn new(data_store: Arc<Mutex<DataStore>>) -> Self {
        Self { data_store }
    }

    pub async fn redeem_vouchers(&self) -> Result<Option<(String, String)>> {
        const URL: &str = "https://voucher.zzd.es";
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_to_permanent_failure("Failed to build a Pocket Client instance")?;
        let response = client
            .get(URL)
            .send()
            .await
            .map_to_permanent_failure("Failed to post voucher")?;
        ensure!(
            response.status() == StatusCode::OK,
            permanent_failure("Failed to list voucher redemptions, status code")
        );
        let response = response
            .json::<VoucherRedemptions>()
            .await
            .map_to_permanent_failure("Failed to parse VoucherResmptions")?;

        for redemption in response.redemptions {
            let voucher = self
                .data_store
                .lock_unwrap()
                .retrieve_voucher(redemption.preimage.clone());
            if let Ok(voucher) = voucher {
                // TODO: Check that the voucher amount.
                //				let invoice = parse_invoice(&redemption.invoice).map_to_permanent_failure("Invalid invoice")?;
                log::info!("Received seal: {:?}", redemption.seal);
                if let Some(passcode) = voucher.passcode {
                    let data = passcode.clone() + &redemption.invoice;
                    log::info!("checking: data to seal: {data} with {passcode}");

                    let seal = sha256::Hash::hash(data.as_bytes());
                    let seal = hex::encode(seal);
                    log::info!("Computed seal: {}", seal);
                    if Some(seal) != redemption.seal {
                        permanent_failure!("Invalid seal");
                    }
                }
                self.data_store
                    .lock_unwrap()
                    .set_voucher_redemption(redemption.preimage, redemption.invoice.clone())?;
                return Ok(Some((voucher.hash, redemption.invoice)));
            }
        }
        Ok(None)
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

        let url = format!("{URL}/{hash}/{amount_sat}/{}", passcode.is_some());
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

        self.data_store
            .lock_unwrap()
            .store_voucher(voucher.clone())?;

        Ok(voucher)
    }

    pub async fn cancel_voucher(&self, hash: String) -> Result<()> {
        const URL: &str = "https://voucher.zzd.es";
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_to_permanent_failure("Failed to build a Pocket Client instance")?;
        let url = format!("{URL}/{hash}");
        let response = client
            .delete(url)
            .send()
            .await
            .map_to_permanent_failure("Failed to post voucher")?;
        ensure!(
            response.status() == StatusCode::OK,
            permanent_failure("Failed to pose voucher, status code")
        );
        Ok(())
    }

    pub fn list_vouchers(&self) -> Result<Vec<Voucher>> {
        self.data_store.lock_unwrap().list_vouchers()
    }
}

fn generate_32_random_bytes() -> Vec<u8> {
    let mut bytes = vec![0; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes
}
