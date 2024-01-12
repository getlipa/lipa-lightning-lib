use crate::async_runtime::Handle;
use crate::data_store::DataStore;
use crate::errors::{Result, RuntimeErrorCode};
use crate::locker::Locker;

use crate::amount::AsSats;
use bitcoin::bip32::{ChildNumber, ExtendedPrivKey};
use bitcoin::hashes::sha256;
use bitcoin::secp256k1::{Message, PublicKey, SecretKey, SECP256K1};
use bitcoin::Network;
use breez_sdk_core::{BreezServices, OpenChannelFeeRequest, OpeningFeeParams};
use graphql::schema::{migrate_funds, migration_balance, MigrateFunds, MigrationBalance};
use graphql::{build_client, post_blocking};
use honey_badger::Auth;
use num_enum::TryFromPrimitive;
use perro::{MapToError, OptionToError, ResultTrait};
use reqwest::blocking::Client;
use std::sync::{Arc, Mutex};

const MIGRATION_DESCRIPTION: &str = "Migration refund. Read more on lipa.swiss/en/migration";

#[derive(PartialEq, Eq, Debug, TryFromPrimitive, Clone)]
#[repr(u8)]
pub(crate) enum MigrationStatus {
    Unknown,
    Pending,
    Failed,
    Completed,
    NotNeeded,
}

#[allow(dead_code)]
pub(crate) fn migrate_funds(
    rt: Handle,
    seed: &[u8; 64],
    data_store: Arc<Mutex<DataStore>>,
    sdk: &BreezServices,
    auth: Arc<Auth>,
    backend_url: &String,
) -> Result<()> {
    if matches!(
        data_store.lock_unwrap().retrive_funds_migration_status()?,
        MigrationStatus::Completed | MigrationStatus::NotNeeded
    ) {
        return Ok(());
    }

    let (private_key, public_key) = derive_ldk_keys(seed)?;
    let public_key = hex::encode(public_key.serialize());

    let token = auth
        .query_token()
        .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)?;
    let client = build_client(Some(&token))
        .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)?;

    let balance_msat = fetch_legacy_balance(&client, backend_url, public_key.clone())? * 1_000;
    if balance_msat == 0 {
        data_store
            .lock_unwrap()
            .append_funds_migration_status(MigrationStatus::NotNeeded)?;
        return Ok(());
    }

    data_store
        .lock_unwrap()
        .append_funds_migration_status(MigrationStatus::Pending)?;

    let lsp_info = rt.block_on(sdk.lsp_info()).map_to_runtime_error(
        RuntimeErrorCode::LspServiceUnavailable,
        "Failed to get LSP info",
    )?;
    let lsp_fee_params = lsp_info
        .opening_fee_params_list
        .get_cheapest_opening_fee_params()
        .map_to_permanent_failure("Failed to get LSP fees")?;

    let lsp_fee_msat = rt
        .block_on(sdk.open_channel_fee(OpenChannelFeeRequest {
            amount_msat: balance_msat,
            expiry: None,
        }))
        .map_to_runtime_error(
            RuntimeErrorCode::NodeUnavailable,
            "Failed to calculate lsp fee for fund migration amount",
        )?
        .fee_msat;
    let amount_to_request = if lsp_fee_msat > 0 {
        add_lsp_fees(balance_msat, &lsp_fee_params).as_msats()
    } else {
        balance_msat.as_msats()
    };

    let invoice = rt
        .block_on(sdk.receive_payment(breez_sdk_core::ReceivePaymentRequest {
            amount_msat: amount_to_request.msats,
            description: MIGRATION_DESCRIPTION.to_string(),
            preimage: None,
            opening_fee_params: Some(lsp_fee_params),
            use_description_hash: None,
            expiry: None,
            cltv: None,
        }))
        .map_to_runtime_error(RuntimeErrorCode::NodeUnavailable, "Failed to issue invoice")?;
    let invoice = invoice.ln_invoice.bolt11;
    let signature = sign_message(&private_key, &invoice);

    match payout(&client, backend_url, public_key, invoice, signature) {
        Ok(()) => data_store
            .lock_unwrap()
            .append_funds_migration_status(MigrationStatus::Completed),
        Err(e) => {
            let _ = data_store
                .lock_unwrap()
                .append_funds_migration_status(MigrationStatus::Failed);
            Err(e)
        }
    }
}

fn fetch_legacy_balance(client: &Client, backend_url: &String, public_key: String) -> Result<u64> {
    let variables = migration_balance::Variables {
        node_pub_key: Some(public_key),
    };
    let data = post_blocking::<MigrationBalance>(client, backend_url, variables)
        .prefix_error("Failed to fetch balance to migrate")
        .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)?;
    let balance_sat = data
        .migration_balance
        .ok_or_runtime_error(
            RuntimeErrorCode::AuthServiceUnavailable,
            "Empty balance field",
        )?
        .balance_amount_sat;
    Ok(balance_sat)
}

