use crate::hinter::{CommandHint, CommandHinter};

use uniffi_lipalightninglib::{Amount, MaxFeeStrategy, TzConfig};

use bitcoin::secp256k1::PublicKey;
use chrono::offset::FixedOffset;
use chrono::{DateTime, Utc};
use colored::Colorize;
use rustyline::config::{Builder, CompletionType};
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::Editor;
use std::collections::HashSet;
use std::path::Path;
use uniffi_lipalightninglib::LiquidityLimit;

use crate::LightningNode;

pub(crate) fn poll_for_user_input(node: &LightningNode, log_file_path: &str) {
    println!("{}", "3L Example Node".blue().bold());
    println!("Detailed logs are available at {}", log_file_path);
    println!("To stop the node, please type \"stop\" for a graceful shutdown.");
    println!(
        "Local Node ID is: {}",
        PublicKey::from_slice(&node.get_node_info().node_pubkey).unwrap()
    );

    let prompt = "3L ϟ ".bold().blue().to_string();
    let history_path = Path::new(".3l_cli_history");
    let mut rl = setup_editor(&history_path);
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
                "calculatelspfee" => {
                    if let Err(message) = calculate_lsp_fee(node, &mut words) {
                        println!("{}", message.red());
                    }
                }
                "paymentamountlimits" => {
                    payment_amount_limits(node);
                }
                "exchangerates" => {
                    if let Err(message) = get_exchange_rate(node) {
                        println!("{}", message.red());
                    }
                }
                "listcurrencies" => {
                    list_currency_codes(node);
                }
                "changecurrency" => {
                    match words
                        .next()
                        .ok_or_else(|| "Error: fiat currency code is required".to_string())
                    {
                        Ok(c) => {
                            change_currency(node, c);
                        }
                        Err(e) => {
                            println!("{}", e.red());
                        }
                    };
                }
                "changetimezone" => {
                    if let Err(message) = change_timezone(node, &mut words) {
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
                "getmaxfeestrategy" => {
                    if let Err(message) = get_max_fee_strategy(node, &mut words) {
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

fn setup_editor(history_path: &Path) -> Editor<CommandHinter, DefaultHistory> {
    let config = Builder::new()
        .auto_add_history(true)
        .completion_type(CompletionType::List)
        .build();

    let mut hints = HashSet::new();
    hints.insert(CommandHint::new("nodeinfo", "nodeinfo"));
    hints.insert(CommandHint::new("lspfee", "lspfee"));
    hints.insert(CommandHint::new(
        "calculatelspfee <amount in SAT>",
        "calculatelspfee ",
    ));
    hints.insert(CommandHint::new("exchangerates", "exchangerates"));
    hints.insert(CommandHint::new("listcurrencies", "listcurrencies"));
    hints.insert(CommandHint::new(
        "changecurrency <currency code>",
        "changecurrency ",
    ));
    hints.insert(CommandHint::new(
        "changetimezone [timezone offset in mins] [timezone id]",
        "changetimezone ",
    ));

    hints.insert(CommandHint::new(
        "invoice <amount in SAT> [description]",
        "invoice ",
    ));
    hints.insert(CommandHint::new(
        "decodeinvoice <invoice>",
        "decodeinvoice ",
    ));
    hints.insert(CommandHint::new(
        "getmaxfeestrategy <payment amount in SAT>",
        "getmaxfeestrategy ",
    ));
    hints.insert(CommandHint::new("payinvoice <invoice>", "payinvoice "));
    hints.insert(CommandHint::new(
        "payopeninvoice <invoice> <amount in SAT>",
        "payopeninvoice ",
    ));

    hints.insert(CommandHint::new("listpayments", "listpayments"));
    hints.insert(CommandHint::new("foreground", "foreground"));
    hints.insert(CommandHint::new("background", "background"));
    hints.insert(CommandHint::new("stop", "stop"));
    hints.insert(CommandHint::new("help", "help"));
    let hinter = CommandHinter { hints };

    let mut rl = Editor::<CommandHinter, DefaultHistory>::with_config(config).unwrap();
    rl.set_helper(Some(hinter));
    let _ = rl.load_history(history_path);
    rl
}

fn help() {
    println!("  nodeinfo");
    println!("  lspfee");
    println!("  calculatelspfee <amount in SAT>");
    println!("  paymentamountlimits");
    println!("  exchangerates");
    println!("  listcurrencies");
    println!("  changecurrency <currency code>");
    println!("  changetimezone [timezone offset in mins] [timezone id]");
    println!();
    println!("  invoice <amount in SAT> [description]");
    println!("  decodeinvoice <invoice>");
    println!("  getmaxfeestrategy <payment amount in SAT>");
    println!("  payinvoice <invoice>");
    println!("  payopeninvoice <invoice> <amount in SAT>");
    println!();
    println!("  listpayments");
    println!();
    println!("  foreground");
    println!("  background");
    println!();
    println!("  stop");
}

fn lsp_fee(node: &LightningNode) {
    let lsp_fee = node.query_lsp_fee().unwrap();
    println!(
        " Min fee: {}",
        amount_to_string(lsp_fee.channel_minimum_fee)
    );
    println!(
        "Fee rate: {}%",
        lsp_fee.channel_fee_permyriad as f64 / 100f64
    );
}

fn calculate_lsp_fee(
    node: &LightningNode,
    words: &mut dyn Iterator<Item = &str>,
) -> Result<(), String> {
    let amount = words
        .next()
        .ok_or_else(|| "Error: amount in SAT is required".to_string())?;
    let amount: u64 = amount
        .parse()
        .map_err(|_| "Error: amount should be an integer number".to_string())?;
    let fee = node.calculate_lsp_fee(amount).unwrap();
    println!(" LSP fee: {} SAT", amount_to_string(fee));
    Ok(())
}

fn payment_amount_limits(node: &LightningNode) {
    let limits = node.get_payment_amount_limits().unwrap();

    println!(
        " Beta maximum receive: {}",
        amount_to_string(limits.max_receive)
    );

    match limits.liquidity_limit {
        LiquidityLimit::MinReceive { amount } => {
            println!(
                " Minimum payment amount: {}. A setup fee will be charged.",
                amount_to_string(amount)
            );
        }
        LiquidityLimit::MaxFreeReceive { amount } => {
            println!(
                " If you want to receive more than {}, a setup fee will be charged.",
                amount_to_string(amount)
            );
        }
        LiquidityLimit::None => {}
    }
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
        "            Local balance: {}",
        amount_to_string(node_info.channels_info.local_balance)
    );
    println!(
        "         Inbound capacity: {}",
        amount_to_string(node_info.channels_info.inbound_capacity)
    );
    println!(
        "        Outbound capacity: {}",
        amount_to_string(node_info.channels_info.outbound_capacity)
    );
    println!(
        " Capacity of all channels: {}",
        amount_to_string(node_info.channels_info.total_channel_capacities)
    );
}

fn get_exchange_rate(node: &LightningNode) -> Result<(), String> {
    match node.get_exchange_rate() {
        Some(r) => {
            let dt: DateTime<Utc> = r.updated_at.into();
            println!(
                "{}: {} SAT - updated at {} UTC",
                r.currency_code,
                r.rate,
                dt.format("%d/%m/%Y %T")
            );
        }
        None => {
            println!("Exchange rate not available");
        }
    }
    Ok(())
}

fn list_currency_codes(node: &LightningNode) {
    let codes = node.list_currency_codes();
    println!("Supported currencies: {codes:?}");
}

fn change_currency(node: &LightningNode, fiat_currency: &str) {
    node.change_fiat_currency(String::from(fiat_currency));
}

fn change_timezone(
    node: &LightningNode,
    words: &mut dyn Iterator<Item = &str>,
) -> Result<(), String> {
    let timezone_utc_offset_mins: i32 = words
        .next()
        .unwrap_or("0")
        .parse()
        .map_err(|_| "Error: offset should be an integer number".to_string())?;
    let timezone_utc_offset_secs = timezone_utc_offset_mins * 60;
    let timezone_id = words.collect::<Vec<_>>().join(" ");

    let tz_config = TzConfig {
        timezone_id,
        timezone_utc_offset_secs,
    };
    println!(
        " Timezone offset secs: {}",
        tz_config.timezone_utc_offset_secs
    );
    println!(" Timezone id:          {}", tz_config.timezone_id);
    node.change_timezone_config(tz_config);
    Ok(())
}

fn create_invoice(
    node: &LightningNode,
    words: &mut dyn Iterator<Item = &str>,
) -> Result<(), String> {
    let amount = words
        .next()
        .ok_or_else(|| "Error: amount in SAT is required".to_string())?;
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

fn decode_invoice(
    node: &LightningNode,
    words: &mut dyn Iterator<Item = &str>,
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
        "  Amount              {:?}",
        invoice_details.amount.map(amount_to_string)
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

fn get_max_fee_strategy(
    node: &LightningNode,
    words: &mut dyn Iterator<Item = &str>,
) -> Result<(), String> {
    let amount_argument = match words.next() {
        Some(amount) => match amount.parse::<u64>() {
            Ok(parsed) => Ok(parsed),
            Err(_) => return Err("Error: SAT amount must be an integer".to_string()),
        },
        None => Err("The payment amount in SAT is required".to_string()),
    }?;

    let max_fee_strategy = node.get_payment_max_fee_strategy(amount_argument);

    match max_fee_strategy {
        MaxFeeStrategy::Relative { max_fee_permyriad } => {
            println!(
                "Max fee strategy: Relative (<= {} %)",
                max_fee_permyriad as f64 / 100.0
            );
        }
        MaxFeeStrategy::Absolute { max_fee_amount } => {
            println!(
                "Max fee strategy: Absolute (<= {})",
                amount_to_string(max_fee_amount)
            );
        }
    }

    Ok(())
}

fn pay_invoice(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<(), String> {
    let invoice = words
        .next()
        .ok_or_else(|| "invoice is required".to_string())?;

    if words.next().is_some() {
        return Err("To many arguments. Specifying an amount is only allowed for open invoices. To pay an open invoice use 'payopeninvoice'.".to_string());
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
            Err(_) => return Err("Error: SAT amount must be an integer".to_string()),
        },
        None => Err(
            "Open amount invoices require an amount in SAT as an additional argument".to_string(),
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

    println!("Total of {} payments\n", payments.len().to_string().bold());
    for payment in payments {
        let payment_type = format!("{:?}", payment.payment_type);
        let created_at: DateTime<Utc> = payment.created_at.time.into();
        let timezone = FixedOffset::east_opt(payment.created_at.timezone_utc_offset_secs).unwrap();
        let created_at = created_at.with_timezone(&timezone);

        let latest_change_at: DateTime<Utc> = payment.latest_state_change_at.time.into();
        let timezone =
            FixedOffset::east_opt(payment.latest_state_change_at.timezone_utc_offset_secs).unwrap();
        let latest_change_at = latest_change_at.with_timezone(&timezone);
        println!(
            "{} payment created at {created_at} {}",
            payment_type.bold(),
            payment.created_at.timezone_id
        );
        println!(
            "and with latest state change at {latest_change_at} {}",
            payment.latest_state_change_at.timezone_id
        );
        println!("      State:        {:?}", payment.payment_state);
        println!("      Amount:       {}", amount_to_string(payment.amount));
        println!(
            "      Network fees: {:?}",
            payment.network_fees.map(amount_to_string)
        );
        println!(
            "      LSP fees:     {:?}",
            payment.lsp_fees.map(amount_to_string)
        );
        println!("      Hash:         {}", payment.hash);
        println!("      Preimage:     {:?}", payment.preimage);
        println!("      Description:  {}", payment.description);
        println!("      Invoice:      {}", payment.invoice_details.invoice);
        println!();
    }

    Ok(())
}

fn amount_to_string(amount: Amount) -> String {
    let fiat = match amount.fiat {
        Some(fiat) => {
            let converted_at: DateTime<Utc> = fiat.converted_at.into();
            format!(
                "{:.2} {} as of {}",
                fiat.minor_units as f64 / 100f64,
                fiat.currency_code,
                converted_at.format("%d/%m/%Y %T UTC"),
            )
        }
        None => "exchange rate uknown".to_string(),
    };
    format!("{} SAT ({fiat})", amount.sats)
}
