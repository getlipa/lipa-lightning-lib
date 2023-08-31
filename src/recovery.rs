use crate::errors::Result;
use crate::EnvironmentCode;

// TODO remove unused_variables after breez sdk implementation
#[allow(unused_variables)]
pub fn recover_lightning_node(
    environment: EnvironmentCode,
    seed: Vec<u8>,
    local_persistence_path: String,
    enable_file_logging: bool,
) -> Result<()> {
    todo!()
}
