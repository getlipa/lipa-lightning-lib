use eel::errors::{Error, Result, RuntimeErrorCode};
use eel::interfaces::RemoteStorage;
use perro::runtime_error;
use rand::Rng;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use storage_mock::Storage;

#[derive(Debug, Clone)]
pub struct Config {
    pub delay: Option<Duration>,
    pub available: bool,
    pub put_availability_percent: u8,
}

impl Config {
    pub fn new(delay: Option<Duration>, available: bool, put_availability_percent: u8) -> Self {
        Self {
            delay,
            available,
            put_availability_percent,
        }
    }

    pub fn default() -> Self {
        Self::new(None, true, 100)
    }
}

#[derive(Debug, Clone)]
pub struct MockedRemoteStorage {
    storage: Arc<Storage>,
    config: Config,
}

impl MockedRemoteStorage {
    pub fn new(storage: Arc<Storage>, config: Config) -> Self {
        Self { storage, config }
    }
}

impl Default for MockedRemoteStorage {
    fn default() -> Self {
        Self::new(
            Arc::new(Storage::new()),
            Config {
                delay: None,
                available: true,
                put_availability_percent: 100,
            },
        )
    }
}

#[allow(dead_code)]
impl MockedRemoteStorage {
    pub fn enable(&mut self) {
        self.config.available = true;
    }

    pub fn disable(&mut self) {
        self.config.available = false;
    }

    fn emulate_availability(&self) -> Result<()> {
        self.wait_delay();

        if !self.config.available {
            return Err(get_emulated_error());
        }
        Ok(())
    }

    fn wait_delay(&self) {
        if let Some(delay) = self.config.delay {
            sleep(delay);
        }
    }
}

impl RemoteStorage for MockedRemoteStorage {
    fn check_health(&self) -> bool {
        if self.emulate_availability().is_err() {
            return false;
        }
        self.storage.check_health()
    }

    fn list_objects(&self, bucket: String) -> Result<Vec<String>> {
        self.emulate_availability()?;
        Ok(self.storage.list_objects(bucket))
    }

    fn get_object(&self, bucket: String, key: String) -> Result<Vec<u8>> {
        self.emulate_availability()?;
        match self.storage.get_object(bucket, key) {
            Some(value) => Ok(value),
            None => Err(runtime_error(
                RuntimeErrorCode::ObjectNotFound,
                "Could not read object '{key}' from bucket '{bucket}'",
            )),
        }
    }

    fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> Result<()> {
        self.emulate_availability()?;
        emulate_reliability(self.config.put_availability_percent)?;
        self.storage.put_object(bucket, key, value);
        Ok(())
    }

    fn delete_object(&self, bucket: String, key: String) -> Result<()> {
        self.emulate_availability()?;
        self.storage.delete_object(bucket, key);
        Ok(())
    }
}

fn emulate_reliability(reliability_percent: u8) -> Result<()> {
    let random_value: u8 = rand::thread_rng().gen_range(0..101);
    if random_value > reliability_percent {
        return Err(get_emulated_error());
    }
    Ok(())
}

fn get_emulated_error() -> Error {
    runtime_error(
        RuntimeErrorCode::GenericError,
        "This is an emulated error, please try again",
    )
}
