#[allow(clippy::derive_partial_eq_without_eq)]
pub mod lspd {
    tonic::include_proto!("lspd");
}

use crate::callbacks::LspCallback;
use crate::encryption::encrypt;
use crate::errors::*;

use bitcoin::hashes::hex::FromHex;
use bitcoin::secp256k1::PublicKey;
use lightning::ln::{PaymentHash, PaymentSecret};
use lightning::routing::gossip::RoutingFees;
use lightning::routing::router::RouteHintHop;
use lspd::{ChannelInformationReply, PaymentInformation};
use prost::Message;
use std::cmp::max;
use std::net::SocketAddr;
use std::str::FromStr;

#[derive(Debug, PartialEq, Eq)]
pub struct LspFee {
    pub channel_minimum_fee_msat: u64,
    pub channel_fee_permyriad: u64, // 100 is 1%
}

#[derive(Debug)]
pub(crate) struct LspInfo {
    pub pubkey: PublicKey,
    pub fee: LspFee,
    pub node_info: NodeInfo,
}

#[derive(Debug)]
pub(crate) struct NodeInfo {
    pubkey: PublicKey,
    #[allow(dead_code)]
    address: SocketAddr,
    fees: RoutingFees,
    cltv_expiry_delta: u16,
    htlc_minimum_msat: Option<u64>,
    htlc_maximum_msat: Option<u64>,
}

pub(crate) struct LspClient {
    lsp: Box<dyn LspCallback>,
}

pub(crate) struct PaymentRequest {
    pub payment_hash: PaymentHash,
    pub payment_secret: PaymentSecret,
    pub payee_pubkey: PublicKey,
    pub amount_msat: u64,
}

impl LspClient {
    pub fn new(lsp: Box<dyn LspCallback>) -> Self {
        Self { lsp }
    }

    pub fn query_info(&self) -> LipaResult<LspInfo> {
        let response = self.lsp.channel_information().map_to_runtime_error(
            RuntimeErrorCode::LspServiceUnavailable,
            "Failed to contact LSP",
        )?;
        parse_lsp_info(&response).prefix_error("Invalid LSP response")
    }

    pub fn register_payment(
        &self,
        payment_request: &PaymentRequest,
        lsp_info: &LspInfo,
    ) -> LipaResult<RouteHintHop> {
        let fee_msat = calculate_fee(payment_request.amount_msat, &lsp_info.fee);
        if fee_msat > payment_request.amount_msat {
            return Err(invalid_input("Payment amount must be bigger than fees"));
        }
        let outgoing_amount_msat = (payment_request.amount_msat - fee_msat) as i64;
        let payment_info = PaymentInformation {
            payment_hash: payment_request.payment_hash.0.to_vec(),
            payment_secret: payment_request.payment_secret.0.to_vec(),
            destination: payment_request.payee_pubkey.serialize().to_vec(),
            incoming_amount_msat: payment_request.amount_msat as i64,
            outgoing_amount_msat,
        };

        let payment_info = payment_info.encode_to_vec();
        let encrypted_payment_info = encrypt(&lsp_info.pubkey, &payment_info)
            .prefix_error("Failed to encrypt payment request")?;
        self.lsp
            .register_payment(encrypted_payment_info)
            .map_to_runtime_error(
                RuntimeErrorCode::LspServiceUnavailable,
                "Failed to contact LSP",
            )?;

        Ok(RouteHintHop {
            src_node_id: lsp_info.node_info.pubkey,
            short_channel_id: 0x10000000000_u64,
            fees: lsp_info.node_info.fees,
            cltv_expiry_delta: lsp_info.node_info.cltv_expiry_delta,
            htlc_minimum_msat: lsp_info.node_info.htlc_minimum_msat,
            htlc_maximum_msat: lsp_info.node_info.htlc_maximum_msat,
        })
    }
}

fn parse_lsp_info(bytes: &[u8]) -> LipaResult<LspInfo> {
    let info = ChannelInformationReply::decode(bytes)
        .map_to_invalid_input("Invalid ChannelInformationReply")?;

    let pubkey =
        PublicKey::from_slice(&info.lsp_pubkey).map_to_invalid_input("Invalid LSP pubkey")?;

    let ln_pubkey = Vec::from_hex(&info.pubkey).map_to_invalid_input("Invalid LN node pubkey")?;
    let ln_pubkey =
        PublicKey::from_slice(&ln_pubkey).map_to_invalid_input("Invalid LN node pubkey")?;

    let fee = LspFee {
        channel_minimum_fee_msat: info.channel_minimum_fee_msat as u64,
        channel_fee_permyriad: info.channel_fee_permyriad as u64,
    };

    let address = SocketAddr::from_str(&info.host).map_to_invalid_input("Invalid LN node host")?;

    let node_info = NodeInfo {
        pubkey: ln_pubkey,
        address,
        fees: RoutingFees {
            base_msat: info.base_fee_msat as u32,
            proportional_millionths: (info.fee_rate * 1_000_000_f64) as u32,
        },
        cltv_expiry_delta: info.time_lock_delta as u16,
        htlc_minimum_msat: Some(info.min_htlc_msat as u64),
        htlc_maximum_msat: None,
    };

    Ok(LspInfo {
        pubkey,
        fee,
        node_info,
    })
}

