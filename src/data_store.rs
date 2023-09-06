use crate::errors::Result;
use crate::migrations::migrate;
use crate::UserPreferences;

use perro::MapToError;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};

pub(crate) struct DataStore {
    #[allow(dead_code)]
    user_preferences: Arc<Mutex<UserPreferences>>,
    #[allow(dead_code)]
    conn: Connection,
}

impl DataStore {
    pub fn new(db_path: &str, user_preferences: Arc<Mutex<UserPreferences>>) -> Result<Self> {
        let mut conn = Connection::open(db_path).map_to_invalid_input("Invalid db path")?;
        migrate(&mut conn)?;
        Ok(DataStore {
            user_preferences,
            conn,
        })
    }
}
