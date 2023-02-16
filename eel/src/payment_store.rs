use crate::config::TzConfig;
use crate::errors::Result;
use bitcoin::hashes::hex::ToHex;
use num_enum::TryFromPrimitive;
use perro::{MapToError, OptionToError};
use rusqlite::{Connection, Row};
use std::convert::TryFrom;
use std::time::SystemTime;

#[derive(PartialEq, Eq, Debug, TryFromPrimitive)]
#[repr(u8)]
pub enum PaymentType {
    Receiving,
    Sending,
}

#[derive(PartialEq, Eq, Debug, TryFromPrimitive)]
#[repr(u8)]
pub enum PaymentState {
    Created,
    Succeeded,
    Failed,
}

#[derive(PartialEq, Eq, Debug)]
pub struct TzTime {
    pub timestamp: SystemTime,
    pub timezone_id: String,
    pub timezone_utc_offset_secs: i32,
}

#[derive(PartialEq, Eq, Debug)]
pub struct Payment {
    pub payment_type: PaymentType,
    pub payment_state: PaymentState,
    pub hash: String,
    pub amount_msat: u64,
    pub invoice: String,
    pub created_at: TzTime,
    pub latest_state_change_at: TzTime,
    pub description: String,
    pub preimage: Option<String>,
    pub network_fees_msat: Option<u64>,
    pub lsp_fees_msat: Option<u64>,
    pub metadata: String,
}

pub(crate) struct PaymentStore {
    db_conn: Connection,
    timezone_config: TzConfig,
}

impl PaymentStore {
    pub fn new(db_path: &str, timezone_config: TzConfig) -> Result<Self> {
        let db_conn = Connection::open(db_path).map_to_invalid_input("Invalid db path")?;

        apply_migrations(&db_conn)?;

        Ok(PaymentStore {
            db_conn,
            timezone_config,
        })
    }

