use crate::data_store::DataStore;
use crate::errors::{Result, RuntimeErrorCode};

use bitcoin::hashes::hex::ToHex;
use bitcoin::hashes::sha256;
use bitcoin::secp256k1::{Message, PublicKey, SecretKey, SECP256K1};
use bitcoin::util::bip32::{ChildNumber, ExtendedPrivKey};
use bitcoin::Network;
use breez_sdk_core::{BreezServices, OpeningFeeParams};
use num_enum::TryFromPrimitive;
use perro::MapToError;
use std::sync::{Arc, Mutex};

const MIGRATION_DESCRIPTION: &str = "Funds migration from the legacy wallet version";

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
pub(crate) async fn migrate_funds(
    seed: &[u8; 64],
    data_store: Arc<Mutex<DataStore>>,
    sdk: &BreezServices,
) -> Result<()> {
    if matches!(
        data_store
            .lock()
            .unwrap()
            .retrive_funds_migration_status()?,
        MigrationStatus::Completed | MigrationStatus::NotNeeded
    ) {
        return Ok(());
    }

    let (private_key, public_key) = derive_ldk_keys(seed)?;
    let public_key = public_key.serialize().to_hex();

    let balance = fetch_balance(public_key.clone())?;
    if balance == 0 {
        data_store
            .lock()
            .unwrap()
            .append_funds_migration_status(MigrationStatus::NotNeeded)?;
        return Ok(());
    }

    data_store
        .lock()
        .unwrap()
        .append_funds_migration_status(MigrationStatus::Pending)?;

    let lsp_info = sdk.lsp_info().await.map_to_runtime_error(
        RuntimeErrorCode::LspServiceUnavailable,
        "Failed to get LSP info",
    )?;
    let lsp_fee = lsp_info
        .opening_fee_params_list
        .get_cheapest_opening_fee_params()
        .map_to_permanent_failure("Failed to get LSP fees")?;
    let amout_to_request = add_lsp_fees(balance, &lsp_fee);

    let invoice = sdk
        .receive_payment(breez_sdk_core::ReceivePaymentRequest {
            amount_sats: amout_to_request,
            description: MIGRATION_DESCRIPTION.to_string(),
            preimage: None,
            opening_fee_params: Some(lsp_fee),
            use_description_hash: None,
            expiry: None,
            cltv: None,
        })
        .await
        .map_to_runtime_error(RuntimeErrorCode::NodeUnavailable, "Failed to issue invoice")?;
    let invoice = invoice.ln_invoice.bolt11;
    let signature = sign_message(&private_key, &invoice);

    match payout(public_key, invoice, signature) {
        Ok(()) => data_store
            .lock()
            .unwrap()
            .append_funds_migration_status(MigrationStatus::Completed),
        Err(e) => {
            let _ = data_store
                .lock()
                .unwrap()
                .append_funds_migration_status(MigrationStatus::Failed);
            Err(e)
        }
    }
}

fn payout(_public_key: String, _invoice: String, _signature: String) -> Result<()> {
    // TODO: Implement.
    Ok(())
}

fn fetch_balance(_public_key: String) -> Result<u64> {
    // TODO: Implement.
    Ok(0)
}

fn add_lsp_fees(amount_msat: u64, lsp_fee: &OpeningFeeParams) -> u64 {
    // TODO: Implement.
    let lsp_fee_msat = amount_msat * lsp_fee.proportional as u64 / 1_000_000;
    let lsp_fee_msat_rounded_to_sat = lsp_fee_msat / 1000 * 1000;
    let fee = std::cmp::max(lsp_fee_msat_rounded_to_sat, lsp_fee.min_msat);
    amount_msat + fee
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
}
