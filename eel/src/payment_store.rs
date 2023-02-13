use crate::errors::Result;
use perro::MapToError;
use rusqlite::{Connection, Row};
use std::time::SystemTime;

const PAYMENT_TYPE_RECEIVING: u8 = 0;
const PAYMENT_TYPE_SENDING: u8 = 1;

const PAYMENT_STATE_CREATED: u8 = 0;
const PAYMENT_STATE_SUCCEEDED: u8 = 1;
const PAYMENT_STATE_FAILED: u8 = 2;

#[derive(PartialEq, Eq, Debug)]
pub enum PaymentType {
    Receiving,
    Sending,
}

impl TryFrom<u8> for PaymentType {
    type Error = ();

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        match value {
            PAYMENT_TYPE_RECEIVING => Ok(PaymentType::Receiving),
            PAYMENT_TYPE_SENDING => Ok(PaymentType::Sending),
            _ => Err(()),
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub enum PaymentState {
    Created,
    Succeeded,
    Failed,
}

impl TryFrom<u8> for PaymentState {
    type Error = ();

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        match value {
            PAYMENT_STATE_CREATED => Ok(PaymentState::Created),
            PAYMENT_STATE_SUCCEEDED => Ok(PaymentState::Succeeded),
            PAYMENT_STATE_FAILED => Ok(PaymentState::Failed),
            _ => Err(()),
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub struct Payment {
    pub payment_type: PaymentType,
    pub payment_state: PaymentState,
    pub hash: Vec<u8>,
    pub amount_msat: u64,
    pub invoice: String,
    pub timestamp: SystemTime,
    pub preimage: Option<Vec<u8>>,
    pub network_fees_msat: Option<u64>,
    pub lsp_fees_msat: Option<u64>,
    pub metadata: Option<Vec<u8>>,
}

pub(crate) struct PaymentStore {
    db_conn: Connection,
}

#[allow(dead_code)]
impl PaymentStore {
    pub fn new(db_path: &str) -> Result<Self> {
        let db_conn = Connection::open(db_path).map_to_invalid_input("Invalid db path")?;

        apply_migrations(&db_conn)?;

        Ok(PaymentStore { db_conn })
    }

    pub fn new_incoming_payment(
        &mut self,
        hash: &[u8],
        amount_msat: u64,
        lsp_fees_msat: u64,
        invoice: &str,
    ) -> Result<()> {
        let tx = self
            .db_conn
            .transaction()
            .map_to_permanent_failure("Failed to begin SQL transaction")?;
        tx.execute(
            "\
            INSERT INTO payments (type, hash, amount_msat, lsp_fees_msat, invoice) \
            VALUES (?1, ?2, ?3, ?4, ?5)\
            ",
            (
                PAYMENT_TYPE_RECEIVING,
                hash,
                amount_msat,
                lsp_fees_msat,
                invoice,
            ),
        )
        .map_to_invalid_input("Failed to add new incoming payment to payments db")?;
        tx.execute(
            "\
            INSERT INTO events (payment_id, type) \
            VALUES (?1, ?2) \
            ",
            (tx.last_insert_rowid(), PAYMENT_STATE_CREATED),
        )
        .map_to_invalid_input("Failed to add new incoming payment to payments db")?;
        tx.commit()
            .map_to_permanent_failure("Failed to commit new incoming payment transaction")
    }

    pub fn payment_succeeded(&self, hash: &[u8]) -> Result<()> {
        self.db_conn
            .execute(
                "\
                INSERT INTO events (payment_id, type) \
                VALUES (
                    (SELECT payment_id FROM payments WHERE hash=?1), ?2)
                ",
                (hash, PAYMENT_STATE_SUCCEEDED),
            )
            .map_to_invalid_input("Failed to add payment confirmed event to payments db")?;

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

    pub fn get_latest_payments(&self, number_of_payments: u32) -> Result<Vec<Payment>> {
        let mut statement = self
            .db_conn
            .prepare("\
            SELECT payments.payment_id, payments.type, hash, preimage, amount_msat, network_fees_msat, lsp_fees_msat, invoice, metadata, recent_events.type as state, recent_events.inserted_at \
            FROM payments \
            JOIN ( \
                SELECT * \
                FROM events \
                GROUP BY payment_id \
                HAVING MAX(event_id) \
            ) AS recent_events ON payments.payment_id=recent_events.payment_id \
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
}

fn payment_from_row(row: &Row) -> rusqlite::Result<Payment> {
    let payment_type: u8 = row.get(1)?;
    let payment_type =
        PaymentType::try_from(payment_type).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let hash = row.get(2)?;
    let preimage = row.get(3)?;
    let amount_msat = row.get(4)?;
    let network_fees_msat = row.get(5)?;
    let lsp_fees_msat = row.get(6)?;
    let invoice = row.get(7)?;
    let metadata = row.get(8)?;
    let payment_state: u8 = row.get(9)?;
    let payment_state =
        PaymentState::try_from(payment_state).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let timestamp: chrono::DateTime<chrono::Utc> = row.get(10)?;
    let timestamp = SystemTime::from(timestamp);
    Ok(Payment {
        payment_type,
        payment_state,
        hash,
        amount_msat,
        invoice,
        timestamp,
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
              hash BLOB NOT NULL,
              amount_msat INTEGER NOT NULL,
              invoice TEXT NOT NULL,
              preimage BLOB,
              network_fees_msat INTEGER,
              lsp_fees_msat INTEGER,
              metadata BLOB
            );
            CREATE TABLE IF NOT EXISTS events (
              event_id INTEGER NOT NULL PRIMARY KEY,
              payment_id INTEGER NOT NULL,
              type INTEGER CHECK( type in (0, 1, 2) ) NOT NULL,
              inserted_at INTEGER NOT NULL DEFAULT CURRENT_TIMESTAMP,
              FOREIGN KEY (payment_id) REFERENCES payments(payment_id)
            );
        ",
        )
        .map_to_permanent_failure("Failed to set up local payment database")
}

#[cfg(test)]
mod tests {
    use crate::payment_store::{apply_migrations, PaymentState, PaymentStore, PaymentType};
    use rusqlite::Connection;
    use std::fs;

    const TEST_DB_PATH: &str = ".3l_local_test";

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
    fn test_payment_storage_flow() {
        let db_name = String::from("new_payment.db3");
        reset_db(&db_name);
        let mut payment_store = PaymentStore::new(&format!("{TEST_DB_PATH}/{db_name}")).unwrap();

        let payments = payment_store.get_latest_payments(100).unwrap();
        assert!(payments.is_empty());

        let hash = vec![1, 2, 3, 4];
        let preimage = vec![5, 6, 7, 8];
        let amount_msat = 100_000_000;
        let lsp_fees_msat = 2_000_000;
        let invoice = String::from("lnbcrt1m1p37fe7udqqpp5e2mktq6ykgp0e9uljdrakvcy06wcwtswgwe7yl6jmfry4dke2t2ssp5s3uja8xn7tpeuctc62xqua6slpj40jrwlkuwmluv48g86r888g7s9qrsgqnp4qfalfq06c807p3mlt4ggtufckg3nq79wnh96zjz748zmhl5vys3dgcqzysrzjqwp6qac7ttkrd6rgwfte70sjtwxfxmpjk6z2h8vgwdnc88clvac7kqqqqyqqqqqqqqqqqqlgqqqqqqgqjqwhtk6ldnue43vtseuajgyypkv20py670vmcea9qrrdcqjrpp0qvr0sqgcldapjmgfeuvj54q6jt2h36a0m9xme3rywacscd3a5ey3fgpgdr8eq");

        payment_store
            .new_incoming_payment(&hash, amount_msat, lsp_fees_msat, &invoice)
            .unwrap();

        let payments = payment_store.get_latest_payments(100).unwrap();
        assert_eq!(payments.len(), 1);
        let payment = payments.get(0).unwrap();
        assert_eq!(payment.payment_type, PaymentType::Receiving);
        assert_eq!(payment.payment_state, PaymentState::Created);
        assert_eq!(payment.hash, hash);
        assert_eq!(payment.amount_msat, amount_msat);
        assert_eq!(payment.invoice, invoice);
        assert_eq!(payment.preimage, None);
        assert_eq!(payment.network_fees_msat, None);
        assert_eq!(payment.lsp_fees_msat, Some(lsp_fees_msat));
        assert_eq!(payment.metadata, None);

        payment_store.fill_preimage(&hash, &preimage).unwrap();

        let payments = payment_store.get_latest_payments(100).unwrap();
        assert_eq!(payments.len(), 1);
        let payment = payments.get(0).unwrap();
        assert_eq!(payment.preimage, Some(preimage));

        payment_store.payment_succeeded(&hash).unwrap();

        let payments = payment_store.get_latest_payments(100).unwrap();
        assert_eq!(payments.len(), 1);
        let payment = payments.get(0).unwrap();
        assert_eq!(payment.payment_state, PaymentState::Succeeded);
    }

    fn reset_db(db_name: &str) {
        let _ = fs::create_dir(TEST_DB_PATH);
        let _ = fs::remove_file(format!("{TEST_DB_PATH}/{db_name}"));
    }
}
