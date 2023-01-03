use crate::errors::{LipaResult, MapToLipaError};
use core::future::Future;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time;
use tokio::time::Duration;

pub(crate) struct AsyncRuntime {
    rt: Runtime,
}

pub(crate) struct Handle {
    handle: tokio::runtime::Handle,
}

impl AsyncRuntime {
    #[allow(clippy::result_large_err)]
    pub fn new() -> LipaResult<Self> {
        let rt = Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("3l-async-runtime")
            .enable_time()
            .enable_io()
            .build()
            .map_to_permanent_failure("Failed to build tokio async runtime")?;
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

    pub fn spawn_repeating_task<Func, F>(
        &self,
        interval: Duration,
        func: Func,
    ) -> RepeatingTaskHandle
    where
        Func: Fn() -> F + Send + Sync + 'static,
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let (stop_sender, mut stop_receiver) = mpsc::channel(1);
        let (status_sender, status_receiver) = mpsc::channel(1);
        let handle = self.handle.spawn(async move {
            let mut interval = time::interval(interval);
            interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
            loop {
                func().await;
                tokio::select! {
                    _ = stop_receiver.recv() => {
                        break;
                    },
                    _ = interval.tick() => {},
                }
            }
            drop(status_sender);
        });
        RepeatingTaskHandle {
            handle,
            stop_sender,
            status_receiver,
        }
    }

    pub fn block_on<F: Future>(&self, future: F) -> F::Output {
        self.handle.block_on(future)
    }
}

pub(crate) struct RepeatingTaskHandle {
    handle: JoinHandle<()>,
    stop_sender: mpsc::Sender<()>,
    status_receiver: mpsc::Receiver<()>,
}

impl RepeatingTaskHandle {
    pub fn request_shutdown(&self) {
        self.stop_sender.blocking_send(()).unwrap();
    }

    pub fn blocking_shutdown(&mut self) {
        self.request_shutdown();
        self.join();
    }

    pub fn join(&mut self) {
        self.status_receiver.blocking_recv();
    }

    // Currently only used in tests
    #[allow(dead_code)]
    fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }
}

#[cfg(test)]
mod tests {
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
                sleep(std::time::Duration::from_millis(100)).await;
                data.fetch_add(1, Ordering::SeqCst);
            }
        };

        let mut handle = handle.spawn_repeating_task(Duration::from_millis(1), inc);

        while data.load(Ordering::SeqCst) < 10 {
            yield_now();
        }
        assert!(data.load(Ordering::SeqCst) >= 10);

        // Test abort task.
        handle.blocking_shutdown();

        assert!(handle.is_finished());
        // The task iteration is always complete, we cannot observe an odd number.
        assert_eq!(data.load(Ordering::SeqCst) % 2, 0);
    }
}
