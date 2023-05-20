use crate::encryption_symmetric::{decrypt, encrypt};
use crate::errors::*;
use crate::interfaces::RemoteStorage;
use crate::types::{
    ChainMonitor, ChannelManager, ChannelManagerReadArgs, NetworkGraph, Router, Scorer,
};
use crate::LightningLogger;
use std::cmp::Ordering;

use crate::tx_broadcaster::TxBroadcaster;
use bitcoin::hash_types::BlockHash;
use bitcoin::hashes::hex::ToHex;
use bitcoin::Network;
use lightning::chain::chaininterface::{BroadcasterInterface, FeeEstimator};
use lightning::chain::chainmonitor::{MonitorUpdateId, Persist};
use lightning::chain::channelmonitor::{ChannelMonitor, ChannelMonitorUpdate};
use lightning::chain::keysinterface::{
    EntropySource, InMemorySigner, KeysManager, NodeSigner, SignerProvider,
    WriteableEcdsaChannelSigner,
};
use lightning::chain::transaction::OutPoint;
use lightning::chain::{ChannelMonitorUpdateStatus, Watch};
use lightning::ln::channelmanager::{ChainParameters, SimpleArcChannelManager};
use lightning::routing::router;
use lightning::routing::scoring::{ProbabilisticScoringParameters, WriteableScore};
use lightning::util::config::UserConfig;
use lightning::util::logger::Logger;
use lightning::util::persist::{KVStorePersister, Persister};
use lightning::util::ser::{ReadableArgs, Writeable};
use lightning_persister::FilesystemPersister;
use log::{debug, error, warn};
use perro::Error::RuntimeError;
use perro::{invalid_input, permanent_failure, runtime_error, MapToError};
use std::fs;
use std::io::{BufReader, Cursor};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, Weak};
use std::thread::sleep;
use std::time::Duration;

static MONITORS_BUCKET: &str = "monitors";
static OBJECTS_BUCKET: &str = "objects";

static MANAGER_KEY: &str = "manager";
static GRAPH_KEY: &str = "network_graph";
static SCORER_KEY: &str = "scorer";

pub(crate) struct StoragePersister {
    storage: Arc<Box<dyn RemoteStorage>>,
    fs_persister: FilesystemPersister,
    encryption_key: [u8; 32],
    chain_monitor: RwLock<Weak<ChainMonitor>>,
}

impl StoragePersister {
    pub fn new(
        storage: Box<dyn RemoteStorage>,
        local_fs_path: String,
        encryption_key: [u8; 32],
    ) -> Self {
        let storage = Arc::new(storage);
        let fs_persister = FilesystemPersister::new(local_fs_path);
        Self {
            storage,
            fs_persister,
            encryption_key,
            chain_monitor: RwLock::new(Weak::new()),
        }
    }

    pub fn add_chain_monitor(&self, chain_monitor: Weak<ChainMonitor>) {
        let mut mutex_chain_monitor = self.chain_monitor.write().unwrap();
        *mutex_chain_monitor = chain_monitor;
    }

    pub fn check_health(&self) -> bool {
        self.storage.check_health()
    }

    #[allow(clippy::type_complexity)]
    pub fn read_channel_monitors<ES: Deref, SP: Deref>(
        &self,
        entropy_source: ES,
        signer_provider: SP,
    ) -> Result<
        Vec<(
            BlockHash,
            ChannelMonitor<<SP::Target as SignerProvider>::Signer>,
        )>,
    >
    where
        ES::Target: EntropySource + Sized,
        SP::Target: SignerProvider + Sized,
    {
        let mut local_channel_monitors = self
            .fs_persister
            .read_channelmonitors(&*entropy_source, &*signer_provider)
            .map_to_permanent_failure("Failed to read channel monitors from disk")?;

        // Fetch remote channel monitors to make sure remote state hasn't advanced
        let mut remote_channel_monitors =
            self.fetch_remote_channel_monitors(entropy_source, signer_provider)?;

        Self::verify_local_state_is_latest_state::<SP>(
            &mut local_channel_monitors,
            &mut remote_channel_monitors,
        )?;

        Ok(local_channel_monitors)
    }

