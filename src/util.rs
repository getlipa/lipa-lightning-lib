use crate::{
    ChannelManager, HTLCStatus, InvoicePayer, LipaLightningError, MillisatAmount, PaymentInfo,
    PaymentInfoStorage, PeerManager,
};
use bitcoin::hashes::sha256::Hash as Sha256;
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::PublicKey;
use bitcoin::Network;
use lightning::chain::keysinterface::{KeysInterface, KeysManager};
use lightning::ln::{PaymentHash, PaymentPreimage};
use lightning::util::config::{ChannelHandshakeConfig, ChannelHandshakeLimits, UserConfig};
use lightning_invoice::payment::PaymentError;
use lightning_invoice::{utils, Currency, Invoice};
use log::{error, info};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

pub(crate) async fn connect_peer_if_necessary(
    pubkey: PublicKey,
    peer_addr: SocketAddr,
    peer_manager: Arc<PeerManager>,
) -> Result<(), LipaLightningError> {
    for node_pubkey in peer_manager.get_peer_node_ids() {
        if node_pubkey == pubkey {
            return Ok(());
        }
    }
    do_connect_peer(pubkey, peer_addr, peer_manager).await
}

pub(crate) async fn do_connect_peer(
    pubkey: PublicKey,
    peer_addr: SocketAddr,
    peer_manager: Arc<PeerManager>,
) -> Result<(), LipaLightningError> {
    match lightning_net_tokio::connect_outbound(Arc::clone(&peer_manager), pubkey, peer_addr).await
    {
        Some(connection_closed_future) => {
            let mut connection_closed_future = Box::pin(connection_closed_future);
            loop {
                match futures::poll!(&mut connection_closed_future) {
                    std::task::Poll::Ready(_) => {
                        error!(
                            "Failed to connect to peer with address {}",
                            peer_addr.to_string()
                        );
                        return Err(LipaLightningError::PeerConnection {
                            peer_id: pubkey.to_string(),
                            peer_addr: peer_addr.to_string(),
                        });
                    }
                    std::task::Poll::Pending => {}
                }
                // Avoid blocking the tokio context by sleeping a bit
                match peer_manager
                    .get_peer_node_ids()
                    .iter()
                    .find(|id| **id == pubkey)
                {
                    Some(_) => return Ok(()),
                    None => tokio::time::sleep(Duration::from_millis(10)).await,
                }
            }
        }
        None => {
            error!(
                "Failed to connect to peer with address {}",
                peer_addr.to_string()
            );
            Err(LipaLightningError::PeerConnection {
                peer_id: pubkey.to_string(),
                peer_addr: peer_addr.to_string(),
            })
        }
    }
}

pub(crate) fn open_channel(
    peer_pubkey: PublicKey,
    channel_amt_sat: u64,
    announced_channel: bool,
    channel_manager: Arc<ChannelManager>,
) -> Result<(), LipaLightningError> {
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
            info!("EVENT: initiated channel with peer {}. ", peer_pubkey);
            Ok(())
        }
        Err(e) => {
            error!("Failed to open channel: {:?}", e);
            Err(LipaLightningError::ChannelOpen {
                peer_id: peer_pubkey.to_string(),
            })
        }
    }
}

