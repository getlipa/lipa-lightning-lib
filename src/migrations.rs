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

const MIGRATION_06_PAYMENT_CHANNEL_OPENING_FEES: &str = "
    ALTER TABLE created_invoices ADD COLUMN channel_opening_fees INTEGER
";

const MIGRATION_07_RESET_FUND_MIGRATION_STATUS: &str = "
    INSERT INTO funds_migration_status (status) VALUES (0);
";

const MIGRATION_08_OFFER_TOPUP_VALUE: &str = "
    ALTER TABLE offers ADD COLUMN topup_value_sats INTEGER;
";

const MIGRATION_09_ADD_INVOICE_EXPIRY: &str = "
    ALTER TABLE created_invoices ADD COLUMN invoice_expiry_timestamp INTEGER NOT NULL DEFAULT 0;
";

const MIGRATION_10_ANALYTICS_CONFIG: &str = "
    CREATE TABLE analytics_config (
        id INTEGER NOT NULL PRIMARY KEY,
        status INTEGER NOT NULL,
        updated_at INTEGER NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
    INSERT INTO analytics_config (status)
    VALUES (0);
";

const MIGRATION_11_LAST_REGISTERED_NOTIFICATION_WEBHOOK_BASE_URL: &str = "
    CREATE TABLE webhook_base_url (
        id INTEGER NOT NULL PRIMARY KEY,
        url TEXT NOT NULL,
        updated_at INTEGER NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
";

const MIGRATION_12_LIGHTNING_ADDRESSES: &str = "
    CREATE TABLE lightning_addresses (
        address TEXT NOT NULL UNIQUE,
        registered_at INTEGER NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
";

const MIGRATION_13_PAYMENT_PERSONAL_NOTE: &str = "
    ALTER TABLE payments ADD COLUMN personal_note TEXT DEFAULT NULL;
";

const MIGRATION_14_LNURL_PAY_RECEIVE_DATA: &str = "
    ALTER TABLE payments ADD COLUMN received_on TEXT DEFAULT NULL;
    ALTER TABLE payments ADD COLUMN received_lnurl_comment TEXT DEFAULT NULL;
";

const MIGRATION_15_LIGHTNING_ADDRESSES_ENABLE_STATUS: &str = "
    ALTER TABLE lightning_addresses ADD COLUMN enable_status INTEGER NOT NULL DEFAULT 0;
";

const MIGRATION_16_HIDDEN_CHANNEL_CLOSE_AMOUNT: &str = "
    CREATE TABLE hidden_channel_close_amount (
        id INTEGER NOT NULL PRIMARY KEY,
        amount_sat INTEGER NOT NULL,
        inserted_at INTEGER NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
";

const MIGRATION_17_HIDDEN_FAILED_SWAPS: &str = "
    CREATE TABLE hidden_failed_swaps (
        id INTEGER NOT NULL PRIMARY KEY,
        swap_address TEXT NOT NULL,
        inserted_at INTEGER NOT NULL DEFAULT CURRENT_TIMESTAMP
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
        M::up(MIGRATION_06_PAYMENT_CHANNEL_OPENING_FEES),
        M::up(MIGRATION_07_RESET_FUND_MIGRATION_STATUS),
        M::up(MIGRATION_08_OFFER_TOPUP_VALUE),
        M::up(MIGRATION_09_ADD_INVOICE_EXPIRY),
        M::up(MIGRATION_10_ANALYTICS_CONFIG),
        M::up(MIGRATION_11_LAST_REGISTERED_NOTIFICATION_WEBHOOK_BASE_URL),
        M::up(MIGRATION_12_LIGHTNING_ADDRESSES),
        M::up(MIGRATION_13_PAYMENT_PERSONAL_NOTE),
        M::up(MIGRATION_14_LNURL_PAY_RECEIVE_DATA),
        M::up(MIGRATION_15_LIGHTNING_ADDRESSES_ENABLE_STATUS),
        M::up(MIGRATION_16_HIDDEN_CHANNEL_CLOSE_AMOUNT),
        M::up(MIGRATION_17_HIDDEN_FAILED_SWAPS),
    ])
}

#[cfg(test)]
mod tests {
    use super::migrations;

    #[test]
    fn db_migrations_test() {
        assert_eq!(migrations().validate(), Ok(()));
    }
}