    #[allow(clippy::type_complexity)]
    pub fn fetch_remote_channel_monitors<ES: Deref, SP: Deref>(
        &self,
        entropy_source: ES,
        signer_provider: SP,
    ) -> Result<
        Vec<(
            BlockHash,
            ChannelMonitor<<SP::Target as SignerProvider>::Signer>,
        )>,
    >
    where
        ES::Target: EntropySource + Sized,
        SP::Target: SignerProvider + Sized,
    {
        let mut remote_channel_monitors = Vec::new();
        for key in self
            .storage
            .list_objects(MONITORS_BUCKET.to_string())
            .map_to_runtime_error(
                RuntimeErrorCode::RemoteStorageError,
                "Failed to get list of ChannelMonitors from remote storage",
            )?
        {
            let encrypted_data = self
                .storage
                .get_object(MONITORS_BUCKET.to_string(), key.clone())
                .map_to_runtime_error(
                    RuntimeErrorCode::RemoteStorageError,
                    format!("Failed to get ChannelMonitor {key} from remote storage"),
                )?;
            let data = decrypt(&encrypted_data, &self.encryption_key)
                .map_to_permanent_failure("Failed to decrypt ChannelMonitor")?;
            let mut buffer = Cursor::new(&data);
            match <(
                BlockHash,
                ChannelMonitor<<SP::Target as SignerProvider>::Signer>,
            )>::read(&mut buffer, (&*entropy_source, &*signer_provider))
            {
                Ok((blockhash, channel_monitor)) => {
                    debug!(
                        "Successfully read ChannelMonitor {} from remote storage",
                        channel_monitor.get_funding_txo().0.to_channel_id().to_hex()
                    );
                    remote_channel_monitors.push((blockhash, channel_monitor));
                }
                Err(e) => {
                    error!(
                        "Failed to deserialize remote ChannelMonitor `{}`: {}",
                        key, e
                    );
                    // TODO: Should we return this information to the caller?
                    //      A corrupt remote ChannelMonitor could be harmless if in this case we
                    //      load the local ChannelMonitors and the situation gets fixed. If this
                    //      is a wallet recovery, we will need to load the remote ChannelMonitors,
                    //      and this channel will be lost.
                }
            }
        }
        Ok(remote_channel_monitors)
    }

