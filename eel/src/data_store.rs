use crate::config::TzConfig;
use crate::errors::Result;
use crate::invoice;
use crate::migrations::get_migrations;
use crate::schema_migration::migrate_schema;

use lightning_invoice::Invoice;
use perro::{MapToError, OptionToError};
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row};
use std::str::FromStr;
use std::time::SystemTime;

use crate::payment::{FiatValues, Payment, PaymentState, PaymentType, TzTime};

pub(crate) struct DataStore {
    db_conn: Connection,
    timezone_config: TzConfig,
}

impl DataStore {
    pub fn new(db_path: &str, timezone_config: TzConfig) -> Result<Self> {
        let mut db_conn = Connection::open(db_path).map_to_invalid_input("Invalid db path")?;

        migrate_schema(&mut db_conn, get_migrations())?;

        Ok(DataStore {
            db_conn,
            timezone_config,
        })
    }

    pub fn update_timezone_config(&mut self, timezone_config: TzConfig) {
        self.timezone_config = timezone_config;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_incoming_payment(
        &mut self,
        hash: &str,
        amount_msat: u64,
        lsp_fees_msat: u64,
        description: &str,
        invoice: &str,
        metadata: &str,
        fiat_values: Option<FiatValues>,
    ) -> Result<()> {
        let (fiat_currency, amount_fiat, amount_usd) =
            fiat_values_option_to_option_tuple(fiat_values);
        let tx = self
            .db_conn
            .transaction()
            .map_to_permanent_failure("Failed to begin SQL transaction")?;
        tx.execute(
            "\
            INSERT INTO payments (type, hash, amount_msat, lsp_fees_msat, description, invoice, metadata, amount_usd, amount_fiat, fiat_currency) \
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)\
            ",
            (
                PaymentType::Receiving as u8,
                hash,
                amount_msat,
                lsp_fees_msat,
                description,
                invoice,
                metadata,
                amount_usd,
                amount_fiat,
                fiat_currency,
            ),
        )
        .map_to_invalid_input("Failed to add new incoming payment to payments db")?;
        tx.execute(
            "\
            INSERT INTO events (payment_id, type, timezone_id, timezone_utc_offset_secs) \
            VALUES (?1, ?2, ?3, ?4) \
            ",
            (
                tx.last_insert_rowid(),
                PaymentState::Created as u8,
                &self.timezone_config.timezone_id,
                self.timezone_config.timezone_utc_offset_secs,
            ),
        )
        .map_to_invalid_input("Failed to add new incoming payment to payments db")?;
        tx.commit()
            .map_to_permanent_failure("Failed to commit new incoming payment transaction")
    }

    pub fn new_outgoing_payment(
        &mut self,
        hash: &str,
        amount_msat: u64,
        description: &str,
        invoice: &str,
        metadata: &str,
        fiat_values: Option<FiatValues>,
    ) -> Result<()> {
        let (fiat_currency, amount_fiat, amount_usd) =
            fiat_values_option_to_option_tuple(fiat_values);
        let tx = self
            .db_conn
            .transaction()
            .map_to_permanent_failure("Failed to begin SQL transaction")?;
        tx.execute(
            "\
            INSERT INTO payments (type, hash, amount_msat, description, invoice, metadata, amount_usd, amount_fiat, fiat_currency) \
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)\
            ",
            (
                PaymentType::Sending as u8,
                hash,
                amount_msat,
                description,
                invoice,
                metadata,
                amount_usd,
                amount_fiat,
                fiat_currency,
            ),
        )
        .map_to_invalid_input("Failed to add new outgoing payment to payments db")?;
        tx.execute(
            "\
            INSERT INTO events (payment_id, type, timezone_id, timezone_utc_offset_secs) \
            VALUES (?1, ?2, ?3, ?4) \
            ",
            (
                tx.last_insert_rowid(),
                PaymentState::Created as u8,
                &self.timezone_config.timezone_id,
                &self.timezone_config.timezone_utc_offset_secs,
            ),
        )
        .map_to_invalid_input("Failed to add new outgoing payment to payments db")?;
        tx.commit()
            .map_to_permanent_failure("Failed to commit new outgoing payment transaction")
    }

    pub fn incoming_payment_succeeded(&self, hash: &str) -> Result<()> {
        self.new_payment_state(hash, PaymentState::Succeeded)
    }

    pub fn outgoing_payment_succeeded(
        &self,
        hash: &str,
        preimage: &str,
        network_fees_msat: u64,
    ) -> Result<()> {
        self.new_payment_state(hash, PaymentState::Succeeded)?;
        self.fill_preimage(hash, preimage)?;
        self.fill_network_fees(hash, network_fees_msat)
    }

    pub fn new_payment_state(&self, hash: &str, state: PaymentState) -> Result<()> {
        self.db_conn
            .execute(
                "\
                INSERT INTO events (payment_id, type, timezone_id, timezone_utc_offset_secs) \
                VALUES (
                    (SELECT payment_id FROM payments WHERE hash=?1), ?2, ?3, ?4)
                ",
                (
                    hash,
                    state as u8,
                    &self.timezone_config.timezone_id,
                    self.timezone_config.timezone_utc_offset_secs,
                ),
            )
            .map_to_invalid_input("Failed to add payment retrying event to payments db")?;

        Ok(())
    }

