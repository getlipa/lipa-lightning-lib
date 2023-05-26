use crate::errors::*;
use crate::lsp::{LspClient, PaymentRequest};
use crate::node_info::{estimate_max_incoming_payment_size, get_channels_info};
use crate::types::{ChannelManager, KeysManager};
use bitcoin::bech32::ToBase32;
use std::str::FromStr;
use std::time::{Duration, SystemTime};

use crate::data_store::DataStore;
use crate::lsp;
use crate::payment::FiatValues;
use bitcoin::hashes::hex::ToHex;
use bitcoin::hashes::{sha256, Hash};
use bitcoin::Network;
use lightning::chain::keysinterface::{NodeSigner, Recipient};
use lightning::ln::channelmanager::ChannelDetails;
use lightning::routing::gossip::RoutingFees;
use lightning::routing::router::{RouteHint, RouteHintHop};
use lightning_invoice::{Currency, Invoice, InvoiceBuilder, InvoiceDescription, SignedRawInvoice};
use log::info;
use perro::{invalid_input, MapToError, MapToErrorForUnitType, ResultTrait};
use secp256k1::ecdsa::RecoverableSignature;

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct InvoiceDetails {
    pub invoice: String,
    pub amount_msat: Option<u64>,
    pub description: String,
    pub payment_hash: String,
    pub payee_pub_key: String,
    pub creation_timestamp: SystemTime,
    pub expiry_interval: Duration,
    pub expiry_timestamp: SystemTime,
}

pub(crate) struct CreateInvoiceParams {
    pub amount_msat: u64,
    pub currency: Currency,
    pub description: String,
    pub metadata: String,
}

pub(crate) fn get_invoice_details(invoice: &Invoice) -> Result<InvoiceDetails> {
    let description = match invoice.description() {
        InvoiceDescription::Direct(d) => d.to_string(),
        InvoiceDescription::Hash(_) => String::new(),
    };

    let payee_pub_key = match invoice.payee_pub_key() {
        None => invoice.recover_payee_pub_key().to_string(),
        Some(p) => p.to_string(),
    };

    Ok(InvoiceDetails {
        invoice: invoice.to_string(),
        amount_msat: invoice.amount_milli_satoshis(),
        description,
        payment_hash: invoice.payment_hash().to_string(),
        payee_pub_key,
        creation_timestamp: invoice.timestamp(),
        expiry_interval: invoice.expiry_time(),
        expiry_timestamp: invoice.timestamp() + invoice.expiry_time(),
    })
}

pub(crate) fn parse_invoice(invoice: &str) -> Result<Invoice> {
    Invoice::from_str(chomp_prefix(invoice.trim()))
        .map_to_invalid_input("Invalid invoice - parse failure")
}

