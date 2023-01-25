use crate::fee_estimator::FeeEstimator;
use crate::filter::FilterImpl;
use crate::logger::LightningLogger;
use crate::storage_persister::StoragePersister;
use crate::tx_broadcaster::TxBroadcaster;

use crate::event_handler::LipaEventHandler;
use lightning::chain::chainmonitor::ChainMonitor as LdkChainMonitor;
use lightning::chain::keysinterface::InMemorySigner;
use lightning::ln::channelmanager::SimpleArcChannelManager;
use lightning::ln::peer_handler::IgnoringMessageHandler;
use lightning::routing::router::DefaultRouter;
use lightning::routing::scoring::ProbabilisticScorer;
use lightning_invoice::payment;
use lightning_net_tokio::SocketDescriptor;
use std::sync::{Arc, Mutex};

pub(crate) type ChainMonitor = LdkChainMonitor<
    InMemorySigner,
    Arc<FilterImpl>,
    Arc<TxBroadcaster>,
    Arc<FeeEstimator>,
    Arc<LightningLogger>,
    Arc<StoragePersister>,
>;

pub(crate) type ChannelManager =
    SimpleArcChannelManager<ChainMonitor, TxBroadcaster, FeeEstimator, LightningLogger>;

pub(crate) type PeerManager = lightning::ln::peer_handler::PeerManager<
    SocketDescriptor,
    Arc<ChannelManager>,
    IgnoringMessageHandler,
    IgnoringMessageHandler,
    Arc<LightningLogger>,
    IgnoringMessageHandler,
>;

pub(crate) type NetworkGraph = lightning::routing::gossip::NetworkGraph<Arc<LightningLogger>>;

pub(crate) type RapidGossipSync =
    lightning_rapid_gossip_sync::RapidGossipSync<Arc<NetworkGraph>, Arc<LightningLogger>>;

type Router = DefaultRouter<
    Arc<NetworkGraph>,
    Arc<LightningLogger>,
    Arc<Mutex<ProbabilisticScorer<Arc<NetworkGraph>, Arc<LightningLogger>>>>,
>;

pub(crate) type InvoicePayer =
    payment::InvoicePayer<Arc<ChannelManager>, Router, Arc<LightningLogger>, Arc<LipaEventHandler>>;

pub(crate) type Scorer = ProbabilisticScorer<Arc<NetworkGraph>, Arc<LightningLogger>>;