    pub fn fill_preimage(&self, hash: &str, preimage: &str) -> Result<()> {
        self.db_conn
            .execute(
                "\
            UPDATE payments \
            SET preimage=?1 \
            WHERE hash=?2 \
            ",
                (preimage, hash),
            )
            .map_to_invalid_input("Failed to insert preimage into db")?;

        Ok(())
    }

    fn fill_network_fees(&self, hash: &str, network_fees_msat: u64) -> Result<()> {
        self.db_conn
            .execute(
                "\
            UPDATE payments \
            SET network_fees_msat=?1 \
            WHERE hash=?2 \
            ",
                (network_fees_msat, hash),
            )
            .map_to_invalid_input("Failed to insert network fee into db")?;

        Ok(())
    }

    pub fn get_latest_payments(&self, number_of_payments: u32) -> Result<Vec<Payment>> {
        self.process_expired_payments()?;

        let mut statement = self
            .db_conn
            .prepare("\
            SELECT payments.payment_id, payments.type, hash, preimage, amount_msat, network_fees_msat, \
            lsp_fees_msat, invoice, metadata, recent_events.type as state, recent_events.inserted_at, \
            recent_events.timezone_id, recent_events.timezone_utc_offset_secs, description, \
            creation_events.inserted_at, creation_events.timezone_id, creation_events.timezone_utc_offset_secs, \
            amount_usd, amount_fiat, fiat_currency \
            FROM payments \
            JOIN recent_events ON payments.payment_id=recent_events.payment_id \
            JOIN creation_events ON payments.payment_id=creation_events.payment_id \
            ORDER BY payments.payment_id DESC \
            LIMIT ? \
            ")
            .map_to_permanent_failure("Failed to prepare SQL query")?;
        let payment_iter = statement
            .query_map([number_of_payments], payment_from_row)
            .map_to_permanent_failure("Failed to bind parameter to prepared SQL query")?;

        let mut payments = Vec::new();
        for payment in payment_iter {
            payments.push(payment.map_to_permanent_failure("Corrupted db")?);
        }

        Ok(payments)
    }

    pub fn get_payment(&self, hash: &str) -> Result<Payment> {
        self.process_expired_payments()?;

        let mut statement = self
            .db_conn
            .prepare("\
            SELECT payments.payment_id, payments.type, hash, preimage, amount_msat, network_fees_msat, \
            lsp_fees_msat, invoice, metadata, recent_events.type as state, recent_events.inserted_at, \
            recent_events.timezone_id, recent_events.timezone_utc_offset_secs, description, \
            creation_events.inserted_at, creation_events.timezone_id, creation_events.timezone_utc_offset_secs, \
            amount_usd, amount_fiat, fiat_currency \
            FROM payments \
            JOIN recent_events ON payments.payment_id=recent_events.payment_id \
            JOIN creation_events ON payments.payment_id=creation_events.payment_id \
            WHERE payments.hash=? \
            ")
            .map_to_permanent_failure("Failed to prepare SQL query")?;
        let mut payment_iter = statement
            .query_map([hash], payment_from_row)
            .map_to_permanent_failure("Failed to bind parameter to prepared SQL query")?;

        let payment = payment_iter
            .next()
            .ok_or_invalid_input(
                "Invalid hash: no payment with the provided payment hash was found",
            )?
            .map_to_permanent_failure("Corrupted db")?;
        Ok(payment)
    }

