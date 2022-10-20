use crate::errors::InitializationError;

use core::future::Future;
use tokio::runtime::{Builder, Runtime};
use tokio::task::JoinHandle;
use tokio::time;
use tokio::time::Duration;

pub struct AsyncRuntime {
    rt: Runtime,
}

pub struct Handle {
    handle: tokio::runtime::Handle,
}

impl AsyncRuntime {
    #[allow(clippy::result_large_err)]
    pub fn new() -> Result<Self, InitializationError> {
        let rt = Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("3l-async-runtime")
            .enable_time()
            .enable_io()
            .build()
            .map_err(|e| InitializationError::AsyncRuntime {
                message: e.to_string(),
            })?;
        Ok(Self { rt })
    }

    pub fn handle(&self) -> Handle {
        let handle = self.rt.handle().clone();
        Handle { handle }
    }
}

#[allow(dead_code)]
impl Handle {
    pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.handle.spawn(future)
    }

    pub fn spawn_repeating_task<Func, F>(&self, interval: Duration, func: Func) -> JoinHandle<()>
    where
        Func: Fn() -> F + Send + Sync + 'static,
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.handle.spawn(async move {
            let mut interval = time::interval(interval);
            interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
            interval.tick().await;
            loop {
                func().await;
                interval.tick().await;
            }
        })
    }

    pub fn block_on<F: Future>(&self, future: F) -> F::Output {
        self.handle.block_on(future)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread::yield_now;
    use tokio::time::sleep;

    #[test]
    pub fn test_new_runtime_construction() {
        AsyncRuntime::new().unwrap();
    }

    #[test]
    pub fn test_spawn() {
        let rt = AsyncRuntime::new().unwrap();
        let handle = rt.handle();
        let data = Arc::new(AtomicUsize::new(0));
        let data_in_spawn = Arc::clone(&data);

        let _handle = handle.spawn(async move {
            data_in_spawn.store(1, Ordering::SeqCst);
            sleep(Duration::from_secs(10)).await;
            data_in_spawn.store(2, Ordering::SeqCst);
        });

        while data.load(Ordering::SeqCst) == 0 {
            yield_now();
        }
        assert_eq!(data.load(Ordering::SeqCst), 1);
    }

    #[test]
    pub fn test_block_on() {
        let rt = AsyncRuntime::new().unwrap();
        let handle = rt.handle();
        let data = Arc::new(AtomicUsize::new(0));
        let data_in_spawn = Arc::clone(&data);

        let result = handle.block_on(async move {
            sleep(Duration::from_millis(1)).await;
            data_in_spawn.store(1, Ordering::SeqCst);
            100
        });
        assert_eq!(result, 100);
        assert_eq!(data.load(Ordering::SeqCst), 1);
    }

    #[test]
    pub fn test_spawn_repeating_task() {
        let rt = AsyncRuntime::new().unwrap();
        let handle = rt.handle();
        let data = Arc::new(AtomicUsize::new(0));
        let data_in_f = Arc::clone(&data);
        let inc = move || {
            let data = Arc::clone(&data_in_f);
            async move {
                data.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(100));
                data.fetch_add(1, Ordering::SeqCst);
            }
        };

        let handle = handle.spawn_repeating_task(Duration::from_millis(1), inc);

        while data.load(Ordering::SeqCst) < 10 {
            yield_now();
        }
        assert!(data.load(Ordering::SeqCst) >= 10);

        // Test abort task.
        handle.abort();
        let mut counter = 0;
        while counter < 20 && !handle.is_finished() {
            std::thread::sleep(std::time::Duration::from_millis(100));
            counter += 1;
        }
        assert!(handle.is_finished());
        // The task iteration is always complete, we cannot observe an odd number.
        assert_eq!(data.load(Ordering::SeqCst) % 2, 0);
    }
}