    fn verify_local_state_is_latest_state<SP: Deref>(
        local_monitors: &mut Vec<(
            BlockHash,
            ChannelMonitor<<SP::Target as SignerProvider>::Signer>,
        )>,
        remote_monitors: &mut Vec<(
            BlockHash,
            ChannelMonitor<<SP::Target as SignerProvider>::Signer>,
        )>,
    ) -> Result<()>
    where
        SP::Target: SignerProvider + Sized,
    {
        if local_monitors.is_empty() {
            // If we don't have any local ChannelMonitors, but have 1 or more remote ChannelMonitors,
            // we can assume that this is a new app installation.
            if !remote_monitors.is_empty() {
                return Err(invalid_input(format!("Invalid seed. No local ChannelMonitors were found, but {} was/were retrieved from remote storage.", remote_monitors.len())));
            }
        }

        // If either the local or the remote state shows the existence of more channels than the other
        // that means that that one is more recent than the other.
        match remote_monitors.len().cmp(&local_monitors.len()) {
            Ordering::Less => {
                // The local state is more recent than the remote one. As part of the node startup,
                // every ChannelMonitor is persisted again so this is likely to get resolved.
                // Let's log it anyway...
                warn!("The remote storage doesn't know about all channels.");
                return Ok(());
            }
            Ordering::Equal => {}
            Ordering::Greater => {
                // Another app installation has moved the state forward. Let's fail the node startup!
                return Err(permanent_failure("The remote channel state is more recent than the local one. It is very likely that another app install has recovered this node. Resuming operation here isn't safe."));
            }
        }

        debug_assert_eq!(remote_monitors.len(), local_monitors.len());

        // Check if for any channel, the remote ChannelMonitor is more recent than the local one
        local_monitors
            .sort_unstable_by_key(|(_, monitor)| (monitor.get_funding_txo().0.to_channel_id()));
        remote_monitors
            .sort_unstable_by_key(|(_, monitor)| (monitor.get_funding_txo().0.to_channel_id()));
        let zipped_monitors = local_monitors.iter().zip(remote_monitors.iter());

        for ((_, local_monitor), (_, remote_monitor)) in zipped_monitors {
            if remote_monitor.get_funding_txo() != local_monitor.get_funding_txo() {
                return Err(permanent_failure(
                    "Unexpected incoherence between local and remote ChannelMonitor state",
                ));
            }
            match remote_monitor
                .get_latest_update_id()
                .cmp(&local_monitor.get_latest_update_id())
            {
                Ordering::Less => {
                    // The remote ChannelMonitor isn't up-to-date. As part of the node startup, every
                    // ChannelMonitor is persisted again so this is likely to get resolved.
                    // Let's log it anyway...
                    warn!("The remote version of a channel monitor {} isn't as recent as the local one", local_monitor.get_funding_txo().0.to_channel_id().to_hex());
                }
                Ordering::Equal => {}
                Ordering::Greater => {
                    // Another app installation has moved the state forward. Let's fail the node startup!
                    return Err(permanent_failure("The remote channel state is more recent than the local one. It is very likely that another app install has recovered this node. Resuming operation here isn't safe."));
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    pub fn read_or_init_channel_manager(
        &self,
        chain_monitor: Arc<ChainMonitor>,
        broadcaster: Arc<TxBroadcaster>,
        keys_manager: Arc<KeysManager>,
        fee_estimator: Arc<crate::FeeEstimator>,
        logger: Arc<LightningLogger>,
        router: Arc<Router>,
        channel_monitors: Vec<&mut ChannelMonitor<InMemorySigner>>,
        user_config: UserConfig,
        chain_params: ChainParameters,
    ) -> Result<ChannelManager> {
        let read_args = ChannelManagerReadArgs::new(
            Arc::clone(&keys_manager),
            Arc::clone(&keys_manager),
            Arc::clone(&keys_manager),
            Arc::clone(&fee_estimator),
            Arc::clone(&chain_monitor),
            Arc::clone(&broadcaster),
            Arc::clone(&router),
            Arc::clone(&logger),
            user_config,
            channel_monitors,
        );

        let local_channel_manager = self.read_local_channel_manager(read_args)?;

        match local_channel_manager {
            None => {
                let channel_manager = SimpleArcChannelManager::new(
                    fee_estimator,
                    chain_monitor,
                    broadcaster,
                    router,
                    logger,
                    Arc::clone(&keys_manager),
                    Arc::clone(&keys_manager),
                    keys_manager,
                    user_config,
                    chain_params,
                );
                Ok(channel_manager)
            }
            Some(c) => Ok(c),
        }
    }

    fn read_local_channel_manager(
        &self,
        read_args: ChannelManagerReadArgs,
    ) -> Result<Option<ChannelManager>> {
        let path = PathBuf::from(self.fs_persister.get_data_dir()).join(Path::new(MANAGER_KEY));
        if !path.exists() {
            return Ok(None);
        }
        if let Ok(f) = fs::File::open(path) {
            let (_block_hash, channel_manager) =
                <(BlockHash, ChannelManager)>::read(
                    &mut BufReader::new(f),
                    read_args,
                )
                    .map_to_permanent_failure("Failed to parse a previously locally persisted ChannelManager. Could it have been corrupted?")?;
            Ok(Some(channel_manager))
        } else {
            error!("Failed to open the local channel manager file.");
            Err(permanent_failure(
                "Failed to open the local ChannelMonitor file",
            ))
        }
    }

    pub fn fetch_remote_channel_manager_serialized(&self) -> Result<Vec<u8>> {
        let encrypted_data = match self
            .storage
            .get_object(OBJECTS_BUCKET.to_string(), MANAGER_KEY.to_string())
        {
            Ok(data) => data,
            Err(RuntimeError {
                code: RuntimeErrorCode::ObjectNotFound,
                ..
            }) => {
                return Err(runtime_error(
                    RuntimeErrorCode::NonExistingWallet,
                    "Failed to find remote ChannelManager",
                ))
            }
            Err(e) => return Err(e),
        };
        decrypt(&encrypted_data, &self.encryption_key)
    }

    pub fn read_or_init_graph(
        &self,
        network: Network,
        logger: Arc<LightningLogger>,
    ) -> Result<NetworkGraph> {
        let path = PathBuf::from(self.fs_persister.get_data_dir()).join(Path::new(GRAPH_KEY));

        if let Ok(file) = fs::File::open(&path) {
            if let Ok(graph) = NetworkGraph::read(&mut BufReader::new(file), logger.clone()) {
                debug!("Successfully read the network graph from the local filesystem");
                return Ok(graph);
            } else {
                error!("Failed to parse network graph data. Deleting and continuing...");
                fs::remove_file(path)
                    .map_to_permanent_failure("Failed to delete an invalid network graph file")?;
            }
        }
        debug!("Couldn't find a previously persisted network graph. Creating a new one...");
        Ok(NetworkGraph::new(network, logger))
    }

    pub fn read_or_init_scorer(
        &self,
        graph: Arc<NetworkGraph>,
        logger: Arc<LightningLogger>,
    ) -> Result<Scorer> {
        let path = PathBuf::from(self.fs_persister.get_data_dir()).join(Path::new(SCORER_KEY));

        let params = ProbabilisticScoringParameters::default();
        if let Ok(file) = fs::File::open(&path) {
            let args = (params.clone(), Arc::clone(&graph), Arc::clone(&logger));
            if let Ok(scorer) = Scorer::read(&mut BufReader::new(file), args) {
                debug!("Successfully read the scorer from the local filesystem");
                return Ok(scorer);
            } else {
                error!("Failed to parse scorer data. Deleting and continuing...");
                fs::remove_file(&path)
                    .map_to_permanent_failure("Failed to delete an invalid scorer file")?;
            }
        }
        debug!("Couldn't find a previously persisted scorer. Creating a new one...");
        Ok(Scorer::new(params, graph, logger))
    }

    pub fn persist_serialized_manager_local(&self, channel_manager: &[u8]) -> Result<()> {
        fs::write(
            get_local_channel_manager_path(&self.fs_persister.get_data_dir()),
            channel_manager,
        )
        .map_to_permanent_failure(
            "Failed to locally persist the ChannelManager recovered from remote storage",
        )
    }

    pub fn persist_channel_monitors_local(
        &self,
        monitors: Vec<(BlockHash, ChannelMonitor<InMemorySigner>)>,
    ) -> Result<()> {
        for (_, monitor) in monitors {
            self.persist_channel_monitor_local(monitor.get_funding_txo().0, &monitor)?;
        }
        Ok(())
    }

    fn persist_channel_monitor_local<ChannelSigner: WriteableEcdsaChannelSigner>(
        &self,
        funding_txo: OutPoint,
        monitor: &ChannelMonitor<ChannelSigner>,
    ) -> Result<()> {
        let key = format!(
            "monitors/{}_{}",
            funding_txo.txid.to_hex(),
            funding_txo.index
        );
        self.fs_persister
            .persist(&key, monitor)
            .map_to_permanent_failure(
                "Failed to locally persist a ChannelMonitor recovered from remote storage",
            )
    }
}

impl<ChannelSigner: WriteableEcdsaChannelSigner> Persist<ChannelSigner> for StoragePersister {
    fn persist_new_channel(
        &self,
        channel_id: OutPoint,
        data: &ChannelMonitor<ChannelSigner>,
        update_id: MonitorUpdateId,
    ) -> ChannelMonitorUpdateStatus {
        // Persist locally
        match self
            .fs_persister
            .persist_new_channel(channel_id, data, update_id)
        {
            ChannelMonitorUpdateStatus::Completed => {}
            ChannelMonitorUpdateStatus::InProgress => {
                error!("Unexpected: FilesystemPersister returned ChannelMonitorUpdateStatus::InProgress");
                return ChannelMonitorUpdateStatus::PermanentFailure;
            }
            ChannelMonitorUpdateStatus::PermanentFailure => {
                error!("Failed to persist a ChannelMonitor in the filesystem.");
                return ChannelMonitorUpdateStatus::PermanentFailure;
            }
        };

        // Launch background task that handles persisting monitor remotely
        let data = data.encode();
        let storage = Arc::clone(&self.storage);
        let chain_monitor = match { self.chain_monitor.read().unwrap() }.upgrade() {
            None => return ChannelMonitorUpdateStatus::PermanentFailure,
            Some(c) => c,
        };
        let encrypted_data = match encrypt(&data, &self.encryption_key) {
            Ok(d) => d,
            Err(_) => {
                error!("Failed to encrypt ChannelMonitor data");
                return ChannelMonitorUpdateStatus::PermanentFailure;
            }
        };

        // Launch thread instead of tokio task due to the use of blocking reqwest in our
        // RemoteStorage implementation.
        // TODO: potentially switch to a tokio task when we rethink our sync/async model
        std::thread::spawn(move || {
            persist_monitor_remotely(
                storage,
                chain_monitor,
                encrypted_data,
                channel_id,
                update_id,
            )
        });

        ChannelMonitorUpdateStatus::InProgress
    }

    fn update_persisted_channel(
        &self,
        channel_id: OutPoint,
        _update: Option<&ChannelMonitorUpdate>,
        data: &ChannelMonitor<ChannelSigner>,
        update_id: MonitorUpdateId,
    ) -> ChannelMonitorUpdateStatus {
        self.persist_new_channel(channel_id, data, update_id)
    }
}

// The channel will block until remote persistence succeeds so let's continuously retry
//
// If there is a non-runtime failure or if the node is shutdown before remote persistence
// succeeds, the local version of the channel monitor will be fresher than the remote one
//
// This should sort itself out as the node will get back to trying to persist remotely
// when it gets back online (adding the ChannelMonitor to the ChainMonitor calls
// persist_new_channel() again).
//
// We only stop retrying when the ChainMonitor stops stating that the update is pending
fn persist_monitor_remotely(
    storage: Arc<Box<dyn RemoteStorage>>,
    chain_monitor: Arc<ChainMonitor>,
    encrypted_data: Vec<u8>,
    channel_id: OutPoint,
    update_id: MonitorUpdateId,
) {
    let key = format!("{}{:04x}", channel_id.txid.to_hex(), channel_id.index);
    loop {
        match storage.put_object(
            MONITORS_BUCKET.to_string(),
            key.clone(),
            encrypted_data.clone(),
        ) {
            Ok(_) => break,
            Err(RuntimeError { .. }) => {
                error!(
                    "Temporary failure to remotely persist the ChannelMonitor {}... Retrying...",
                    key
                );
            }
            Err(e) => {
                error!(
                    "Failed to remotely persist the ChannelMonitor {} - {}",
                    key,
                    e.to_string()
                );
                return;
            }
        }
        if !chain_monitor
            .list_pending_monitor_updates()
            .contains_key(&channel_id)
        {
            error!("Failed to remotely persist ChannelMonitor {} - ChainMonitor stopped listing this ChannelMonitor as having pending updates", key);
            return;
        }
        sleep(Duration::from_secs(5));
    }

    // Let ChainMonitor know that remote persistence has succeeded
    if chain_monitor
        .channel_monitor_updated(channel_id, update_id)
        .is_err()
    {
        error!("Attempted to inform the ChainMonitor about a successfully persisted ChannelMonitor but the ChainMonitor doesn't know about the ChannelMonitor");
    }
    debug!("Successfully remotely persisted the ChannelMonitor {}", key);
}

impl<
        'a,
        M: Deref,
        T: Deref,
        ES: Deref,
        NS: Deref,
        SP: Deref,
        F: Deref,
        R: Deref,
        L: Deref,
        S: WriteableScore<'a>,
    > Persister<'a, M, T, ES, NS, SP, F, R, L, S> for StoragePersister
where
    M::Target: 'static + Watch<<SP::Target as SignerProvider>::Signer>,
    T::Target: 'static + BroadcasterInterface,
    ES::Target: 'static + EntropySource,
    NS::Target: 'static + NodeSigner,
    SP::Target: 'static + SignerProvider,
    F::Target: 'static + FeeEstimator,
    R::Target: 'static + router::Router,
    L::Target: 'static + Logger,
{
    fn persist_manager(
        &self,
        channel_manager: &lightning::ln::channelmanager::ChannelManager<M, T, ES, NS, SP, F, R, L>,
    ) -> std::result::Result<(), std::io::Error> {
        // Persist locally
        match Persister::<'_, M, T, ES, NS, SP, F, R, L, S>::persist_manager(
            &self.fs_persister,
            channel_manager,
        ) {
            Ok(_) => {}
            Err(e) => {
                error!("Failed to persist the ChannelManager in the filesystem.");
                // Returns an error therefor stopping the Background processor.
                // We could eventually ignore this error as we do for errors in remote persistence,
                // but an error in local filesystem persistence indicates the existence
                // of a more permanent issue.
                return Err(e);
            }
        };

        // Persist remotely
        let encrypted_data = match encrypt(&channel_manager.encode(), &self.encryption_key) {
            Ok(data) => data,
            Err(e) => {
                error!("Failed to encrypt the ChannelManager: {}", e.to_string());
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to encrypt the ChannelManager: {e}"),
                ));
            }
        };

        let persisting_result = self.storage.put_object(
            OBJECTS_BUCKET.to_string(),
            MANAGER_KEY.to_string(),
            encrypted_data,
        );

        if let Err(e) = persisting_result {
            // We ignore errors on persisting the channel manager remotely hoping that it
            // will succeed next time and meanwhile the user will not try to
            // recover the wallet from an outdated backup (which will result in
            // force close for some new channels).

            error!(
                "Error on remotely persisting channel manager. Ignoring. Error: {}",
                e.to_string()
            );
        }

        Ok(())
    }

    fn persist_graph(
        &self,
        network_graph: &lightning::routing::gossip::NetworkGraph<L>,
    ) -> std::result::Result<(), std::io::Error> {
        Persister::<'_, M, T, ES, NS, SP, F, R, L, S>::persist_graph(
            &self.fs_persister,
            network_graph,
        )
    }