fn payout(
    client: &Client,
    backend_url: &String,
    public_key: String,
    invoice: String,
    signature: String,
) -> Result<()> {
    let variables = migrate_funds::Variables {
        invoice: Some(invoice),
        base16_invoice_signature: Some(signature),
        ldk_node_pub_key: Some(public_key),
    };
    let _ = post_blocking::<MigrateFunds>(client, backend_url, variables)
        .prefix_error("Failed to payout")
        .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)?;
    Ok(())
}

fn add_lsp_fees(amount_msat: u64, lsp_fee: &OpeningFeeParams) -> u64 {
    const MILLION: u64 = 1_000_000;

    // As in receive_payment()
    // https://github.com/breez/breez-sdk/blob/main/libs/sdk-core/src/breez_services.rs#L1634
    const MIN_REQUEST_MSAT: u64 = 1_000;
    if amount_msat < MIN_REQUEST_MSAT {
        return lsp_fee.min_msat + MIN_REQUEST_MSAT;
    }

    let one_minus_proportional = MILLION - lsp_fee.proportional as u64;

    if amount_msat * lsp_fee.proportional as u64 / one_minus_proportional < lsp_fee.min_msat {
        amount_msat + lsp_fee.min_msat
    } else {
        let result = amount_msat * MILLION / one_minus_proportional;
        result / 1_000 * 1_000
    }
}

fn derive_ldk_keys(seed: &[u8; 64]) -> Result<(SecretKey, PublicKey)> {
    let mut first_half = [0u8; 32];
    first_half.copy_from_slice(&seed[..32]);
    // Note that when we aren't serializing the key, network doesn't matter
    let master_key = ExtendedPrivKey::new_master(Network::Testnet, &first_half)
        .map_to_permanent_failure("Failed to build master key")?;
    let child_number = ChildNumber::from_hardened_idx(0)
        .map_to_permanent_failure("Failed to create hardened from index")?;
    let private_key = master_key
        .ckd_priv(SECP256K1, child_number)
        .map_to_permanent_failure("Failed to build node secret")?
        .private_key;

    let public_key = PublicKey::from_secret_key(SECP256K1, &private_key);
    Ok((private_key, public_key))
}

fn sign_message(private_key: &SecretKey, message: &str) -> String {
    let message = format!("I want to payout my funds to {message}");
    let message = Message::from_hashed_data::<sha256::Hash>(message.as_bytes());
    private_key.sign_ecdsa(message).serialize_der().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::secp256k1::ecdsa::Signature;
    use std::str::FromStr;

    #[test]
    fn test_signing() {
        let seed = [0u8; 64];
        let (private_key, public_key) = derive_ldk_keys(&seed).unwrap();
        let signature = sign_message(&private_key, "invoice");
        let signature = Signature::from_str(&signature).unwrap();
        let message = "I want to payout my funds to invoice".as_bytes();
        let message = Message::from_hashed_data::<sha256::Hash>(message);
        signature.verify(&message, &public_key).unwrap();
    }

    #[test]
    #[rustfmt::skip]
    fn test_lsp_fee() {
        let lsp_fee = OpeningFeeParams {
            min_msat: 2_000_000,
            proportional: 10_000,
            valid_until: String::new(),
            max_idle_time: 0,
            max_client_to_self_delay: 0,
            promise: String::new(),
        };

		assert_eq!(add_lsp_fees(      1_000, &lsp_fee),   2_001_000);
		assert_eq!(add_lsp_fees(  1_000_000, &lsp_fee),   3_000_000);
		assert_eq!(add_lsp_fees( 11_000_000, &lsp_fee),  13_000_000);
		assert_eq!(add_lsp_fees(811_000_000, &lsp_fee), 819_191_000);

        assert_calculation(&lsp_fee,         1_000);
        assert_calculation(&lsp_fee,        11_000);
        assert_calculation(&lsp_fee,       111_000);
        assert_calculation(&lsp_fee,     1_111_000);
        assert_calculation(&lsp_fee,    11_111_000);
        assert_calculation(&lsp_fee,   111_111_000);
        assert_calculation(&lsp_fee, 1_111_111_000);
    }

    fn assert_calculation(lsp_fee: &OpeningFeeParams, amount_msats: u64) {
        let amount_with_fees = add_lsp_fees(amount_msats, lsp_fee);
        let receive = what_receive_for(lsp_fee, amount_with_fees);
        assert_eq!(amount_msats, receive);
    }

    fn what_receive_for(lsp_fee: &OpeningFeeParams, amount_msats: u64) -> u64 {
        let lsp_fee_msat = amount_msats * lsp_fee.proportional as u64 / 1_000_000;
        let lsp_fee_msat_rounded_to_sat = lsp_fee_msat / 1000 * 1000;
        let final_fee = std::cmp::max(lsp_fee_msat_rounded_to_sat, lsp_fee.min_msat);
        amount_msats - final_fee
    }
}
