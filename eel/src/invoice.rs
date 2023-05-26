use crate::errors::*;
use crate::interfaces::ExchangeRate;
use crate::lsp::{LspClient, PaymentRequest};
use crate::node_info::{estimate_max_incoming_payment_size, get_channels_info};
use crate::types::{ChannelManager, KeysManager};
use bitcoin::bech32::ToBase32;
use std::str::FromStr;
use std::time::Duration;

use crate::data_store::DataStore;
use crate::lsp;
use bitcoin::hashes::hex::ToHex;
use bitcoin::hashes::{sha256, Hash};
use bitcoin::Network;
use lightning::chain::keysinterface::{NodeSigner, Recipient};
use lightning::ln::channelmanager::ChannelDetails;
use lightning::routing::gossip::RoutingFees;
use lightning::routing::router::{RouteHint, RouteHintHop};
use lightning_invoice::{Currency, Invoice, InvoiceBuilder};
use lightning_invoice::{ParseOrSemanticError, SignedRawInvoice};
use log::info;
use perro::{invalid_input, MapToError, MapToErrorForUnitType, ResultTrait};
use secp256k1::ecdsa::RecoverableSignature;

pub(crate) struct CreateInvoiceParams {
    pub amount_msat: u64,
    pub currency: Currency,
    pub description: String,
    pub metadata: String,
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeInvoiceError {
    #[error("Parse error: {msg}")]
    ParseError { msg: String },
    #[error("Semantic error: {msg}")]
    SemanticError { msg: String },
    #[error("Network mismatch (expected {expected}, found {found})")]
    NetworkMismatch { expected: Network, found: Network },
}

pub(crate) fn decode_invoice(
    invoice: &str,
    expected: Network,
) -> std::result::Result<Invoice, DecodeInvoiceError> {
    let invoice = match Invoice::from_str(chomp_prefix(invoice.trim())) {
        Ok(invoice) => match invoice.amount_milli_satoshis() {
            Some(0) => Err(DecodeInvoiceError::SemanticError {
                msg: "Invoice amount contains leading zeros".to_string(),
            }),
            _ => Ok(invoice),
        },
        Err(ParseOrSemanticError::ParseError(err)) => Err(DecodeInvoiceError::ParseError {
            msg: err.to_string(),
        }),
        Err(ParseOrSemanticError::SemanticError(err)) => Err(DecodeInvoiceError::SemanticError {
            msg: err.to_string(),
        }),
    }?;
    let found = network_from_currency(invoice.currency());
    if expected != found {
        return Err(DecodeInvoiceError::NetworkMismatch { expected, found });
    }
    Ok(invoice)
}

fn network_from_currency(currency: Currency) -> Network {
    match currency {
        Currency::Bitcoin => Network::Bitcoin,
        Currency::BitcoinTestnet => Network::Testnet,
        Currency::Regtest => Network::Regtest,
        Currency::Simnet => Network::Signet,
        Currency::Signet => Network::Signet,
    }
}

fn chomp_prefix(string: &str) -> &str {
    string.strip_prefix("lightning:").unwrap_or(string)
}

pub(crate) async fn create_invoice(
    params: CreateInvoiceParams,
    channel_manager: &ChannelManager,
    lsp_client: &LspClient,
    keys_manager: &KeysManager,
    data_store: &mut DataStore,
    fiat_currency: &str,
    exchange_rates: Vec<ExchangeRate>,
) -> Result<SignedRawInvoice> {
    let amount_msat = params.amount_msat;

    // Do we need a new channel to receive this payment?
    let channels_info = get_channels_info(&channel_manager.list_channels());
    let max_incoming_payment_size = estimate_max_incoming_payment_size(&channels_info);
    let needs_channel_opening = max_incoming_payment_size < amount_msat;

    let payee_pubkey = channel_manager.get_our_node_id();

    let (payment_hash, payment_secret, private_routes, lsp_fee) = if needs_channel_opening {
        let lsp_info = lsp_client
            .query_info()
            .await
            .lift_invalid_input()
            .prefix_error("Failed to query LSPD")?;

        let lsp_fee = lsp::calculate_fee(amount_msat, &lsp_info.fee);
        if lsp_fee >= amount_msat {
            return Err(invalid_input("Payment amount must be higher than lsp fees"));
        }
        let incoming_amount_msat = amount_msat - lsp_fee;

        info!(
            "Not enough inbound capacity for {} msat, needs channel opening, will only receive {} msat due to LSP fees",
            amount_msat, incoming_amount_msat
        );

        let (payment_hash, payment_secret) = channel_manager
            .create_inbound_payment(Some(incoming_amount_msat), 1000, None)
            .map_to_invalid_input("Amount is greater than total bitcoin supply")?;

        let payment_request = PaymentRequest {
            payment_hash,
            payment_secret,
            payee_pubkey,
            amount_msat,
        };
        let hint_hop = lsp_client
            .register_payment(&payment_request, &lsp_info)
            .await
            .lift_invalid_input()
            .prefix_error("Failed to register payment")?;
        (
            payment_hash,
            payment_secret,
            vec![RouteHint(vec![hint_hop])],
            lsp_fee,
        )
    } else {
        let amount_msat = if amount_msat > 0 {
            Some(amount_msat)
        } else {
            None
        };
        let (payment_hash, payment_secret) = channel_manager
            .create_inbound_payment(amount_msat, 1000, None)
            .map_to_invalid_input("Amount is greater than total bitcoin supply")?;

        (
            payment_hash,
            payment_secret,
            construct_private_routes(&channel_manager.list_usable_channels()),
            0,
        )
    };

    let payment_hash = sha256::Hash::from_slice(&payment_hash.0)
        .map_to_permanent_failure("Failed to convert payment hash")?;
    let mut builder = InvoiceBuilder::new(params.currency)
        .description(params.description.clone())
        .payment_hash(payment_hash)
        .payment_secret(payment_secret)
        .payee_pub_key(payee_pubkey)
        .current_timestamp()
        .expiry_time(Duration::from_secs(10 * 60))
        .min_final_cltv_expiry_delta(144)
        .basic_mpp();
    if amount_msat > 0 {
        builder = builder.amount_milli_satoshis(amount_msat);
    }
    for private_route in private_routes {
        builder = builder.private_route(private_route);
    }

    let raw_invoice = builder
        .build_raw()
        .map_to_permanent_failure("Failed to construct invoice")?;

    let signature = keys_manager
        .sign_invoice(
            raw_invoice.hrp.to_string().as_bytes(),
            &raw_invoice.data.to_base32(),
            Recipient::Node,
        )
        .map_to_permanent_failure("Failed to sign invoice")?;
    let invoice = raw_invoice
        .sign(|_| Ok::<RecoverableSignature, ()>(signature))
        .map_to_permanent_failure("Failed to sign invoice")?;

    data_store
        .new_incoming_payment(
            &payment_hash.to_hex(),
            amount_msat,
            lsp_fee,
            &params.description,
            &invoice.to_string(),
            &params.metadata,
            fiat_currency,
            exchange_rates,
        )
        .map_to_permanent_failure("Failed to store new payment in payment db")?;

    Ok(invoice)
}

fn construct_private_routes(channels: &Vec<ChannelDetails>) -> Vec<RouteHint> {
    let mut route_hints = Vec::new();
    for channel in channels {
        if channel.is_usable && !channel.is_public {
            if let (Some(channel_counterparty_forwarding_info), Some(short_channel_id)) = (
                channel.counterparty.forwarding_info.clone(),
                channel.get_inbound_payment_scid(),
            ) {
                let fees = RoutingFees {
                    base_msat: channel_counterparty_forwarding_info.fee_base_msat,
                    proportional_millionths: channel_counterparty_forwarding_info
                        .fee_proportional_millionths,
                };
                let hint_hop = RouteHintHop {
                    src_node_id: channel.counterparty.node_id,
                    short_channel_id,
                    fees,
                    cltv_expiry_delta: channel_counterparty_forwarding_info.cltv_expiry_delta,
                    htlc_minimum_msat: channel.inbound_htlc_minimum_msat,
                    htlc_maximum_msat: channel.inbound_htlc_maximum_msat,
                };
                route_hints.push(RouteHint(vec![hint_hop]));
            }
        }
    }
    route_hints
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::channels::channel;

    #[test]
    fn test_parse_invoice() {
        let invoice_with_leading_0_amount = "lntb0m1pjgws2fdqqpp5l8a6n6q0eyu8y4ae7num7v52nf8kr0tjzfk9tgy4fjwtkpq7twnqsp5k9c2zqtrfen7q7g84upgluw9p35dgjfn6g04hrjn6klewlqrrwvq9qyysgqnp4qvwrr7m20cc4c5dx2l78ujjeu78wau5dq7df8xvx0d3sgytqsqqkyxqzjccqzysrzjq0ucfsctzrrr7xrnyatdgtrwp4e4qamrl66psz6m67za9hz2xhdhtapyqqqqqqqq95qqqqqqqqqqqqqqjqrzjq0ucfsctzrrr7xrnyatdgtrwp4e4qamrl66psz6m67za9hz2xhdhtapyqqqqqqqqxcqqqqqqqqqqqqqq2qhad02pjcq8jrlp4m4t6prrt36wyrg2d7xupn9dnv4dzmh4adewkps6uarrjgwgff75puac3d73y9ar9z7ahhaantk99jurm34l5px0qpjhtahr";
        let err = decode_invoice(invoice_with_leading_0_amount, Network::Testnet).unwrap_err();
        assert!(matches!(err, DecodeInvoiceError::SemanticError { .. }));
    }

    #[test]
    fn test_construct_private_routes() {
        assert_eq!(construct_private_routes(&Vec::new()), Vec::new());

        let mut channel1 = channel();
        channel1.is_usable = true;
        assert_eq!(construct_private_routes(&vec![channel1.clone()]).len(), 1);

        let mut public_channel = channel();
        public_channel.is_usable = true;
        public_channel.is_public = true;
        assert_eq!(
            construct_private_routes(&vec![public_channel.clone()]).len(),
            0
        );

        let mut channel2 = channel();
        channel2.is_usable = true;
        assert_eq!(
            construct_private_routes(&vec![
                public_channel.clone(),
                channel1.clone(),
                channel2.clone()
            ])
            .len(),
            2
        );
    }
}
