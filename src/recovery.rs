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
    // With the use of Breez SDK, at least for now, we don't need a specific recovery function.
    // The consumer can simply construct a LightningNode and, assuming the seed has been used before,
    // the funds will be immediately available.
    // We might need this method if/when we need to recover some info not provided by the SDK (e.g.
    // payment history)
    // TODO: consider removing this method
    Ok(())
}
