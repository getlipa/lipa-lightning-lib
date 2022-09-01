use crate::hex_utils;
use bitcoin::secp256k1::PublicKey;
use lightning::util::ser::Writeable;
use std::io;
use std::io::{BufRead, Write};
use std::net::{SocketAddr, ToSocketAddrs};
use uniffi_lipalightninglib::LipaLightning;

pub(crate) fn poll_for_user_input(lipa_lightning: &LipaLightning) {
    println!("LDK startup successful. To view available commands: \"help\".");
    println!("LDK logs are available at .ldk/logs");
    println!("To stop the LDK node, please type \"stop\" for a graceful shutdown.");
    println!(
        "Local Node ID is {}.",
        PublicKey::from_slice(&*lipa_lightning.get_my_node_id()).unwrap()
    );
    let stdin = io::stdin();
    let mut line_reader = stdin.lock().lines();
    loop {
        print!("> ");
        io::stdout().flush().unwrap(); // Without flushing, the `>` doesn't print
        let line = match line_reader.next() {
            Some(l) => l.unwrap(),
            None => break,
        };
        let mut words = line.split_whitespace();
        if let Some(word) = words.next() {
            match word {
                "help" => help(),
                "openchannel" => {
                    let peer_pubkey_and_ip_addr = words.next();
                    let channel_value_sat = words.next();
                    if peer_pubkey_and_ip_addr.is_none() || channel_value_sat.is_none() {
                        println!("ERROR: openchannel has 2 required arguments: `openchannel pubkey@host:port channel_amt_satoshis` [--public]");
                        continue;
                    }
                    let peer_pubkey_and_ip_addr = peer_pubkey_and_ip_addr.unwrap();
                    let (pubkey, peer_addr) =
                        match parse_peer_info(peer_pubkey_and_ip_addr.to_string()) {
                            Ok(info) => info,
                            Err(e) => {
                                println!("{:?}", e.into_inner().unwrap());
                                continue;
                            }
                        };

                    let chan_amt_sat: Result<u64, _> = channel_value_sat.unwrap().parse();
                    if chan_amt_sat.is_err() {
                        println!("ERROR: channel amount must be a number");
                        continue;
                    }

                    match lipa_lightning.connect_open_channel(
                        pubkey.encode(),
                        peer_addr.to_string(),
                        chan_amt_sat.unwrap(),
                    ) {
                        Ok(_) => {
                            println!("SUCCESS: opened channel to {}", pubkey);
                        }
                        Err(e) => {
                            println!("ERROR: couldn't open channel - {:?}", e);
                        }
                    }
                }
                "sendpayment" => {
                    let invoice_str = words.next();
                    if invoice_str.is_none() {
                        println!("ERROR: sendpayment requires an invoice: `sendpayment <invoice>`");
                        continue;
                    }
                    match lipa_lightning.send_payment(invoice_str.unwrap().to_string()) {
                        Ok(_) => {
                            println!("INFO: Initiated a payment");
                        }
                        Err(e) => {
                            println!("ERROR: couldn't pay invoice - {:?}", e);
                        }
                    }
                }
                "keysend" => {
                    let dest_pubkey = match words.next() {
                        Some(dest) => match hex_utils::to_compressed_pubkey(dest) {
                            Some(pk) => pk,
                            None => {
                                println!("ERROR: couldn't parse destination pubkey");
                                continue;
                            }
                        },
                        None => {
                            println!("ERROR: keysend requires a destination pubkey: `keysend <dest_pubkey> <amt_msat>`");
                            continue;
                        }
                    };
                    let amt_msat_str = match words.next() {
                        Some(amt) => amt,
                        None => {
                            println!("ERROR: keysend requires an amount in millisatoshis: `keysend <dest_pubkey> <amt_msat>`");
                            continue;
                        }
                    };
                    let amt_msat: u64 = match amt_msat_str.parse() {
                        Ok(amt) => amt,
                        Err(e) => {
                            println!("ERROR: couldn't parse amount_msat: {}", e);
                            continue;
                        }
                    };
                    match lipa_lightning.send_spontaneous_payment(amt_msat, dest_pubkey.encode()) {
                        Ok(_) => {
                            println!(
                                "INFO: spontaneous \"keysend\" payment of {} msats initiated",
                                amt_msat
                            );
                        }
                        Err(e) => {
                            println!(
                                "ERROR: couldn't send spontaneous \"keysend\" payment - {:?}",
                                e
                            );
                        }
                    };
                }
                "getinvoice" => {
                    let amt_str = words.next();
                    if amt_str.is_none() {
                        println!("ERROR: getinvoice requires an amount in millisatoshis");
                        continue;
                    }

                    let amt_msat: Result<u64, _> = amt_str.unwrap().parse();
                    if amt_msat.is_err() {
                        println!("ERROR: getinvoice provided payment amount was not a number");
                        continue;
                    }

                    let expiry_secs_str = words.next();
                    if expiry_secs_str.is_none() {
                        println!("ERROR: getinvoice requires an expiry in seconds");
                        continue;
                    }

                    let expiry_secs: Result<u32, _> = expiry_secs_str.unwrap().parse();
                    if expiry_secs.is_err() {
                        println!("ERROR: getinvoice provided expiry was not a number");
                        continue;
                    }

                    match lipa_lightning.create_invoice(amt_msat.unwrap(), expiry_secs.unwrap()) {
                        Ok(inv) => {
                            println!("SUCCESS: generated invoice: {}", inv);
                            continue;
                        }
                        Err(_) => {
                            println!("ERROR: create_invoice() method returned an error");
                            continue;
                        }
                    };
                }
                "connectpeer" => {
                    /*let peer_pubkey_and_ip_addr = words.next();
                    if peer_pubkey_and_ip_addr.is_none() {
                        println!("ERROR: connectpeer requires peer connection info: `connectpeer pubkey@host:port`");
                        continue;
                    }
                    let (pubkey, peer_addr) =
                        match parse_peer_info(peer_pubkey_and_ip_addr.unwrap().to_string()) {
                            Ok(info) => info,
                            Err(e) => {
                                println!("{:?}", e.into_inner().unwrap());
                                continue;
                            }
                        };
                    if connect_peer_if_necessary(pubkey, peer_addr, peer_manager.clone())
                        .await
                        .is_ok()
                    {
                        println!("SUCCESS: connected to peer {}", pubkey);
                    }*/
                }
                //"listchannels" => list_channels(&channel_manager, &network_graph),
                /*"listpayments" => {
                    list_payments(inbound_payments.clone(), outbound_payments.clone())
                }*/
                "closechannel" => {
                    /*let channel_id_str = words.next();
                    if channel_id_str.is_none() {
                        println!("ERROR: closechannel requires a channel ID: `closechannel <channel_id> <peer_pubkey>`");
                        continue;
                    }
                    let channel_id_vec = hex_utils::to_vec(channel_id_str.unwrap());
                    if channel_id_vec.is_none() || channel_id_vec.as_ref().unwrap().len() != 32 {
                        println!("ERROR: couldn't parse channel_id");
                        continue;
                    }
                    let mut channel_id = [0; 32];
                    channel_id.copy_from_slice(&channel_id_vec.unwrap());

                    let peer_pubkey_str = words.next();
                    if peer_pubkey_str.is_none() {
                        println!("ERROR: closechannel requires a peer pubkey: `closechannel <channel_id> <peer_pubkey>`");
                        continue;
                    }
                    let peer_pubkey_vec = match hex_utils::to_vec(peer_pubkey_str.unwrap()) {
                        Some(peer_pubkey_vec) => peer_pubkey_vec,
                        None => {
                            println!("ERROR: couldn't parse peer_pubkey");
                            continue;
                        }
                    };
                    let peer_pubkey = match PublicKey::from_slice(&peer_pubkey_vec) {
                        Ok(peer_pubkey) => peer_pubkey,
                        Err(_) => {
                            println!("ERROR: couldn't parse peer_pubkey");
                            continue;
                        }
                    };

                    close_channel(channel_id, peer_pubkey, channel_manager.clone());*/
                }
                "forceclosechannel" => {
                    /*let channel_id_str = words.next();
                    if channel_id_str.is_none() {
                        println!("ERROR: forceclosechannel requires a channel ID: `forceclosechannel <channel_id> <peer_pubkey>`");
                        continue;
                    }
                    let channel_id_vec = hex_utils::to_vec(channel_id_str.unwrap());
                    if channel_id_vec.is_none() || channel_id_vec.as_ref().unwrap().len() != 32 {
                        println!("ERROR: couldn't parse channel_id");
                        continue;
                    }
                    let mut channel_id = [0; 32];
                    channel_id.copy_from_slice(&channel_id_vec.unwrap());

                    let peer_pubkey_str = words.next();
                    if peer_pubkey_str.is_none() {
                        println!("ERROR: forceclosechannel requires a peer pubkey: `forceclosechannel <channel_id> <peer_pubkey>`");
                        continue;
                    }
                    let peer_pubkey_vec = match hex_utils::to_vec(peer_pubkey_str.unwrap()) {
                        Some(peer_pubkey_vec) => peer_pubkey_vec,
                        None => {
                            println!("ERROR: couldn't parse peer_pubkey");
                            continue;
                        }
                    };
                    let peer_pubkey = match PublicKey::from_slice(&peer_pubkey_vec) {
                        Ok(peer_pubkey) => peer_pubkey,
                        Err(_) => {
                            println!("ERROR: couldn't parse peer_pubkey");
                            continue;
                        }
                    };

                    force_close_channel(channel_id, peer_pubkey, channel_manager.clone());*/
                }
                "nodeinfo" => {
                    node_info(lipa_lightning);
                }
                /*"listpeers" => list_peers(peer_manager.clone()),
                "signmessage" => {
                    const MSG_STARTPOS: usize = "signmessage".len() + 1;
                    if line.as_bytes().len() <= MSG_STARTPOS {
                        println!("ERROR: signmsg requires a message");
                        continue;
                    }
                    println!(
                        "{:?}",
                        lightning::util::message_signing::sign(
                            &line.as_bytes()[MSG_STARTPOS..],
                            &keys_manager.get_node_secret(Recipient::Node).unwrap()
                        )
                    );
                }*/
                "stop" => {
                    break;
                }
                _ => println!("Unknown command. See `\"help\" for available commands."),
            }
        }
    }
}