    fn process_expired_payments(&self) -> Result<()> {
        let mut statement = self
            .db_conn
            .prepare("\
            SELECT payments.payment_id, payments.type, hash, preimage, amount_msat, network_fees_msat, \
            lsp_fees_msat, invoice, metadata, recent_events.type as state, recent_events.inserted_at, \
            recent_events.timezone_id, recent_events.timezone_utc_offset_secs, description, \
            creation_events.inserted_at, creation_events.timezone_id, creation_events.timezone_utc_offset_secs, \
            amount_usd, amount_fiat, fiat_currency \
            FROM payments \
            JOIN recent_events ON payments.payment_id=recent_events.payment_id \
            JOIN creation_events ON payments.payment_id=creation_events.payment_id \
            WHERE state NOT IN (?1, ?2) \
            ")
            .map_to_permanent_failure("Failed to prepare SQL query")?;
        let non_expired_payment_iter = statement
            .query_map(
                [
                    PaymentState::Succeeded as u8,
                    PaymentState::InvoiceExpired as u8,
                ],
                payment_from_row,
            )
            .map_to_permanent_failure("Failed to bind parameter to prepared SQL query")?;

        for payment in non_expired_payment_iter {
            let payment = payment.map_to_permanent_failure("Corrupted db")?;
            debug_assert!(
                payment.payment_state != PaymentState::Succeeded
                    && payment.payment_state != PaymentState::InvoiceExpired
            );
            if payment.has_expired() {
                self.new_payment_state(&payment.hash, PaymentState::InvoiceExpired)?;
            }
        }

        Ok(())
    }

    pub fn update_exchange_rate(&self, currency_code: &str, rate: u32) -> Result<()> {
        self.db_conn
            .execute(
                "\
                REPLACE INTO exchange_rates (fiat_currency, rate) \
                VALUES (?1, ?2)
                ",
                (currency_code, rate),
            )
            .map_to_invalid_input("Failed to update exchange rate in db")?;

        Ok(())
    }

    pub fn get_exchange_rate(&self, currency_code: &str) -> Result<Option<u32>> {
        self.db_conn
            .query_row(
                "\
            SELECT rate \
            FROM exchange_rates \
            WHERE fiat_currency=? \
            ",
                [currency_code],
                |row| row.get(0),
            )
            .optional()
            .map_to_permanent_failure("Failed to query exchange rate in db")
    }
}

fn fiat_values_option_to_option_tuple(
    fiat_values: Option<FiatValues>,
) -> (Option<String>, Option<u64>, Option<u64>) {
    fiat_values
        .map(|f| (Some(f.fiat), Some(f.amount), Some(f.amount_usd)))
        .unwrap_or((None, None, None))
}

fn payment_from_row(row: &Row) -> rusqlite::Result<Payment> {
    let payment_type: u8 = row.get(1)?;
    let payment_type = PaymentType::try_from(payment_type)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, Type::Integer, Box::new(e)))?;
    let hash = row.get(2)?;
    let preimage = row.get(3)?;
    let amount_msat = row.get(4)?;
    let network_fees_msat = row.get(5)?;
    let lsp_fees_msat = row.get(6)?;
    let invoice: String = row.get(7)?;
    let invoice_details = invoice::get_invoice_details(
        &Invoice::from_str(&invoice)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, Type::Text, Box::new(e)))?,
    )
    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, Type::Text, Box::new(e)))?;
    let metadata = row.get(8)?;
    let payment_state: u8 = row.get(9)?;
    let payment_state = PaymentState::try_from(payment_state)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, Type::Integer, Box::new(e)))?;
    let latest_state_change_at_timestamp: chrono::DateTime<chrono::Utc> = row.get(10)?;
    let latest_state_change_at_timezone_id = row.get(11)?;
    let latest_state_change_at_timezone_utc_offset_secs = row.get(12)?;
    let latest_state_change_at = TzTime {
        time: SystemTime::from(latest_state_change_at_timestamp),
        timezone_id: latest_state_change_at_timezone_id,
        timezone_utc_offset_secs: latest_state_change_at_timezone_utc_offset_secs,
    };
    let description = row.get(13)?;
    let created_at_timestamp: chrono::DateTime<chrono::Utc> = row.get(14)?;
    let created_at_timezone_id = row.get(15)?;
    let created_at_timezone_utc_offset_secs = row.get(16)?;
    let created_at = TzTime {
        time: SystemTime::from(created_at_timestamp),
        timezone_id: created_at_timezone_id,
        timezone_utc_offset_secs: created_at_timezone_utc_offset_secs,
    };
    let amount_usd: Option<u64> = row.get(17)?;
    let amount_fiat: Option<u64> = row.get(18)?;
    let fiat_currency: Option<String> = row.get(19)?;
    let fiat_value =
        amount_usd
            .zip(amount_fiat)
            .zip(fiat_currency)
            .map(|((amount_usd, amount), fiat)| FiatValues {
                fiat,
                amount,
                amount_usd,
            });
    Ok(Payment {
        payment_type,
        payment_state,
        hash,
        amount_msat,
        invoice_details,
        created_at,
        latest_state_change_at,
        description,
        preimage,
        network_fees_msat,
        lsp_fees_msat,
        fiat_values: fiat_value,
        metadata,
    })
}