    pub fn new_incoming_payment(
        &mut self,
        hash: &[u8],
        amount_msat: u64,
        lsp_fees_msat: u64,
        description: &str,
        invoice: &str,
        metadata: &str,
    ) -> Result<()> {
        let tx = self
            .db_conn
            .transaction()
            .map_to_permanent_failure("Failed to begin SQL transaction")?;
        tx.execute(
            "\
            INSERT INTO payments (type, hash, amount_msat, lsp_fees_msat, description, invoice, metadata) \
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)\
            ",
            (
                PaymentType::Receiving as u8,
                hash,
                amount_msat,
                lsp_fees_msat,
                description,
                invoice,
                metadata
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
        hash: &[u8],
        amount_msat: u64,
        description: &str,
        invoice: &str,
        metadata: &str,
    ) -> Result<()> {
        let tx = self
            .db_conn
            .transaction()
            .map_to_permanent_failure("Failed to begin SQL transaction")?;
        tx.execute(
            "\
            INSERT INTO payments (type, hash, amount_msat, description, invoice, metadata) \
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)\
            ",
            (
                PaymentType::Sending as u8,
                hash,
                amount_msat,
                description,
                invoice,
                metadata,
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

    pub fn incoming_payment_succeeded(&self, hash: &[u8]) -> Result<()> {
        self.insert_payment_succeded_event(hash)
    }

    pub fn outgoing_payment_succeeded(
        &self,
        hash: &[u8],
        preimage: &[u8],
        network_fees_msat: u64,
    ) -> Result<()> {
        self.insert_payment_succeded_event(hash)?;
        self.fill_preimage(hash, preimage)?;
        self.fill_network_fees(hash, network_fees_msat)
    }

    fn insert_payment_succeded_event(&self, hash: &[u8]) -> Result<()> {
        self.db_conn
            .execute(
                "\
                INSERT INTO events (payment_id, type, timezone_id, timezone_utc_offset_secs) \
                VALUES (
                    (SELECT payment_id FROM payments WHERE hash=?1), ?2, ?3, ?4)
                ",
                (
                    hash,
                    PaymentState::Succeeded as u8,
                    &self.timezone_config.timezone_id,
                    self.timezone_config.timezone_utc_offset_secs,
                ),
            )
            .map_to_invalid_input("Failed to add payment confirmed event to payments db")?;

        Ok(())
    }

    pub fn payment_failed(&self, hash: &[u8]) -> Result<()> {
        self.db_conn
            .execute(
                "\
                INSERT INTO events (payment_id, type, timezone_id, timezone_utc_offset_secs) \
                VALUES (
                    (SELECT payment_id FROM payments WHERE hash=?1), ?2, ?3, ?4)
                ",
                (
                    hash,
                    PaymentState::Failed as u8,
                    &self.timezone_config.timezone_id,
                    self.timezone_config.timezone_utc_offset_secs,
                ),
            )
            .map_to_invalid_input("Failed to add payment failed event to payments db")?;

        Ok(())
    }

    pub fn fill_preimage(&self, hash: &[u8], preimage: &[u8]) -> Result<()> {
        self.db_conn
            .execute(
                "\
            UPDATE payments \
            SET preimage=?1 \
            WHERE hash=?2 \
            ",
                (preimage, hash),
            )
            .map_to_invalid_input("Failed to insert preimage into payment db")?;

        Ok(())
    }

    fn fill_network_fees(&self, hash: &[u8], network_fees_msat: u64) -> Result<()> {
        self.db_conn
            .execute(
                "\
            UPDATE payments \
            SET network_fees_msat=?1 \
            WHERE hash=?2 \
            ",
                (network_fees_msat, hash),
            )
            .map_to_invalid_input("Failed to insert network fee into payment db")?;

        Ok(())
    }

    pub fn get_latest_payments(&self, number_of_payments: u32) -> Result<Vec<Payment>> {
        let mut statement = self
            .db_conn
            .prepare("\
            SELECT payments.payment_id, payments.type, hash, preimage, amount_msat, network_fees_msat, \
            lsp_fees_msat, invoice, metadata, recent_events.type as state, recent_events.inserted_at, \
            recent_events.timezone_id, recent_events.timezone_utc_offset_secs, description, \
            creation_events.inserted_at, creation_events.timezone_id, creation_events.timezone_utc_offset_secs \
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
            payments.push(payment.map_to_permanent_failure("Corrupted payment db")?);
        }

        Ok(payments)
    }

    pub fn get_payment(&self, hash: &[u8]) -> Result<Payment> {
        let mut statement = self
            .db_conn
            .prepare("\
            SELECT payments.payment_id, payments.type, hash, preimage, amount_msat, network_fees_msat, \
            lsp_fees_msat, invoice, metadata, recent_events.type as state, recent_events.inserted_at, \
            recent_events.timezone_id, recent_events.timezone_utc_offset_secs, description, \
            creation_events.inserted_at, creation_events.timezone_id, creation_events.timezone_utc_offset_secs \
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
            .map_to_permanent_failure("Corrupted payment db")?;
        Ok(payment)
    }

    pub fn payment_exists(&self, hash: &[u8]) -> Result<bool> {
        let mut statement = self
            .db_conn
            .prepare(
                "\
            SELECT payment_id \
            FROM payments \
            WHERE payments.hash=? \
            ",
            )
            .map_to_permanent_failure("Failed to prepare SQL query")?;
        let mut payment_iter = statement
            .query([hash])
            .map_to_permanent_failure("Failed to bind parameter to prepared SQL query")?;

        Ok(payment_iter
            .next()
            .map_to_permanent_failure("Corrupted payment db")?
            .is_some())
    }
}

fn payment_from_row(row: &Row) -> rusqlite::Result<Payment> {
    let payment_type: u8 = row.get(1)?;
    let payment_type =
        PaymentType::try_from(payment_type).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let hash: Vec<u8> = row.get(2)?;
    let hash = hash.to_hex();
    let preimage: Option<Vec<u8>> = row.get(3)?;
    let preimage = preimage.map(|p| p.to_hex());
    let amount_msat = row.get(4)?;
    let network_fees_msat = row.get(5)?;
    let lsp_fees_msat = row.get(6)?;
    let invoice = row.get(7)?;
    let metadata = row.get(8)?;
    let payment_state: u8 = row.get(9)?;
    let payment_state =
        PaymentState::try_from(payment_state).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let latest_state_change_at_timestamp: chrono::DateTime<chrono::Utc> = row.get(10)?;
    let latest_state_change_at_timezone_id = row.get(11)?;
    let latest_state_change_at_timezone_utc_offset_secs = row.get(12)?;
    let latest_state_change_at = TzTime {
        timestamp: SystemTime::from(latest_state_change_at_timestamp),
        timezone_id: latest_state_change_at_timezone_id,
        timezone_utc_offset_secs: latest_state_change_at_timezone_utc_offset_secs,
    };
    let description = row.get(13)?;
    let created_at_timestamp: chrono::DateTime<chrono::Utc> = row.get(14)?;
    let created_at_timezone_id = row.get(15)?;
    let created_at_timezone_utc_offset_secs = row.get(16)?;
    let created_at = TzTime {
        timestamp: SystemTime::from(created_at_timestamp),
        timezone_id: created_at_timezone_id,
        timezone_utc_offset_secs: created_at_timezone_utc_offset_secs,
    };
    Ok(Payment {
        payment_type,
        payment_state,
        hash,
        amount_msat,
        invoice,
        created_at,
        latest_state_change_at,
        description,
        preimage,
        network_fees_msat,
        lsp_fees_msat,
        metadata,
    })
}

fn apply_migrations(db_conn: &Connection) -> Result<()> {
    db_conn
        .execute_batch(
            "\
            CREATE TABLE IF NOT EXISTS payments (
              payment_id INTEGER NOT NULL PRIMARY KEY,
              type INTEGER CHECK( type IN (0, 1) ) NOT NULL,
              hash BLOB NOT NULL UNIQUE,
              amount_msat INTEGER NOT NULL,
              invoice TEXT NOT NULL,
              description TEXT NOT NULL,
              preimage BLOB,
              network_fees_msat INTEGER,
              lsp_fees_msat INTEGER,
              metadata TEXT
            );
            CREATE TABLE IF NOT EXISTS events (
              event_id INTEGER NOT NULL PRIMARY KEY,
              payment_id INTEGER NOT NULL,
              type INTEGER CHECK( type in (0, 1, 2) ) NOT NULL,
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
        .map_to_permanent_failure("Failed to set up local payment database")
}

#[cfg(test)]
mod tests {
    use crate::config::TzConfig;
    use crate::payment_store::{apply_migrations, PaymentState, PaymentStore, PaymentType};
    use bitcoin::hashes::hex::ToHex;
    use rusqlite::Connection;
    use std::fs;
    use std::thread::sleep;
    use std::time::Duration;

    const TEST_DB_PATH: &str = ".3l_local_test";
    const TEST_TZ_ID: &str = "test_timezone_id";
    const TEST_TZ_OFFSET: i32 = -1352;

    #[test]
    fn test_migrations() {
        let db_name = String::from("migrations.db3");
        reset_db(&db_name);
        let db_conn = Connection::open(format!("{TEST_DB_PATH}/{db_name}")).unwrap();
        apply_migrations(&db_conn).unwrap();
        // Applying migrations on an already setup db is fine
        let db_conn = Connection::open(format!("{TEST_DB_PATH}/{db_name}")).unwrap();
        apply_migrations(&db_conn).unwrap();
    }

    #[test]
    fn test_payment_exists() {
        let db_name = String::from("payment_exists.db3");
        reset_db(&db_name);
        let tz_config = TzConfig {
            timezone_id: String::from(TEST_TZ_ID),
            timezone_utc_offset_secs: TEST_TZ_OFFSET,
        };
        let mut payment_store =
            PaymentStore::new(&format!("{TEST_DB_PATH}/{db_name}"), tz_config).unwrap();

        let hash = vec![1, 2, 3, 4];
        let _preimage = vec![5, 6, 7, 8];
        let amount_msat = 100_000_000;
        let lsp_fees_msat = 2_000_000;
        let description = String::from("Test description 1");
        let invoice = String::from("lnbcrt1m1p37fe7udqqpp5e2mktq6ykgp0e9uljdrakvcy06wcwtswgwe7yl6jmfry4dke2t2ssp5s3uja8xn7tpeuctc62xqua6slpj40jrwlkuwmluv48g86r888g7s9qrsgqnp4qfalfq06c807p3mlt4ggtufckg3nq79wnh96zjz748zmhl5vys3dgcqzysrzjqwp6qac7ttkrd6rgwfte70sjtwxfxmpjk6z2h8vgwdnc88clvac7kqqqqyqqqqqqqqqqqqlgqqqqqqgqjqwhtk6ldnue43vtseuajgyypkv20py670vmcea9qrrdcqjrpp0qvr0sqgcldapjmgfeuvj54q6jt2h36a0m9xme3rywacscd3a5ey3fgpgdr8eq");
        let metadata = String::from("Test metadata 1");

        assert!(!payment_store.payment_exists(&hash).unwrap());

        payment_store
            .new_incoming_payment(
                &hash,
                amount_msat,
                lsp_fees_msat,
                &description,
                &invoice,
                &metadata,
            )
            .unwrap();

        assert!(payment_store.payment_exists(&hash).unwrap());
    }

    #[test]
    fn test_payment_storage_flow() {
        let db_name = String::from("new_payment.db3");
        reset_db(&db_name);
        let tz_config = TzConfig {
            timezone_id: String::from(TEST_TZ_ID),
            timezone_utc_offset_secs: TEST_TZ_OFFSET,
        };
        let mut payment_store =
            PaymentStore::new(&format!("{TEST_DB_PATH}/{db_name}"), tz_config).unwrap();

        let payments = payment_store.get_latest_payments(100).unwrap();
        assert!(payments.is_empty());

        // New incoming payment
        let hash = vec![1, 2, 3, 4];
        let preimage = vec![5, 6, 7, 8];
        let amount_msat = 100_000_000;
        let lsp_fees_msat = 2_000_000;
        let description = String::from("Test description 1");
        let invoice = String::from("lnbcrt1m1p37fe7udqqpp5e2mktq6ykgp0e9uljdrakvcy06wcwtswgwe7yl6jmfry4dke2t2ssp5s3uja8xn7tpeuctc62xqua6slpj40jrwlkuwmluv48g86r888g7s9qrsgqnp4qfalfq06c807p3mlt4ggtufckg3nq79wnh96zjz748zmhl5vys3dgcqzysrzjqwp6qac7ttkrd6rgwfte70sjtwxfxmpjk6z2h8vgwdnc88clvac7kqqqqyqqqqqqqqqqqqlgqqqqqqgqjqwhtk6ldnue43vtseuajgyypkv20py670vmcea9qrrdcqjrpp0qvr0sqgcldapjmgfeuvj54q6jt2h36a0m9xme3rywacscd3a5ey3fgpgdr8eq");
        let metadata = String::from("Test metadata 1");

        payment_store
            .new_incoming_payment(
                &hash,
                amount_msat,
                lsp_fees_msat,
                &description,
                &invoice,
                &metadata,
            )
            .unwrap();

        let payments = payment_store.get_latest_payments(100).unwrap();
        assert_eq!(payments.len(), 1);
        let payment = payments.get(0).unwrap();
        assert_eq!(payment.payment_type, PaymentType::Receiving);
        assert_eq!(payment.payment_state, PaymentState::Created);
        assert_eq!(payment.hash, hash.to_hex());
        assert_eq!(payment.amount_msat, amount_msat);
        assert_eq!(payment.invoice, invoice);
        assert_eq!(payment.description, description);
        assert_eq!(payment.preimage, None);
        assert_eq!(payment.network_fees_msat, None);
        assert_eq!(payment.lsp_fees_msat, Some(lsp_fees_msat));
        assert_eq!(payment.metadata, metadata);

        assert_eq!(payment.created_at.timezone_id, TEST_TZ_ID);
        assert_eq!(payment.created_at.timezone_utc_offset_secs, TEST_TZ_OFFSET);
        assert_eq!(payment.created_at, payment.latest_state_change_at);
        let created_at = payment.created_at.timestamp;

        payment_store.fill_preimage(&hash, &preimage).unwrap();

        let payments = payment_store.get_latest_payments(100).unwrap();
        assert_eq!(payments.len(), 1);
        let payment = payments.get(0).unwrap();
        assert_eq!(payment.preimage, Some(preimage.to_hex()));

        // To be able to test the difference between created_at and latest_state_change_at
        sleep(Duration::from_secs(1));

        payment_store.incoming_payment_succeeded(&hash).unwrap();

        let payments = payment_store.get_latest_payments(100).unwrap();
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
        assert_eq!(payment.created_at.timestamp, created_at);
        assert_ne!(
            payment.created_at.timestamp,
            payment.latest_state_change_at.timestamp
        );
        assert!(payment.created_at.timestamp < payment.latest_state_change_at.timestamp);

        // New outgoing payment that fails
        let hash = vec![5, 6, 7, 8];
        let _preimage = vec![1, 2, 3, 4];
        let amount_msat = 5_000_000;
        let _network_fees_msat = 2_000;
        let description = String::from("Test description 2");
        let invoice = String::from("lnbcrt50u1p37590hdqqpp5wkf8saa4g3ejjhyh89uf5svhlus0ajrz0f9dm6tqnwxtupq3lyeqsp528valrymd092ev6s0srcwcnc3eufhnv453fzj7m5nscj2ejzvx7q9qrsgqnp4qfalfq06c807p3mlt4ggtufckg3nq79wnh96zjz748zmhl5vys3dgcqzysrzjqfky0rtekx6249z2dgvs4wc474q7yg3sx2u7hlvpua5ep5zla3akzqqqqyqqqqqqqqqqqqlgqqqqqqgqjq7n9ukth32d98unkxe692hgd7ke2vskmfz8d2s0part2ycd4vqneq3qgrj2jkvkq2vraa29xsll9lajgdq33yn76ny4h3wacsfxrdudcp575kp6");
        let metadata = String::from("Test metadata 2");

        payment_store
            .new_outgoing_payment(&hash, amount_msat, &description, &invoice, &metadata)
            .unwrap();

        let payments = payment_store.get_latest_payments(100).unwrap();
        assert_eq!(payments.len(), 2);
        let payment = payments.get(0).unwrap();
        assert_eq!(payment.payment_type, PaymentType::Sending);
        assert_eq!(payment.payment_state, PaymentState::Created);
        assert_eq!(payment.hash, hash.to_hex());
        assert_eq!(payment.amount_msat, amount_msat);
        assert_eq!(payment.invoice, invoice);
        assert_eq!(payment.description, description);
        assert_eq!(payment.preimage, None);
        assert_eq!(payment.network_fees_msat, None);
        assert_eq!(payment.lsp_fees_msat, None);
        assert_eq!(payment.metadata, metadata);

        assert_eq!(payment.created_at.timezone_id, TEST_TZ_ID);
        assert_eq!(payment.created_at.timezone_utc_offset_secs, TEST_TZ_OFFSET);
        assert_eq!(payment.created_at, payment.latest_state_change_at);
        let created_at = payment.created_at.timestamp;

        // To be able to test the difference between created_at and latest_state_change_at
        sleep(Duration::from_secs(1));

        payment_store.payment_failed(&hash).unwrap();
        let payments = payment_store.get_latest_payments(100).unwrap();
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
        assert_eq!(payment.created_at.timestamp, created_at);
        assert_ne!(
            payment.created_at.timestamp,
            payment.latest_state_change_at.timestamp
        );
        assert!(payment.created_at.timestamp < payment.latest_state_change_at.timestamp);

        // New outgoing payment that succeedes
        let hash = vec![1, 3, 5, 7];
        let preimage = vec![2, 4, 6, 8];
        let amount_msat = 10_000_000;
        let network_fees_msat = 500;
        let description = String::from("Test description 3");
        let invoice = String::from("lnbcrt100u1p375x7sdqqpp57argaznwm93lk9tvtpgj5mjr2pqh6gr4yp3rcsuzcv3xvz7hvg2ssp5edk06za3w47ww4x20zvja82ysql87ekn8zzvgg67ylkpt8pnjfws9qrsgqnp4qfalfq06c807p3mlt4ggtufckg3nq79wnh96zjz748zmhl5vys3dgcqzysrzjqfky0rtekx6249z2dgvs4wc474q7yg3sx2u7hlvpua5ep5zla3akzqqqqyqqqqqqqqqqqqlgqqqqqqgqjqgdqgl6n4qmkchkuvdzjjlun8lc524g57qwn2ctwxywdckxucwccjf692rynl4rnjq2qnepntg28umsvcdrthmn9fnlezu0kskmpujzcpvsvuml");
        let metadata = String::from("Test metadata 3");

        payment_store
            .new_outgoing_payment(&hash, amount_msat, &description, &invoice, &metadata)
            .unwrap();

        let payments = payment_store.get_latest_payments(100).unwrap();
        assert_eq!(payments.len(), 3);
        let payment = payments.get(0).unwrap();
        assert_eq!(payment.payment_type, PaymentType::Sending);
        assert_eq!(payment.payment_state, PaymentState::Created);
        assert_eq!(payment.hash, hash.to_hex());
        assert_eq!(payment.amount_msat, amount_msat);
        assert_eq!(payment.invoice, invoice);
        assert_eq!(payment.description, description);
        assert_eq!(payment.preimage, None);
        assert_eq!(payment.network_fees_msat, None);
        assert_eq!(payment.lsp_fees_msat, None);
        assert_eq!(payment.metadata, metadata);

        assert_eq!(payment.created_at.timezone_id, TEST_TZ_ID);
        assert_eq!(payment.created_at.timezone_utc_offset_secs, TEST_TZ_OFFSET);
        assert_eq!(payment.created_at, payment.latest_state_change_at);
        let created_at = payment.created_at.timestamp;

        // To be able to test the difference between created_at and latest_state_change_at
        sleep(Duration::from_secs(1));

        payment_store
            .outgoing_payment_succeeded(&hash, &preimage, network_fees_msat)
            .unwrap();
        let payments = payment_store.get_latest_payments(100).unwrap();
        assert_eq!(payments.len(), 3);
        let payment = payments.get(0).unwrap();
        assert_eq!(payment.payment_state, PaymentState::Succeeded);
        assert_eq!(payment.preimage, Some(preimage.to_hex()));
        assert_eq!(payment.network_fees_msat, Some(network_fees_msat));
        assert_eq!(payment.created_at.timezone_id, TEST_TZ_ID);
        assert_eq!(payment.created_at.timezone_utc_offset_secs, TEST_TZ_OFFSET);
        assert_eq!(payment.latest_state_change_at.timezone_id, TEST_TZ_ID);
        assert_eq!(
            payment.latest_state_change_at.timezone_utc_offset_secs,
            TEST_TZ_OFFSET
        );
        assert_eq!(payment.created_at.timestamp, created_at);
        assert_ne!(
            payment.created_at.timestamp,
            payment.latest_state_change_at.timestamp
        );
        assert!(payment.created_at.timestamp < payment.latest_state_change_at.timestamp);

        let payment_by_hash = payment_store.get_payment(&hash).unwrap();
        assert_eq!(payment, &payment_by_hash);
    }

    fn reset_db(db_name: &str) {
        let _ = fs::create_dir(TEST_DB_PATH);
        let _ = fs::remove_file(format!("{TEST_DB_PATH}/{db_name}"));
    }
}