    fn persist_scorer(&self, scorer: &S) -> std::result::Result<(), std::io::Error> {
        Persister::<'_, M, T, ES, NS, SP, F, R, L, S>::persist_scorer(&self.fs_persister, scorer)
    }
}

pub(crate) fn has_local_install(local_persistence_path: &str) -> bool {
    has_local_channel_manager(local_persistence_path)
        || has_local_channel_monitors(local_persistence_path)
}

fn has_local_channel_manager(local_persistence_path: &str) -> bool {
    fs::File::open(get_local_channel_manager_path(local_persistence_path)).is_ok()
}

fn has_local_channel_monitors(local_persistence_path: &str) -> bool {
    let channel_monitors_dir_path = get_local_channel_monitors_dir_path(local_persistence_path);
    match fs::read_dir(channel_monitors_dir_path) {
        Ok(mut dir_entries) => dir_entries.next().is_some(),
        Err(_) => false,
    }
}

fn get_local_channel_manager_path(local_persistence_path: &str) -> PathBuf {
    PathBuf::from(local_persistence_path).join(Path::new(MANAGER_KEY))
}

fn get_local_channel_monitors_dir_path(local_persistence_path: &str) -> PathBuf {
    PathBuf::from(local_persistence_path).join(Path::new(MONITORS_BUCKET))
}

