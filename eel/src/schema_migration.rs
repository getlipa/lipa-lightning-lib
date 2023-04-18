use crate::errors::Result;

use log::info;
use perro::{permanent_failure, MapToError};
use rusqlite::{Connection, TransactionBehavior};

pub(crate) type Migration = fn(&Connection) -> Result<()>;

pub(crate) fn migrate_schema<F>(connection: &mut Connection, mut migrations: Vec<F>) -> Result<()>
where
    F: FnOnce(&Connection) -> Result<()>,
{
    let start_version = query_schema_version(connection)?;
    if start_version > migrations.len() {
        return Err(permanent_failure(
            "Database schema version is newer than any known migration",
        ));
    }
    info!("Current database schema version: {start_version}");

    for (index, migration) in migrations.drain(start_version..).enumerate() {
        let to_version = start_version + index + 1;
        info!("Migrating to version {to_version} ...");

        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Exclusive)
            .map_to_permanent_failure("Failed to begin database transaction")?;

        migration(&tx)?;
        update_schema_version(&tx, to_version as i32)?;

        tx.commit()
            .map_to_permanent_failure("Failed to commit database transaction")?;
    }
    info!("Database schema migration done");

    Ok(())
}

fn query_schema_version(connection: &Connection) -> Result<usize> {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get::<usize, usize>(0))
        .map_to_permanent_failure("Failed to query schema version")
}

fn update_schema_version(connection: &Connection, version: i32) -> Result<()> {
    connection
        .execute(&format!("PRAGMA user_version = {version}"), ())
        .map_to_permanent_failure("Failed to update schema version")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::errors::{Error, Result};
    use crate::schema_migration::{
        migrate_schema, query_schema_version, update_schema_version, Migration,
    };

    use rusqlite::Connection;

    static CREATE_TABLE: Migration = |connection: &Connection| -> Result<()> {
        connection
            .execute("CREATE TABLE t (id INTEGER PRIMARY KEY)", ())
            .unwrap();
        Ok(())
    };
    static ALTER_TABLE: Migration = |connection: &Connection| -> Result<()> {
        connection
            .execute("ALTER TABLE t ADD COLUMN new_column INTEGER", ())
            .unwrap();
        Ok(())
    };

    #[test]
    fn test_schema_version() {
        let connection = Connection::open_in_memory().unwrap();
        let start_version = query_schema_version(&connection).unwrap();
        assert_eq!(start_version, 0);

        update_schema_version(&connection, 1).unwrap();
        assert_eq!(query_schema_version(&connection).unwrap(), 1);

        update_schema_version(&connection, 101).unwrap();
        assert_eq!(query_schema_version(&connection).unwrap(), 101);

        update_schema_version(&connection, 21).unwrap();
        assert_eq!(query_schema_version(&connection).unwrap(), 21);
    }

    #[test]
    fn test_no_migration() {
        let mut connection = Connection::open_in_memory().unwrap();

        assert!(connection.execute("SELECT id FROM t", ()).is_err());
        migrate_schema(&mut connection, Vec::<Migration>::new()).unwrap();
        assert!(connection.execute("SELECT id FROM t", ()).is_err());
    }

    #[test]
    fn test_single_migration() {
        let mut connection = Connection::open_in_memory().unwrap();

        assert!(connection.execute("SELECT id FROM t", ()).is_err());
        migrate_schema(&mut connection, vec![CREATE_TABLE]).unwrap();
        assert!(connection.execute("SELECT id FROM t", ()).is_ok());
        assert!(connection.execute("SELECT new_column FROM t", ()).is_err());

        // Migration is idempotent.
        migrate_schema(&mut connection, vec![CREATE_TABLE]).unwrap();
        migrate_schema(&mut connection, vec![CREATE_TABLE]).unwrap();
    }

    #[test]
    fn test_multiple_migrations_one_by_one() {
        let mut connection = Connection::open_in_memory().unwrap();

        assert!(connection.execute("SELECT id FROM t", ()).is_err());
        migrate_schema(&mut connection, vec![CREATE_TABLE]).unwrap();
        migrate_schema(&mut connection, vec![CREATE_TABLE, ALTER_TABLE]).unwrap();
        assert!(connection.execute("SELECT id FROM t", ()).is_ok());
        assert!(connection.execute("SELECT new_column FROM t", ()).is_ok());

        // Migration is idempotent.
        migrate_schema(&mut connection, vec![CREATE_TABLE, ALTER_TABLE]).unwrap();
    }

    #[test]
    fn test_multiple_migrations_all_at_once() {
        let mut connection = Connection::open_in_memory().unwrap();

        assert!(connection.execute("SELECT id FROM t", ()).is_err());
        migrate_schema(&mut connection, vec![CREATE_TABLE, ALTER_TABLE]).unwrap();
        assert!(connection.execute("SELECT id FROM t", ()).is_ok());
        assert!(connection.execute("SELECT new_column FROM t", ()).is_ok());

        // Migration is idempotent.
        migrate_schema(&mut connection, vec![CREATE_TABLE, ALTER_TABLE]).unwrap();
    }

    #[test]
    fn test_migration_inconsitency() {
        let mut connection = Connection::open_in_memory().unwrap();

        migrate_schema(&mut connection, vec![CREATE_TABLE, ALTER_TABLE]).unwrap();
        let result = migrate_schema(&mut connection, vec![CREATE_TABLE]);
        assert!(matches!(result, Err(Error::PermanentFailure { .. })));
    }
}