#[cfg(test)]
mod tests {
    use crate::config::TzConfig;
    use crate::data_store::DataStore;
    use crate::interfaces::ExchangeRates;
    use crate::payment::{FiatValues, PaymentState, PaymentType};

    use lightning::ln::PaymentSecret;
    use lightning_invoice::{Currency, InvoiceBuilder};
    use secp256k1::hashes::{sha256, Hash};
    use secp256k1::{Secp256k1, SecretKey};
    use std::fs;
    use std::thread::sleep;
    use std::time::Duration;

    const TEST_DB_PATH: &str = ".3l_local_test";
    const TEST_TZ_ID: &str = "test_timezone_id";
    const TEST_TZ_OFFSET: i32 = -1352;

    #[test]
    fn test_payment_exists() {
        let db_name = String::from("payment_exists.db3");
        reset_db(&db_name);
        let tz_config = TzConfig {
            timezone_id: String::from(TEST_TZ_ID),
            timezone_utc_offset_secs: TEST_TZ_OFFSET,
        };
        let mut data_store =
            DataStore::new(&format!("{TEST_DB_PATH}/{db_name}"), tz_config).unwrap();

        let hash = "1234";
        let _preimage = "5678";
        let amount_msat = 100_000_000;
        let lsp_fees_msat = 2_000_000;
        let description = String::from("Test description 1");
        let invoice = String::from("lnbcrt1m1p37fe7udqqpp5e2mktq6ykgp0e9uljdrakvcy06wcwtswgwe7yl6jmfry4dke2t2ssp5s3uja8xn7tpeuctc62xqua6slpj40jrwlkuwmluv48g86r888g7s9qrsgqnp4qfalfq06c807p3mlt4ggtufckg3nq79wnh96zjz748zmhl5vys3dgcqzysrzjqwp6qac7ttkrd6rgwfte70sjtwxfxmpjk6z2h8vgwdnc88clvac7kqqqqyqqqqqqqqqqqqlgqqqqqqgqjqwhtk6ldnue43vtseuajgyypkv20py670vmcea9qrrdcqjrpp0qvr0sqgcldapjmgfeuvj54q6jt2h36a0m9xme3rywacscd3a5ey3fgpgdr8eq");
        let metadata = String::from("Test metadata 1");
        let fiat_value = None;

        assert!(data_store.get_payment(hash).is_err());

        data_store
            .new_incoming_payment(
                hash,
                amount_msat,
                lsp_fees_msat,
                &description,
                &invoice,
                &metadata,
                fiat_value,
            )
            .unwrap();

        assert!(data_store.get_payment(hash).is_ok());
    }