pub(crate) fn do_send_payment(
    invoice_payer: &InvoicePayer,
    invoice: &Invoice,
    payment_storage: PaymentInfoStorage,
) -> Result<(), LipaLightningError> {
    let status = match invoice_payer.pay_invoice(invoice) {
        Ok(_payment_id) => {
            let payee_pubkey = invoice.recover_payee_pub_key();
            let amt_msat = invoice.amount_milli_satoshis().unwrap();
            info!(
                "EVENT: initiated sending {} msats to {}",
                amt_msat, payee_pubkey
            );
            HTLCStatus::Pending
        }
        Err(PaymentError::Invoice(e)) => {
            error!("Invalid invoice: {}", e);
            return Err(LipaLightningError::InvoiceInvalid {
                info: e.to_string(),
            });
        }
        Err(PaymentError::Routing(e)) => {
            error!("Failed to find route: {}", e.err);
            return Err(LipaLightningError::Routing { info: e.err });
        }
        Err(PaymentError::Sending(e)) => {
            error!("Failed to send payment: {:?}", e);
            //HTLCStatus::Failed
            return Err(LipaLightningError::PaymentFail {
                info: format!("{:?}", e),
            });
        }
    };
    #[allow(clippy::clone_on_copy)]
    let payment_hash = PaymentHash(invoice.payment_hash().clone().into_inner());
    let payment_secret = Some(*invoice.payment_secret());

    let mut payments = match payment_storage.lock() {
        Ok(p) => p,
        Err(_) => {
            error!("Failed to acquire lock for payment_storage");
            return Err(LipaLightningError::InternalError {
                info: "Failed to acquire lock".to_string(),
            });
        }
    };

    payments.insert(
        payment_hash,
        PaymentInfo {
            preimage: None,
            secret: payment_secret,
            status,
            amt_msat: MillisatAmount(invoice.amount_milli_satoshis()),
        },
    );
    Ok(())
}

pub(crate) fn keysend(
    invoice_payer: Arc<InvoicePayer>,
    payee_pubkey: PublicKey,
    amt_msat: u64,
    keys: Arc<KeysManager>,
    payment_storage: PaymentInfoStorage,
) -> Result<(), LipaLightningError> {
    let payment_preimage = keys.get_secure_random_bytes();

    let status = match invoice_payer.pay_pubkey(
        payee_pubkey,
        PaymentPreimage(payment_preimage),
        amt_msat,
        40,
    ) {
        Ok(_payment_id) => {
            info!(
                "EVENT: initiated sending {} msats to {}",
                amt_msat, payee_pubkey
            );
            HTLCStatus::Pending
        }
        Err(PaymentError::Invoice(e)) => {
            error!("Failed to send keysend payment - Invalid payee: {}", e);
            return Err(LipaLightningError::InvalidPayee {
                info: e.to_string(),
            });
        }
        Err(PaymentError::Routing(e)) => {
            error!("Failed to find route: {}", e.err);
            return Err(LipaLightningError::Routing { info: e.err });
        }
        Err(PaymentError::Sending(e)) => {
            error!("Failed to send payment: {:?}", e);
            //HTLCStatus::Failed
            return Err(LipaLightningError::PaymentFail {
                info: format!("{:?}", e),
            });
        }
    };

    let mut payments = match payment_storage.lock() {
        Ok(p) => p,
        Err(_) => {
            error!("Failed to acquire lock for payment_storage");
            return Err(LipaLightningError::InternalError {
                info: "Failed to acquire lock".to_string(),
            });
        }
    };
    payments.insert(
        PaymentHash(Sha256::hash(&payment_preimage).into_inner()),
        PaymentInfo {
            preimage: None,
            secret: None,
            status,
            amt_msat: MillisatAmount(Some(amt_msat)),
        },
    );
    Ok(())
}

pub(crate) fn get_invoice(
    amt_msat: u64,
    payment_storage: PaymentInfoStorage,
    channel_manager: Arc<ChannelManager>,
    keys_manager: Arc<KeysManager>,
    network: Network,
    expiry_secs: u32,
) -> Result<String, LipaLightningError> {
    let mut payments = match payment_storage.lock() {
        Ok(p) => p,
        Err(_) => {
            error!("Failed to acquire lock for payment_storage");
            return Err(LipaLightningError::InternalError {
                info: "Failed to acquire lock".to_string(),
            });
        }
    };
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
        Ok(inv) => inv,
        Err(e) => {
            error!("Failed to create invoice: {:?}", e);
            return Err(LipaLightningError::InvoiceCreation {
                info: e.to_string(),
            });
        }
    };

    #[allow(clippy::clone_on_copy)]
    let payment_hash = PaymentHash(invoice.payment_hash().clone().into_inner());
    payments.insert(
        payment_hash,
        PaymentInfo {
            preimage: None,
            secret: Some(*invoice.payment_secret()),
            status: HTLCStatus::Pending,
            amt_msat: MillisatAmount(Some(amt_msat)),
        },
    );
    Ok(invoice.to_string())
}
