use eel::errors::{Result, RuntimeErrorCode};
use eel::interfaces::RemoteStorage;
use log::debug;
use perro::{invalid_input, MapToError};
use std::fmt::Debug;
use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub struct FileStorage {
    base_path_buf: PathBuf,
}

impl FileStorage {
    pub fn new(base_path: &str) -> Self {
        Self {
            base_path_buf: PathBuf::from(base_path),
        }
    }
}

impl RemoteStorage for FileStorage {
    fn get_object(&self, bucket: String, key: String) -> Result<Vec<u8>> {
        debug!("get_object({}, {})", bucket, key);
        let mut path_buf = self.base_path_buf.clone();
        path_buf.push(bucket);
        path_buf.push(key);

        if !path_buf.exists() {
            return Err(invalid_input(format!("Not found: {path_buf:?}")));
        }

        Ok(fs::read(path_buf).unwrap())
    }

    fn check_health(&self) -> bool {
        debug!("check_health()");
        true
    }

    fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> Result<()> {
        debug!("put_object({}, {}, value.len={})", bucket, key, value.len());
        let mut path_buf = self.base_path_buf.clone();
        path_buf.push(bucket);
        fs::create_dir_all(path_buf.clone()).unwrap();
        path_buf.push(key);
        fs::write(&path_buf, value).unwrap();
        Ok(())
    }

    fn list_objects(&self, bucket: String) -> Result<Vec<String>> {
        debug!("list_objects({})", bucket);
        let mut path_buf = self.base_path_buf.clone();
        path_buf.push(bucket);
        let list = if let Ok(res) = fs::read_dir(path_buf) {
            res.map(|res| res.map(|e| e.file_name().to_str().unwrap().to_string()))
                .collect::<std::result::Result<Vec<_>, io::Error>>()
                .unwrap()
        } else {
            Vec::new()
        };
        Ok(list)
    }

    fn delete_object(&self, bucket: String, key: String) -> Result<()> {
        debug!("delete_object({}, {})", bucket, key);
        let mut path_buf = self.base_path_buf.clone();
        path_buf.push(bucket);
        path_buf.push(key);
        fs::remove_file(path_buf).map_to_runtime_error(
            RuntimeErrorCode::RemoteStorageError,
            "Failed to delete object",
        )
    }
}
