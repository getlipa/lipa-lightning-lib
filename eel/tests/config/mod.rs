use bitcoin::Network;
use eel::config::{Config, TzConfig};
use eel::keys_manager::generate_secret;

pub const LOCAL_PERSISTENCE_PATH: &str = ".3l_local_test";

pub fn get_testing_config() -> Config {
    Config {
        network: Network::Regtest,
        seed: generate_secret("".to_string()).unwrap().get_seed_as_array(),
        esplora_api_url: "http://localhost:30000".to_string(),
        rgs_url: "http://localhost:8080/snapshot/".to_string(),
        lsp_url: "http://127.0.0.1:6666".to_string(),
        lsp_token: "iQUvOsdk4ognKshZB/CKN2vScksLhW8i13vTO+8SPvcyWJ+fHi8OLgUEvW1N3k2l".to_string(),
        local_persistence_path: LOCAL_PERSISTENCE_PATH.to_string(),
        timezone_config: TzConfig {
            timezone_id: String::from("int_test_timezone_id"),
            timezone_utc_offset_secs: 1234,
        },
    }
}