    #[test]
    fn test_payment_storage_flow() {
        let db_name = String::from("new_payment.db3");
        reset_db(&db_name);
        let tz_config = TzConfig {
            timezone_id: String::from(TEST_TZ_ID),
            timezone_utc_offset_secs: TEST_TZ_OFFSET,
        };
        let mut data_store =
            DataStore::new(&format!("{TEST_DB_PATH}/{db_name}"), tz_config).unwrap();

        let payments = data_store.get_latest_payments(100).unwrap();
        assert!(payments.is_empty());

        // New incoming payment
        let hash = "1234";
        let preimage = "5678";
        let amount_msat = 100_000_000;
        let lsp_fees_msat = 2_000_000;
        let description = String::from("Test description 1");
        let invoice = build_invoice(amount_msat, 100);
        let metadata = String::from("Test metadata 1");
        let fiat_value = Some(FiatValues {
            fiat: String::from("EUR"),
            amount: 4013,
            amount_usd: 3913,
        });

        data_store
            .new_incoming_payment(
                hash,
                amount_msat,
                lsp_fees_msat,
                &description,
                &invoice,
                &metadata,
                fiat_value.clone(),
            )
            .unwrap();

        let payments = data_store.get_latest_payments(100).unwrap();
        assert_eq!(payments.len(), 1);
        let payment = payments.get(0).unwrap();
        assert_eq!(payment.payment_type, PaymentType::Receiving);
        assert_eq!(payment.payment_state, PaymentState::Created);
        assert_eq!(payment.hash, hash);
        assert_eq!(payment.amount_msat, amount_msat);
        assert_eq!(payment.invoice_details.invoice, invoice);
        assert_eq!(payment.description, description);
        assert_eq!(payment.preimage, None);
        assert_eq!(payment.network_fees_msat, None);
        assert_eq!(payment.lsp_fees_msat, Some(lsp_fees_msat));
        assert_eq!(payment.metadata, metadata);
        assert_eq!(payment.fiat_values, fiat_value);

        assert_eq!(payment.created_at.timezone_id, TEST_TZ_ID);
        assert_eq!(payment.created_at.timezone_utc_offset_secs, TEST_TZ_OFFSET);
        assert_eq!(payment.created_at, payment.latest_state_change_at);
        let created_at = payment.created_at.time;

        data_store.fill_preimage(hash, preimage).unwrap();

        let payments = data_store.get_latest_payments(100).unwrap();
        assert_eq!(payments.len(), 1);
        let payment = payments.get(0).unwrap();
        assert_eq!(payment.preimage, Some(preimage.to_string()));

        // To be able to test the difference between created_at and latest_state_change_at
        sleep(Duration::from_secs(1));

        data_store.incoming_payment_succeeded(hash).unwrap();

        let payments = data_store.get_latest_payments(100).unwrap();
        assert_eq!(payments.len(), 1);
        let payment = payments.get(0).unwrap();
        assert_eq!(payment.payment_state, PaymentState::Succeeded);
        assert_eq!(payment.created_at.timezone_id, TEST_TZ_ID);
        assert_eq!(payment.created_at.timezone_utc_offset_secs, TEST_TZ_OFFSET);
        assert_eq!(payment.latest_state_change_at.timezone_id, TEST_TZ_ID);
        assert_eq!(
            payment.latest_state_change_at.timezone_utc_offset_secs,
            TEST_TZ_OFFSET
        );
        assert_eq!(payment.created_at.time, created_at);
        assert_ne!(payment.created_at.time, payment.latest_state_change_at.time);
        assert!(payment.created_at.time < payment.latest_state_change_at.time);

        // New outgoing payment that fails
        let hash = "5678";
        let _preimage = "1234";
        let amount_msat = 5_000_000;
        let _network_fees_msat = 2_000;
        let description = String::from("Test description 2");
        let invoice = build_invoice(amount_msat, 100);
        let metadata = String::from("Test metadata 2");
        let fiat_value = Some(FiatValues {
            fiat: String::from("CHF"),
            amount: 4253,
            amount_usd: 4103,
        });

        data_store
            .new_outgoing_payment(
                hash,
                amount_msat,
                &description,
                &invoice,
                &metadata,
                fiat_value.clone(),
            )
            .unwrap();

        let payments = data_store.get_latest_payments(100).unwrap();
        assert_eq!(payments.len(), 2);
        let payment = payments.get(0).unwrap();
        assert_eq!(payment.payment_type, PaymentType::Sending);
        assert_eq!(payment.payment_state, PaymentState::Created);
        assert_eq!(payment.hash, hash);
        assert_eq!(payment.amount_msat, amount_msat);
        assert_eq!(payment.invoice_details.invoice, invoice);
        assert_eq!(payment.description, description);
        assert_eq!(payment.preimage, None);
        assert_eq!(payment.network_fees_msat, None);
        assert_eq!(payment.lsp_fees_msat, None);
        assert_eq!(payment.metadata, metadata);
        assert_eq!(payment.fiat_values, fiat_value);

        assert_eq!(payment.created_at.timezone_id, TEST_TZ_ID);
        assert_eq!(payment.created_at.timezone_utc_offset_secs, TEST_TZ_OFFSET);
        assert_eq!(payment.created_at, payment.latest_state_change_at);
        let created_at = payment.created_at.time;

        // To be able to test the difference between created_at and latest_state_change_at
        sleep(Duration::from_secs(1));

        data_store
            .new_payment_state(hash, PaymentState::Failed)
            .unwrap();
        let payments = data_store.get_latest_payments(100).unwrap();
        assert_eq!(payments.len(), 2);
        let payment = payments.get(0).unwrap();
        assert_eq!(payment.payment_state, PaymentState::Failed);
        assert_eq!(payment.created_at.timezone_id, TEST_TZ_ID);
        assert_eq!(payment.created_at.timezone_utc_offset_secs, TEST_TZ_OFFSET);
        assert_eq!(payment.latest_state_change_at.timezone_id, TEST_TZ_ID);
        assert_eq!(
            payment.latest_state_change_at.timezone_utc_offset_secs,
            TEST_TZ_OFFSET
        );
        assert_eq!(payment.created_at.time, created_at);
        assert_ne!(payment.created_at.time, payment.latest_state_change_at.time);
        assert!(payment.created_at.time < payment.latest_state_change_at.time);

        data_store
            .new_payment_state(hash, PaymentState::Retried)
            .unwrap();
        let payments = data_store.get_latest_payments(100).unwrap();
        assert_eq!(payments.len(), 2);
        let payment = payments.get(0).unwrap();
        assert_eq!(payment.payment_state, PaymentState::Retried);
        assert_eq!(payment.created_at.timezone_id, TEST_TZ_ID);
        assert_eq!(payment.created_at.timezone_utc_offset_secs, TEST_TZ_OFFSET);
        assert_eq!(payment.latest_state_change_at.timezone_id, TEST_TZ_ID);
        assert_eq!(
            payment.latest_state_change_at.timezone_utc_offset_secs,
            TEST_TZ_OFFSET
        );
        assert_eq!(payment.created_at.time, created_at);
        assert_ne!(payment.created_at.time, payment.latest_state_change_at.time);
        assert!(payment.created_at.time < payment.latest_state_change_at.time);

        // New outgoing payment that succeedes
        let hash = "1357";
        let preimage = "2468";
        let amount_msat = 10_000_000;
        let network_fees_msat = 500;
        let description = String::from("Test description 3");
        let invoice = build_invoice(amount_msat, 100);
        let metadata = String::from("Test metadata 3");
        let fiat_value = Some(FiatValues {
            fiat: String::from("USD"),
            amount: 3845,
            amount_usd: 3592,
        });

        data_store
            .new_outgoing_payment(
                hash,
                amount_msat,
                &description,
                &invoice,
                &metadata,
                fiat_value.clone(),
            )
            .unwrap();

        let payments = data_store.get_latest_payments(100).unwrap();
        assert_eq!(payments.len(), 3);
        let payment = payments.get(0).unwrap();
        assert_eq!(payment.payment_type, PaymentType::Sending);
        assert_eq!(payment.payment_state, PaymentState::Created);
        assert_eq!(payment.hash, hash);
        assert_eq!(payment.amount_msat, amount_msat);
        assert_eq!(payment.invoice_details.invoice, invoice);
        assert_eq!(payment.description, description);
        assert_eq!(payment.preimage, None);
        assert_eq!(payment.network_fees_msat, None);
        assert_eq!(payment.lsp_fees_msat, None);
        assert_eq!(payment.metadata, metadata);
        assert_eq!(payment.fiat_values, fiat_value);

        assert_eq!(payment.created_at.timezone_id, TEST_TZ_ID);
        assert_eq!(payment.created_at.timezone_utc_offset_secs, TEST_TZ_OFFSET);
        assert_eq!(payment.created_at, payment.latest_state_change_at);
        let created_at = payment.created_at.time;

        // To be able to test the difference between created_at and latest_state_change_at
        sleep(Duration::from_secs(1));

        data_store
            .outgoing_payment_succeeded(hash, preimage, network_fees_msat)
            .unwrap();
        let payments = data_store.get_latest_payments(100).unwrap();
        assert_eq!(payments.len(), 3);
        let payment = payments.get(0).unwrap();
        assert_eq!(payment.payment_state, PaymentState::Succeeded);
        assert_eq!(payment.preimage, Some(preimage.to_string()));
        assert_eq!(payment.network_fees_msat, Some(network_fees_msat));
        assert_eq!(payment.created_at.timezone_id, TEST_TZ_ID);
        assert_eq!(payment.created_at.timezone_utc_offset_secs, TEST_TZ_OFFSET);
        assert_eq!(payment.latest_state_change_at.timezone_id, TEST_TZ_ID);
        assert_eq!(
            payment.latest_state_change_at.timezone_utc_offset_secs,
            TEST_TZ_OFFSET
        );
        assert_eq!(payment.created_at.time, created_at);
        assert_ne!(payment.created_at.time, payment.latest_state_change_at.time);
        assert!(payment.created_at.time < payment.latest_state_change_at.time);

        let payment_by_hash = data_store.get_payment(hash).unwrap();
        assert_eq!(payment, &payment_by_hash);
    }