fn help() {
    println!("openchannel pubkey@host:port <amt_satoshis>");
    println!("sendpayment <invoice>");
    println!("keysend <dest_pubkey> <amt_msats>");
    println!("getinvoice <amt_msats> <expiry_secs>");
    //println!("connectpeer pubkey@host:port");
    //println!("listchannels");
    //println!("listpayments");
    //println!("closechannel <channel_id> <peer_pubkey>");
    //println!("forceclosechannel <channel_id> <peer_pubkey>");
    println!("nodeinfo");
    //println!("listpeers");
    //println!("signmessage <message>");
    println!("stop");
}

fn node_info(lipa_lightning: &LipaLightning) {
    let node_info = lipa_lightning.get_node_info();
    println!("\t{{");
    println!(
        "\t\t node_pubkey: {}",
        PublicKey::from_slice(&*node_info.node_pubkey).unwrap()
    );
    println!("\t\t num_channels: {}", node_info.num_channels);
    println!(
        "\t\t num_usable_channels: {}",
        node_info.num_usable_channels
    );
    println!("\t\t local_balance_msat: {}", node_info.local_balance_msat);
    println!("\t\t num_peers: {}", node_info.num_peers);
    println!("\t}},");
}
/*
fn list_peers(peer_manager: Arc<PeerManager>) {
    println!("\t{{");
    for pubkey in peer_manager.get_peer_node_ids() {
        println!("\t\t pubkey: {}", pubkey);
    }
    println!("\t}},");
}

fn list_channels(channel_manager: &Arc<ChannelManager>, network_graph: &Arc<NetworkGraph>) {
    print!("[");
    for chan_info in channel_manager.list_channels() {
        println!("");
        println!("\t{{");
        println!("\t\tchannel_id: {},", hex_utils::hex_str(&chan_info.channel_id[..]));
        if let Some(funding_txo) = chan_info.funding_txo {
            println!("\t\tfunding_txid: {},", funding_txo.txid);
        }

        println!(
            "\t\tpeer_pubkey: {},",
            hex_utils::hex_str(&chan_info.counterparty.node_id.serialize())
        );
        if let Some(node_info) = network_graph
            .read_only()
            .nodes()
            .get(&NodeId::from_pubkey(&chan_info.counterparty.node_id))
        {
            if let Some(announcement) = &node_info.announcement_info {
                println!("\t\tpeer_alias: {}", announcement.alias);
            }
        }

        if let Some(id) = chan_info.short_channel_id {
            println!("\t\tshort_channel_id: {},", id);
        }
        println!("\t\tis_channel_ready: {},", chan_info.is_channel_ready);
        println!("\t\tchannel_value_satoshis: {},", chan_info.channel_value_satoshis);
        println!("\t\tlocal_balance_msat: {},", chan_info.balance_msat);
        if chan_info.is_usable {
            println!("\t\tavailable_balance_for_send_msat: {},", chan_info.outbound_capacity_msat);
            println!("\t\tavailable_balance_for_recv_msat: {},", chan_info.inbound_capacity_msat);
        }
        println!("\t\tchannel_can_send_payments: {},", chan_info.is_usable);
        println!("\t\tpublic: {},", chan_info.is_public);
        println!("\t}},");
    }
    println!("]");
}

fn list_payments(inbound_payments: PaymentInfoStorage, outbound_payments: PaymentInfoStorage) {
    let inbound = inbound_payments.lock().unwrap();
    let outbound = outbound_payments.lock().unwrap();
    print!("[");
    for (payment_hash, payment_info) in inbound.deref() {
        println!("");
        println!("\t{{");
        println!("\t\tamount_millisatoshis: {},", payment_info.amt_msat);
        println!("\t\tpayment_hash: {},", hex_utils::hex_str(&payment_hash.0));
        println!("\t\thtlc_direction: inbound,");
        println!(
            "\t\thtlc_status: {},",
            match payment_info.status {
                HTLCStatus::Pending => "pending",
                HTLCStatus::Succeeded => "succeeded",
                HTLCStatus::Failed => "failed",
            }
        );

        println!("\t}},");
    }

    for (payment_hash, payment_info) in outbound.deref() {
        println!("");
        println!("\t{{");
        println!("\t\tamount_millisatoshis: {},", payment_info.amt_msat);
        println!("\t\tpayment_hash: {},", hex_utils::hex_str(&payment_hash.0));
        println!("\t\thtlc_direction: outbound,");
        println!(
            "\t\thtlc_status: {},",
            match payment_info.status {
                HTLCStatus::Pending => "pending",
                HTLCStatus::Succeeded => "succeeded",
                HTLCStatus::Failed => "failed",
            }
        );

        println!("\t}},");
    }
    println!("]");
}

pub(crate) async fn connect_peer_if_necessary(
    pubkey: PublicKey, peer_addr: SocketAddr, peer_manager: Arc<PeerManager>,
) -> Result<(), ()> {
    for node_pubkey in peer_manager.get_peer_node_ids() {
        if node_pubkey == pubkey {
            return Ok(());
        }
    }
    let res = do_connect_peer(pubkey, peer_addr, peer_manager).await;
    if res.is_err() {
        println!("ERROR: failed to connect to peer");
    }
    res
}

pub(crate) async fn do_connect_peer(
    pubkey: PublicKey, peer_addr: SocketAddr, peer_manager: Arc<PeerManager>,
) -> Result<(), ()> {
    match lightning_net_tokio::connect_outbound(Arc::clone(&peer_manager), pubkey, peer_addr).await
    {
        Some(connection_closed_future) => {
            let mut connection_closed_future = Box::pin(connection_closed_future);
            loop {
                match futures::poll!(&mut connection_closed_future) {
                    std::task::Poll::Ready(_) => {
                        return Err(());
                    }
                    std::task::Poll::Pending => {}
                }
                // Avoid blocking the tokio context by sleeping a bit
                match peer_manager.get_peer_node_ids().iter().find(|id| **id == pubkey) {
                    Some(_) => return Ok(()),
                    None => tokio::time::sleep(Duration::from_millis(10)).await,
                }
            }
        }
        None => Err(()),
    }
}

fn open_channel(
    peer_pubkey: PublicKey, channel_amt_sat: u64, announced_channel: bool,
    channel_manager: Arc<ChannelManager>,
) -> Result<(), ()> {
    let config = UserConfig {
        channel_handshake_limits: ChannelHandshakeLimits {
            // lnd's max to_self_delay is 2016, so we want to be compatible.
            their_to_self_delay: 2016,
            ..Default::default()
        },
        channel_handshake_config: ChannelHandshakeConfig {
            announced_channel,
            ..Default::default()
        },
        ..Default::default()
    };

    match channel_manager.create_channel(peer_pubkey, channel_amt_sat, 0, 0, Some(config)) {
        Ok(_) => {
            println!("EVENT: initiated channel with peer {}. ", peer_pubkey);
            return Ok(());
        }
        Err(e) => {
            println!("ERROR: failed to open channel: {:?}", e);
            return Err(());
        }
    }
}

fn send_payment<E: EventHandler>(
    invoice_payer: &InvoicePayer<E>, invoice: &Invoice, payment_storage: PaymentInfoStorage,
) {
    let status = match invoice_payer.pay_invoice(invoice) {
        Ok(_payment_id) => {
            let payee_pubkey = invoice.recover_payee_pub_key();
            let amt_msat = invoice.amount_milli_satoshis().unwrap();
            println!("EVENT: initiated sending {} msats to {}", amt_msat, payee_pubkey);
            print!("> ");
            HTLCStatus::Pending
        }
        Err(PaymentError::Invoice(e)) => {
            println!("ERROR: invalid invoice: {}", e);
            print!("> ");
            return;
        }
        Err(PaymentError::Routing(e)) => {
            println!("ERROR: failed to find route: {}", e.err);
            print!("> ");
            return;
        }
        Err(PaymentError::Sending(e)) => {
            println!("ERROR: failed to send payment: {:?}", e);
            print!("> ");
            HTLCStatus::Failed
        }
    };
    let payment_hash = PaymentHash(invoice.payment_hash().clone().into_inner());
    let payment_secret = Some(invoice.payment_secret().clone());

    let mut payments = payment_storage.lock().unwrap();
    payments.insert(
        payment_hash,
        PaymentInfo {
            preimage: None,
            secret: payment_secret,
            status,
            amt_msat: MillisatAmount(invoice.amount_milli_satoshis()),
        },
    );
}

fn keysend<E: EventHandler, K: KeysInterface>(
    invoice_payer: &InvoicePayer<E>, payee_pubkey: PublicKey, amt_msat: u64, keys: &K,
    payment_storage: PaymentInfoStorage,
) {
    let payment_preimage = keys.get_secure_random_bytes();

    let status = match invoice_payer.pay_pubkey(
        payee_pubkey,
        PaymentPreimage(payment_preimage),
        amt_msat,
        40,
    ) {
        Ok(_payment_id) => {
            println!("EVENT: initiated sending {} msats to {}", amt_msat, payee_pubkey);
            print!("> ");
            HTLCStatus::Pending
        }
        Err(PaymentError::Invoice(e)) => {
            println!("ERROR: invalid payee: {}", e);
            print!("> ");
            return;
        }
        Err(PaymentError::Routing(e)) => {
            println!("ERROR: failed to find route: {}", e.err);
            print!("> ");
            return;
        }
        Err(PaymentError::Sending(e)) => {
            println!("ERROR: failed to send payment: {:?}", e);
            print!("> ");
            HTLCStatus::Failed
        }
    };

    let mut payments = payment_storage.lock().unwrap();
    payments.insert(
        PaymentHash(Sha256::hash(&payment_preimage).into_inner()),
        PaymentInfo {
            preimage: None,
            secret: None,
            status,
            amt_msat: MillisatAmount(Some(amt_msat)),
        },
    );
}

fn get_invoice(
    amt_msat: u64, payment_storage: PaymentInfoStorage, channel_manager: Arc<ChannelManager>,
    keys_manager: Arc<KeysManager>, network: Network, expiry_secs: u32,
) {
    let mut payments = payment_storage.lock().unwrap();
    let currency = match network {
        Network::Bitcoin => Currency::Bitcoin,
        Network::Testnet => Currency::BitcoinTestnet,
        Network::Regtest => Currency::Regtest,
        Network::Signet => Currency::Signet,
    };
    let invoice = match utils::create_invoice_from_channelmanager(
        &channel_manager,
        keys_manager,
        currency,
        Some(amt_msat),
        "ldk-tutorial-node".to_string(),
        expiry_secs,
    ) {
        Ok(inv) => {
            println!("SUCCESS: generated invoice: {}", inv);
            inv
        }
        Err(e) => {
            println!("ERROR: failed to create invoice: {:?}", e);
            return;
        }
    };

    let payment_hash = PaymentHash(invoice.payment_hash().clone().into_inner());
    payments.insert(
        payment_hash,
        PaymentInfo {
            preimage: None,
            secret: Some(invoice.payment_secret().clone()),
            status: HTLCStatus::Pending,
            amt_msat: MillisatAmount(Some(amt_msat)),
        },
    );
}

fn close_channel(
    channel_id: [u8; 32], counterparty_node_id: PublicKey, channel_manager: Arc<ChannelManager>,
) {
    match channel_manager.close_channel(&channel_id, &counterparty_node_id) {
        Ok(()) => println!("EVENT: initiating channel close"),
        Err(e) => println!("ERROR: failed to close channel: {:?}", e),
    }
}

fn force_close_channel(
    channel_id: [u8; 32], counterparty_node_id: PublicKey, channel_manager: Arc<ChannelManager>,
) {
    match channel_manager.force_close_broadcasting_latest_txn(&channel_id, &counterparty_node_id) {
        Ok(()) => println!("EVENT: initiating channel force-close"),
        Err(e) => println!("ERROR: failed to force-close channel: {:?}", e),
    }
}
*/
pub(crate) fn parse_peer_info(
    peer_pubkey_and_ip_addr: String,
) -> Result<(PublicKey, SocketAddr), std::io::Error> {
    let mut pubkey_and_addr = peer_pubkey_and_ip_addr.split('@');
    let pubkey = pubkey_and_addr.next();
    let peer_addr_str = pubkey_and_addr.next();
    if peer_addr_str.is_none() || peer_addr_str.is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "ERROR: incorrectly formatted peer info. Should be formatted as: `pubkey@host:port`",
        ));
    }

    let peer_addr = peer_addr_str
        .unwrap()
        .to_socket_addrs()
        .map(|mut r| r.next());
    if peer_addr.is_err() || peer_addr.as_ref().unwrap().is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "ERROR: couldn't parse pubkey@host:port into a socket address",
        ));
    }

    let pubkey = hex_utils::to_compressed_pubkey(pubkey.unwrap());
    if pubkey.is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "ERROR: unable to parse given pubkey for node",
        ));
    }

    Ok((pubkey.unwrap(), peer_addr.unwrap().unwrap()))
}
