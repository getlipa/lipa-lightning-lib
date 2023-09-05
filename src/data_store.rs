use crate::errors::Result;
use crate::migrations::migrate;

use perro::MapToError;
use rusqlite::Connection;

pub(crate) struct DataStore {
    #[allow(dead_code)]
    conn: Connection,
}

impl DataStore {
    pub fn new(db_path: &str) -> Result<Self> {
        let mut conn = Connection::open(db_path).map_to_invalid_input("Invalid db path")?;
        migrate(&mut conn)?;
        Ok(DataStore { conn })
    }
}
