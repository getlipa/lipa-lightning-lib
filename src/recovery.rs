use crate::eel_interface_impl::RemoteStorageGraphql;
use crate::environment::Environment;
use crate::{build_auth, enable_backtrace, sanitize_input, EnvironmentCode};
use eel::errors::Result;
use perro::MapToError;
use std::fs;
use std::sync::Arc;

pub fn recover_lightning_node(
    environment: EnvironmentCode,
    seed: Vec<u8>,
    local_persistence_path: String,
) -> Result<()> {
    enable_backtrace();
    fs::create_dir_all(&local_persistence_path).map_to_permanent_failure(format!(
        "Failed to create directory: {}",
        local_persistence_path,
    ))?;

    let seed = sanitize_input::strong_type_seed(&seed)?;

    let environment = Environment::load(environment);

    let auth = Arc::new(build_auth(&seed, environment.backend_url.clone())?);

    let remote_storage = Box::new(RemoteStorageGraphql::new(
        environment.backend_url,
        environment.backend_health_url,
        auth,
    ));

    eel::recovery::recover_lightning_node(seed, local_persistence_path, remote_storage)
}