#[cfg(test)]
mod tests {
    use crate::errors::Error;
    use crate::storage_persister::StoragePersister;
    use bitcoin::BlockHash;
    use lightning::chain::channelmonitor::ChannelMonitor;
    use lightning::chain::keysinterface::{InMemorySigner, KeysManager};
    use lightning::util::ser::ReadableArgs;
    use std::fs;
    use std::io::Cursor;
    use std::sync::Arc;

    const MONITOR_1_STATE_1_PATH: &str = "tests/resources/monitors/state_1/1c4eb9c1d721ae7616ee480a735d175818510326701695ad4163aa00c6a320dd_0";
    const MONITOR_1_STATE_2_PATH: &str = "tests/resources/monitors/state_2/1c4eb9c1d721ae7616ee480a735d175818510326701695ad4163aa00c6a320dd_0";
    const MONITOR_1_STATE_3_PATH: &str = "tests/resources/monitors/state_3/1c4eb9c1d721ae7616ee480a735d175818510326701695ad4163aa00c6a320dd_0";

    const MONITOR_2_STATE_1_PATH: &str = "tests/resources/monitors/state_1/e22f85d8e341ba2f0e9d069e6e9585a062b817553f4738264e3019f174b95612_0";
    const MONITOR_2_STATE_2_PATH: &str = "tests/resources/monitors/state_2/e22f85d8e341ba2f0e9d069e6e9585a062b817553f4738264e3019f174b95612_0";
    const MONITOR_2_STATE_3_PATH: &str = "tests/resources/monitors/state_3/e22f85d8e341ba2f0e9d069e6e9585a062b817553f4738264e3019f174b95612_0";

