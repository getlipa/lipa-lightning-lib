use crate::errors::Result;
use crate::locker::Locker;
use crate::support::Support;
use crate::{
    BreezHealthCheckStatus, DecodeDataError, DecodedData, ExchangeRate, InvoiceDetails,
    LnUrlPayDetails, LnUrlWithdrawDetails, NodeInfo, RuntimeErrorCode, UnsupportedDataType,
};
use breez_sdk_core::{parse, BreezServices, InputType, Network};
use hex::encode;
use log::{error, info, log, Level};
use perro::{ensure, MapToError, OptionToError};
use regex::{Captures, Regex};
use std::str;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use uuid::Uuid;

pub struct Util {
    support: Arc<Support>,
}

impl Util {
    pub(crate) fn new(support: Arc<Support>) -> Self {
        Self { support }
    }

    /// Decode a user-provided string (usually obtained from QR-code or pasted).
    ///
    /// Requires network: **yes**
    pub fn decode_data(&self, data: String) -> std::result::Result<DecodedData, DecodeDataError> {
        match self.support.rt.handle().block_on(parse(&data)) {
            Ok(InputType::Bolt11 { invoice }) => {
                ensure!(
                    invoice.network == Network::Bitcoin,
                    DecodeDataError::Unsupported {
                        typ: UnsupportedDataType::Network {
                            network: invoice.network.to_string(),
                        },
                    }
                );

                Ok(DecodedData::Bolt11Invoice {
                    invoice_details: InvoiceDetails::from_ln_invoice(
                        invoice,
                        &self.support.get_exchange_rate(),
                    ),
                })
            }
            Ok(InputType::LnUrlPay { data }) => Ok(DecodedData::LnUrlPay {
                lnurl_pay_details: LnUrlPayDetails::from_lnurl_pay_request_data(
                    data,
                    &self.support.get_exchange_rate(),
                )?,
            }),
            Ok(InputType::BitcoinAddress { address }) => Ok(DecodedData::OnchainAddress {
                onchain_address_details: address,
            }),
            Ok(InputType::LnUrlAuth { .. }) => Err(DecodeDataError::Unsupported {
                typ: UnsupportedDataType::LnUrlAuth,
            }),
            Ok(InputType::LnUrlError { data }) => {
                Err(DecodeDataError::LnUrlError { msg: data.reason })
            }
            Ok(InputType::LnUrlWithdraw { data }) => Ok(DecodedData::LnUrlWithdraw {
                lnurl_withdraw_details: LnUrlWithdrawDetails::from_lnurl_withdraw_request_data(
                    data,
                    &self.support.get_exchange_rate(),
                ),
            }),
            Ok(InputType::NodeId { .. }) => Err(DecodeDataError::Unsupported {
                typ: UnsupportedDataType::NodeId,
            }),
            Ok(InputType::Url { .. }) => Err(DecodeDataError::Unsupported {
                typ: UnsupportedDataType::Url,
            }),
            Err(e) => Err(DecodeDataError::Unrecognized { msg: e.to_string() }),
        }
    }

    /// Get the wallet UUID v5 from the wallet pubkey
    ///
    /// If the auth flow has never succeeded in this Auth instance, this method will require network
    /// access.
    ///
    /// Requires network: **yes**
    pub fn query_wallet_pubkey_id(&self) -> Result<String> {
        self.support
            .auth
            .get_wallet_pubkey_id()
            .map_to_runtime_error(
                RuntimeErrorCode::AuthServiceUnavailable,
                "Failed to authenticate in order to get the wallet pubkey id",
            )
    }

    /// Get the payment UUID v5 from the payment hash
    ///
    /// Returns a UUID v5 derived from the payment hash. This will always return the same output
    /// given the same input.
    ///
    /// Parameters:
    /// * `payment_hash` - a payment hash represented in hex
    ///
    /// Requires network: **no**
    pub fn derive_payment_uuid(&self, payment_hash: String) -> Result<String> {
        derive_payment_uuid(payment_hash)
    }

    /// Request some basic info about the node
    ///
    /// Requires network: **no**
    pub fn get_node_info(&self) -> Result<NodeInfo> {
        self.support.get_node_info()
    }

