use crate::eel_interface_impl::RemoteStorageGraphql;
use crate::environment::Environment;
use crate::errors::{Result, RuntimeErrorCode};
use crate::logger::init_logger_once;
use crate::{build_auth, enable_backtrace, sanitize_input, EnvironmentCode, LOGS_DIR, LOG_LEVEL};
use perro::{MapToError, ResultTrait};
use std::fs;
use std::path::Path;
use std::sync::Arc;

pub fn recover_lightning_node(
    environment: EnvironmentCode,
    seed: Vec<u8>,
    local_persistence_path: String,
    enable_file_logging: bool,
) -> Result<()> {
    enable_backtrace();
    fs::create_dir_all(&local_persistence_path).map_to_permanent_failure(format!(
        "Failed to create directory: {}",
        local_persistence_path,
    ))?;
    if enable_file_logging {
        init_logger_once(
            LOG_LEVEL,
            &Path::new(&local_persistence_path).join(LOGS_DIR),
        )?;
    }

    let seed = sanitize_input::strong_type_seed(&seed)
        .map_runtime_error_using(RuntimeErrorCode::from_eel_runtime_error_code)?;

    let environment = Environment::load(environment);

    let auth = Arc::new(
        build_auth(&seed, environment.backend_url.clone())
            .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)?,
    );

    let remote_storage = Box::new(RemoteStorageGraphql::new(
        environment.backend_url,
        environment.backend_health_url,
        auth,
    ));

    eel::recovery::recover_lightning_node(seed, local_persistence_path, remote_storage)
        .map_runtime_error_using(RuntimeErrorCode::from_eel_runtime_error_code)
}
