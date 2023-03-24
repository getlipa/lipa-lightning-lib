use crate::eel_interface_impl::RemoteStorageGraphql;
use crate::{build_auth, sanitize_input};
use eel::errors::Result;
use perro::MapToError;
use std::fs;
use std::sync::Arc;

pub fn recover_lightning_node(
    seed: Vec<u8>,
    local_persistence_path: String,
    graphql_url: String,
    backend_health_url: String,
) -> Result<()> {
    fs::create_dir_all(&local_persistence_path).map_to_permanent_failure(format!(
        "Failed to create directory: {}",
        local_persistence_path,
    ))?;

    let seed = sanitize_input::strong_type_seed(&seed)?;

    let auth = Arc::new(build_auth(&seed, graphql_url.clone())?);

    let remote_storage = Box::new(RemoteStorageGraphql::new(
        graphql_url,
        backend_health_url,
        auth,
    )?);

    eel::recovery::recover_lightning_node(seed, local_persistence_path, remote_storage)
}