    const MONITOR_3_STATE_3_PATH: &str = "tests/resources/monitors/state_3/aa7e2798a1cdca1e0700b2e5f07fe82002b6d084a9758553c8310b9f702ed61f_0";

    #[test]
    fn fresh_start() {
        let mut local_monitors = Vec::new();
        let mut remote_monitors = Vec::new();

        StoragePersister::verify_local_state_is_latest_state::<Arc<KeysManager>>(
            &mut local_monitors,
            &mut remote_monitors,
        )
        .unwrap();
    }

    #[test]
    fn normal_start() {
        let mut local_monitors = Vec::new();
        local_monitors.push(read_channel_monitor(MONITOR_2_STATE_1_PATH));
        local_monitors.push(read_channel_monitor(MONITOR_1_STATE_1_PATH));

        let mut remote_monitors = Vec::new();
        remote_monitors.push(read_channel_monitor(MONITOR_1_STATE_1_PATH));
        remote_monitors.push(read_channel_monitor(MONITOR_2_STATE_1_PATH));

        StoragePersister::verify_local_state_is_latest_state::<Arc<KeysManager>>(
            &mut local_monitors,
            &mut remote_monitors,
        )
        .unwrap();
    }

    #[test]
    fn recovery_start() {
        let mut local_monitors = Vec::new();

        let mut remote_monitors = Vec::new();
        remote_monitors.push(read_channel_monitor(MONITOR_1_STATE_1_PATH));
        remote_monitors.push(read_channel_monitor(MONITOR_2_STATE_1_PATH));

        let result = StoragePersister::verify_local_state_is_latest_state::<Arc<KeysManager>>(
            &mut local_monitors,
            &mut remote_monitors,
        );

        assert!(matches!(result, Err(perro::Error::InvalidInput { .. })));
    }