    /// Get exchange rate on the BTC/default currency pair
    /// Please keep in mind that this method doesn't make any network calls. It simply retrieves
    /// previously fetched values that are frequently updated by a background task.
    ///
    /// The fetched exchange rates will be persisted across restarts to alleviate the consequences of a
    /// slow or unresponsive exchange rate service.
    ///
    /// The return value is an optional to deal with the possibility
    /// of no exchange rate values being known.
    ///
    /// Requires network: **no**
    pub fn get_exchange_rate(&self) -> Option<ExchangeRate> {
        let rates = self.support.task_manager.lock_unwrap().get_exchange_rates();
        let currency_code = self
            .support
            .user_preferences
            .lock_unwrap()
            .fiat_currency
            .clone();
        rates
            .iter()
            .find(|r| r.currency_code == currency_code)
            .cloned()
    }

    /// Prints additional debug information to the logs.
    ///
    /// Throws an error in case that the necessary information can't be retrieved.
    ///
    /// Requires network: **yes**
    pub fn log_debug_info(&self) -> Result<()> {
        self.support
            .rt
            .handle()
            .block_on(self.support.sdk.sync())
            .log_ignore_error(Level::Error, "Failed to sync node");

        let available_lsps = self
            .support
            .rt
            .handle()
            .block_on(self.support.sdk.list_lsps())
            .map_to_runtime_error(RuntimeErrorCode::NodeUnavailable, "Couldn't list lsps")?;

        let connected_lsp = self
            .support
            .rt
            .handle()
            .block_on(self.support.sdk.lsp_id())
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to get current lsp id",
            )?
            .unwrap_or("<no connection>".to_string());

        let node_state = self.support.sdk.node_info().map_to_runtime_error(
            RuntimeErrorCode::NodeUnavailable,
            "Failed to read node info",
        )?;

        let channels = self
            .support
            .rt
            .handle()
            .block_on(
                self.support
                    .sdk
                    .execute_dev_command("listpeerchannels".to_string()),
            )
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Couldn't execute `listpeerchannels` command",
            )?;

        let payments = self
            .support
            .rt
            .handle()
            .block_on(
                self.support
                    .sdk
                    .execute_dev_command("listpayments".to_string()),
            )
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Couldn't execute `listpayments` command",
            )?;

        let diagnostics = self
            .support
            .rt
            .handle()
            .block_on(self.support.sdk.generate_diagnostic_data())
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Couldn't call generate_diagnostic_data",
            )?;

        info!("3L version: {}", env!("GITHUB_REF"));
        info!("Wallet pubkey id: {:?}", self.query_wallet_pubkey_id());
        // Print connected peers, balances, inbound/outbound capacities, on-chain funds.
        info!("Node state:\n{node_state:?}");
        info!(
            "List of available lsps:\n{}",
            replace_byte_arrays_by_hex_string(&format!("{available_lsps:?}"))
        );
        info!("Connected lsp id: {connected_lsp}");
        info!(
            "List of peer channels:\n{}",
            replace_byte_arrays_by_hex_string(&channels)
        );
        info!(
            "List of payments:\n{}",
            replace_byte_arrays_by_hex_string(&payments)
        );
        info!("Diagnostic data:\n{diagnostics}");
        Ok(())
    }

    /// Returns the health check status of Breez and Greenlight services.
    ///
    /// Requires network: **yes**
    pub fn query_health_status(&self) -> Result<BreezHealthCheckStatus> {
        Ok(self
            .support
            .rt
            .handle()
            .block_on(BreezServices::service_health_check(
                self.support
                    .node_config
                    .breez_sdk_config
                    .breez_sdk_api_key
                    .clone(),
            ))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to get health status",
            )?
            .status)
    }
}

pub(crate) fn unix_timestamp_to_system_time(timestamp: u64) -> SystemTime {
    let duration = Duration::from_secs(timestamp);
    SystemTime::UNIX_EPOCH + duration
}

// Replaces all occurrences of byte arrays with their hex representation:
// 'Hello [15, 16, 255] world' -> 'Hello "0f10ff" world'
pub(crate) fn replace_byte_arrays_by_hex_string(original: &str) -> String {
    try_replacing_byte_arrays_by_hex_string(original).unwrap_or_else(|e| {
        error!("Failed to replace byte arrays by hex string: {e}");
        original.to_string()
    })
}
fn try_replacing_byte_arrays_by_hex_string(
    original: &str,
) -> std::result::Result<String, perro::Error<RuntimeErrorCode>> {
    let byte_array_pattern = Regex::new(r"\[([\d\s,]+)]")
        .map_to_permanent_failure("Invalid regex to replace byte arrays")?;

    replace_all(&byte_array_pattern, original, |caps: &Captures| {
        let bytes_as_string = caps
            .get(1)
            .ok_or_permanent_failure("Captures::get(1) returned None")?
            .as_str();
        let bytes = bytes_as_string
            .split(',')
            .map(|byte| byte.trim().parse::<u8>())
            .collect::<std::result::Result<Vec<u8>, _>>()
            .map_to_permanent_failure(format!(
                "Failed to parse into byte array: {bytes_as_string}"
            ))?;

        Ok(encode(bytes))
    })
}

