mod setup;

use std::env;

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
        setup::shutdown_nigiri();
        assert_eq!(node.get_node_info().num_peers, 0);

        // Todo test reconnecting (not implemented yet)
        setup::start_nigiri();
    }
}
