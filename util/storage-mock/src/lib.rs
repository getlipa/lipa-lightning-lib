use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug)]
pub struct Storage {
    // Put the map into RefCell to allow mutation by immutable ref in MemoryStorage::put_object().
    #[allow(clippy::type_complexity)]
    pub objects: Mutex<RefCell<HashMap<(String, String), Vec<u8>>>>,
    pub health: Mutex<bool>,
}

impl Storage {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            objects: Mutex::new(RefCell::new(HashMap::new())),
            health: Mutex::new(true),
        }
    }

    pub fn get_object(&self, bucket: String, key: String) -> Option<Vec<u8>> {
        Some(
            self.objects
                .lock()
                .unwrap()
                .borrow()
                .get(&(bucket, key))?
                .clone(),
        )
    }

    pub fn check_health(&self) -> bool {
        *self.health.lock().unwrap()
    }

    pub fn put_object(&self, bucket: String, key: String, value: Vec<u8>) {
        *self.health.lock().unwrap() = true;
        self.objects
            .lock()
            .unwrap()
            .borrow_mut()
            .insert((bucket, key), value);
    }

    pub fn list_objects(&self, bucket: String) -> Vec<String> {
        self.objects
            .lock()
            .unwrap()
            .borrow()
            .keys()
            .filter(|(b, _)| &bucket == b)
            .map(|(_, k)| k.clone())
            .collect()
    }

    pub fn delete_object(&self, bucket: String, key: String) {
        self.objects
            .lock()
            .unwrap()
            .borrow_mut()
            .remove(&(bucket, key));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // todo: add tests

    #[test]
    fn it_works() {
        let storage = Storage::new();

        storage.put_object("bucket".to_string(), "key".to_string(), vec![1, 2, 3]);

        assert!(storage.check_health());
        assert_eq!(
            storage.list_objects("bucket".to_string()),
            vec!["key".to_string()]
        );

        assert_eq!(
            storage.get_object("bucket".to_string(), "key".to_string()),
            Some(vec![1, 2, 3])
        );

        storage.delete_object("bucket".to_string(), "key".to_string());
    }

    #[test]
    fn test_reading_of_non_existing_bucket() {
        let storage = Storage::new();

        assert!(storage.check_health());
        assert_eq!(
            storage.list_objects("non_existing_bucket".to_string()),
            Vec::<String>::new()
        );

        assert!(storage
            .get_object("non_existing_bucket".to_string(), "key".to_string())
            .is_none());
    }

    #[test]
    fn test_reading_of_non_existing_key() {
        let storage = Storage::new();

        storage.put_object("bucket".to_string(), "key".to_string(), vec![1, 2, 3]);

        assert!(storage.check_health());
        assert_eq!(
            storage.list_objects("bucket".to_string()),
            vec!["key".to_string()]
        );

        assert_eq!(
            storage.get_object("bucket".to_string(), "non_existing_key".to_string()),
            None
        );

        storage.delete_object("bucket".to_string(), "key".to_string());
    }
}
