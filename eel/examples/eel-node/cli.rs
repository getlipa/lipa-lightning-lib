use bitcoin::secp256k1::PublicKey;
use chrono::{DateTime, Utc};
use colored::Colorize;
use rustyline::config::Builder;
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::Editor;
use std::path::Path;

use crate::LightningNode;

pub(crate) fn poll_for_user_input(node: &LightningNode, log_file_path: &str) {
    println!("{}", "Eel Example Node".yellow().bold());
    println!("Detailed logs are available at {}", log_file_path);
    println!("To stop the node, please type \"stop\" for a graceful shutdown.");
    println!(
        "Local Node ID is: {}",
        PublicKey::from_slice(&node.get_node_info().node_pubkey).unwrap()
    );

    let config = Builder::new().auto_add_history(true).build();
    let mut rl = Editor::<(), DefaultHistory>::with_config(config).unwrap();
    let history_path = Path::new(".eel_cli_history");
    let _ = rl.load_history(history_path);

    let prompt = "eel ϟ ".bold().yellow().to_string();
    loop {
        let line = match rl.readline(&prompt) {
            Ok(line) => line,
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                println!("{}", e.to_string().red());
                continue;
            }
        };

        let mut words = line.split_whitespace();
        if let Some(word) = words.next() {
            match word {
                "help" => help(),
                "nodeinfo" => {
                    node_info(node);
                }
                "lspfee" => {
                    lsp_fee(node);
                }
                "exchangerates" => {
                    if let Err(message) = get_exchange_rates(node) {
                        println!("{}", message.red());
                    }
                }
                "invoice" => {
                    if let Err(message) = create_invoice(node, &mut words) {
                        println!("{}", message.red());
                    }
                }
                "decodeinvoice" => {
                    if let Err(message) = decode_invoice(node, &mut words) {
                        println!("{}", message.red());
                    }
                }
                "payinvoice" => {
                    if let Err(message) = pay_invoice(node, &mut words) {
                        println!("{}", message.red());
                    }
                }
                "payopeninvoice" => {
                    if let Err(message) = pay_open_invoice(node, &mut words) {
                        println!("{}", message.red());
                    }
                }
                "listpayments" => {
                    if let Err(message) = list_payments(node) {
                        println!("{}", message.red());
                    }
                }
                "foreground" => {
                    node.foreground();
                }
                "background" => {
                    node.background();
                }
                "stop" => {
                    break;
                }
                _ => println!(
                    "{}",
                    "Unknown command. See \"help\" for available commands.".red()
                ),
            }
        }
    }
    let _ = rl.append_history(history_path);
}

fn help() {
    println!("  nodeinfo");
    println!("  lspfee");
    println!("  exchangerates");
    println!("");
    println!("  invoice <amount in millisats> [description]");
    println!("  decodeinvoice <invoice>");
    println!("  payinvoice <invoice>");
    println!("  payopeninvoice <invoice> <amount in millisats>");
    println!("");
    println!("  listpayments");
    println!("");
    println!("  foreground");
    println!("  background");
    println!("");
    println!("  stop");
}

fn lsp_fee(node: &LightningNode) {
    let lsp_fee = node.query_lsp_fee().unwrap();
    println!(
        " Min fee: {} sats",
        lsp_fee.channel_minimum_fee_msat as f64 / 1_000f64
    );
    println!(
        "Fee rate: {}%",
        lsp_fee.channel_fee_permyriad as f64 / 100f64
    );
}

fn node_info(node: &LightningNode) {
    let node_info = node.get_node_info();
    println!(
        "Node PubKey: {}",
        PublicKey::from_slice(&node_info.node_pubkey).unwrap()
    );
    println!("Number of connected peers: {}", node_info.num_peers);
    println!(
        "       Number of channels: {}",
        node_info.channels_info.num_channels
    );
    println!(
        "Number of usable channels: {}",
        node_info.channels_info.num_usable_channels
    );
    println!(
        "    Local balance in sat: {}",
        node_info.channels_info.local_balance_sat
    );
    println!(
        " Inbound capacity in sat: {}",
        node_info.channels_info.inbound_capacity_sat
    );
    println!(
        "Outbound capacity in sat: {}",
        node_info.channels_info.outbound_capacity_sat
    );
}

