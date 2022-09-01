use std::fmt::Debug;

pub trait PersistCallback: Send + Sync + Debug {
    /// Check if a file or directory exists
    fn exists(&self, path: String) -> bool;

    /// Read filenames in the given path
    fn read_dir(&self, path: String) -> Vec<String>;

    /// Write data to a file
    ///
    /// # Return
    /// Returns `true` if successful and `false` otherwise.
    ///
    /// Must only return after being certain that data was persisted safely.
    /// Failure to do so will result in loss of funds.
    ///
    /// Returning `false` will likely result in a channel being force-closed.
    fn write_to_file(&self, path: String, data: Vec<u8>) -> bool;

    /// Read data from file
    fn read(&self, path: String) -> Vec<u8>;
}
