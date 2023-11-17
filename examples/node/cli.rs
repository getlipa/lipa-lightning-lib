use crate::hinter::{CommandHint, CommandHinter};

use anyhow::{anyhow, bail, Context, Result};
use chrono::offset::FixedOffset;
use chrono::{DateTime, Local, Utc};
use colored::Colorize;
use parrot::PaymentSource;
use rustyline::config::{Builder, CompletionType};
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::Editor;
use std::collections::HashSet;
use std::path::Path;
use std::time::SystemTime;
use uniffi_lipalightninglib::InvoiceCreationMetadata;
use uniffi_lipalightninglib::PaymentMetadata;
use uniffi_lipalightninglib::{
    Amount, DecodedData, ExchangeRate, FiatValue, InvoiceDetails, LightningNode, LiquidityLimit,
    LnUrlPayDetails, MaxRoutingFeeMode, OfferKind, PaymentState, TzConfig,
};

pub(crate) fn poll_for_user_input(node: &LightningNode, log_file_path: &str) {
    println!("{}", "3L Example Node".blue().bold());
    println!("Detailed logs are available at {}", log_file_path);
    println!("To stop the node, please type \"stop\" for a graceful shutdown.");
    println!(
        "Local Node ID is: {}",
        &node.get_node_info().unwrap().node_pubkey
    );

    let prompt = "3L ϟ ".bold().blue().to_string();
    let history_path = Path::new(".3l_cli_history");
    let mut rl = setup_editor(history_path);
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
                "walletpubkeyid" => {
                    wallet_pubkey_id(node);
                }
                "lspfee" => {
                    lsp_fee(node);
                }
                "calculatelspfee" => {
                    if let Err(message) = calculate_lsp_fee(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "paymentamountlimits" => {
                    payment_amount_limits(node);
                }
                "exchangerates" => {
                    get_exchange_rate(node);
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
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "invoice" => {
                    if let Err(message) = create_invoice(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "decodedata" => {
                    if let Err(message) = decode_data(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "getmaxroutingfeemode" => {
                    if let Err(message) = get_max_routing_fee_mode(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "payinvoice" => {
                    if let Err(message) = pay_invoice(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "payopeninvoice" => {
                    if let Err(message) = pay_open_invoice(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "getswapaddress" => {
                    if let Err(message) = get_swap_address(node) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "listfailedswaps" => {
                    if let Err(message) = list_failed_swaps(node) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "refundfailedswap" => {
                    if let Err(message) = refund_failed_swap(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "paylnurlp" => {
                    if let Err(message) = pay_lnurlp(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "registertopup" => {
                    if let Err(message) = register_topup(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "resettopup" => {
                    if let Err(message) = node.reset_fiat_topup() {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "getregisteredtopup" => {
                    if let Err(message) = get_registered_topup(node) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "listoffers" => {
                    if let Err(message) = list_offers(node) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "listpayments" => {
                    if let Err(message) = list_payments(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "paymentuuid" => match payment_uuid(node, &mut words) {
                    Ok(uuid) => println!("{uuid}"),
                    Err(message) => eprintln!("{}", format!("{message:#}").red()),
                },
                "sweep" => {
                    let address = words
                        .next()
                        .ok_or_else(|| "Address is required".to_string());

                    let address = match address {
                        Ok(a) => a.to_string(),
                        Err(e) => {
                            println!("{}", e.red());
                            return;
                        }
                    };
                    match sweep(node, address.clone()) {
                        Ok(txid) => {
                            println!();
                            println!("Transaction Id: {}", txid);
                            println!("Payout address: {}", address)
                        }
                        Err(message) => println!("{}", format!("{message:#}").red()),
                    }
                }
                "logdebug" => {
                    if let Err(message) = node.log_debug_info() {
                        println!("{}", format!("{message:#}").red());
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
    hints.insert(CommandHint::new("walletpubkeyid", "walletpubkeyid"));
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
    hints.insert(CommandHint::new("decodedata <data>", "decodedata "));
    hints.insert(CommandHint::new(
        "getmaxroutingfeemode <payment amount in SAT>",
        "getmaxroutingfeemode ",
    ));
    hints.insert(CommandHint::new("payinvoice <invoice>", "payinvoice "));
    hints.insert(CommandHint::new(
        "payopeninvoice <invoice> <amount in SAT>",
        "payopeninvoice ",
    ));
    hints.insert(CommandHint::new(
        "paylnurlp <lnurlp> <amount in SAT>",
        "paylnurlp ",
    ));

    hints.insert(CommandHint::new("getswapaddress", "getswapaddress"));
    hints.insert(CommandHint::new("listfailedswaps", "listfailedswaps"));
    hints.insert(CommandHint::new(
        "refundfailedswap <swap address> <to address>",
        "refundfailedswap ",
    ));

    hints.insert(CommandHint::new(
        "registertopup <IBAN> <currency> [email]",
        "registertopup ",
    ));
    hints.insert(CommandHint::new("resettopup", "resettopup"));
    hints.insert(CommandHint::new("getregisteredtopup", "getregisteredtopup"));
    hints.insert(CommandHint::new("listoffers", "listoffers"));

    hints.insert(CommandHint::new(
        "listpayments [number of payments = 2]",
        "listpayments ",
    ));
    hints.insert(CommandHint::new(
        "paymentuuid <payment hash>",
        "paymentuuid",
    ));
    hints.insert(CommandHint::new("sweep <address>", "sweep"));
    hints.insert(CommandHint::new("logdebug", "logdebug"));
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
    println!("  walletpubkeyid");
    println!("  lspfee");
    println!("  calculatelspfee <amount in SAT>");
    println!("  paymentamountlimits");
    println!("  exchangerates");
    println!("  listcurrencies");
    println!("  changecurrency <currency code>");
    println!("  changetimezone [timezone offset in mins] [timezone id]");
    println!();
    println!("  invoice <amount in SAT> [description]");
    println!("  decodedata <data>");
    println!("  getmaxroutingfeemode <payment amount in SAT>");
    println!("  payinvoice <invoice>");
    println!("  payopeninvoice <invoice> <amount in SAT>");
    println!("  paylnurlp <lnurlp> <amount in SAT>");
    println!();
    println!("  getswapaddress");
    println!("  listfailedswaps");
    println!("  refundfailedswap <swap address> <to address>");
    println!();
    println!("  registertopup <IBAN> <currency> [email]");
    println!("  resettopup");
    println!("  getregisteredtopup");
    println!("  listoffers");
    println!();
    println!("  listpayments");
    println!("  paymentuuid <payment hash>");
    println!();
    println!("  sweep <address>");
    println!();
    println!("  logdebug");
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

fn calculate_lsp_fee(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let amount: u64 = words
        .next()
        .ok_or(anyhow!("Amount in SAT is required"))?
        .parse()
        .context("Amount should be a positive integer number")?;
    let response = node.calculate_lsp_fee(amount)?;
    println!(" LSP fee: {} SAT", amount_to_string(response.lsp_fee));
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
    let node_info = match node.get_node_info() {
        Ok(n) => n,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };
    let peers_list = if node_info.peers.is_empty() {
        vec!["None".to_string()]
    } else {
        node_info.peers
    };

    println!("Node PubKey: {}", node_info.node_pubkey);
    println!("Connected peer(s): {}", peers_list.join(", "));
    println!(
        "         On-chain balance: {}",
        amount_to_string(node_info.onchain_balance)
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
}

fn wallet_pubkey_id(node: &LightningNode) {
    match node.get_wallet_pubkey_id() {
        Some(wallet_pubkey_id) => println!("{wallet_pubkey_id}"),
        None => eprintln!("Wallet PubKey Id is currently unavailable."),
    }
}

fn get_exchange_rate(node: &LightningNode) {
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
}

fn list_currency_codes(node: &LightningNode) {
    let codes = node.list_currency_codes();
    println!("Supported currencies: {codes:?}");
}

fn change_currency(node: &LightningNode, fiat_currency: &str) {
    node.change_fiat_currency(String::from(fiat_currency));
}

fn change_timezone(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let timezone_utc_offset_mins: i32 = words
        .next()
        .unwrap_or("0")
        .parse()
        .context("Offset should be an integer number")?;
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

fn create_invoice(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let amount: u64 = words
        .next()
        .ok_or(anyhow!("Amount in SAT is required"))?
        .parse()
        .context("Amount should be a positive integer number")?;
    let description = words.collect::<Vec<_>>().join(" ");
    let invoice_details = node.create_invoice(
        amount,
        None,
        description,
        InvoiceCreationMetadata {
            request_currency: "sat".to_string(),
        },
    )?;
    println!("{}", invoice_details.invoice);
    Ok(())
}

fn decode_data(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let data = words.next().ok_or(anyhow!("Data is required"))?;

    match node.decode_data(data.to_string())? {
        DecodedData::Bolt11Invoice { invoice_details } => print_invoice_details(invoice_details),
        DecodedData::LnUrlPay { lnurl_pay_details } => print_lnurl_pay_details(lnurl_pay_details),
    }

    Ok(())
}

fn print_invoice_details(invoice_details: InvoiceDetails) {
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
}

fn print_lnurl_pay_details(lnurl_pay_details: LnUrlPayDetails) {
    println!("LNURL-pay details:");
    println!(
        "  Callback              {}",
        lnurl_pay_details.request_data.callback
    );
    println!(
        "  Max Sendable          {}",
        amount_to_string(lnurl_pay_details.max_sendable)
    );
    println!(
        "  Min Sendable          {}",
        amount_to_string(lnurl_pay_details.min_sendable)
    );
    println!(
        "  Metadata              {}",
        lnurl_pay_details.request_data.metadata_str
    );
    println!(
        "  Comment Allowed       {:?}",
        lnurl_pay_details.request_data.comment_allowed
    );
    println!(
        "  Domain                {}",
        lnurl_pay_details.request_data.domain
    );
    println!(
        "  LN Address            {:?}",
        lnurl_pay_details.request_data.ln_address
    );
}

fn get_max_routing_fee_mode(
    node: &LightningNode,
    words: &mut dyn Iterator<Item = &str>,
) -> Result<()> {
    let amount: u64 = words
        .next()
        .ok_or(anyhow!("The payment amount in SAT is required"))?
        .parse()
        .context("Amount should be a positive integer number")?;

    let max_fee_strategy = node.get_payment_max_routing_fee_mode(amount);

    match max_fee_strategy {
        MaxRoutingFeeMode::Relative { max_fee_permyriad } => {
            println!(
                "Max fee strategy: Relative (<= {} %)",
                max_fee_permyriad as f64 / 100.0
            );
        }
        MaxRoutingFeeMode::Absolute { max_fee_amount } => {
            println!(
                "Max fee strategy: Absolute (<= {})",
                amount_to_string(max_fee_amount)
            );
        }
    }

    Ok(())
}

fn pay_invoice(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let invoice = words.next().ok_or(anyhow!("Invoice is required"))?;

    if words.next().is_some() {
        bail!("To many arguments. Specifying an amount is only allowed for open invoices. To pay an open invoice use 'payopeninvoice'");
    }

    let result = node.decode_data(invoice.to_string())?;
    if let DecodedData::Bolt11Invoice { invoice_details } = result {
        node.pay_invoice(
            invoice_details,
            PaymentMetadata {
                source: PaymentSource::Clipboard,
                process_started_at: SystemTime::now(),
            },
        )?
    } else {
        bail!("Provided data is not a BOLT-11 invoice");
    }

    Ok(())
}

fn pay_open_invoice(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let invoice = words.next().ok_or(anyhow!("Invoice is required"))?;

    let amount: u64 = words
        .next()
        .ok_or(anyhow!("The payment amount in SAT is required"))?
        .parse()
        .context("Amount should be a positive integer number")?;

    let result = node.decode_data(invoice.to_string())?;
    if let DecodedData::Bolt11Invoice { invoice_details } = result {
        node.pay_open_invoice(
            invoice_details,
            amount,
            PaymentMetadata {
                source: PaymentSource::Clipboard,
                process_started_at: SystemTime::now(),
            },
        )?;
    } else {
        bail!("Provided data is not a BOLT-11 invoice");
    }

    Ok(())
}

fn get_swap_address(node: &LightningNode) -> Result<()> {
    let swap_address_info = node.generate_swap_address(None)?;

    println!("Swap Address Information:");
    println!("  Address             {}", swap_address_info.address);
    println!(
        "  Minimum deposit     {}",
        amount_to_string(swap_address_info.min_deposit)
    );
    println!(
        "  Maximum deposit     {}",
        amount_to_string(swap_address_info.max_deposit)
    );

    Ok(())
}

fn list_failed_swaps(node: &LightningNode) -> Result<()> {
    let failed_swaps = node.get_unresolved_failed_swaps()?;

    println!(
        "Total of {} failed swaps\n",
        failed_swaps.len().to_string().bold()
    );
    for swap in failed_swaps {
        let created_at: DateTime<Local> = swap.created_at.into();
        println!("Failed swap created at {created_at}:");
        println!("      Address:        {}", swap.address);
        println!("      Amount:         {}", amount_to_string(swap.amount));
    }

    Ok(())
}

fn refund_failed_swap(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let swap_address = words.next().ok_or(anyhow!("Swap address is required"))?;
    let to_address = words.next().ok_or(anyhow!("To address is required"))?;

    let fee_rate = node.query_onchain_fee_rate()?;

    let failed_swaps = node
        .get_unresolved_failed_swaps()
        .map_err(|e| anyhow!("Failed to fetch currently unresolved failed swaps: {e}"))?;
    let failed_swap = failed_swaps
        .into_iter()
        .find(|s| s.address.eq(swap_address))
        .ok_or(anyhow!(
            "No unresolved failed swap with provided swap address was found"
        ))?;
    let resolve_failed_swap_info = node
        .prepare_resolve_failed_swap(failed_swap, to_address.into(), fee_rate)
        .map_err(|e| anyhow!("Failed to prepare the resolution of the failed swap: {e}"))?;
    let txid = node
        .resolve_failed_swap(resolve_failed_swap_info)
        .map_err(|e| anyhow!("Failed to resolve failed swap: {e}"))?;
    println!("Successfully broadcasted refund transaction - txid: {txid}");

    Ok(())
}
fn pay_lnurlp(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let lnurlp = words.next().ok_or(anyhow!("LNURL pay is required"))?;

    let amount: u64 = words
        .next()
        .ok_or(anyhow!("The payment amount in SAT is required"))?
        .parse()
        .context("Amount should be a positive integer number")?;

    let lnurlp_details = match node.decode_data(lnurlp.into()) {
        Ok(DecodedData::LnUrlPay { lnurl_pay_details }) => lnurl_pay_details,
        Ok(DecodedData::Bolt11Invoice { .. }) => {
            bail!("A BOLT-11 invoice was provided instead of an LNURL-pay")
        }
        Err(_) => bail!("Invalid lnurlp"),
    };

    let hash = node.pay_lnurlp(lnurlp_details.request_data, amount)?;
    println!("Started to pay lnurlp - payment hash is {hash}");

    Ok(())
}

fn register_topup(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let iban = words.next().ok_or(anyhow!("IBAN is required"))?;

    let currency = words.next().ok_or(anyhow!("Currency is required"))?;

    let email = words.next().map(String::from);

    let topup_info = node.register_fiat_topup(email, iban.to_string(), currency.to_string())?;
    println!("{topup_info:?}");

    Ok(())
}

fn list_offers(node: &LightningNode) -> Result<()> {
    let offers = node.query_uncompleted_offers()?;

    println!("Total of {} offers\n", offers.len().to_string().bold());
    for offer in offers {
        let kind = match offer.offer_kind {
            OfferKind::Pocket { .. } => "Pocket",
        };

        let created_at: DateTime<Local> = offer.created_at.into();
        let expires_at: Option<DateTime<Local>> = offer.expires_at.map(|e| e.into());

        println!("{kind} offer created at {created_at}:");
        println!("      Expires at:         {expires_at:?}");
        println!(
            "      Amount:             {}",
            amount_to_string(offer.amount)
        );
        println!("      LNURL-w:            {:?}", offer.lnurlw);
        match offer.offer_kind {
            OfferKind::Pocket {
                id,
                exchange_rate,
                topup_value_minor_units,
                exchange_fee_minor_units,
                exchange_fee_rate_permyriad,
                error,
            } => {
                println!("                   ID:    {id}");
                println!(
                    "      Value exchanged:    {:.2} {}",
                    topup_value_minor_units as f64 / 100f64,
                    exchange_rate.currency_code,
                );
                println!(
                    "      Exchange fee rate:  {}%",
                    exchange_fee_rate_permyriad as f64 / 100_f64
                );
                println!(
                    "      Exchange fee value: {:.2} {}",
                    exchange_fee_minor_units as f64 / 100f64,
                    exchange_rate.currency_code,
                );
                let exchanged_at: DateTime<Utc> = exchange_rate.updated_at.into();
                println!(
                    "             Exchange at: {}",
                    exchanged_at.format("%d/%m/%Y %T UTC"),
                );

                if let Some(e) = error {
                    println!("             Failure reason: {e:?}");
                }
            }
        }
        println!("      Status:             {:?}", offer.status);
        println!();
    }

    Ok(())
}

fn list_payments(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let number_of_payments: u32 = words
        .next()
        .unwrap_or("2")
        .parse()
        .context("Number of payments should be a positive integer number")?;
    let payments = node.get_latest_payments(number_of_payments)?;

    println!("Total of {} payments", payments.len().to_string().bold());
    for payment in payments.into_iter().rev() {
        println!();
        let payment_type = format!("{:?}", payment.payment_type);
        let created_at: DateTime<Utc> = payment.created_at.time.into();
        let timezone = FixedOffset::east_opt(payment.created_at.timezone_utc_offset_secs)
            .ok_or(anyhow!("east_opt failed"))?;
        let created_at = created_at.with_timezone(&timezone);

        println!(
            "{} payment created at {created_at} {}",
            payment_type.bold(),
            payment.created_at.timezone_id
        );
        println!("      State:        {:?}", payment.payment_state);
        if payment.payment_state == PaymentState::Failed {
            println!("      Fail reason:  {:?}", payment.fail_reason);
        }
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
        println!("      Offer:        {}", offer_to_string(payment.offer));
    }

    Ok(())
}

fn payment_uuid(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<String> {
    let payment_hash = words.next().ok_or(anyhow!("Payment Hash is required"))?;
    Ok(node.get_payment_uuid(payment_hash.to_string())?)
}

fn sweep(node: &LightningNode, address: String) -> Result<String> {
    let fee_rate = node.query_onchain_fee_rate()?;
    Ok(node.sweep(address.to_string(), fee_rate)?)
}

fn offer_to_string(offer: Option<OfferKind>) -> String {
    match offer {
        Some(OfferKind::Pocket {
            id,
            exchange_rate:
                ExchangeRate {
                    currency_code,
                    rate,
                    updated_at,
                },
            topup_value_minor_units,
            exchange_fee_minor_units,
            exchange_fee_rate_permyriad,
            ..
        }) => {
            let updated_at: DateTime<Utc> = updated_at.into();
            format!(
				"Pocket exchange ({id}) of {:.2} {currency_code} at {} at rate {rate} SATS per {currency_code}, fee was {:.2}% or {:.2} {currency_code}",
				topup_value_minor_units as f64 / 100f64,
				updated_at.format("%d/%m/%Y %T UTC"),
				exchange_fee_rate_permyriad as f64 / 100f64,
				exchange_fee_minor_units as f64 / 100f64,
			)
        }
        None => "None".to_string(),
    }
}

fn fiat_value_to_string(value: FiatValue) -> String {
    let converted_at: DateTime<Utc> = value.converted_at.into();
    format!(
        "{:.2} {} as of {}",
        value.minor_units as f64 / 100f64,
        value.currency_code,
        converted_at.format("%d/%m/%Y %T UTC"),
    )
}

fn amount_to_string(amount: Amount) -> String {
    let fiat = match amount.fiat {
        Some(fiat) => fiat_value_to_string(fiat),
        None => "exchange rate unknown".to_string(),
    };
    format!("{} SAT ({fiat})", amount.sats)
}

fn get_registered_topup(node: &LightningNode) -> Result<()> {
    let topup_info = node.retrieve_latest_fiat_topup_info()?;
    println!("{topup_info:?}");

    Ok(())
}