fn get_exchange_rates(node: &LightningNode) -> Result<(), String> {
    let rates = node.get_exchange_rates().map_err(|e| e.to_string())?;
    println!("{}: {} sats", rates.currency_code, rates.rate);
    println!("USD: {} sats", rates.usd_rate);
    Ok(())
}

fn create_invoice<'a>(
    node: &LightningNode,
    words: &mut dyn Iterator<Item = &'a str>,
) -> Result<(), String> {
    let amount = words
        .next()
        .ok_or_else(|| "Error: amount in millisats is required".to_string())?;
    let amount: u64 = amount
        .parse()
        .map_err(|_| "Error: amount should be an integer number".to_string())?;
    let description = words.collect::<Vec<_>>().join(" ");
    let invoice_details = node
        .create_invoice(amount, description, String::new())
        .map_err(|e| e.to_string())?;
    println!("{}", invoice_details.invoice);
    Ok(())
}

fn decode_invoice<'a>(
    node: &LightningNode,
    words: &mut dyn Iterator<Item = &'a str>,
) -> Result<(), String> {
    let invoice = words
        .next()
        .ok_or_else(|| "Error: invoice is required".to_string())?;

    let invoice_details = match node.decode_invoice(invoice.to_string()) {
        Ok(id) => id,
        Err(e) => return Err(e.to_string()),
    };

    println!("Invoice details:");
    println!(
        "  Amount msats        {}",
        invoice_details.amount_msat.unwrap()
    );
    println!("  Description         {}", invoice_details.description);
    println!("  Payment hash        {}", invoice_details.payment_hash);
    println!("  Payee public key    {}", invoice_details.payee_pub_key);
    println!(
        "  Invoice timestamp   {:?}",
        invoice_details.creation_timestamp
    );
    println!(
        "  Expiry interval     {:?}",
        invoice_details.expiry_interval
    );

    Ok(())
}

fn pay_invoice(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<(), String> {
    let invoice = words
        .next()
        .ok_or_else(|| "invoice is required".to_string())?;

    if words.next().is_some() {
        return Err("To many arguments. Specifying an amount is only allowed for open invoices.  To pay an open invoice use 'payopeninvoice'.".to_string());
    }

    match node.pay_invoice(invoice.to_string(), String::new()) {
        Ok(_) => {}
        Err(e) => return Err(e.to_string()),
    };

    Ok(())
}

fn pay_open_invoice(
    node: &LightningNode,
    words: &mut dyn Iterator<Item = &str>,
) -> Result<(), String> {
    let invoice = words
        .next()
        .ok_or_else(|| "invoice is required".to_string())?;

    let amount_argument = match words.next() {
        Some(amount) => match amount.parse::<u64>() {
            Ok(parsed) => Ok(parsed),
            Err(_) => return Err("Error: millisat amount must be an integer".to_string()),
        },
        None => Err(
            "Open amount invoices require an amount in millisats as an additional argument"
                .to_string(),
        ),
    }?;

    match node.pay_open_invoice(invoice.to_string(), amount_argument, String::new()) {
        Ok(_) => {}
        Err(e) => return Err(e.to_string()),
    };

    Ok(())
}

fn list_payments(node: &LightningNode) -> Result<(), String> {
    let payments = match node.get_latest_payments(100) {
        Ok(p) => p,
        Err(e) => return Err(e.to_string()),
    };

    println!("Total of {} payments\n", payments.len());

    for payment in payments {
        let created_at: DateTime<Utc> = payment.created_at.time.into();
        let latest_state_change_at: DateTime<Utc> = payment.latest_state_change_at.time.into();
        println!(
            "{:?} payment created at {} and with latest state change at {}",
            payment.payment_type,
            created_at.format("%d/%m/%Y %T"),
            latest_state_change_at.format("%d/%m/%Y %T")
        );
        println!("      State:              {:?}", payment.payment_state);
        println!("      Amount msat:        {}", payment.amount_msat);
        println!("      Network fees msat:  {:?}", payment.network_fees_msat);
        println!("      LSP fees:           {:?}", payment.lsp_fees_msat);
        println!("      Hash:               {}", payment.hash);
        println!("      Preimage:           {:?}", payment.preimage);
        println!("      Description:        {}", payment.description);
        println!(
            "      Invoice:            {}",
            payment.invoice_details.invoice
        );
        println!();
    }

    Ok(())
}