pub(crate) fn validate_invoice(network: Network, invoice: &Invoice) -> Result<()> {
    let invoice_network = network_from_currency(invoice.currency());

    if network != invoice_network {
        return Err(invalid_input("Invalid invoice: network mismatch"));
    }

    if invoice.timestamp() + invoice.expiry_time() < SystemTime::now() {
        return Err(invalid_input("Invalid invoice: expired"));
    }

    Ok(())
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
    fiat_values: Option<FiatValues>,
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
        let (payment_hash, payment_secret) = channel_manager
            .create_inbound_payment(Some(amount_msat), 1000, None)
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
        .amount_milli_satoshis(amount_msat)
        .current_timestamp()
        .expiry_time(Duration::from_secs(10 * 60))
        .min_final_cltv_expiry_delta(144);
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
            fiat_values,
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
    use std::time::UNIX_EPOCH;

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

    const THOUSAND_SATS: u64 = 1_000_000;

    const SECONDS_IN_AN_HOUR: u64 = 3600;

    const REGTEST_INVOICE: &str = "lnbcrt10u1p36gm69pp56haryefc0cvsdc7ucwgnm4j3kul5pjkdlc94vwkju5xktwsvtv6sdpyf36kkefvypyjqstdypvk7atjyprxzargv4eqcqzpgxqrrsssp5e777z0f2g05l99yw8cuvhnq68e7xstulcan5tzvdh4f6642f836q9qyyssqw2g88srqdqrqngzcrzq877hz64sf320kgh5yjwwg7negxeuq909kac33tgheq7re5k7luh6q3xam6jk46p0cepkx89hfdl9g0mx24csqgxhk8x";
    const REGTEST_INVOICE_HASH: &str =
        "d5fa3265387e1906e3dcc3913dd651b73f40cacdfe0b563ad2e50d65ba0c5b35";
    const REGTEST_INVOICE_PAYEE_PUB_KEY: &str =
        "02cc8a9ee52470a08bfe54194cb9b25021bed4c05db6c08118f6d92f97c070b234";
    const REGTEST_INVOICE_DURATION_FROM_UNIX_EPOCH: Duration = Duration::from_secs(1671720773);
    const REGTEST_INVOICE_EXPIRY: Duration = Duration::from_secs(SECONDS_IN_AN_HOUR);
    const REGTEST_INVOICE_DESCRIPTION: &str = "Luke, I Am Your Father";

    const MAINNET_INVOICE: &str = "lnbc10u1p36gu9hpp5dmg5up4kyhkefpxue5smrgucg889esu6zc9vyntnzmqyyr4dycaqdqqcqpjsp5h0n3nc53t2tcm8a9kjpsdgql7ex2c7qrpc0dn4ja9c64adycxx7s9q7sqqqqqqqqqqqqqqqqqqqsqqqqqysgqmqz9gxqyjw5qrzjqwryaup9lh50kkranzgcdnn2fgvx390wgj5jd07rwr3vxeje0glcll63swcqxvlas5qqqqlgqqqqqeqqjqmstdrvfcyq9as46xuu63dfgstehmthqlxg8ljuyqk2z9mxvhjfzh0a6jm53rrgscyd7v0y7dj4zckq69tlsdex0352y89wmvvv0j3gspnku4sz";
    const MAINNET_INVOICE_HASH: &str =
        "6ed14e06b625ed9484dccd21b1a39841ce5cc39a160ac24d7316c0420ead263a";
    const MAINNET_INVOICE_PAYEE_PUB_KEY: &str =
        "025f73fbb3fe0c6d07e0df48dd1addb82bb2d27400183881214f5183b00333fd85";
    const MAINNET_INVOICE_DURATION_FROM_UNIX_EPOCH: Duration = Duration::from_secs(1671721143);
    const MAINNET_INVOICE_EXPIRY: Duration = Duration::from_secs(604800);
    const MAINNET_INVOICE_DESCRIPTION: &str = "";

    const ONCHAIN_ADDRESS: &str = "bc1qsuhxszxhk7nnzy2888sj66ru7kcwp70jexvd8z";

    #[test]
    fn test_invoice_parsing() {
        // Test valid hardcoded regtest invoice
        let invoice = parse_invoice(REGTEST_INVOICE).unwrap();
        let invoice_details = get_invoice_details(&invoice).unwrap();
        assert_eq!(invoice_details.payment_hash, REGTEST_INVOICE_HASH);
        assert_eq!(
            invoice_details
                .creation_timestamp
                .duration_since(UNIX_EPOCH)
                .unwrap(),
            REGTEST_INVOICE_DURATION_FROM_UNIX_EPOCH
        );
        assert_invoice_details(
            invoice_details,
            THOUSAND_SATS,
            REGTEST_INVOICE_DESCRIPTION,
            SystemTime::UNIX_EPOCH + REGTEST_INVOICE_DURATION_FROM_UNIX_EPOCH,
            REGTEST_INVOICE_EXPIRY,
            REGTEST_INVOICE_PAYEE_PUB_KEY,
            REGTEST_INVOICE_HASH,
        );

        // Test valid hardcoded mainnet invoice
        let invoice = parse_invoice(MAINNET_INVOICE).unwrap();
        let invoice_details = get_invoice_details(&invoice).unwrap();
        assert_eq!(invoice_details.payment_hash, MAINNET_INVOICE_HASH);
        assert_eq!(
            invoice_details
                .creation_timestamp
                .duration_since(UNIX_EPOCH)
                .unwrap(),
            MAINNET_INVOICE_DURATION_FROM_UNIX_EPOCH
        );
        assert_invoice_details(
            invoice_details,
            THOUSAND_SATS,
            MAINNET_INVOICE_DESCRIPTION,
            SystemTime::UNIX_EPOCH + MAINNET_INVOICE_DURATION_FROM_UNIX_EPOCH,
            MAINNET_INVOICE_EXPIRY,
            MAINNET_INVOICE_PAYEE_PUB_KEY,
            MAINNET_INVOICE_HASH,
        );

        // Test invalid hardcoded invoice (fail to parse)
        let parse_invoice_result = parse_invoice(ONCHAIN_ADDRESS);
        assert!(parse_invoice_result.is_err());
        assert!(parse_invoice_result
            .err()
            .unwrap()
            .to_string()
            .contains("Invalid invoice - parse failure"));
    }

    #[allow(clippy::too_many_arguments)]
    fn assert_invoice_details(
        invoice_details: InvoiceDetails,
        amount_msat: u64,
        description: &str,
        creation_timestamp: SystemTime,
        expiry: Duration,
        payee_pub_key: &str,
        payment_hash: &str,
    ) {
        assert_eq!(invoice_details.amount_msat.unwrap(), amount_msat);
        assert_eq!(invoice_details.description, description);
        assert_eq!(invoice_details.creation_timestamp, creation_timestamp);
        assert_eq!(invoice_details.expiry_interval, expiry);
        assert_eq!(invoice_details.payee_pub_key, payee_pub_key);
        assert_eq!(invoice_details.payment_hash, payment_hash);
    }
}
