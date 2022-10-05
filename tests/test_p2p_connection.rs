mod setup;

use std::env;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

// Caution: Run this test sequentially,
// otherwise they will corrupt eachother, because they're manipulating their environment:
// cargo test --test '*' -- --test-threads 1
#[cfg(test)]
mod p2p_connection_tests {
    use super::*;

    #[test]
    fn test_successful_p2p_connection() {
        let node = setup::setup().unwrap();

        assert_eq!(node.get_node_info().num_peers, 1)
    }

    #[test]
    fn test_failing_p2p_connection() {
        if env::var("LSP_NODE_PUB_KEY").is_err() {
            dotenv::from_path("examples/node/.env").unwrap();
        }

        let lsp_node_address = env::var("LSP_NODE_ADDRESS").unwrap();
        env::set_var("LSP_NODE_ADDRESS", "127.0.0.1:1337"); // nothing running on this port

        if setup::setup().is_ok() {
            panic!("Setup must fail because node cannot connect to LSP");
        }

        // Cleanup: Fix env variable for other tests
        env::set_var("LSP_NODE_ADDRESS", lsp_node_address);
    }

    #[test]
    fn test_flaky_p2p_connection() {
        let node = setup::setup().unwrap();
        assert_eq!(node.get_node_info().num_peers, 1);

        // Kill LSP node
        shutdown_nigiri();
        assert_eq!(node.get_node_info().num_peers, 0);

        // Todo test reconnecting (not implemented yet)
        start_nigiri();
        block_until_nigiri_ready();
    }

    fn start_nigiri() {
        Command::new("nigiri")
            .arg("start")
            .arg("--ln")
            .output()
            .expect("Failed to start Nigiri");
    }

    fn shutdown_nigiri() {
        Command::new("nigiri")
            .arg("stop")
            .output()
            .expect("Failed to shutdown Nigiri");
    }

    fn block_until_nigiri_ready() {
        while !is_nigiri_lnd_synced_to_chain() {
            sleep(Duration::from_millis(100));
        }
    }

    fn is_nigiri_lnd_synced_to_chain() -> bool {
        let lnd_getinfo_cmd = Command::new("nigiri")
            .arg("lnd")
            .arg("getinfo")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        let output = Command::new("jq")
            .arg(".synced_to_chain")
            .stdin(lnd_getinfo_cmd.stdout.unwrap())
            .output()
            .unwrap();

        output.stdout == b"true\n"
    }
}