fn replace_all(
    re: &Regex,
    original: &str,
    replacement: impl Fn(&Captures) -> std::result::Result<String, perro::Error<RuntimeErrorCode>>,
) -> std::result::Result<String, perro::Error<RuntimeErrorCode>> {
    let mut new = String::new();
    let mut last_match = 0;
    for caps in re.captures_iter(original) {
        let m = caps
            .get(0)
            .ok_or_permanent_failure("Captures::get(0) returned None")?;
        let subslice = original
            .get(last_match..m.start())
            .ok_or_permanent_failure("Indexing the match failed")?;
        new.push_str(subslice);
        new.push('\"');
        new.push_str(&replacement(&caps)?);
        new.push('\"');
        last_match = m.end();
    }
    let tail = original
        .get(last_match..)
        .ok_or_permanent_failure("Indexing the tail failed")?;
    new.push_str(tail);
    Ok(new)
}

pub(crate) trait LogIgnoreError {
    fn log_ignore_error(self, level: Level, message: &str);
}

impl<T, E: std::fmt::Display> LogIgnoreError for std::result::Result<T, E> {
    fn log_ignore_error(self, level: Level, message: &str) {
        if let Err(e) = self {
            log!(level, "{message}: {e}")
        }
    }
}

fn derive_payment_uuid(payment_hash: String) -> Result<String> {
    let hash = hex::decode(payment_hash).map_to_invalid_input("Invalid payment hash encoding")?;

    Ok(Uuid::new_v5(&Uuid::NAMESPACE_OID, &hash)
        .hyphenated()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::Error;

    #[test]
    fn test_replace_byte_arrays_by_hex_string() {
        let original = "Hello [15, 16, 255] world";
        let expected = "Hello \"0f10ff\" world";
        let actual = replace_byte_arrays_by_hex_string(original);
        assert_eq!(expected, &actual);
    }

    #[test]
    fn string_starts_and_ends_with_array_parsed_to_hex() {
        let original = "[186, 190] make some [192, 255, 238]";
        let expected = "\"babe\" make some \"c0ffee\"";
        let actual = replace_byte_arrays_by_hex_string(original);
        assert_eq!(expected, &actual);
    }

    #[test]
    fn arrays_within_words_parsed_to_hex() {
        let original = "Lipa W[161][30]t";
        let expected = "Lipa W\"a1\"\"1e\"t";
        let actual = replace_byte_arrays_by_hex_string(original);
        assert_eq!(expected, &actual);
    }

    #[test]
    fn empty_array_not_parsed_to_hex() {
        let original = "Hello [] world";
        let modified = replace_byte_arrays_by_hex_string(original);
        assert_eq!(original, &modified);
    }

    #[test]
    fn flawed_byte_array_not_parsed_to_hex() {
        let original = "Hello [15, 16, 1234] world";
        let parsed = replace_byte_arrays_by_hex_string(original);
        assert_eq!(original, &parsed);
    }

    const PAYMENT_HASH: &str = "0b78877a596f18d5f6effde3dda1df25a5cf20439ff1ac91478d7e518211040f";
    const PAYMENT_UUID: &str = "c6e597bd-0a98-5b46-8e74-f6098f5d16a3";

    #[test]
    fn test_payment_uuid() {
        let payment_uuid = derive_payment_uuid(PAYMENT_HASH.to_string());

        assert_eq!(payment_uuid, Ok(PAYMENT_UUID.to_string()));
    }

    #[test]
    fn test_payment_uuid_invalid_input() {
        let invalid_hash_encoding = derive_payment_uuid("INVALID_HEX_STRING".to_string());

        assert!(matches!(
            invalid_hash_encoding,
            Err(Error::InvalidInput { .. })
        ));

        assert_eq!(
            &invalid_hash_encoding.unwrap_err().to_string()[0..43],
            "InvalidInput: Invalid payment hash encoding"
        );
    }
}
