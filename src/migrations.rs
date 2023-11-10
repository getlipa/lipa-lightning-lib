use crate::errors::Result;

use perro::MapToError;
use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

const MIGRATION_01_INIT: &str = "
    CREATE TABLE payments (
        hash TEXT NOT NULL PRIMARY KEY ON CONFLICT REPLACE,
        timezone_id TEXT NOT NULL,
        timezone_utc_offset_secs INTEGER NOT NULL,
        fiat_currency TEXT NOT NULL,
        exchange_rates_history_snaphot_id INTEGER NULL
    );

    CREATE TABLE exchange_rates_history (
        id INTEGER NOT NULL PRIMARY KEY,
        snapshot_id INTEGER NOT NULL,
        fiat_currency TEXT NOT NULL,
        rate INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        UNIQUE(snapshot_id, fiat_currency) ON CONFLICT IGNORE
    );

    CREATE TABLE offers (
        payment_hash TEXT NOT NULL PRIMARY KEY ON CONFLICT REPLACE,
        pocket_id TEXT NULL,
        fiat_currency TEXT NULL,
        rate INTEGER NULL,
        exchanged_at INTEGER NULL,
        topup_value_minor_units INTEGER NULL,
        exchange_fee_minor_units INTEGER NULL,
        exchange_fee_rate_permyriad INTEGER NULL
    );

    CREATE TABLE exchange_rates (
        fiat_currency TEXT NOT NULL PRIMARY KEY,
        rate INTEGER NOT NULL,
        updated_at INTEGER NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
";

const MIGRATION_02_FUNDS_MIGRATION_STATUS: &str = "
    CREATE TABLE funds_migration_status (
        id INTEGER NOT NULL PRIMARY KEY,
        status INTEGER NOT NULL,
        updated_at INTEGER NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
    INSERT INTO funds_migration_status (status)
    VALUES (0);
";

const MIGRATION_03_OFFER_ERROR_MESSAGE: &str = "
    ALTER TABLE offers ADD COLUMN error TEXT NULL;
";

const MIGRATION_04_CREATED_INVOICES: &str = "
    CREATE TABLE created_invoices (
        id INTEGER NOT NULL PRIMARY KEY,
        hash INTEGER NOT NULL,
        invoice TEXT NOT NULL
    );
";

const MIGRATION_05_FIAT_TOPUP_INFO: &str = "
    CREATE TABLE fiat_topup_info (
        order_id TEXT NOT NULL PRIMARY KEY,
        created_at INTEGER NOT NULL,
        debitor_iban TEXT NOT NULL,
        creditor_reference TEXT NOT NULL,
        creditor_iban TEXT NOT NULL,
        creditor_bank_name TEXT NOT NULL,
        creditor_bank_street TEXT NOT NULL,
        creditor_bank_postal_code TEXT NOT NULL,
        creditor_bank_town TEXT NOT NULL,
        creditor_bank_country TEXT NOT NULL,
        creditor_bank_bic TEXT NOT NULL,
        creditor_name TEXT NOT NULL,
        creditor_street TEXT NOT NULL,
        creditor_postal_code TEXT NOT NULL,
        creditor_town TEXT NOT NULL,
        creditor_country TEXT NOT NULL,
        currency TEXT NOT NULL
    );
";

pub(crate) fn migrate(conn: &mut Connection) -> Result<()> {
    migrations()
        .to_latest(conn)
        .map_to_permanent_failure("Failed to migrate the db")
}

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(MIGRATION_01_INIT),
        M::up(MIGRATION_02_FUNDS_MIGRATION_STATUS),
        M::up(MIGRATION_03_OFFER_ERROR_MESSAGE),
        M::up(MIGRATION_04_CREATED_INVOICES),
        M::up(MIGRATION_05_FIAT_TOPUP_INFO),
    ])
}

#[cfg(test)]
mod tests {
    use super::migrations;

    #[test]
    fn db_migrations_test() {
        assert!(migrations().validate().is_ok());
    }
}