    #[test]
    fn local_knows_about_1_more_channel() {
        let mut local_monitors = Vec::new();
        local_monitors.push(read_channel_monitor(MONITOR_1_STATE_3_PATH));
        local_monitors.push(read_channel_monitor(MONITOR_3_STATE_3_PATH));
        local_monitors.push(read_channel_monitor(MONITOR_2_STATE_3_PATH));

        let mut remote_monitors = Vec::new();
        remote_monitors.push(read_channel_monitor(MONITOR_1_STATE_2_PATH));
        remote_monitors.push(read_channel_monitor(MONITOR_2_STATE_2_PATH));

        StoragePersister::verify_local_state_is_latest_state::<Arc<KeysManager>>(
            &mut local_monitors,
            &mut remote_monitors,
        )
        .unwrap();
    }

    #[test]
    fn remote_knows_about_1_more_channel() {
        let mut local_monitors = Vec::new();
        local_monitors.push(read_channel_monitor(MONITOR_1_STATE_2_PATH));
        local_monitors.push(read_channel_monitor(MONITOR_2_STATE_2_PATH));

        let mut remote_monitors = Vec::new();
        remote_monitors.push(read_channel_monitor(MONITOR_1_STATE_3_PATH));
        remote_monitors.push(read_channel_monitor(MONITOR_3_STATE_3_PATH));
        remote_monitors.push(read_channel_monitor(MONITOR_2_STATE_3_PATH));

        let result = StoragePersister::verify_local_state_is_latest_state::<Arc<KeysManager>>(
            &mut local_monitors,
            &mut remote_monitors,
        );

        assert!(matches!(result, Err(Error::PermanentFailure { .. })))
    }

