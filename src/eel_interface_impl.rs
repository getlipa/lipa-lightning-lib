use eel::errors::{Result, RuntimeErrorCode};
use eel::interfaces::{EventHandler, RemoteStorage};
use eel::MapToError;

use honey_badger::Auth;
use mole::ChannelStatePersistenceClient;
use perro::runtime_error;
use std::sync::Arc;

const MONITORS_BUCKET: &str = "monitors";
const OBJECTS_BUCKET: &str = "objects";
const MANAGER_KEY: &str = "manager";

pub(crate) struct RemoteStorageGraphql {
    remote_csp_client: ChannelStatePersistenceClient,
}

impl RemoteStorageGraphql {
    pub fn new(backend_url: String, backend_health_url: String, auth: Arc<Auth>) -> Result<Self> {
        Ok(Self {
            remote_csp_client: ChannelStatePersistenceClient::new(
                backend_url,
                backend_health_url,
                auth,
            ),
        })
    }
}

impl RemoteStorage for RemoteStorageGraphql {
    fn check_health(&self) -> bool {
        self.remote_csp_client.check_health()
    }

    fn list_objects(&self, bucket: String) -> Result<Vec<String>> {
        match bucket.as_str() {
            MONITORS_BUCKET => self.remote_csp_client.get_channel_monitor_ids().map_to_runtime_error(
                RuntimeErrorCode::RemoteStorageError,
                "Failed to read list of channel monitors from remote storage....."),
            OBJECTS_BUCKET => unimplemented!("List objects does not have any purpose for the manager for now, as there is only one manager and we want to fetch that one."),
            _ => Err(runtime_error(
                RuntimeErrorCode::RemoteStorageError,
                format!("Retrieving data of type {bucket} from remote storage is not supported."),
            )),
        }
    }

    fn get_object(&self, bucket: String, key: String) -> Result<Vec<u8>> {
        match bucket.as_str() {
            MONITORS_BUCKET => self
                .remote_csp_client
                .read_channel_monitor(&key)
                .map_to_runtime_error(
                    RuntimeErrorCode::RemoteStorageError,
                    "Failed read channel monitor from remote storage.",
                ),
            OBJECTS_BUCKET => self
                .remote_csp_client
                .read_channel_manager()
                .map_to_runtime_error(
                    RuntimeErrorCode::RemoteStorageError,
                    "Failed to read channel manager from remote storage.",
                ),
            _ => unimplemented!(
                "Retrieving data of type {bucket} from remote storage is not supported."
            ),
        }
    }

    fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> Result<()> {
        match bucket.as_str() {
            MONITORS_BUCKET => self
                .remote_csp_client
                .write_channel_monitor(
                    &key,
                    &value,
                    env!("CARGO_PKG_VERSION"), // Version of 3L
                    &Vec::<u8>::new(),         // Field may be used in the future
                )
                .map_to_runtime_error(
                    RuntimeErrorCode::RemoteStorageError,
                    "Failed to write channel manager to remote storage.",
                ),
            OBJECTS_BUCKET => {
                if key == MANAGER_KEY {
                    self.remote_csp_client
                        .write_channel_manager(&value)
                        .map_to_runtime_error(
                            RuntimeErrorCode::RemoteStorageError,
                            "Failed to write channel manager to remote storage.",
                        )
                } else {
                    unimplemented!("Storing arbitrary {OBJECTS_BUCKET} is not yet supported!");
                }
            }
            _ => {
                unimplemented!("Storing data of type {bucket} to remote storage is not supported.")
            }
        }
    }

    fn delete_object(&self, _bucket: String, _key: String) -> Result<()> {
        unimplemented!("Deleting objects is not yet supported!");
    }
}

pub(crate) struct EventsImpl {
    pub events_callback: Box<dyn crate::callbacks::EventsCallback>,
}

impl EventHandler for EventsImpl {
    fn payment_received(&self, payment_hash: String, amount_msat: u64) {
        self.events_callback
            .payment_received(payment_hash, amount_msat);
    }

    fn payment_sent(&self, payment_hash: String, payment_preimage: String, fee_paid_msat: u64) {
        self.events_callback
            .payment_sent(payment_hash, payment_preimage, fee_paid_msat);
    }

    fn payment_failed(&self, payment_hash: String) {
        self.events_callback.payment_failed(payment_hash);
    }

    fn channel_closed(&self, channel_id: String, reason: String) {
        self.events_callback.channel_closed(channel_id, reason);
    }
}