    fn reset_db(db_name: &str) {
        let _ = fs::create_dir(TEST_DB_PATH);
        let _ = fs::remove_file(format!("{TEST_DB_PATH}/{db_name}"));
    }

    #[test]
    fn test_fiat_value_from_exchange_rate() {
        let exchange_rates = ExchangeRates {
            currency_code: "EUR".to_string(),
            rate: 5_000,
            usd_rate: 5_000,
        };
        assert_eq!(
            FiatValues::from_amount_msat(1_000, &exchange_rates).amount,
            0
        );
        assert_eq!(
            FiatValues::from_amount_msat(10_000, &exchange_rates).amount,
            2
        );
        assert_eq!(
            FiatValues::from_amount_msat(100_000, &exchange_rates).amount,
            20
        );
        assert_eq!(
            FiatValues::from_amount_msat(1_000_000, &exchange_rates).amount,
            200
        );
        assert_eq!(
            FiatValues::from_amount_msat(10_000_000, &exchange_rates).amount,
            2_000
        );
    }

    #[test]
    fn test_process_expired_payments() {
        let db_name = String::from("process_expired_payments.db3");
        reset_db(&db_name);
        let tz_config = TzConfig {
            timezone_id: String::from(TEST_TZ_ID),
            timezone_utc_offset_secs: TEST_TZ_OFFSET,
        };
        let mut data_store =
            DataStore::new(&format!("{TEST_DB_PATH}/{db_name}"), tz_config).unwrap();

        let amount_msat = 5_000_000;
        let _network_fees_msat = 2_000;
        let description = String::from("Test description 2");
        let invoice = String::from("lnbcrt50u1p37590hdqqpp5wkf8saa4g3ejjhyh89uf5svhlus0ajrz0f9dm6tqnwxtupq3lyeqsp528valrymd092ev6s0srcwcnc3eufhnv453fzj7m5nscj2ejzvx7q9qrsgqnp4qfalfq06c807p3mlt4ggtufckg3nq79wnh96zjz748zmhl5vys3dgcqzysrzjqfky0rtekx6249z2dgvs4wc474q7yg3sx2u7hlvpua5ep5zla3akzqqqqyqqqqqqqqqqqqlgqqqqqqgqjq7n9ukth32d98unkxe692hgd7ke2vskmfz8d2s0part2ycd4vqneq3qgrj2jkvkq2vraa29xsll9lajgdq33yn76ny4h3wacsfxrdudcp575kp6");
        let metadata = String::from("Test metadata 2");
        let fiat_value = Some(FiatValues {
            fiat: String::from("CHF"),
            amount: 4253,
            amount_usd: 4103,
        });

        // Create a payment for each possible state payments can be in
        //      * Receiving payments can only have state "Created", "Succeeded" or "InvoiceExpired"
        //      * Sending payments can have any of the 5 existing states
        for i in 0..5 {
            data_store
                .new_outgoing_payment(
                    &i.to_string(),
                    amount_msat,
                    &description,
                    &invoice,
                    &metadata,
                    fiat_value.clone(),
                )
                .unwrap();
        }
        for i in 5..8 {
            data_store
                .new_incoming_payment(
                    &i.to_string(),
                    amount_msat,
                    0,
                    &description,
                    &invoice,
                    &metadata,
                    fiat_value.clone(),
                )
                .unwrap();
        }

        // Set the states
        data_store
            .new_payment_state("1", PaymentState::Succeeded)
            .unwrap();
        data_store
            .new_payment_state("2", PaymentState::Failed)
            .unwrap();
        data_store
            .new_payment_state("3", PaymentState::Retried)
            .unwrap();
        data_store
            .new_payment_state("4", PaymentState::InvoiceExpired)
            .unwrap();
        data_store
            .new_payment_state("6", PaymentState::Succeeded)
            .unwrap();
        data_store
            .new_payment_state("7", PaymentState::InvoiceExpired)
            .unwrap();

        data_store.process_expired_payments().unwrap();

        assert_eq!(
            data_store.get_payment("0").unwrap().payment_state,
            PaymentState::Created
        );
        assert_eq!(
            data_store.get_payment("1").unwrap().payment_state,
            PaymentState::Succeeded
        );
        assert_eq!(
            data_store.get_payment("2").unwrap().payment_state,
            PaymentState::InvoiceExpired
        );
        assert_eq!(
            data_store.get_payment("3").unwrap().payment_state,
            PaymentState::Retried
        );
        assert_eq!(
            data_store.get_payment("4").unwrap().payment_state,
            PaymentState::InvoiceExpired
        );
        assert_eq!(
            data_store.get_payment("5").unwrap().payment_state,
            PaymentState::InvoiceExpired
        );
        assert_eq!(
            data_store.get_payment("6").unwrap().payment_state,
            PaymentState::Succeeded
        );
        assert_eq!(
            data_store.get_payment("7").unwrap().payment_state,
            PaymentState::InvoiceExpired
        );
    }

