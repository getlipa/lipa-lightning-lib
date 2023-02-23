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
    println!("LDK startup successful. To view available commands: \"help\".");
    println!("Detailed logs are available at {}", log_file_path);
    println!("To stop the LDK node, please type \"stop\" for a graceful shutdown.");
    println!(
        "Local Node ID is: {}",
        PublicKey::from_slice(&node.get_node_info().node_pubkey).unwrap()
    );

    let config = Builder::new().auto_add_history(true).build();
    let mut rl = Editor::<(), DefaultHistory>::with_config(config).unwrap();
    let history_path = Path::new("./.ldk/cli_history");
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
                "exchangerate" => {
                    if let Err(message) = get_exchange_rate(node, &mut words) {
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
    println!("  exchangerate <currency code>");
    println!("");
    println!("  invoice <amount in millisats> [description]");
    println!("  decodeinvoice <invoice>");
    println!("  payinvoice <invoice>");
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
        "    Local balance in msat: {}",
        node_info.channels_info.local_balance_msat
    );
    println!(
        " Inbound capacity in msat: {}",
        node_info.channels_info.inbound_capacity_msat
    );
    println!(
        "Outbound capacity in msat: {}",
        node_info.channels_info.outbound_capacity_msat
    );
}

fn get_exchange_rate<'a>(
    node: &LightningNode,
    words: &mut dyn Iterator<Item = &'a str>,
) -> Result<(), String> {
    let code = words
        .next()
        .ok_or_else(|| "Error: currency code is required".to_string())?;
    let rate = node
        .get_exchange_rate(code.to_string())
        .map_err(|e| e.to_string())?;
    println!("{}", rate);
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
    let invoice = node
        .create_invoice(amount, description, String::new())
        .map_err(|e| e.to_string())?;
    println!("{}", invoice);
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
        invoice_details.invoice_timestamp
    );
    println!(
        "  Expiry interval     {:?}",
        invoice_details.expiry_interval
    );

    Ok(())
}

fn pay_invoice<'a>(
    node: &LightningNode,
    words: &mut dyn Iterator<Item = &'a str>,
) -> Result<(), String> {
    let invoice = words
        .next()
        .ok_or_else(|| "invoice is required".to_string())?;

    match node.pay_invoice(invoice.to_string(), String::new()) {
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
        let created_at: DateTime<Utc> = payment.created_at.timestamp.into();
        let latest_state_change_at: DateTime<Utc> = payment.latest_state_change_at.timestamp.into();
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
        println!("      Invoice:            {}", payment.invoice);
        println!();
    }

    Ok(())
}
