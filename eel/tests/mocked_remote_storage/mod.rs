use eel::errors::{runtime_error, LipaError, LipaResult, RuntimeErrorCode};
use eel::interfaces::RemoteStorage;
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

    fn emulate_availability(&self) -> LipaResult<()> {
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

    fn list_objects(&self, bucket: String) -> LipaResult<Vec<String>> {
        self.emulate_availability()?;
        Ok(self.storage.list_objects(bucket))
    }

    fn object_exists(&self, bucket: String, key: String) -> LipaResult<bool> {
        self.emulate_availability()?;
        Ok(self.storage.object_exists(bucket, key))
    }

    fn get_object(&self, bucket: String, key: String) -> LipaResult<Vec<u8>> {
        self.emulate_availability()?;
        Ok(self.storage.get_object(bucket, key))
    }

    fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> LipaResult<()> {
        self.emulate_availability()?;
        emulate_reliability(self.config.put_availability_percent)?;
        self.storage.put_object(bucket, key, value);
        Ok(())
    }

    fn delete_object(&self, bucket: String, key: String) -> LipaResult<()> {
        self.emulate_availability()?;
        self.storage.delete_object(bucket, key);
        Ok(())
    }
}

fn emulate_reliability(reliability_percent: u8) -> LipaResult<()> {
    let random_value: u8 = rand::thread_rng().gen_range(0..101);
    if random_value > reliability_percent {
        return Err(get_emulated_error());
    }
    Ok(())
}

fn get_emulated_error() -> LipaError {
    runtime_error(
        RuntimeErrorCode::GenericError,
        "This is an emulated error, please try again",
    )
}
