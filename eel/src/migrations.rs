use crate::errors::Result;
use crate::schema_migration::Migration;

use perro::MapToError;
use rusqlite::Connection;

pub(crate) fn get_migrations() -> Vec<Migration> {
    vec![
        migration_01_init,
        migration_02_store_exchange_rates,
        migration_03_store_spendable_outputs,
        migration_04_store_exchange_rates_history,
        migration_05_store_payment_failure_reason,
    ]
}

fn migration_01_init(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(
            "\
            CREATE TABLE IF NOT EXISTS payments (
              payment_id INTEGER NOT NULL PRIMARY KEY,
              type INTEGER CHECK( type IN (0, 1) ) NOT NULL,
              hash TEXT NOT NULL UNIQUE,
              amount_msat INTEGER NOT NULL,
              invoice TEXT NOT NULL,
              description TEXT NOT NULL,
              preimage TEXT,
              network_fees_msat INTEGER,
              lsp_fees_msat INTEGER,
              amount_usd INTEGER,
              amount_fiat INTEGER,
              fiat_currency TEXT,
              metadata TEXT
            );
            CREATE TABLE IF NOT EXISTS events (
              event_id INTEGER NOT NULL PRIMARY KEY,
              payment_id INTEGER NOT NULL,
              type INTEGER CHECK( type in (0, 1, 2, 3, 4) ) NOT NULL,
              inserted_at INTEGER NOT NULL DEFAULT CURRENT_TIMESTAMP,
              timezone_id TEXT NOT NULL,
              timezone_utc_offset_secs INTEGER NOT NULL,
              FOREIGN KEY (payment_id) REFERENCES payments(payment_id)
            );
            CREATE VIEW IF NOT EXISTS creation_events
            AS
            SELECT *
            FROM events
            JOIN (
                SELECT MIN(event_id) AS min_event_id
                FROM events
                GROUP BY payment_id
            ) AS min_ids ON min_event_id=events.event_id;
            CREATE VIEW IF NOT EXISTS recent_events
            AS
            SELECT *
            FROM events
            JOIN (
                SELECT MAX(event_id) AS max_event_id
                FROM events
                GROUP BY payment_id
            ) AS max_ids ON max_event_id=events.event_id;
        ",
        )
        .map_to_permanent_failure("Failed to set up local database")
}

fn migration_02_store_exchange_rates(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(
            "\
            CREATE TABLE exchange_rates (
                fiat_currency TEXT NOT NULL PRIMARY KEY,
                rate INTEGER NOT NULL,
                updated_at INTEGER NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
        ",
        )
        .map_to_permanent_failure("Failed to set up local database")
}

fn migration_03_store_spendable_outputs(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(
            "\
            CREATE TABLE spendable_outputs (
                id INTEGER NOT NULL PRIMARY KEY,
                spendable_output BLOB NOT NULL,
                inserted_at INTEGER NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
        ",
        )
        .map_to_permanent_failure("Failed to set up local database")
}

fn migration_04_store_exchange_rates_history(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(
            "\
            CREATE TABLE exchange_rates_history (
                id INTEGER NOT NULL PRIMARY KEY,
                snapshot_id INTEGER NOT NULL,
                fiat_currency TEXT NOT NULL,
                rate INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                UNIQUE(snapshot_id, fiat_currency) ON CONFLICT IGNORE
            );",
        )
        .map_to_permanent_failure("Failed to set up local database")?;
    connection
        .execute_batch(
            "\
            ALTER TABLE payments ADD COLUMN exchange_rates_history_snaphot_id INTEGER NULL;
            ALTER TABLE payments DROP COLUMN amount_usd;
            ALTER TABLE payments DROP COLUMN amount_fiat;
        ",
        )
        .map_to_permanent_failure("Failed to set up local database")
}

fn migration_05_store_payment_failure_reason(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(
            "\
            ALTER TABLE events ADD COLUMN fail_reason INTEGER NULL;
        ",
        )
        .map_to_permanent_failure("Failed to set up local database")
}
