use eel::errors::{LipaResult, MapToLipaError, RuntimeErrorCode};
use eel::interfaces::RemoteStorage;
use log::debug;
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
    fn object_exists(&self, bucket: String, key: String) -> LipaResult<bool> {
        debug!("object_exists({}, {})", bucket, key);
        let mut path_buf = self.base_path_buf.clone();
        path_buf.push(bucket);
        path_buf.push(key);
        Ok(path_buf.exists())
    }

    fn get_object(&self, bucket: String, key: String) -> LipaResult<Vec<u8>> {
        debug!("get_object({}, {})", bucket, key);
        let mut path_buf = self.base_path_buf.clone();
        path_buf.push(bucket);
        path_buf.push(key);
        Ok(fs::read(path_buf).unwrap())
    }

    fn check_health(&self) -> bool {
        debug!("check_health()");
        true
    }

    fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> LipaResult<()> {
        debug!("put_object({}, {}, value.len={})", bucket, key, value.len());
        let mut path_buf = self.base_path_buf.clone();
        path_buf.push(bucket);
        fs::create_dir_all(path_buf.clone()).unwrap();
        path_buf.push(key);
        fs::write(&path_buf, value).unwrap();
        Ok(())
    }

    fn list_objects(&self, bucket: String) -> LipaResult<Vec<String>> {
        debug!("list_objects({})", bucket);
        let mut path_buf = self.base_path_buf.clone();
        path_buf.push(bucket);
        let list = if let Ok(res) = fs::read_dir(path_buf) {
            res.map(|res| res.map(|e| e.file_name().to_str().unwrap().to_string()))
                .collect::<Result<Vec<_>, io::Error>>()
                .unwrap()
        } else {
            Vec::new()
        };
        Ok(list)
    }

    fn delete_object(&self, bucket: String, key: String) -> LipaResult<()> {
        debug!("delete_object({}, {})", bucket, key);
        let mut path_buf = self.base_path_buf.clone();
        path_buf.push(bucket);
        path_buf.push(key);
        fs::remove_file(path_buf)
            .map_to_runtime_error(RuntimeErrorCode::GenericError, "Failed to delete object")
    }
}
