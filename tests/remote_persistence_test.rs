#[path = "print_events_handler/mod.rs"]
mod print_events_handler;
#[path = "../eel/tests/setup/mod.rs"]
mod setup;
mod setup_3l;

#[cfg(feature = "nigiri")]
mod node_info_test {
    use crate::setup::nigiri;
    use crate::setup::nigiri::NodeInstance;
    use crate::setup_3l::NodeHandle;
    use crate::try_cmd_repeatedly;

    use bitcoin::hashes::hex::ToHex;
    use serial_test::file_serial;
    use std::process::{Command, Output};
    use std::thread::sleep;
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
            let node_id = node_info.node_pubkey.to_hex();
            assert_eq!(node_info.num_peers, 1);

            // open 2 channels
            nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &node_id, false).unwrap();
            nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &node_id, false).unwrap();
            try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);

            nigiri::wait_for_new_channel_to_confirm(NodeInstance::LspdLnd, &node_id);

            log::info!("Shutting down the node to trigger persistence flow...");
        } // Shut down the node

        // Contains 2 files, one for each channel, with the channel id as the file name
        let monitor_dir_contents = get_monitors_dir_contents().stdout;

        NodeHandle::reset_state();
        assert!(!get_monitors_dir_contents().status.success()); // prove monitor files are gone

        // Recover node from remote persistence
        let node = node_handle.start().unwrap();

        // prove monitor files are back for the same channels (file names = same channel ids)
        assert_eq!(monitor_dir_contents, get_monitors_dir_contents().stdout);

        assert_eq!(node.get_node_info().channels_info.num_channels, 2);
    }

    fn get_monitors_dir_contents() -> Output {
        Command::new("ls")
            .args([".3l_local_test/monitors"])
            .output()
            .expect("Failed to execute process")
    }
}
