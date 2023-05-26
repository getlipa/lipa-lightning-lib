use crate::fee_estimator::FeeEstimator;
use crate::logger::LightningLogger;
use crate::storage_persister::StoragePersister;
use crate::tx_broadcaster::TxBroadcaster;
use crate::wallet::WalletKeysManager;

use lightning::chain::chainmonitor::ChainMonitor as LdkChainMonitor;
use lightning::chain::keysinterface::InMemorySigner;
use lightning::ln::peer_handler::IgnoringMessageHandler;
use lightning::routing::router::DefaultRouter;
use lightning::routing::scoring::ProbabilisticScorer;
use lightning_net_tokio::SocketDescriptor;
use lightning_transaction_sync::EsploraSyncClient;
use std::sync::{Arc, Mutex};

pub(crate) type TxSync = EsploraSyncClient<Arc<LightningLogger>>;

pub(crate) type ChainMonitor = LdkChainMonitor<
    InMemorySigner,
    Arc<TxSync>,
    Arc<TxBroadcaster>,
    Arc<FeeEstimator>,
    Arc<LightningLogger>,
    Arc<StoragePersister>,
>;

// Similar to LDK's SimpleArcChannelManager, but with our KeysManager type
pub type SimpleArcChannelManager<M, T, F, L> = lightning::ln::channelmanager::ChannelManager<
    Arc<M>,
    Arc<T>,
    Arc<KeysManager>,
    Arc<KeysManager>,
    Arc<KeysManager>,
    Arc<F>,
    Arc<
        DefaultRouter<
            Arc<lightning::routing::gossip::NetworkGraph<Arc<L>>>,
            Arc<L>,
            Arc<
                Mutex<
                    ProbabilisticScorer<
                        Arc<lightning::routing::gossip::NetworkGraph<Arc<L>>>,
                        Arc<L>,
                    >,
                >,
            >,
        >,
    >,
    Arc<L>,
>;

pub(crate) type ChannelManager =
    SimpleArcChannelManager<ChainMonitor, TxBroadcaster, FeeEstimator, LightningLogger>;

pub(crate) type ChannelManagerReadArgs<'a> = lightning::ln::channelmanager::ChannelManagerReadArgs<
    'a,
    Arc<ChainMonitor>,
    Arc<TxBroadcaster>,
    Arc<KeysManager>,
    Arc<KeysManager>,
    Arc<KeysManager>,
    Arc<FeeEstimator>,
    Arc<Router>,
    Arc<LightningLogger>,
>;

pub(crate) type PeerManager = lightning::ln::peer_handler::PeerManager<
    SocketDescriptor,
    Arc<ChannelManager>,
    IgnoringMessageHandler,
    IgnoringMessageHandler,
    Arc<LightningLogger>,
    IgnoringMessageHandler,
    Arc<KeysManager>,
>;

pub(crate) type NetworkGraph = lightning::routing::gossip::NetworkGraph<Arc<LightningLogger>>;

pub(crate) type RapidGossipSync =
    lightning_rapid_gossip_sync::RapidGossipSync<Arc<NetworkGraph>, Arc<LightningLogger>>;

pub(crate) type Router = DefaultRouter<
    Arc<NetworkGraph>,
    Arc<LightningLogger>,
    Arc<Mutex<ProbabilisticScorer<Arc<NetworkGraph>, Arc<LightningLogger>>>>,
>;

pub(crate) type Scorer = ProbabilisticScorer<Arc<NetworkGraph>, Arc<LightningLogger>>;

pub(crate) type KeysManager = WalletKeysManager;
