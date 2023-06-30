use crate::errors::Result;
use crate::errors::RuntimeErrorCode;
use crate::lsp::{LspFee, LspInfo, NodeInfo};

use bitcoin::secp256k1::PublicKey;
use lightning::routing::gossip::RoutingFees;
use perro::{MapToError, OptionToError};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::cmp::max;
use std::net::SocketAddr;
use std::str::FromStr;

pub fn calculate_fee(value_msat: u64, fee: &LspFee) -> u64 {
    let fee_value = value_msat * fee.channel_fee_permyriad / 10_000;
    max(fee_value, fee.channel_minimum_fee_msat)
}

#[derive(Clone, Debug)]
pub(crate) struct LspClient {
    pub url: String,
    pub http_client: Client,
}

#[derive(Debug, Serialize, Deserialize)]
struct GetInfoResponse {
    pub pubkey: String,
    pub connection_methods: Vec<GetInfoAddress>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct GetInfoAddress {
    #[serde(rename = "type")]
    pub item_type: GetInfoAddressType,
    pub port: u16,
    pub address: String,
}

/// Type of connection
#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
#[allow(clippy::upper_case_acronyms)]
enum GetInfoAddressType {
    DNS,
    IPV4,
    IPV6,
    TORV2,
    TORV3,
    WEBSOCKET,
}

#[derive(Serialize, Deserialize)]
struct ProposalRequest {
    pub bolt11: String,
}

#[derive(Serialize, Deserialize)]
struct ProposalResponse {
    pub jit_bolt11: String,
}

const GET_INFO_PATH: &str = "/api/v1/info";
const PROPOSAL_PATH: &str = "/api/v1/proposal";

impl LspClient {
    pub fn new(url: String) -> Result<Self> {
        // TODO: Configure timeout.
        let http_client = Client::new();
        Ok(LspClient { url, http_client })
    }

    pub async fn query_info(&self) -> Result<LspInfo> {
        let get_info_response: GetInfoResponse = self
            .http_client
            .get(format!("{}{}", &self.url, GET_INFO_PATH))
            .send()
            .await
            .map_to_runtime_error(
                RuntimeErrorCode::LspServiceUnavailable,
                "Failed to get reponse",
            )?
            .json()
            .await
            .map_to_runtime_error(RuntimeErrorCode::LspServiceUnavailable, "Invalid json")?;

        let pubkey = PublicKey::from_str(&get_info_response.pubkey)
            .map_to_runtime_error(RuntimeErrorCode::LspServiceUnavailable, "Invalid pubkey")?;
        let address = get_info_response
            .connection_methods
            .iter()
            .filter(|address| {
                matches!(
                    address.item_type,
                    GetInfoAddressType::IPV4 | GetInfoAddressType::IPV6
                )
            })
            .min_by_key(|address| match address.item_type {
                GetInfoAddressType::IPV4 => 0,
                GetInfoAddressType::IPV6 => 1,
                _ => unreachable!(), // TODO: Return permanent failure.
            })
            .ok_or_permanent_failure("No suitable connection method found")?;
        let address = format!("{}:{}", address.address, address.port);
        let address =
            SocketAddr::from_str(&address).map_to_invalid_input("Invalid LN node host")?;

        let fee = LspFee {
            channel_minimum_fee_msat: 10_000_000,
            channel_fee_permyriad: 5,
        };

        let node_info = NodeInfo {
            pubkey,
            address,
            fees: RoutingFees {
                base_msat: 0,
                proportional_millionths: (fee.channel_fee_permyriad * 100) as u32,
            },
            cltv_expiry_delta: 0,
            htlc_minimum_msat: None,
            htlc_maximum_msat: None,
        };

        Ok(LspInfo {
            pubkey,
            fee,
            node_info,
        })
    }

    pub async fn wrap_invoice(&self, bolt11: String) -> Result<String> {
        let payload = ProposalRequest { bolt11 };

        let proposal_response: ProposalResponse = self
            .http_client
            .post(format!("{}{}", &self.url, PROPOSAL_PATH))
            .json(&payload)
            .send()
            .await
            .map_to_runtime_error(
                RuntimeErrorCode::LspServiceUnavailable,
                "Failed to get reponse",
            )?
            .json()
            .await
            .map_to_runtime_error(RuntimeErrorCode::LspServiceUnavailable, "Invalid json")?;

        // TODO: Verify that LSP fee matches our expectation and check that the
        //       payment hash is the same as in the original invoice.
        Ok(proposal_response.jit_bolt11)
    }
}
