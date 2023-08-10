#[path = "print_events_handler/mod.rs"]
mod print_events_handler;
mod setup_3l;
#[path = "../eel/tests/setup_env/mod.rs"]
mod setup_env;

#[cfg(feature = "nigiri")]
mod remote_persistence_test {
    use crate::setup_3l::NodeHandle;
    use crate::setup_env::nigiri;
    use crate::setup_env::nigiri::NodeInstance;
    use crate::{try_cmd_repeatedly, wait_for, wait_for_eq};

    use bitcoin::hashes::hex::ToHex;
    use bitcoin::secp256k1::PublicKey;
    use serial_test::file_serial;
    use std::process::{Command, Output};
    use std::str::FromStr;
    use std::time::Duration;

    const HALF_SEC: Duration = Duration::from_millis(500);
    const N_RETRIES: u8 = 10;

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_remote_persistence() {
        nigiri::setup_environment_with_lsp();

        let node_handle = NodeHandle::new();

        {
            let node = node_handle.start().unwrap();
            let node_info = node.get_node_info();
            let node_id = PublicKey::from_str(&node_info.node_pubkey)
                .unwrap()
                .to_hex();
            assert_eq!(node_info.peers.len(), 1);

            // open 2 channels
            nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &node_id, false).unwrap();
            nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &node_id, false).unwrap();
            try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);

            wait_for!(nigiri::is_channel_confirmed(
                NodeInstance::LspdLnd,
                &node_id
            ));

            log::info!("Shutting down the node to trigger persistence flow...");
        } // Shut down the node

        // Contains 2 files, one for each channel, with the channel id as the file name
        let original_monitor_dir = get_monitors_dir_content();

        NodeHandle::reset_state();
        assert!(!monitors_dir_exists()); // prove monitor files are gone

        // Recover node from remote persistence
        node_handle.recover().unwrap();

        let node = node_handle.start().unwrap();

        // prove monitor files are back for the same channels (file names = same channel ids)
        wait_for_eq!(original_monitor_dir, get_monitors_dir_content());

        assert_eq!(node.get_node_info().channels_info.num_channels, 2);
    }

    fn read_monitors_dir() -> Output {
        Command::new("ls")
            .args([".3l_local_test/monitors"])
            .output()
            .expect("Failed to execute process")
    }

    fn get_monitors_dir_content() -> String {
        String::from_utf8(read_monitors_dir().stdout).unwrap()
    }

    fn monitors_dir_exists() -> bool {
        read_monitors_dir().status.success()
    }
}
