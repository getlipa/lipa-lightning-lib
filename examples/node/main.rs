mod cli;
mod hex_utils;

use bitcoin::Network;
use rand::{thread_rng, Rng};
use simplelog;
use std::fmt::{Debug, Formatter};
use std::fs;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use uniffi_lipalightninglib::callbacks::PersistCallback;
use uniffi_lipalightninglib::config::LipaLightningConfig;
use uniffi_lipalightninglib::LipaLightning;

#[cfg(target_os = "windows")]
extern crate winapi;

struct RustPersistCallback {}

impl Debug for RustPersistCallback {
    fn fmt(&self, _f: &mut Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

impl PersistCallback for RustPersistCallback {
    fn exists(&self, path: String) -> bool {
        Path::new(&(".ldk/".to_string() + &*path)).exists()
    }

    fn read_dir(&self, path: String) -> Vec<String> {
        let mut dir_entries = Vec::new();
        if !self.exists(path.clone()) {
            return dir_entries;
        }
        let mut pathbuf = PathBuf::from(".ldk");
        pathbuf.push(path);
        for file_option in fs::read_dir(pathbuf).unwrap() {
            let file = file_option.unwrap();
            let owned_file_name = file.file_name();
            let filename = owned_file_name.to_str().unwrap();
            dir_entries.push(filename.to_string());
        }
        dir_entries
    }

    fn write_to_file(&self, path: String, data: Vec<u8>) -> bool {
        let mut dest_file = PathBuf::from(".ldk");
        dest_file.push(path);

        let mut tmp_file = dest_file.clone();
        tmp_file.set_extension("tmp");

        let parent_directory = tmp_file.parent().unwrap();
        fs::create_dir_all(parent_directory).unwrap();
        {
            // Note that going by rust-lang/rust@d602a6b, on MacOS it is only safe to use
            // rust stdlib 1.36 or higher.
            /*let mut buf = BufWriter::new(fs::File::create(&tmp_file).unwrap());
            data.write(&mut buf).unwrap();
            buf.into_inner().unwrap().sync_all().unwrap();*/
            fs::write(&tmp_file, data).unwrap();
        }
        // Fsync the parent directory on Unix.
        #[cfg(not(target_os = "windows"))]
        {
            fs::rename(&tmp_file, &dest_file).unwrap();
            let dir_file = fs::OpenOptions::new()
                .read(true)
                .open(parent_directory)
                .unwrap();
            unsafe {
                libc::fsync(dir_file.as_raw_fd());
            }
        };
        #[cfg(target_os = "windows")]
        {
            if dest_file.exists() {
                unsafe {
                    winapi::um::winbase::ReplaceFileW(
                        path_to_windows_str(dest_file).as_ptr(),
                        path_to_windows_str(tmp_file).as_ptr(),
                        std::ptr::null(),
                        winapi::um::winbase::REPLACEFILE_IGNORE_MERGE_ERRORS,
                        std::ptr::null_mut() as *mut winapi::ctypes::c_void,
                        std::ptr::null_mut() as *mut winapi::ctypes::c_void,
                    )
                };
            } else {
                call!(unsafe {
                    winapi::um::winbase::MoveFileExW(
                        path_to_windows_str(tmp_file).as_ptr(),
                        path_to_windows_str(dest_file).as_ptr(),
                        winapi::um::winbase::MOVEFILE_WRITE_THROUGH
                            | winapi::um::winbase::MOVEFILE_REPLACE_EXISTING,
                    )
                });
            }
        }

        true
    }

    fn read(&self, path: String) -> Vec<u8> {
        let mut pathbuf = PathBuf::from(".ldk");
        pathbuf.push(path);

        fs::read(pathbuf).unwrap()
    }
}

fn main() {
    // Start nigiri (needs docker to be running)
    /*Command::new("nigiri")
    .arg("start")
    .arg("--ln")
    .status()
    .expect("Failed to start nigiri.");*/

    // Create dir for node data persistence
    fs::create_dir_all(".ldk").unwrap();

    init_logger();

    let persist_callback = Box::new(RustPersistCallback {});

    // Create random seed and persist to disk or read from disk if previously created
    let seed = if !persist_callback.exists("keys_seed".to_string()) {
        let mut seed = [0; 32];
        thread_rng().fill_bytes(&mut seed);
        persist_callback.write_to_file("keys_seed".to_string(), seed.clone().to_vec());
        seed
    } else {
        let seed_vec = persist_callback.read("keys_seed".to_string());
        <[u8; 32]>::try_from(&*seed_vec).unwrap()
    };

    // Create Lightning config
    let config = LipaLightningConfig {
        /*seed: Vec::from(seed),
        electrum_url: "localhost".to_string(),
        ldk_peer_listening_port: 9732,
        network: Network::Regtest,*/
        seed: Vec::from(seed),
        electrum_port: 50000,
        electrum_host: "localhost".to_string(),
        ldk_peer_listening_port: 9732,
        network: Network::Regtest,
    };

    let lipa_lightning = Arc::new(LipaLightning::new(config, persist_callback));

    sleep(Duration::from_secs(1));

    // Lauch CLI
    cli::poll_for_user_input(&lipa_lightning);

    // Stop 3L
    println!("Shutting down node by calling stop()");
    lipa_lightning.stop();

    // Stop nigiri & delete data
    /*Command::new("nigiri")
    .arg("stop")
    .arg("--delete")
    .status()
    .expect("Failed to stop nigiri.");*/
}

fn init_logger() {
    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(".ldk/logs.txt")
        .unwrap();
    simplelog::CombinedLogger::init(vec![
        simplelog::TermLogger::new(
            log::LevelFilter::Warn,
            simplelog::Config::default(),
            simplelog::TerminalMode::Mixed,
            simplelog::ColorChoice::Auto,
        ),
        simplelog::WriteLogger::new(
            log::LevelFilter::Trace,
            simplelog::Config::default(),
            log_file,
        ),
    ])
    .unwrap();
}
