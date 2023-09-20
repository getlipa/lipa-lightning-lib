use std::sync::{Mutex, MutexGuard};

pub(crate) trait Locker<T> {
    fn lock_unwrap(&self) -> MutexGuard<'_, T>;
}

impl<T> Locker<T> for Mutex<T> {
    fn lock_unwrap(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap()
    }
}