    fn build_invoice(amount_msat: u64, expiry_secs: u64) -> String {
        let private_key = SecretKey::from_slice(
            &[
                0xe1, 0x26, 0xf6, 0x8f, 0x7e, 0xaf, 0xcc, 0x8b, 0x74, 0xf5, 0x4d, 0x26, 0x9f, 0xe2,
                0x06, 0xbe, 0x71, 0x50, 0x00, 0xf9, 0x4d, 0xac, 0x06, 0x7d, 0x1c, 0x04, 0xa8, 0xca,
                0x3b, 0x2d, 0xb7, 0x34,
            ][..],
        )
        .unwrap();

        let payment_hash = sha256::Hash::from_slice(&[0; 32][..]).unwrap();
        let payment_secret = PaymentSecret([42u8; 32]);

        let invoice = InvoiceBuilder::new(Currency::Bitcoin)
            .amount_milli_satoshis(amount_msat)
            .description("Coins pls!".into())
            .payment_hash(payment_hash)
            .payment_secret(payment_secret)
            .current_timestamp()
            .expiry_time(Duration::from_secs(expiry_secs))
            .min_final_cltv_expiry_delta(144)
            .build_signed(|hash| Secp256k1::new().sign_ecdsa_recoverable(hash, &private_key))
            .unwrap();

        invoice.to_string()
    }

    #[test]
    fn test_exchange_rate_storage() {
        let db_name = String::from("rates.db3");
        reset_db(&db_name);
        let tz_config = TzConfig {
            timezone_id: String::from(TEST_TZ_ID),
            timezone_utc_offset_secs: TEST_TZ_OFFSET,
        };
        let data_store = DataStore::new(&format!("{TEST_DB_PATH}/{db_name}"), tz_config).unwrap();

        assert!(data_store.get_exchange_rate("USD").unwrap().is_none());

        data_store.update_exchange_rate("USD", 1234).unwrap();
        assert_eq!(data_store.get_exchange_rate("USD").unwrap(), Some(1234));

        data_store.update_exchange_rate("EUR", 5678).unwrap();
        assert_eq!(data_store.get_exchange_rate("USD").unwrap(), Some(1234));
        assert_eq!(data_store.get_exchange_rate("EUR").unwrap(), Some(5678));

        data_store.update_exchange_rate("USD", 4321).unwrap();
        assert_eq!(data_store.get_exchange_rate("USD").unwrap(), Some(4321));
        assert_eq!(data_store.get_exchange_rate("EUR").unwrap(), Some(5678));
    }
}
