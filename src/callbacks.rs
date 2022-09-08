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

pub trait RedundantStorageCallback: Send + Sync + Debug {
    fn object_exists(&self, bucket: String, key: String) -> bool;

    fn get_object(&self, bucket: String, key: String) -> Vec<u8>;

    /// Check health of the local and remote storage.
    /// The library will likely call this method before starting a transaction.
    /// Hint: request and cache an access tocken if needed.
    ///
    /// Returning `false` for `monitors` bucket will likely result in the
    /// library rejecting to start a transaction.
    fn check_health(&self, bucket: String) -> bool;

    /// Atomically put an object in the bucket (create the bucket if it does not exists).
    ///
    /// # Return
    /// Returns `true` if successful and `false` otherwise.
    ///
    /// Must only return after being certain that data was persisted safely.
    /// Failure to do so for `monitors` bucket may result in loss of funds.
    ///
    /// Returning `false` for `monitors` bucket will likely result in a channel
    /// being force-closed.
    fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> bool;

    /// List objects in the given bucket.
    ///
    /// # Return
    /// Return a list of object keys present in the bucket.
    fn list_objects(&self, bucket: String) -> Vec<String>;
}
