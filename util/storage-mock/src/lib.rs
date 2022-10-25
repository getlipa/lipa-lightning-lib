use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug)]
pub struct Storage {
    // Put the map into RefCell to allow mutation by immutable ref in MemoryStorage::put_object().
    pub objects: Mutex<RefCell<HashMap<(String, String), Vec<u8>>>>,
    pub health: Mutex<HashMap<String, bool>>,
}

impl Storage {
    pub fn new() -> Self {
        Self {
            objects: Mutex::new(RefCell::new(HashMap::new())),
            health: Mutex::new(HashMap::new()),
        }
    }

    pub fn object_exists(&self, bucket: String, key: String) -> bool {
        self.objects
            .lock()
            .unwrap()
            .borrow()
            .contains_key(&(bucket, key))
    }

    pub fn get_object(&self, bucket: String, key: String) -> Vec<u8> {
        self.objects
            .lock()
            .unwrap()
            .borrow()
            .get(&(bucket, key))
            .unwrap()
            .clone()
    }

    pub fn check_health(&self, bucket: String) -> bool {
        match self.health.lock().unwrap().get(&bucket) {
            Some(health) => *health,
            None => false,
        }
    }

    pub fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> bool {
        self.health.lock().unwrap().insert(bucket.clone(), true);
        self.objects
            .lock()
            .unwrap()
            .borrow_mut()
            .insert((bucket, key), value);
        true
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
}

#[cfg(test)]
mod tests {
    use super::*;

    // todo: add tests

    #[test]
    fn it_works() {
        let storage = Storage::new();

        storage.put_object("bucket".to_string(), "key".to_string(), vec![1, 2, 3]);

        assert!(storage.check_health("bucket".to_string()));
        assert_eq!(
            storage.list_objects("bucket".to_string()),
            vec!["key".to_string()]
        );
        assert!(storage.object_exists("bucket".to_string(), "key".to_string()));
        assert_eq!(
            storage.get_object("bucket".to_string(), "key".to_string()),
            vec![1, 2, 3]
        );
    }
}
