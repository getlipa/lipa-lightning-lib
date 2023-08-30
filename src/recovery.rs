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
    todo!()
}
