use crate::fee_estimator::FeeEstimator;
use crate::filter::FilterImpl;
use crate::logger::LightningLogger;
use crate::storage_persister::StoragePersister;
use crate::tx_broadcaster::TxBroadcaster;

use lightning::chain::chainmonitor::ChainMonitor as LdkChainMonitor;
use lightning::chain::keysinterface::InMemorySigner;
use lightning::ln::channelmanager::SimpleArcChannelManager;
use lightning::ln::peer_handler::IgnoringMessageHandler;
use lightning_net_tokio::SocketDescriptor;
use std::sync::Arc;

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
    Arc<LightningLogger>,
    IgnoringMessageHandler,
>;
