use crate::setup::mocked_remote_storage::MockedRemoteStorage;
use crate::setup::{mocked_remote_storage, NodeHandle};
use std::sync::Arc;
use storage_mock::Storage;

pub fn mocked_storage_node() -> NodeHandle<MockedRemoteStorage> {
    mocked_storage_node_configurable(mocked_remote_storage::Config::default())
}

pub fn mocked_storage_node_configurable(
    config: mocked_remote_storage::Config,
) -> NodeHandle<MockedRemoteStorage> {
    let storage = MockedRemoteStorage::new(Arc::new(Storage::new()), config);
    NodeHandle::new(storage)
}