    #[test]
    fn local_is_more_recent() {
        let mut local_monitors = Vec::new();
        local_monitors.push(read_channel_monitor(MONITOR_2_STATE_2_PATH));
        local_monitors.push(read_channel_monitor(MONITOR_1_STATE_2_PATH));

        let mut remote_monitors = Vec::new();
        remote_monitors.push(read_channel_monitor(MONITOR_1_STATE_1_PATH));
        remote_monitors.push(read_channel_monitor(MONITOR_2_STATE_1_PATH));

        StoragePersister::verify_local_state_is_latest_state::<Arc<KeysManager>>(
            &mut local_monitors,
            &mut remote_monitors,
        )
        .unwrap();
    }

    #[test]
    fn remote_is_more_recent() {
        let mut local_monitors = Vec::new();
        local_monitors.push(read_channel_monitor(MONITOR_2_STATE_1_PATH));
        local_monitors.push(read_channel_monitor(MONITOR_1_STATE_1_PATH));

        let mut remote_monitors = Vec::new();
        remote_monitors.push(read_channel_monitor(MONITOR_1_STATE_2_PATH));
        remote_monitors.push(read_channel_monitor(MONITOR_2_STATE_2_PATH));

        let result = StoragePersister::verify_local_state_is_latest_state::<Arc<KeysManager>>(
            &mut local_monitors,
            &mut remote_monitors,
        );

        assert!(matches!(result, Err(Error::PermanentFailure { .. })))
    }

    fn read_channel_monitor(path: &str) -> (BlockHash, ChannelMonitor<InMemorySigner>) {
        let keys_manager = KeysManager::new(&[0u8; 32], 0, 0);
        let data = fs::read(path).unwrap();
        let mut buffer = Cursor::new(&data);
        <(BlockHash, ChannelMonitor<InMemorySigner>)>::read(
            &mut buffer,
            (&keys_manager, &keys_manager),
        )
        .unwrap()
    }

    #[test]
    fn check_instance() {
        use std::time::{Duration, Instant};

        let now = Instant::now();
        let duration = Duration::from_secs(1);
        std::thread::sleep(duration);
        let after = Instant::now();
        eprintln!("moment {now:?} after {duration:?} is {after:?}");

        now.checked_sub(Duration::from_millis(1)).unwrap();
        now.checked_sub(Duration::from_millis(10)).unwrap();
        now.checked_sub(Duration::from_millis(100)).unwrap();
        now.checked_sub(Duration::from_secs(1)).unwrap();
        now.checked_sub(Duration::from_secs(10)).unwrap();
        now.checked_sub(Duration::from_secs(60)).unwrap();
        now.checked_sub(Duration::from_secs(10 * 60)).unwrap();
        now.checked_sub(Duration::from_secs(30 * 60)).unwrap();
        now.checked_sub(Duration::from_secs(60 * 60)).unwrap();
        now.checked_sub(Duration::from_secs(2 * 60 * 60)).unwrap();
        now.checked_sub(Duration::from_secs(4 * 60 * 60)).unwrap();
        now.checked_sub(Duration::from_secs(8 * 60 * 60)).unwrap();
        now.checked_sub(Duration::from_secs(12 * 60 * 60)).unwrap();
        now.checked_sub(Duration::from_secs(24 * 60 * 60)).unwrap();
    }

    #[test]
    fn debug_panic() {
        use std::time::SystemTime;
        use std::time::{Duration, Instant};

        let duration_since_epoch = Duration::new(1684151216, 494_733_500);
        let wall_clock_now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap();
        let now = Instant::now();
        let _last_updated = if wall_clock_now > duration_since_epoch {
            now - (wall_clock_now - duration_since_epoch)
        } else {
            now
        };
    }
}
