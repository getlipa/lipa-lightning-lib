use crate::hinter::{CommandHint, CommandHinter};
use crate::overview::overview;

use anyhow::{anyhow, bail, ensure, Context, Result};
use breez_sdk_core::BitcoinAddressData;
use chrono::offset::FixedOffset;
use chrono::{DateTime, Local, Utc};
use colored::Colorize;
use parrot::PaymentSource;
use qrcode::render::unicode;
use qrcode::QrCode;
use rustyline::config::{Builder, CompletionType};
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::Editor;
use std::cmp::min;
use std::collections::HashSet;
use std::path::Path;
use std::time::SystemTime;
use uniffi_lipalightninglib::{
    ActionRequiredItem, Activity, Amount, ChannelCloseInfo, ChannelCloseState, DecodedData,
    ExchangeRate, FailedSwapInfo, FeatureFlag, FiatValue, IncomingPaymentInfo,
    InvoiceCreationMetadata, InvoiceDetails, LightningNode, LiquidityLimit, LnUrlPayDetails,
    LnUrlWithdrawDetails, MaxRoutingFeeMode, OfferInfo, OfferKind, OutgoingPaymentInfo,
    PaymentInfo, PaymentMetadata, RangeHit, Recipient, TzConfig,
};

pub(crate) fn poll_for_user_input(node: &LightningNode, log_file_path: &str) {
    println!("{}", "3L Example Node".blue().bold());
    println!("Detailed logs are available at {}", log_file_path);
    println!("To stop the node, please type \"stop\" for a graceful shutdown.");
    println!(
        "Local Node ID is: {}",
        &node.util().get_node_info().unwrap().node_pubkey
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
                "h" | "help" => help(),
                "n" | "nodeinfo" => {
                    node_info(node);
                }
                "walletpubkeyid" => {
                    if let Err(message) = wallet_pubkey_id(node) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "lspfee" => {
                    lsp_fee(node);
                }
                "calculatelspfee" => {
                    if let Err(message) = calculate_lsp_fee(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "calculateswaplspfee" => {
                    if let Err(message) = calculate_swap_lsp_fee(node, &mut words) {
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
                    if let Err(message) = change_currency(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "changetimezone" => {
                    if let Err(message) = change_timezone(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "i" | "invoice" => {
                    if let Err(message) = create_invoice(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "d" | "decodedata" => {
                    if let Err(message) = decode_data(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "parsephonenumber" => {
                    let number = words.collect::<Vec<_>>().join(" ");
                    match node.phone_number().parse_to_lightning_address(number) {
                        Ok(address) => println!("{address}"),
                        Err(message) => println!("{}", format!("{message:#}").red()),
                    }
                }
                "getmaxroutingfeemode" => {
                    if let Err(message) = get_max_routing_fee_mode(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "getinvoiceaffordability" => {
                    if let Err(message) = get_invoice_affordability(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "p" | "payinvoice" => {
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
                "getfailedswapresolvingfees" => {
                    if let Err(message) = get_failed_swap_resolving_fees(node, &mut words) {
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
                "withdrawlnurlw" => {
                    if let Err(message) = withdraw_lnurlw(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "registertopup" => {
                    if let Err(message) = register_topup(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "resettopup" => {
                    if let Err(message) = node.fiat_topup().reset() {
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
                "collectlastoffer" => {
                    if let Err(message) = collect_last_offer(node) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "calculatelightningpayoutfee" => {
                    if let Err(message) = calculate_lightning_payout_fee(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "listactionableitems" => {
                    if let Err(message) = list_actionable_items(node) {
                        println!("{}", format!("{message:#}").red())
                    }
                }
                "hidechannelcloseitem" => {
                    if let Err(message) = node
                        .actions_required()
                        .hide_unrecoverable_channel_close_funds_item()
                    {
                        println!("{}", format!("{message:#}").red())
                    }
                }
                "hidefailedswapitem" => {
                    if let Err(message) = hide_failed_swap(node, &mut words) {
                        println!("{}", format!("{message:#}").red())
                    }
                }
                "l" | "listactivities" => {
                    if let Err(message) = list_activities(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "getactivity" => {
                    if let Err(message) = get_activity(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "o" | "overview" => {
                    if let Err(message) = overview(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "listrecipients" => match node.list_recipients() {
                    Ok(list) => {
                        let list = list
                            .into_iter()
                            .map(|r| match r {
                                Recipient::LightningAddress { address } => address,
                                Recipient::PhoneNumber { e164 } => e164,
                                r => panic!("{r:?}"),
                            })
                            .collect::<Vec<_>>();
                        println!("{}", list.join("\n"));
                    }
                    Err(message) => eprintln!("{}", format!("{message:#}").red()),
                },
                "paymentuuid" => match payment_uuid(node, &mut words) {
                    Ok(uuid) => println!("{uuid}"),
                    Err(message) => eprintln!("{}", format!("{message:#}").red()),
                },
                "personalnote" => {
                    if let Err(message) = set_personal_note(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "swaponchaintolightning" => {
                    if let Err(message) = swap_onchain_to_lightning(node) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "getchannelcloseresolvingfees" => {
                    if let Err(message) = get_channel_close_resolving_fees(node) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "sweep" => {
                    if let Err(message) = sweep(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "clearwalletinfo" => {
                    if let Err(message) = clear_wallet_info(node) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "clearwallet" => {
                    if let Err(message) = clear_wallet(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "registerlightningaddress" => match node.lightning_address().register() {
                    Ok(address) => println!("{address}"),
                    Err(message) => println!("{}", format!("{message:#}").red()),
                },
                "querylightningaddress" => match node.lightning_address().get() {
                    Ok(address) => println!("{address:?}"),
                    Err(message) => println!("{}", format!("{message:#}").red()),
                },
                "registerphonenumber" => {
                    let phone_number = words.collect::<Vec<_>>().join(" ");
                    match node.phone_number().register(phone_number) {
                        Ok(_) => {}
                        Err(message) => println!("{}", format!("{message:#}").red()),
                    }
                }
                "verifyphonenumber" => {
                    let otp = words.next().ok_or_else(|| "OTP is required".to_string());
                    let otp = match otp {
                        Ok(a) => a.to_string(),
                        Err(e) => {
                            println!("{}", e.red());
                            return;
                        }
                    };
                    let phone_number = words.collect::<Vec<_>>().join(" ");
                    match node.phone_number().verify(phone_number, otp) {
                        Ok(_) => {}
                        Err(message) => println!("{}", format!("{message:#}").red()),
                    }
                }
                "queryverifiedphonenumber" => match node.phone_number().get() {
                    Ok(n) => println!("{n:?}"),
                    Err(message) => println!("{}", format!("{message:#}").red()),
                },
                "setfeatureflag" => {
                    if let Err(message) = set_feature_flag(node, &mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "logdebug" => {
                    if let Err(message) = node.util().log_debug_info() {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "health" => match node.util().query_health_status() {
                    Ok(status) => println!("{status:?}"),
                    Err(message) => println!("{}", format!("{message:#}").red()),
                },
                "foreground" => {
                    node.config().foreground();
                }
                "background" => {
                    node.config().background();
                }
                "closechannels" => {
                    if let Err(message) = node.close_all_channels_with_current_lsp() {
                        println!("{}", format!("{message:#}").red());
                    }
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
    hints.insert(CommandHint::new("n", "n"));
    hints.insert(CommandHint::new("nodeinfo", "nodeinfo"));
    hints.insert(CommandHint::new("walletpubkeyid", "walletpubkeyid"));
    hints.insert(CommandHint::new("lspfee", "lspfee"));
    hints.insert(CommandHint::new(
        "calculatelspfee <amount in SAT>",
        "calculatelspfee ",
    ));
    hints.insert(CommandHint::new(
        "calculateswaplspfee <amount in SAT>",
        "calculateswaplspfee ",
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

    hints.insert(CommandHint::new("i <amount in SAT> [description]", "i "));
    hints.insert(CommandHint::new(
        "invoice <amount in SAT> [description]",
        "invoice ",
    ));
    hints.insert(CommandHint::new("d <data>", "d "));
    hints.insert(CommandHint::new("decodedata <data>", "decodedata "));
    hints.insert(CommandHint::new(
        "parsephonenumber <phone number>",
        "parsephonenumber ",
    ));
    hints.insert(CommandHint::new(
        "getmaxroutingfeemode <payment amount in SAT>",
        "getmaxroutingfeemode ",
    ));
    hints.insert(CommandHint::new(
        "getinvoiceaffordability <amount in SAT>",
        "getinvoiceaffordability ",
    ));
    hints.insert(CommandHint::new("p <invoice>", "p "));
    hints.insert(CommandHint::new("payinvoice <invoice>", "payinvoice "));
    hints.insert(CommandHint::new(
        "payopeninvoice <invoice> <amount in SAT>",
        "payopeninvoice ",
    ));
    hints.insert(CommandHint::new(
        "paylnurlp <lnurlp> <amount in SAT> [comment]",
        "paylnurlp ",
    ));
    hints.insert(CommandHint::new(
        "withdrawlnurlw <lnurlw> <amount in SAT>",
        "withdrawlnurlw ",
    ));

    hints.insert(CommandHint::new("getswapaddress", "getswapaddress"));
    hints.insert(CommandHint::new("listfailedswaps", "listfailedswaps"));
    hints.insert(CommandHint::new(
        "getfailedswapresolvingfees <swap address>",
        "getfailedswapresolvingfees ",
    ));
    hints.insert(CommandHint::new(
        "refundfailedswap <swap address> <to address>",
        "refundfailedswap ",
    ));

    hints.insert(CommandHint::new(
        "registertopup <IBAN> <currency> <provider> [email] [referral code]",
        "registertopup ",
    ));
    hints.insert(CommandHint::new("resettopup", "resettopup"));
    hints.insert(CommandHint::new("getregisteredtopup", "getregisteredtopup"));
    hints.insert(CommandHint::new("listoffers", "listoffers"));
    hints.insert(CommandHint::new("collectlastoffer", "collectlastoffer"));
    hints.insert(CommandHint::new(
        "calculatelightningpayoutfee <offer id>",
        "calculatelightningpayoutfee ",
    ));

    hints.insert(CommandHint::new(
        "listactionableitems",
        "listactionableitems",
    ));
    hints.insert(CommandHint::new(
        "hidechannelcloseitem",
        "hidechannelcloseitem",
    ));
    hints.insert(CommandHint::new(
        "hidefailedswapitem <swap address>",
        "hidefailedswapitem ",
    ));

    hints.insert(CommandHint::new(
        "o [number of activities = 10] [fun mode = false]",
        "o ",
    ));
    hints.insert(CommandHint::new(
        "overview [number of activities = 10] [fun mode = false]",
        "overview ",
    ));
    hints.insert(CommandHint::new("l [number of activities = 2]", "l "));
    hints.insert(CommandHint::new(
        "listactivities [number of activities = 2]",
        "listactivities ",
    ));
    hints.insert(CommandHint::new("getactivity <hash>", "getactivity "));
    hints.insert(CommandHint::new("listrecipients", "listrecipients"));
    hints.insert(CommandHint::new(
        "registerlightningaddress",
        "registerlightningaddress",
    ));
    hints.insert(CommandHint::new(
        "querylightningaddress",
        "querylightningaddress",
    ));
    hints.insert(CommandHint::new(
        "registerphonenumber <phone number>",
        "registerphonenumber ",
    ));
    hints.insert(CommandHint::new(
        "verifyphonenumber <otp> <phone number>",
        "verifyphonenumber ",
    ));
    hints.insert(CommandHint::new(
        "queryverifiedphonenumber",
        "queryverifiedphonenumber",
    ));
    hints.insert(CommandHint::new(
        "setfeatureflag <feature> <enabled>",
        "setfeatureflag ",
    ));
    hints.insert(CommandHint::new(
        "paymentuuid <payment hash>",
        "paymentuuid ",
    ));
    hints.insert(CommandHint::new(
        "personalnote <payment hash> [note]",
        "personalnote ",
    ));
    hints.insert(CommandHint::new("sweep <address>", "sweep "));
    hints.insert(CommandHint::new("clearwalletinfo", "clearwalletinfo"));
    hints.insert(CommandHint::new("clearwallet <address>", "clearwallet "));
    hints.insert(CommandHint::new(
        "getchannelcloseresolvingfees",
        "getchannelcloseresolvingfees",
    ));
    hints.insert(CommandHint::new(
        "swaponchaintolightning",
        "swaponchaintolightning",
    ));
    hints.insert(CommandHint::new("logdebug", "logdebug"));
    hints.insert(CommandHint::new("health", "health"));
    hints.insert(CommandHint::new("foreground", "foreground"));
    hints.insert(CommandHint::new("background", "background"));
    hints.insert(CommandHint::new("closechannels", "closechannels"));
    hints.insert(CommandHint::new("stop", "stop"));
    hints.insert(CommandHint::new("help", "help"));
    hints.insert(CommandHint::new("h", "h"));
    let hinter = CommandHinter { hints };

    let mut rl = Editor::<CommandHinter, DefaultHistory>::with_config(config).unwrap();
    rl.set_helper(Some(hinter));
    let _ = rl.load_history(history_path);
    rl
}

fn help() {
    println!("  n | nodeinfo");
    println!("  walletpubkeyid");
    println!("  lspfee");
    println!("  calculatelspfee <amount in SAT>");
    println!("  calculateswaplspfee <amount in SAT>");
    println!("  paymentamountlimits");
    println!("  exchangerates");
    println!("  listcurrencies");
    println!("  changecurrency <currency code>");
    println!("  changetimezone [timezone offset in mins] [timezone id]");
    println!();
    println!("  i | invoice <amount in SAT> [description]");
    println!("  d | decodedata <data>");
    println!("  parsephonenumber <phone number>");
    println!("  getmaxroutingfeemode <payment amount in SAT>");
    println!("  getinvoiceaffordability <amount in SAT>");
    println!("  p | payinvoice <invoice>");
    println!("  payopeninvoice <invoice> <amount in SAT>");
    println!("  paylnurlp <lnurlp> <amount in SAT> [comment]");
    println!("  withdrawlnurlw <lnurlw> <amount in SAT>");
    println!();
    println!("  getswapaddress");
    println!("  listfailedswaps");
    println!("  getfailedswapresolvingfees <swap address>");
    println!("  refundfailedswap <swap address> <to address>");
    println!();
    println!("  registertopup <IBAN> <currency> <provider> [email] [referral code]");
    println!("  resettopup");
    println!("  getregisteredtopup");
    println!("  listoffers");
    println!("  collectlastoffer");
    println!("  calculatelightningpayoutfee <offer id>");
    println!();
    println!("  listactionableitems");
    println!("  hidechannelcloseitem");
    println!("  hidefailedswapitem <swap address>");
    println!();
    println!("  o | overview [number of activities = 10] [fun mode = false]");
    println!("  l | listactivities [number of activities = 2]");
    println!("  getactivity <hash>");
    println!("  listrecipients");
    println!("  registerlightningaddress");
    println!("  querylightningaddress");
    println!("  registerphonenumber <phone number>");
    println!("  verifyphonenumber <otp> <phone number>");
    println!("  queryverifiedphonenumber");
    println!("  paymentuuid <payment hash>");
    println!("  personalnote <payment hash> [note]");
    println!();
    println!("  getchannelcloseresolvingfees");
    println!("  sweep <address>");
    println!("  swaponchaintolightning");
    println!("  clearwalletinfo");
    println!("  clearwallet <address>");
    println!();
    println!("  setfeatureflag <feature> <enabled>");
    println!("  logdebug");
    println!("  health");
    println!();
    println!("  foreground");
    println!("  background");
    println!();
    println!("  closechannels");
    println!();
    println!("  stop");
}

fn lsp_fee(node: &LightningNode) {
    let lsp_fee = node.lightning().get_lsp_fee().unwrap();
    println!(
        " Min fee: {}",
        amount_to_string(&lsp_fee.channel_minimum_fee)
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
    let response = node.lightning().calculate_lsp_fee_for_amount(amount)?;
    println!(" LSP fee: {}", amount_to_string(&response.lsp_fee));
    Ok(())
}

fn calculate_swap_lsp_fee(
    node: &LightningNode,
    words: &mut dyn Iterator<Item = &str>,
) -> Result<()> {
    let amount: u64 = words
        .next()
        .ok_or(anyhow!("Amount in SAT is required"))?
        .parse()
        .context("Amount should be a positive integer number")?;
    let response = node.onchain().swap().calculate_lsp_fee_for_amount(amount)?;
    println!("LSP fee for Swaps: {}", amount_to_string(&response.lsp_fee));
    Ok(())
}

fn payment_amount_limits(node: &LightningNode) {
    let limits = node.lightning().determine_receive_amount_limits().unwrap();

    println!(
        " Beta maximum receive: {}",
        amount_to_string(&limits.max_receive)
    );

    match limits.liquidity_limit {
        LiquidityLimit::MinReceive { amount } => {
            println!(
                " Minimum payment amount: {}. A setup fee will be charged.",
                amount_to_string(&amount)
            );
        }
        LiquidityLimit::MaxFreeReceive { amount } => {
            println!(
                " If you want to receive more than {}, a setup fee will be charged.",
                amount_to_string(&amount)
            );
        }
        LiquidityLimit::None => {}
    }
}

fn node_info(node: &LightningNode) {
    let node_info = match node.util().get_node_info() {
        Ok(n) => n,
        Err(e) => {
            eprintln!("{e}");
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
        "                      On-chain balance: {}",
        amount_to_string(&node_info.onchain_balance)
    );
    println!(
        "                         Local balance: {}",
        amount_to_string(&node_info.channels_info.local_balance)
    );
    println!(
        "    Max receivable in a single payment: {}",
        amount_to_string(&node_info.channels_info.max_receivable_single_payment)
    );
    println!(
        "                Total inbound capacity: {}",
        amount_to_string(&node_info.channels_info.total_inbound_capacity)
    );
    println!(
        "                     Outbound capacity: {}",
        amount_to_string(&node_info.channels_info.outbound_capacity)
    );
}

fn wallet_pubkey_id(node: &LightningNode) -> Result<()> {
    let wallet_pubkey_id = node.util().query_wallet_pubkey_id()?;

    println!("{wallet_pubkey_id}");

    Ok(())
}

fn get_exchange_rate(node: &LightningNode) {
    match node.util().get_exchange_rate() {
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
    let codes = node.config().list_currencies();
    println!("Supported currencies: {codes:?}");
}

fn change_currency(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let fiat_currency = words
        .next()
        .ok_or(anyhow!("Fiat currency code is required"))?;
    node.config()
        .set_fiat_currency(String::from(fiat_currency))?;
    Ok(())
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
    node.config().set_timezone_config(tz_config);
    Ok(())
}

fn create_invoice(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let amount: u64 = words
        .next()
        .ok_or(anyhow!("Amount in SAT is required"))?
        .parse()
        .context("Amount should be a positive integer number")?;
    let description = words.collect::<Vec<_>>().join(" ");
    let invoice_details = node.lightning().bolt11().create(
        amount,
        None,
        description,
        InvoiceCreationMetadata {
            request_currency: "sat".to_string(),
        },
    )?;
    println!("{}", invoice_details.invoice);

    let code = QrCode::new(invoice_details.invoice.to_uppercase())?;
    let code = code.render::<unicode::Dense1x2>().build();
    println!("{code}");

    Ok(())
}

fn decode_data(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let data = words.next().ok_or(anyhow!("Data is required"))?;

    match node.util().decode_data(data.to_string())? {
        DecodedData::Bolt11Invoice { invoice_details } => print_invoice_details(invoice_details),
        DecodedData::LnUrlPay { lnurl_pay_details } => print_lnurl_pay_details(lnurl_pay_details),
        DecodedData::LnUrlWithdraw {
            lnurl_withdraw_details,
        } => print_lnurl_withdraw_details(lnurl_withdraw_details),
        DecodedData::OnchainAddress {
            onchain_address_details,
        } => print_bitcoin_address_data(onchain_address_details),
    }

    Ok(())
}

fn print_invoice_details(invoice_details: InvoiceDetails) {
    println!("Invoice details:");
    println!(
        "  Amount              {:?}",
        invoice_details.amount.map(|a| amount_to_string(&a))
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
    println!("  Domain                {}", lnurl_pay_details.domain);
    println!(
        "  Short Description     {}",
        lnurl_pay_details.short_description
    );
    println!(
        "  Long Description      {:?}",
        lnurl_pay_details.long_description
    );
    println!(
        "  Min Sendable          {}",
        amount_to_string(&lnurl_pay_details.min_sendable)
    );
    println!(
        "  Max Sendable          {}",
        amount_to_string(&lnurl_pay_details.max_sendable)
    );
    println!(
        "  Max Comment Length    {}",
        lnurl_pay_details.max_comment_length
    );
    println!("---- Internal LnUrlPayRequestData struct ----");
    println!(
        "  Callback              {}",
        lnurl_pay_details.request_data.callback
    );
    let len = min(lnurl_pay_details.request_data.metadata_str.len(), 50);
    println!(
        "  Metadata              {}…",
        lnurl_pay_details
            .request_data
            .metadata_str
            .get(0..len)
            .expect("String is shorter than itself")
    );
    println!(
        "  Comment Allowed       {:?}",
        lnurl_pay_details.request_data.comment_allowed
    );
    println!(
        "  Lightning Address     {:?}",
        lnurl_pay_details.request_data.ln_address
    );
}

fn print_lnurl_withdraw_details(lnurl_withdraw_details: LnUrlWithdrawDetails) {
    println!("LNURL-withdraw details:");
    println!(
        "  Callback              {}",
        lnurl_withdraw_details.request_data.callback
    );
    println!(
        "  Max Withdrawable      {}",
        amount_to_string(&lnurl_withdraw_details.max_withdrawable)
    );
    println!(
        "  Min Withdrawable      {}",
        amount_to_string(&lnurl_withdraw_details.min_withdrawable)
    );
    println!(
        "  K1                    {}",
        lnurl_withdraw_details.request_data.k1
    );
    println!(
        "  Default Description   {}",
        lnurl_withdraw_details.request_data.default_description
    );
}

fn print_bitcoin_address_data(bitcoin_address_data: BitcoinAddressData) {
    println!("Bitcoin Address data:");
    println!("  Address               {}", bitcoin_address_data.address);
    println!("  Network               {}", bitcoin_address_data.network);
    println!(
        "  Amount SAT            {:?}",
        bitcoin_address_data.amount_sat
    );
    println!("  Message               {:?}", bitcoin_address_data.message);
    println!("  Label                 {:?}", bitcoin_address_data.label);
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

    let max_fee_strategy = node.lightning().determine_max_routing_fee_mode(amount);

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
                amount_to_string(&max_fee_amount)
            );
        }
    }

    Ok(())
}

fn get_invoice_affordability(
    node: &LightningNode,
    words: &mut dyn Iterator<Item = &str>,
) -> Result<()> {
    let amount_sat: u64 = words
        .next()
        .ok_or(anyhow!("Amount is required"))?
        .parse()
        .context("Couldn't parse amount as u64")?;

    let invoice_affordability = node
        .lightning()
        .determine_payment_affordability(amount_sat)
        .context("Couldn't get invoice affordability")?;

    println!("{invoice_affordability:?}");

    Ok(())
}

fn pay_invoice(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let invoice = words.next().ok_or(anyhow!("Invoice is required"))?;

    ensure!(words.next().is_none(),
        "To many arguments. Specifying an amount is only allowed for open invoices. To pay an open invoice use 'payopeninvoice'"
    );

    let result = node.util().decode_data(invoice.to_string())?;
    if let DecodedData::Bolt11Invoice { invoice_details } = result {
        node.lightning().bolt11().pay(
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

    let result = node.util().decode_data(invoice.to_string())?;
    if let DecodedData::Bolt11Invoice { invoice_details } = result {
        node.lightning().bolt11().pay_open_amount(
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
    let swap_address_info = node.onchain().swap().create()?;

    println!("Swap Address Information:");
    println!("  Address             {}", swap_address_info.address);
    println!(
        "  Minimum deposit     {}",
        amount_to_string(&swap_address_info.min_deposit)
    );
    println!(
        "  Maximum deposit     {}",
        amount_to_string(&swap_address_info.max_deposit)
    );

    Ok(())
}

fn list_failed_swaps(node: &LightningNode) -> Result<()> {
    let failed_swaps = failed_swap_from_actions_required_list(&node.actions_required().list()?);

    println!(
        "Total of {} failed swaps\n",
        failed_swaps.len().to_string().bold()
    );
    for swap in failed_swaps {
        print_failed_swap(&swap);
        println!();
    }

    Ok(())
}

fn print_failed_swap(swap: &FailedSwapInfo) {
    let created_at: DateTime<Local> = swap.created_at.into();
    println!("Failed swap created at {created_at}:");
    println!("      Address:        {}", swap.address);
    println!("      Amount:         {}", amount_to_string(&swap.amount));
}

fn refund_failed_swap(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let swap_address = words.next().ok_or(anyhow!("Swap address is required"))?;
    let to_address = words.next().ok_or(anyhow!("To address is required"))?;
    let address_data = get_bitcoin_address_data(node.util().decode_data(to_address.to_string())?)?;

    let failed_swaps = failed_swap_from_actions_required_list(
        &node
            .actions_required()
            .list()
            .map_err(|e| anyhow!("Failed to fetch currently unresolved failed swaps: {e}"))?,
    );
    let failed_swap = failed_swaps
        .into_iter()
        .find(|s| s.address.eq(swap_address))
        .ok_or(anyhow!(
            "No unresolved failed swap with provided swap address was found"
        ))?;
    let resolve_failed_swap_info = node
        .onchain()
        .swap()
        .prepare_sweep(failed_swap, address_data)
        .map_err(|e| anyhow!("Failed to prepare the resolution of the failed swap: {e}"))?;
    let txid = node
        .onchain()
        .swap()
        .sweep(resolve_failed_swap_info)
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

    let comment = words.collect::<Vec<_>>().join(" ");
    let comment = if comment.is_empty() {
        None
    } else {
        Some(comment)
    };

    let lnurlp_details = match node.util().decode_data(lnurlp.into()) {
        Ok(DecodedData::LnUrlPay { lnurl_pay_details }) => lnurl_pay_details,
        Ok(DecodedData::LnUrlWithdraw { .. }) => {
            bail!("An LNURL-Withdraw was provided instead of an LNURL-Pay")
        }
        Ok(DecodedData::Bolt11Invoice { .. }) => {
            bail!("A BOLT-11 invoice was provided instead of an LNURL-pay")
        }
        Ok(DecodedData::OnchainAddress { .. }) => {
            bail!("An on-chain address was provided instead of an LNURL-pay")
        }
        Err(_) => bail!("Invalid lnurlp"),
    };

    let hash = node
        .lightning()
        .lnurl()
        .pay(lnurlp_details.request_data, amount, comment)?;
    println!("Started to pay lnurlp - payment hash is {hash}");

    Ok(())
}

fn withdraw_lnurlw(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let lnurlw = words.next().ok_or(anyhow!("LNURL withdraw is required"))?;

    let amount: u64 = words
        .next()
        .ok_or(anyhow!("The withdraw amount in SAT is required"))?
        .parse()
        .context("Amount should be a positive integer number")?;

    let lnurlw_details = match node.util().decode_data(lnurlw.into()) {
        Ok(DecodedData::LnUrlWithdraw {
            lnurl_withdraw_details,
        }) => lnurl_withdraw_details,
        Ok(DecodedData::LnUrlPay { .. }) => {
            bail!("An LNURL-Pay was provided instead of an LNURL-Withdraw")
        }
        Ok(DecodedData::Bolt11Invoice { .. }) => {
            bail!("A BOLT-11 invoice was provided instead of an LNURL-Withdraw")
        }
        Ok(DecodedData::OnchainAddress { .. }) => {
            bail!("An on-chain address was provided instead of an LNURL-Withdraw")
        }
        Err(_) => bail!("Invalid lnurlw"),
    };

    let hash = node
        .lightning()
        .lnurl()
        .withdraw(lnurlw_details.request_data, amount)?;
    println!("Started to withdraw lnurlw - payment hash is {hash}");

    Ok(())
}

fn register_topup(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let iban = words.next().ok_or(anyhow!("IBAN is required"))?;

    let currency = words.next().ok_or(anyhow!("Currency is required"))?;

    let provider = words.next().ok_or(anyhow!("Provider is required"))?;

    let email = words.next().map(String::from);
    let referral = words.next().map(String::from);

    let topup_info = node.fiat_topup().register(
        email,
        iban.to_string(),
        currency.to_string(),
        provider.to_string(),
        referral,
    )?;
    println!("{topup_info:?}");

    Ok(())
}

fn list_offers(node: &LightningNode) -> Result<()> {
    let offers = offer_info_from_actions_required_list(&node.actions_required().list()?);

    println!("Total of {} offers\n", offers.len().to_string().bold());
    for offer in offers {
        print_offer(&offer);
        println!();
    }

    Ok(())
}

fn collect_last_offer(node: &LightningNode) -> Result<()> {
    let offer = offer_info_from_actions_required_list(&node.actions_required().list()?)
        .into_iter()
        .max_by_key(|o| o.created_at)
        .ok_or(anyhow!("No uncompleted offers available"))?;
    let payment_hash = node.fiat_topup().request_collection(offer)?;
    println!("Collected offer payment hash: {payment_hash}");

    Ok(())
}

fn print_offer(offer: &OfferInfo) {
    let kind = match offer.offer_kind {
        OfferKind::Pocket { .. } => "Pocket",
    };

    let created_at: DateTime<Local> = offer.created_at.into();
    let expires_at: Option<DateTime<Local>> = offer.expires_at.map(Into::into);

    println!("{kind} offer created at {created_at}:");
    println!("      Expires at:         {expires_at:?}");
    println!(
        "      Amount:             {}",
        amount_to_string(&offer.amount)
    );
    println!("      LNURL-w:            {:?}", offer.lnurlw);
    match &offer.offer_kind {
        OfferKind::Pocket {
            id,
            exchange_rate,
            topup_value_minor_units,
            exchange_fee_minor_units,
            exchange_fee_rate_permyriad,
            error,
            ..
        } => {
            println!("                   ID:    {id}");
            println!(
                "      Value exchanged:    {:.2} {}",
                *topup_value_minor_units as f64 / 100f64,
                exchange_rate.currency_code,
            );
            println!(
                "      Exchange fee rate:  {}%",
                *exchange_fee_rate_permyriad as f64 / 100_f64
            );
            println!(
                "      Exchange fee value: {:.2} {}",
                *exchange_fee_minor_units as f64 / 100f64,
                exchange_rate.currency_code,
            );
            let exchanged_at: DateTime<Utc> = exchange_rate.updated_at.into();
            println!(
                "             Exchanged at:     {}",
                exchanged_at.format("%d/%m/%Y %T UTC"),
            );

            if let Some(e) = error {
                println!("             Failure reason: {e:?}");
            }
        }
    }
    println!("      Status:             {:?}", offer.status);
}

fn list_actionable_items(node: &LightningNode) -> Result<()> {
    let items = node.actions_required().list()?;

    println!(
        "Total of {} actionable items\n",
        items.len().to_string().bold()
    );
    for item in items {
        match item {
            ActionRequiredItem::UncompletedOffer { offer } => {
                print_offer(&offer);
            }
            ActionRequiredItem::UnresolvedFailedSwap { failed_swap } => {
                print_failed_swap(&failed_swap);
            }
            ActionRequiredItem::ChannelClosesFundsAvailable { available_funds } => {
                println!("Funds from channel closes are available to be recovered");
                println!(
                    "      Available funds: {}",
                    amount_to_string(&available_funds)
                );
            }
        }
        println!();
    }

    Ok(())
}

fn list_activities(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let number_of_activities: u32 = words
        .next()
        .unwrap_or("2")
        .parse()
        .context("Number of activities should be a positive integer number")?;
    let activities = node.activities().list(number_of_activities)?;
    let pending_activities = activities.pending_activities;
    let completed_activities = activities.completed_activities;

    let line = format!(
        " Total of {} {} activities ",
        completed_activities.len().to_string().bold(),
        "completed".bold()
    );
    println!("{}", line.reversed());
    for activity in completed_activities.into_iter().rev() {
        print_activity(activity)?;
    }

    println!();
    let line = format!(
        " Total of {} {} activities ",
        pending_activities.len().to_string().bold(),
        "pending".bold()
    );
    println!("{}", line.reversed());
    for activity in pending_activities.into_iter().rev() {
        print_activity(activity)?;
    }

    Ok(())
}

fn get_activity(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let hash = words.next().ok_or(anyhow!("Payment hash is required"))?;

    let activity = node.activities().get(hash.to_string())?;
    print_activity(activity)
}

fn print_activity(activity: Activity) -> Result<()> {
    match activity {
        Activity::IncomingPayment {
            incoming_payment_info,
        } => print_incoming_payment(incoming_payment_info),
        Activity::OutgoingPayment {
            outgoing_payment_info,
        } => print_outgoing_payment(outgoing_payment_info),
        Activity::OfferClaim {
            incoming_payment_info,
            offer_kind,
        } => {
            print_incoming_payment(incoming_payment_info)?;
            println!("      Offer:            {}", offer_to_string(offer_kind));
            Ok(())
        }
        Activity::Swap {
            incoming_payment_info,
            swap_info,
        } => {
            if let Some(incoming_payment_info) = incoming_payment_info {
                print_incoming_payment(incoming_payment_info)?;
            }
            println!("      Swap:            {swap_info:?}");
            Ok(())
        }
        Activity::ReverseSwap {
            outgoing_payment_info,
            reverse_swap_info,
        } => {
            print_outgoing_payment(outgoing_payment_info)?;
            println!("      Reverse Swap:    {reverse_swap_info:?}");
            Ok(())
        }
        Activity::ChannelClose { channel_close_info } => print_channel_close(channel_close_info),
    }
}

fn print_payment(payment: PaymentInfo) -> Result<()> {
    let created_at: DateTime<Utc> = payment.created_at.time.into();
    let timezone = FixedOffset::east_opt(payment.created_at.timezone_utc_offset_secs)
        .ok_or(anyhow!("east_opt failed"))?;
    let created_at = created_at.with_timezone(&timezone);

    println!(
        "payment created at {created_at} {}",
        payment.created_at.timezone_id
    );
    println!("      State:            {:?}", payment.payment_state);
    println!(
        "      Amount:           {}",
        amount_to_string(&payment.amount)
    );
    println!("      Hash:             {}", payment.hash);
    println!("      Preimage:         {:?}", payment.preimage);
    println!("      Description:      {}", payment.description);
    println!(
        "      Invoice:          {}",
        payment.invoice_details.invoice
    );
    println!("      Personal note:    {:?}", payment.personal_note);
    Ok(())
}

fn print_incoming_payment(payment: IncomingPaymentInfo) -> Result<()> {
    println!();
    print!("{} ", "Incoming".bold());
    print_payment(payment.payment_info)?;
    println!(
        "      Requested Amount: {}",
        amount_to_string(&payment.requested_amount)
    );
    println!(
        "      LSP fees:         {}",
        amount_to_string(&payment.lsp_fees),
    );
    println!("      Received on:      {:?}", payment.received_on);
    println!(
        "      LNURL comment:    {:?}",
        payment.received_lnurl_comment
    );
    Ok(())
}

fn print_outgoing_payment(payment: OutgoingPaymentInfo) -> Result<()> {
    println!();
    print!("{} ", "Outgoing".bold());
    print_payment(payment.payment_info)?;
    println!(
        "      Network fees:     {}",
        amount_to_string(&payment.network_fees)
    );
    println!("      Recipient:        {:?}", payment.recipient);
    println!(
        "      Comment sent:     {:?}",
        payment.comment_for_recipient
    );
    Ok(())
}

fn print_channel_close(channel_close: ChannelCloseInfo) -> Result<()> {
    match channel_close.state {
        ChannelCloseState::Pending => println!("\nUnconfirmed channel close"),
        ChannelCloseState::Confirmed => {
            let closed_at = channel_close.closed_at.ok_or(anyhow!(
                "Confirmed channel close doesn't have closed_at time"
            ))?;
            let datetime: DateTime<Utc> = closed_at.time.into();
            let timezone = FixedOffset::east_opt(closed_at.timezone_utc_offset_secs)
                .ok_or(anyhow!("east_opt failed"))?;
            println!("\nChannel closed at {}", datetime.with_timezone(&timezone))
        }
    };
    println!(
        "      Amount:           {}",
        amount_to_string(&channel_close.amount)
    );
    println!("      Closing txid:     {}", channel_close.closing_tx_id);
    Ok(())
}

fn payment_uuid(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<String> {
    let payment_hash = words.next().ok_or(anyhow!("Payment Hash is required"))?;
    Ok(node.util().derive_payment_uuid(payment_hash.to_string())?)
}

fn set_personal_note(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let payment_hash = words.next().ok_or(anyhow!("Payment Hash is required"))?;
    let note = words.collect::<Vec<_>>().join(" ").to_string();
    node.activities()
        .set_personal_note(payment_hash.to_string(), note)?;

    Ok(())
}

fn sweep(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let address = words.next().ok_or(anyhow!("Address is required"))?;
    let address_data = node.util().decode_data(address.to_string())?;
    let address_data = get_bitcoin_address_data(address_data)?;
    let sweep_info = node.onchain().channel_close().prepare_sweep(address_data)?;
    println!("     Payout address: {}", sweep_info.address);
    println!(
        "             Amount: {}",
        amount_to_string(&sweep_info.amount)
    );
    println!(
        "On-chain fee amount: {}",
        amount_to_string(&sweep_info.onchain_fee_amount)
    );
    println!("  On-chain fee rate: {}", sweep_info.onchain_fee_rate);

    let txid = node.onchain().channel_close().sweep(sweep_info)?;
    println!("     Transaction Id: {txid}");
    Ok(())
}

fn clear_wallet_info(node: &LightningNode) -> Result<()> {
    match node
        .onchain()
        .reverse_swap()
        .determine_clear_wallet_feasibility()?
    {
        RangeHit::Below { min } => bail!("Balance is below min: {}", amount_to_string(&min)),
        RangeHit::In => (),
        RangeHit::Above { max } => bail!("Balance is above max: {}", amount_to_string(&max)),
    };

    let clear_wallet_info = node.onchain().reverse_swap().prepare_clear_wallet()?;

    println!("Clear Wallet Information:");
    println!(
        "      Total Amount to be Cleared: {}",
        amount_to_string(&clear_wallet_info.clear_amount)
    );
    println!(
        "      Total Estimated Fees: {}",
        amount_to_string(&clear_wallet_info.total_estimated_fees)
    );
    println!(
        "      Total On-chain Fees: {}",
        amount_to_string(&clear_wallet_info.onchain_fee)
    );
    println!(
        "      Swap Fee: {}",
        amount_to_string(&clear_wallet_info.swap_fee)
    );

    Ok(())
}

fn clear_wallet(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let address = words.next().ok_or(anyhow!("Address is required"))?;

    let clear_wallet_info = node.onchain().reverse_swap().prepare_clear_wallet()?;

    let result = node.util().decode_data(address.to_string())?;
    if let DecodedData::OnchainAddress {
        onchain_address_details,
    } = result
    {
        node.onchain()
            .reverse_swap()
            .clear_wallet(clear_wallet_info, onchain_address_details)?;
    } else {
        bail!("Provided data is not an on-chain address");
    }

    Ok(())
}

fn offer_to_string(offer: OfferKind) -> String {
    match offer {
        OfferKind::Pocket {
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
            lightning_payout_fee,
            ..
        } => {
            let updated_at: DateTime<Utc> = updated_at.into();
            format!(
				"Pocket exchange ({id}) of {:.2} {currency_code} at {} at rate {rate} SATS per {currency_code}, fee was {:.2}% or {:.2}, payout fee charged {} {currency_code}",
				topup_value_minor_units as f64 / 100f64,
				updated_at.format("%d/%m/%Y %T UTC"),
				exchange_fee_rate_permyriad as f64 / 100f64,
				exchange_fee_minor_units as f64 / 100f64,
                lightning_payout_fee
                    .map(|f| amount_to_string(&f))
                    .unwrap_or("N/A".to_string()),
			)
        }
    }
}

fn fiat_value_to_string(value: &FiatValue) -> String {
    let converted_at: DateTime<Utc> = value.converted_at.into();
    format!(
        "{:.2} {} as of {}",
        value.minor_units as f64 / 100f64,
        value.currency_code,
        converted_at.format("%d/%m/%Y %T UTC"),
    )
}

fn amount_to_string(amount: &Amount) -> String {
    let fiat = match &amount.fiat {
        Some(fiat) => fiat_value_to_string(fiat),
        None => "exchange rate unknown".to_string(),
    };
    format!("{} SAT ({fiat})", amount.sats)
}

fn get_registered_topup(node: &LightningNode) -> Result<()> {
    let topup_info = node.fiat_topup().get_info()?;
    println!("{topup_info:?}");

    Ok(())
}

fn calculate_lightning_payout_fee(
    node: &LightningNode,
    words: &mut dyn Iterator<Item = &str>,
) -> Result<()> {
    let offer_id = words.next().ok_or(anyhow!("<offer id> is required"))?;

    let uncompleted_offers = offer_info_from_actions_required_list(
        &node
            .actions_required()
            .list()
            .context("Couldn't fetch uncompleted offers")?,
    );

    let offer = uncompleted_offers
        .into_iter()
        .find(|o| match &o.offer_kind {
            OfferKind::Pocket { id, .. } => id == offer_id,
        })
        .ok_or(anyhow!("Couldn't find offer with id: {offer_id}"))?;

    let lightning_payout_fee = node.fiat_topup().calculate_payout_fee(offer)?;
    println!(
        "Lightning payout fee: {}",
        amount_to_string(&lightning_payout_fee)
    );

    Ok(())
}

fn get_failed_swap_resolving_fees(
    node: &LightningNode,
    words: &mut dyn Iterator<Item = &str>,
) -> Result<()> {
    let swap_address = words.next().ok_or(anyhow!("Swap address is required"))?;

    let failed_swaps = failed_swap_from_actions_required_list(
        &node
            .actions_required()
            .list()
            .map_err(|e| anyhow!("Failed to fetch currently unresolved failed swaps: {e}"))?,
    );

    let failed_swap = failed_swaps
        .into_iter()
        .find(|s| s.address.eq(swap_address))
        .ok_or(anyhow!(
            "No unresolved failed swap with provided swap address was found"
        ))?;

    let resolving_fees = node
        .onchain()
        .swap()
        .determine_resolving_fees(failed_swap)?;

    let resolving_fees = match resolving_fees {
        None => {
            println!("Failed swap funds cannot be resolved because they are too little.");
            return Ok(());
        }
        Some(f) => f,
    };

    println!(
        "Sweep on-chain fees: {}",
        amount_to_string(&resolving_fees.sweep_onchain_fee_estimate)
    );

    match resolving_fees.swap_fees {
        Some(f) => {
            println!("Retry swap fees: {}", amount_to_string(&f.total_fees));
            println!(
                "    Swap fee:              {}",
                amount_to_string(&f.swap_fee)
            );
            println!(
                "    On-chain fee:          {}",
                amount_to_string(&f.onchain_fee)
            );
            println!(
                "    Channel opening fee:   {}",
                amount_to_string(&f.channel_opening_fee)
            );
        }
        None => println!("Retry swap fees: Unavailable"),
    }

    Ok(())
}

fn get_channel_close_resolving_fees(node: &LightningNode) -> Result<()> {
    let resolving_fees = node.onchain().channel_close().determine_resolving_fees()?;

    let resolving_fees = match resolving_fees {
        None => {
            println!("Channel close funds cannot be resolved because they are too little.");
            return Ok(());
        }
        Some(f) => f,
    };

    println!(
        "Sweep on-chain fees: {}",
        amount_to_string(&resolving_fees.sweep_onchain_fee_estimate)
    );

    match resolving_fees.swap_fees {
        Some(f) => {
            println!(
                "Swap-to-lightning fees: {}",
                amount_to_string(&f.total_fees)
            );
            println!(
                "    Swap fee:              {}",
                amount_to_string(&f.swap_fee)
            );
            println!("    On-chain fee: {}", amount_to_string(&f.onchain_fee));
            println!(
                "    Channel opening fee:   {}",
                amount_to_string(&f.channel_opening_fee)
            );
        }
        None => println!("Swap fees: Unavailable"),
    }

    Ok(())
}

fn swap_onchain_to_lightning(node: &LightningNode) -> Result<()> {
    let resolving_fees = node
        .onchain()
        .channel_close()
        .determine_resolving_fees()?
        .ok_or(anyhow!(
            "Channel funds cannot be resolved as they are too little"
        ))?;

    let txid = node
        .onchain()
        .channel_close()
        .swap(resolving_fees.sats_per_vbyte)?;

    println!("Sweeping transaction: {txid}");

    Ok(())
}

fn set_feature_flag(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let feature = words.next().ok_or(anyhow!(
        "<feature> is required; allowed: lightningaddress, la, phonenumber, pn"
    ))?;
    let feature = match feature {
		"lightningaddress" | "la" => FeatureFlag::LightningAddress,
		"phonenumber" | "pn" => FeatureFlag::PhoneNumber,
		feature => bail!("Invalid feature flag name: `{feature}`; allowed: lightningaddress, la, phonenumber, pn"),
	};
    let enabled: bool = words
        .next()
        .ok_or(anyhow!("<enabled> is required"))?
        .parse()?;
    node.config()
        .set_feature_flag(feature, enabled)
        .map_err(Into::into)
}

fn hide_failed_swap(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let swap_address = words.next().ok_or(anyhow!("Swap address is required"))?;

    let failed_swaps = failed_swap_from_actions_required_list(
        &node
            .actions_required()
            .list()
            .map_err(|e| anyhow!("Failed to fetch currently unresolved failed swaps: {e}"))?,
    );
    let failed_swap = failed_swaps
        .into_iter()
        .find(|s| s.address.eq(swap_address))
        .ok_or(anyhow!(
            "No unresolved failed swap with provided swap address was found"
        ))?;

    node.actions_required()
        .hide_unrecoverable_failed_swap_item(failed_swap)
        .map_err(Into::into)
}

fn offer_info_from_actions_required_list(list: &[ActionRequiredItem]) -> Vec<OfferInfo> {
    list.iter()
        .filter_map(|i| match i {
            ActionRequiredItem::UncompletedOffer { offer } => Some(offer.clone()),
            ActionRequiredItem::UnresolvedFailedSwap { .. } => None,
            ActionRequiredItem::ChannelClosesFundsAvailable { .. } => None,
        })
        .collect::<Vec<_>>()
}

fn failed_swap_from_actions_required_list(list: &[ActionRequiredItem]) -> Vec<FailedSwapInfo> {
    list.iter()
        .filter_map(|i| match i {
            ActionRequiredItem::UncompletedOffer { .. } => None,
            ActionRequiredItem::UnresolvedFailedSwap { failed_swap } => Some(failed_swap.clone()),
            ActionRequiredItem::ChannelClosesFundsAvailable { .. } => None,
        })
        .collect::<Vec<_>>()
}

fn get_bitcoin_address_data(decoded_data: DecodedData) -> Result<BitcoinAddressData> {
    if let DecodedData::OnchainAddress {
        onchain_address_details,
    } = decoded_data
    {
        Ok(onchain_address_details)
    } else {
        bail!("Not an onchain address")
    }
}