pub(crate) fn calculate_fee(value_msat: u64, fee: &LspFee) -> u64 {
    let fee_value = value_msat * fee.channel_fee_permyriad / 10_000 / 1_000 * 1_000;
    max(fee_value, fee.channel_minimum_fee_msat)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::hashes::hex::{FromHex, ToHex};

    #[test]
    fn test_parse_invalid_lsp_info() {
        let bytes = Vec::from_hex("0a066e").unwrap();
        assert!(parse_lsp_info(&bytes)
            .unwrap_err()
            .to_string()
            .starts_with("InvalidInput: Invalid ChannelInformationReply"));
    }

    #[test]
    fn test_parse_lsp_info() {
        let bytes = "0a066e696769726912423033333066613837343134326135626163643137383831653831356131666465313661313437613063343037343630303931643133353430306261393538323564641a0e3132372e302e302e313a3937333520f7853d280630e807398dedb5a0f7c6b03e40900148d80450285a2103ca7819d982a95b29bcdbf00a06d99639b523da40e5f43402027097965f5788066080a7ed016880897a";
        let bytes = Vec::from_hex(bytes).unwrap();
        let lsp_info = parse_lsp_info(&bytes).unwrap();

        assert_eq!(
            lsp_info.pubkey.serialize().to_hex(),
            "03ca7819d982a95b29bcdbf00a06d99639b523da40e5f43402027097965f578806"
        );
        let fee = LspFee {
            channel_minimum_fee_msat: 2_000_000,
            channel_fee_permyriad: 40,
        };
        assert_eq!(lsp_info.fee, fee);
        assert_eq!(
            lsp_info.node_info.pubkey.to_hex(),
            "0330fa874142a5bacd17881e815a1fde16a147a0c407460091d135400ba95825dd"
        );
        assert_eq!(lsp_info.node_info.address.to_string(), "127.0.0.1:9735");
        let routing_fees = RoutingFees {
            base_msat: 1000,
            proportional_millionths: 1,
        };
        assert_eq!(lsp_info.node_info.fees, routing_fees);
        assert_eq!(lsp_info.node_info.cltv_expiry_delta, 144);
        assert_eq!(lsp_info.node_info.htlc_minimum_msat, Some(600));
        assert_eq!(lsp_info.node_info.htlc_maximum_msat, None);
    }

    #[test]
    #[rustfmt::skip]
    pub fn test_calculate_fee() {
        let fee = LspFee {
            channel_minimum_fee_msat: 2_000_000,
            channel_fee_permyriad: 40,
        };
        assert_eq!(calculate_fee(             0, &fee),  2_000_000);
        assert_eq!(calculate_fee(             2, &fee),  2_000_000);
        assert_eq!(calculate_fee(   200_000_000, &fee),  2_000_000);
        assert_eq!(calculate_fee( 1_000_000_000, &fee),  4_000_000);
        // 1000000001 * 0.004 = 4000000.004 -> 4000 sats
        assert_eq!(calculate_fee( 1_000_000_001, &fee),  4_000_000);
        // 1000000250 * 0.004 = 4000001.0 -> 4000 sats
        assert_eq!(calculate_fee( 1_000_000_250, &fee),  4_000_000);
        // 1000000251 * 0.004 = 4000001.004 -> 4000 sats
        assert_eq!(calculate_fee( 1_000_000_251, &fee),  4_000_000);
        // 1000249999 * 0.004 = 4000999.996 -> 4000 sats
        assert_eq!(calculate_fee( 1_000_249_999, &fee),  4_000_000);
        // 1000250000 * 0.004 = 4001000.0 -> 4001 sats
        assert_eq!(calculate_fee( 1_000_250_000, &fee),  4_001_000);
        assert_eq!(calculate_fee( 2_000_000_000, &fee),  8_000_000);
        assert_eq!(calculate_fee(20_000_000_000, &fee), 80_000_000);

        let zero_fee = LspFee {
            channel_minimum_fee_msat: 0,
            channel_fee_permyriad: 0,
        };
        assert_eq!(calculate_fee(0, &zero_fee), 0);
        assert_eq!(calculate_fee(100_000_000, &zero_fee), 0);
    }
}
