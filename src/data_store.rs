use crate::errors::Result;
use crate::fund_migration::MigrationStatus;
use crate::migrations::migrate;
use crate::{ExchangeRate, OfferKind, PocketOfferError, TzConfig, UserPreferences};

use crate::fiat_topup::FiatTopupInfo;
use chrono::{DateTime, Utc};
use crow::{PermanentFailureCode, TemporaryFailureCode};
use perro::MapToError;
use rusqlite::Row;
use rusqlite::{backup, params, Connection};
use std::time::{Duration, SystemTime};

pub(crate) const BACKUP_DB_FILENAME_SUFFIX: &str = ".backup";

pub(crate) struct LocalPaymentData {
    pub user_preferences: UserPreferences,
    pub exchange_rate: ExchangeRate,
    pub offer: Option<OfferKind>,
}

#[derive(Clone, Copy)]
pub(crate) enum BackupStatus {
    Complete,
    WaitingForBackup,
}

pub(crate) struct DataStore {
    conn: Connection,
    backup_conn: Connection,
    pub backup_status: BackupStatus,
}

#[derive(PartialEq, Debug, Clone)]
pub(crate) struct CreatedInvoice {
    pub hash: String,
    pub invoice: String,
    pub channel_opening_fees: Option<u64>,
}

impl DataStore {
    pub fn new(db_path: &str) -> Result<Self> {
        let mut conn = Connection::open(db_path).map_to_invalid_input("Invalid db path")?;
        let backup_conn = Connection::open(format!("{db_path}{BACKUP_DB_FILENAME_SUFFIX}"))
            .map_to_permanent_failure("Failed to open backup db connection")?;
        migrate(&mut conn)?;
        Ok(DataStore {
            conn,
            backup_conn,
            backup_status: BackupStatus::WaitingForBackup,
        })
    }

    pub fn store_payment_info(
        &mut self,
        payment_hash: &str,
        user_preferences: UserPreferences,
        exchange_rates: Vec<ExchangeRate>,
        offer: Option<OfferKind>,
    ) -> Result<()> {
        self.backup_status = BackupStatus::WaitingForBackup;
        let tx = self
            .conn
            .transaction()
            .map_to_permanent_failure("Failed to begin SQL transaction")?;

        let snapshot_id = insert_exchange_rate_snapshot(&tx, exchange_rates)?;

        tx.execute(
            "\
            INSERT INTO payments (hash, timezone_id, timezone_utc_offset_secs, fiat_currency, exchange_rates_history_snaphot_id)\
            VALUES (?1, ?2, ?3, ?4, ?5)\
            ",
            (
                payment_hash,
                &user_preferences.timezone_config.timezone_id,
                user_preferences.timezone_config.timezone_utc_offset_secs,
                user_preferences.fiat_currency,
                snapshot_id,
            ),
        )
        .map_to_permanent_failure("Failed to add payment info to db")?;

        if let Some(OfferKind::Pocket {
            id: pocket_id,
            exchange_rate:
                ExchangeRate {
                    currency_code,
                    rate,
                    updated_at,
                },
            topup_value_minor_units,
            exchange_fee_minor_units,
            exchange_fee_rate_permyriad,
            error,
        }) = offer
        {
            let exchanged_at: DateTime<Utc> = updated_at.into();
            tx.execute(
            "\
                INSERT INTO offers (payment_hash, pocket_id, fiat_currency, rate, exchanged_at, topup_value_minor_units, exchange_fee_minor_units, exchange_fee_rate_permyriad, error)\
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)\
                ",
        (
                    payment_hash,
                    &pocket_id,
                    &currency_code,
                    &rate,
                    &exchanged_at,
                    topup_value_minor_units,
                    exchange_fee_minor_units,
                    exchange_fee_rate_permyriad,
                    from_offer_error(error)
                ),
            )
            .map_to_invalid_input("Failed to add new incoming pocket offer to offers db")?;
        };

        tx.commit()
            .map_to_permanent_failure("Failed to commit the db transaction")
    }

    pub fn retrieve_payment_info(&self, payment_hash: &str) -> Result<Option<LocalPaymentData>> {
        let mut statement = self
            .conn
            .prepare(
                " \
            SELECT timezone_id, timezone_utc_offset_secs, payments.fiat_currency, h.rate, h.updated_at,  \
            o.pocket_id, o.fiat_currency, o.rate, o.exchanged_at, o.topup_value_minor_units, \
			o.exchange_fee_minor_units, o.exchange_fee_rate_permyriad, o.error \
            FROM payments \
            LEFT JOIN exchange_rates_history h on payments.exchange_rates_history_snaphot_id=h.snapshot_id \
                AND payments.fiat_currency=h.fiat_currency \
            LEFT JOIN offers o ON o.payment_hash=payments.hash \
            WHERE hash=? \
            ",
            )
            .map_to_permanent_failure("Failed to prepare SQL query")?;

        let mut payment_iter = statement
            .query_map([payment_hash], local_payment_data_from_row)
            .map_to_permanent_failure("Failed to bind parameter to prepared SQL query")?;

        match payment_iter.next() {
            None => Ok(None),
            Some(p) => Ok(Some(p.map_to_permanent_failure("Corrupted db")?)),
        }
    }

    pub fn store_created_invoice(
        &mut self,
        hash: &str,
        invoice: &str,
        channel_opening_fees: &Option<u64>,
    ) -> Result<()> {
        self.backup_status = BackupStatus::WaitingForBackup;
        self.conn
            .execute(
                "\
            INSERT INTO created_invoices (hash, invoice, channel_opening_fees)\
            VALUES (?1, ?2, ?3)\
            ",
                params![hash, invoice, channel_opening_fees],
            )
            .map_to_permanent_failure("Failed to store created invoice to local db")?;
        Ok(())
    }

    pub fn retrieve_created_invoices(
        &self,
        number_of_invoices: u32,
    ) -> Result<Vec<CreatedInvoice>> {
        let mut statement = self
            .conn
            .prepare(
                "\
            SELECT hash, invoice, channel_opening_fees \
            FROM created_invoices \
            ORDER BY id DESC \
            LIMIT ?1;
        ",
            )
            .map_to_permanent_failure("Failed to retrieve created invoice from local db")?;

        let invoice_iter = statement
            .query_map([number_of_invoices], |r| {
                Ok(CreatedInvoice {
                    hash: r.get(0)?,
                    invoice: r.get(1)?,
                    channel_opening_fees: r.get(2)?,
                })
            })
            .map_to_permanent_failure("Failed to bind parameter to prepared SQL query")?;

        let mut invoices = Vec::new();
        for rate in invoice_iter {
            invoices.push(rate.map_to_permanent_failure("Corrupted db")?);
        }
        Ok(invoices)
    }

    pub fn retrieve_created_invoice_by_hash(&self, hash: &str) -> Result<Option<CreatedInvoice>> {
        let mut statement = self
            .conn
            .prepare(
                "\
            SELECT invoice, channel_opening_fees \
            FROM created_invoices \
            WHERE hash=?1;
        ",
            )
            .map_to_permanent_failure("Failed to retrieve created invoice from local db")?;

        let mut invoice_iter = statement
            .query_map([hash], |r| {
                Ok(CreatedInvoice {
                    hash: hash.to_string(),
                    invoice: r.get(0)?,
                    channel_opening_fees: r.get(1)?,
                })
            })
            .map_to_permanent_failure("Failed to bind parameter to prepared SQL query")?
            .filter_map(|i| i.ok());

        Ok(invoice_iter.next())
    }

    pub fn update_exchange_rate(
        &mut self,
        currency_code: &str,
        rate: u32,
        updated_at: SystemTime,
    ) -> Result<()> {
        self.backup_status = BackupStatus::WaitingForBackup;
        let dt: DateTime<Utc> = updated_at.into();
        self.conn
            .execute(
                "\
                REPLACE INTO exchange_rates (fiat_currency, rate, updated_at) \
                VALUES (?1, ?2, ?3)
                ",
                (currency_code, rate, dt),
            )
            .map_to_invalid_input("Failed to update exchange rate in db")?;

        Ok(())
    }

    pub fn get_all_exchange_rates(&self) -> Result<Vec<ExchangeRate>> {
        let mut statement = self
            .conn
            .prepare(
                " \
            SELECT fiat_currency, rate, updated_at \
            FROM exchange_rates \
            ",
            )
            .map_to_permanent_failure("Failed to prepare SQL query")?;

        let rate_iter = statement
            .query_map([], exchange_rate_from_row)
            .map_to_permanent_failure("Failed to bind parameter to prepared SQL query")?;

        let mut rates = Vec::new();
        for rate in rate_iter {
            rates.push(rate.map_to_permanent_failure("Corrupted db")?);
        }
        Ok(rates)
    }

    #[allow(dead_code)]
    pub fn append_funds_migration_status(&mut self, status: MigrationStatus) -> Result<()> {
        self.backup_status = BackupStatus::WaitingForBackup;
        self.conn
            .execute(
                "INSERT INTO funds_migration_status (status) VALUES (?1)",
                (status as u8,),
            )
            .map_to_permanent_failure("Failed to add funds migration ststus to db")?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn retrive_funds_migration_status(&self) -> Result<MigrationStatus> {
        let status_from_row = |row: &Row| {
            let status: u8 = row.get(0)?;
            MigrationStatus::try_from(status).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Integer,
                    Box::new(e),
                )
            })
        };
        self.conn
            .query_row(
                "SELECT status, updated_at FROM funds_migration_status ORDER BY id DESC LIMIT 1",
                (),
                status_from_row,
            )
            .map_to_permanent_failure("Failed to query funds migration status")
    }

    pub fn store_fiat_topup_info(&self, fiat_topup_info: FiatTopupInfo) -> Result<()> {
        let dt: DateTime<Utc> = SystemTime::now().into();
        self.conn
            .execute(
                "INSERT INTO fiat_topup_info (order_id, created_at, debitor_iban, creditor_reference, creditor_iban, creditor_bank_name,
                             creditor_bank_street, creditor_bank_postal_code, creditor_bank_town, creditor_bank_country,
                             creditor_bank_bic, creditor_name, creditor_street, creditor_postal_code, creditor_town,
                             creditor_country, currency)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17);",
                params![fiat_topup_info.order_id, dt, fiat_topup_info.debitor_iban, fiat_topup_info.creditor_reference, fiat_topup_info.creditor_iban, fiat_topup_info.creditor_bank_name, fiat_topup_info.creditor_bank_street, fiat_topup_info.creditor_bank_postal_code, fiat_topup_info.creditor_bank_town, fiat_topup_info.creditor_bank_country, fiat_topup_info.creditor_bank_bic, fiat_topup_info.creditor_name, fiat_topup_info.creditor_street, fiat_topup_info.creditor_postal_code, fiat_topup_info.creditor_town, fiat_topup_info.creditor_country, fiat_topup_info.currency],
            )
            .map_to_permanent_failure("Failed to store fiat topup info in db")?;
        Ok(())
    }

    pub fn clear_fiat_topup_info(&self) -> Result<()> {
        self.conn
            .execute("DELETE FROM fiat_topup_info;", params![])
            .map_to_permanent_failure("Failed to delete fiat topup info")?;

        Ok(())
    }

    pub fn retrieve_latest_fiat_topup_info(&self) -> Result<Option<FiatTopupInfo>> {
        let mut statement = self.conn.prepare(
            "SELECT order_id, debitor_iban, creditor_reference, creditor_iban, creditor_bank_name, creditor_bank_street, creditor_bank_postal_code, creditor_bank_town, creditor_bank_country, creditor_bank_bic, creditor_name, creditor_street, creditor_postal_code, creditor_town, creditor_country, currency FROM fiat_topup_info ORDER BY created_at DESC LIMIT 1",
        ).map_to_permanent_failure("Failed to prepare query latest fiat topup info statement")?;

        let mut fiat_topup_info_iter = statement
            .query_map([], fiat_topup_info_from_row)
            .map_to_permanent_failure("Failed to bind parameter to prepared SQL query")?;

        match fiat_topup_info_iter.next() {
            None => Ok(None),
            Some(f) => Ok(f.map_to_permanent_failure("Corrupted db")?),
        }
    }
    pub(crate) fn backup_db(&mut self) -> Result<()> {
        let backup = backup::Backup::new(&self.conn, &mut self.backup_conn)
            .map_to_permanent_failure("Failed to create backup instance")?;
        backup
            .run_to_completion(5, Duration::from_millis(250), None)
            .map_to_permanent_failure("Failed to backup db")
    }
}

// Store all provided exchange rates.
// For every row it takes ~13 bytes (4 + 3 + 2 + 4), if we have 100 fiat currencies it adds 1300 bytes.
// For 1000 payments it will add ~1 MB.
fn insert_exchange_rate_snapshot(
    connection: &Connection,
    exchange_rates: Vec<ExchangeRate>,
) -> Result<Option<u64>> {
    if exchange_rates.is_empty() {
        return Ok(None);
    }
    let snapshot_id = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_to_permanent_failure("Failed to obtain duration since unix epoch")?
        .as_secs();
    for exchange_rate in exchange_rates {
        let updated_at: DateTime<Utc> = exchange_rate.updated_at.into();
        connection
            .execute(
                "\
                INSERT INTO exchange_rates_history (snapshot_id, fiat_currency, rate, updated_at) \
                VALUES (?1, ?2, ?3, ?4)",
                (
                    snapshot_id,
                    exchange_rate.currency_code,
                    exchange_rate.rate,
                    updated_at,
                ),
            )
            .map_to_invalid_input("Failed to insert exchange rate history in db")?;
    }
    Ok(Some(snapshot_id))
}

fn exchange_rate_from_row(row: &Row) -> rusqlite::Result<ExchangeRate> {
    let fiat_currency: String = row.get(0)?;
    let rate: u32 = row.get(1)?;
    let updated_at: chrono::DateTime<chrono::Utc> = row.get(2)?;
    Ok(ExchangeRate {
        currency_code: fiat_currency,
        rate,
        updated_at: SystemTime::from(updated_at),
    })
}

fn offer_kind_from_row(row: &Row) -> rusqlite::Result<Option<OfferKind>> {
    let pocket_id: Option<String> = row.get(5)?;
    match pocket_id {
        Some(pocket_id) => {
            let fiat_currency: String = row.get(6)?;
            let rate: u32 = row.get(7)?;
            let exchanged_at: chrono::DateTime<chrono::Utc> = row.get(8)?;
            let exchanged_at = SystemTime::from(exchanged_at);
            let topup_value_minor_units: u64 = row.get(9)?;
            let exchange_fee_minor_units: u64 = row.get(10)?;
            let exchange_fee_rate_permyriad: u16 = row.get(11)?;

            let exchange_rate = ExchangeRate {
                currency_code: fiat_currency,
                rate,
                updated_at: exchanged_at,
            };

            Ok(Some(OfferKind::Pocket {
                id: pocket_id,
                exchange_rate,
                topup_value_minor_units,
                exchange_fee_minor_units,
                exchange_fee_rate_permyriad,
                error: to_offer_error(row.get(12)?),
            }))
        }
        None => Ok(None),
    }
}

fn local_payment_data_from_row(row: &Row) -> rusqlite::Result<LocalPaymentData> {
    let timezone_id: String = row.get(0)?;
    let timezone_utc_offset_secs: i32 = row.get(1)?;
    let fiat_currency: String = row.get(2)?;
    let rate: u32 = row.get(3)?;
    let updated_at: chrono::DateTime<chrono::Utc> = row.get(4)?;
    let offer = offer_kind_from_row(row)?;

    Ok(LocalPaymentData {
        user_preferences: UserPreferences {
            fiat_currency: fiat_currency.clone(),
            timezone_config: TzConfig {
                timezone_id,
                timezone_utc_offset_secs,
            },
        },
        exchange_rate: ExchangeRate {
            currency_code: fiat_currency,
            rate,
            updated_at: SystemTime::from(updated_at),
        },
        offer,
    })
}

pub fn from_offer_error(error: Option<PocketOfferError>) -> Option<String> {
    error.map(|e| match e {
        PocketOfferError::TemporaryFailure { code } => match code {
            TemporaryFailureCode::NoRoute => "no_route".to_string(),
            TemporaryFailureCode::InvoiceExpired => "invoice_expired".to_string(),
            TemporaryFailureCode::Unexpected => "error".to_string(),
            TemporaryFailureCode::Unknown { msg } => msg,
        },
        PocketOfferError::PermanentFailure { code } => match code {
            PermanentFailureCode::ThresholdExceeded => "threshold_exceeded".to_string(),
            PermanentFailureCode::OrderInactive => "order_inactive".to_string(),
            PermanentFailureCode::CompaniesUnsupported => "companies_unsupported".to_string(),
            PermanentFailureCode::CountryUnsupported => "country_unsupported".to_string(),
            PermanentFailureCode::OtherRiskDetected => "other_risk_detected".to_string(),
            PermanentFailureCode::CustomerRequested => "customer_requested".to_string(),
            PermanentFailureCode::AccountNotMatching => "account_not_matching".to_string(),
            PermanentFailureCode::PayoutExpired => "payout_expired".to_string(),
        },
    })
}

pub fn to_offer_error(code: Option<String>) -> Option<PocketOfferError> {
    code.map(|c| match &*c {
        "no_route" => PocketOfferError::TemporaryFailure {
            code: TemporaryFailureCode::NoRoute,
        },
        "invoice_expired" => PocketOfferError::TemporaryFailure {
            code: TemporaryFailureCode::InvoiceExpired,
        },
        "error" => PocketOfferError::TemporaryFailure {
            code: TemporaryFailureCode::Unexpected,
        },
        "threshold_exceeded" => PocketOfferError::PermanentFailure {
            code: PermanentFailureCode::ThresholdExceeded,
        },
        "order_inactive" => PocketOfferError::PermanentFailure {
            code: PermanentFailureCode::OrderInactive,
        },
        "companies_unsupported" => PocketOfferError::PermanentFailure {
            code: PermanentFailureCode::CompaniesUnsupported,
        },
        "country_unsupported" => PocketOfferError::PermanentFailure {
            code: PermanentFailureCode::CountryUnsupported,
        },
        "other_risk_detected" => PocketOfferError::PermanentFailure {
            code: PermanentFailureCode::OtherRiskDetected,
        },
        "customer_requested" => PocketOfferError::PermanentFailure {
            code: PermanentFailureCode::CustomerRequested,
        },
        "account_not_matching" => PocketOfferError::PermanentFailure {
            code: PermanentFailureCode::AccountNotMatching,
        },
        "payout_expired" => PocketOfferError::PermanentFailure {
            code: PermanentFailureCode::PayoutExpired,
        },
        e => PocketOfferError::TemporaryFailure {
            code: TemporaryFailureCode::Unknown { msg: e.to_string() },
        },
    })
}

fn fiat_topup_info_from_row(row: &Row) -> rusqlite::Result<Option<FiatTopupInfo>> {
    Ok(Some(FiatTopupInfo {
        order_id: row.get(0)?,
        debitor_iban: row.get(1)?,
        creditor_reference: row.get(2)?,
        creditor_iban: row.get(3)?,
        creditor_bank_name: row.get(4)?,
        creditor_bank_street: row.get(5)?,
        creditor_bank_postal_code: row.get(6)?,
        creditor_bank_town: row.get(7)?,
        creditor_bank_country: row.get(8)?,
        creditor_bank_bic: row.get(9)?,
        creditor_name: row.get(10)?,
        creditor_street: row.get(11)?,
        creditor_postal_code: row.get(12)?,
        creditor_town: row.get(13)?,
        creditor_country: row.get(14)?,
        currency: row.get(15)?,
    }))
}

#[cfg(test)]
mod tests {
    use crate::config::TzConfig;
    use crate::data_store::{CreatedInvoice, DataStore};
    use crate::fund_migration::MigrationStatus;
    use crate::{ExchangeRate, OfferKind, PocketOfferError, UserPreferences};

    use crate::fiat_topup::FiatTopupInfo;
    use crow::TopupError::TemporaryFailure;
    use crow::{PermanentFailureCode, TemporaryFailureCode};
    use std::fs;
    use std::thread::sleep;
    use std::time::{Duration, SystemTime};

    const TEST_DB_PATH: &str = ".3l_local_test";

    #[test]
    fn test_store_payment_info() {
        let db_name = String::from("db.db3");
        reset_db(&db_name);
        let mut data_store = DataStore::new(&format!("{TEST_DB_PATH}/{db_name}")).unwrap();

        let user_preferences = UserPreferences {
            fiat_currency: "EUR".to_string(),
            timezone_config: TzConfig {
                timezone_id: "Bern".to_string(),
                timezone_utc_offset_secs: -1234,
            },
        };

        let exchange_rates = vec![
            ExchangeRate {
                currency_code: "EUR".to_string(),
                rate: 4123,
                updated_at: SystemTime::now(),
            },
            ExchangeRate {
                currency_code: "USD".to_string(),
                rate: 3950,
                updated_at: SystemTime::now(),
            },
        ];
        let offer_kind = OfferKind::Pocket {
            id: "id".to_string(),
            exchange_rate: ExchangeRate {
                currency_code: "EUR".to_string(),
                rate: 5123,
                updated_at: SystemTime::now(),
            },
            topup_value_minor_units: 51245,
            exchange_fee_minor_units: 123,
            exchange_fee_rate_permyriad: 50,
            error: Some(TemporaryFailure {
                code: TemporaryFailureCode::NoRoute,
            }),
        };

        let offer_kind_no_error = OfferKind::Pocket {
            id: "id".to_string(),
            exchange_rate: ExchangeRate {
                currency_code: "EUR".to_string(),
                rate: 5123,
                updated_at: SystemTime::now(),
            },
            topup_value_minor_units: 51245,
            exchange_fee_minor_units: 123,
            exchange_fee_rate_permyriad: 50,
            error: None,
        };

        data_store
            .store_payment_info("hash", user_preferences.clone(), Vec::new(), None)
            .unwrap();

        // The second call will not fail.
        data_store
            .store_payment_info(
                "hash",
                user_preferences.clone(),
                exchange_rates.clone(),
                Some(offer_kind.clone()),
            )
            .unwrap();

        data_store
            .store_payment_info(
                "hash - no offer",
                user_preferences.clone(),
                exchange_rates.clone(),
                None,
            )
            .unwrap();

        data_store
            .store_payment_info(
                "hash - no error",
                user_preferences.clone(),
                exchange_rates,
                Some(offer_kind_no_error.clone()),
            )
            .unwrap();

        assert!(data_store
            .retrieve_payment_info("non existent hash")
            .unwrap()
            .is_none());

        let local_payment_data = data_store.retrieve_payment_info("hash").unwrap().unwrap();
        assert_eq!(local_payment_data.offer.unwrap(), offer_kind);
        assert_eq!(
            local_payment_data.user_preferences,
            user_preferences.clone()
        );
        assert_eq!(
            local_payment_data.exchange_rate.currency_code,
            user_preferences.fiat_currency
        );
        assert_eq!(local_payment_data.exchange_rate.rate, 4123);

        let local_payment_data = data_store
            .retrieve_payment_info("hash - no offer")
            .unwrap()
            .unwrap();
        assert!(local_payment_data.offer.is_none());
        assert_eq!(
            local_payment_data.user_preferences,
            user_preferences.clone()
        );
        assert_eq!(
            local_payment_data.exchange_rate.currency_code,
            user_preferences.fiat_currency
        );
        assert_eq!(local_payment_data.exchange_rate.rate, 4123);

        let local_payment_data = data_store
            .retrieve_payment_info("hash - no error")
            .unwrap()
            .unwrap();
        assert_eq!(local_payment_data.offer.unwrap(), offer_kind_no_error);
    }
    #[test]
    fn test_offer_storage() {
        let db_name = String::from("offers.db3");
        reset_db(&db_name);
        let mut data_store = DataStore::new(&format!("{TEST_DB_PATH}/{db_name}")).unwrap();

        // Temporary failures
        let offer_kind_no_route = build_offer_kind_with_error(PocketOfferError::TemporaryFailure {
            code: TemporaryFailureCode::NoRoute,
        });
        store_payment_with_offer_and_test(
            offer_kind_no_route,
            &mut data_store,
            "offer_kind_no_route",
        );

        let offer_kind_invoice_expired =
            build_offer_kind_with_error(PocketOfferError::TemporaryFailure {
                code: TemporaryFailureCode::InvoiceExpired,
            });
        store_payment_with_offer_and_test(
            offer_kind_invoice_expired,
            &mut data_store,
            "offer_kind_invoice_expired",
        );

        let offer_kind_unexpected =
            build_offer_kind_with_error(PocketOfferError::TemporaryFailure {
                code: TemporaryFailureCode::Unexpected,
            });
        store_payment_with_offer_and_test(
            offer_kind_unexpected,
            &mut data_store,
            "offer_kind_unexpected",
        );

        let offer_kind_unknown = build_offer_kind_with_error(PocketOfferError::TemporaryFailure {
            code: TemporaryFailureCode::Unknown { msg: "Test".into() },
        });
        store_payment_with_offer_and_test(
            offer_kind_unknown,
            &mut data_store,
            "offer_kind_unknown",
        );

        // Permanent failures
        let offer_kind_threshold_exceeded =
            build_offer_kind_with_error(PocketOfferError::PermanentFailure {
                code: PermanentFailureCode::ThresholdExceeded,
            });
        store_payment_with_offer_and_test(
            offer_kind_threshold_exceeded,
            &mut data_store,
            "offer_kind_threshold_exceeded",
        );

        let offer_kind_order_inactive =
            build_offer_kind_with_error(PocketOfferError::PermanentFailure {
                code: PermanentFailureCode::OrderInactive,
            });
        store_payment_with_offer_and_test(
            offer_kind_order_inactive.clone(),
            &mut data_store,
            "offer_kind_order_inactive",
        );

        let offer_kind_companies_unsupported =
            build_offer_kind_with_error(PocketOfferError::PermanentFailure {
                code: PermanentFailureCode::CompaniesUnsupported,
            });
        store_payment_with_offer_and_test(
            offer_kind_companies_unsupported,
            &mut data_store,
            "offer_kind_companies_unsupported",
        );

        let offer_kind_country_unsuported =
            build_offer_kind_with_error(PocketOfferError::PermanentFailure {
                code: PermanentFailureCode::CountryUnsupported,
            });
        store_payment_with_offer_and_test(
            offer_kind_country_unsuported,
            &mut data_store,
            "offer_kind_country_unsuported",
        );

        let offer_kind_other_risk_detected =
            build_offer_kind_with_error(PocketOfferError::PermanentFailure {
                code: PermanentFailureCode::OtherRiskDetected,
            });
        store_payment_with_offer_and_test(
            offer_kind_other_risk_detected,
            &mut data_store,
            "offer_kind_other_risk_detected",
        );

        let offer_kind_customer_requested =
            build_offer_kind_with_error(PocketOfferError::PermanentFailure {
                code: PermanentFailureCode::CustomerRequested,
            });
        store_payment_with_offer_and_test(
            offer_kind_customer_requested,
            &mut data_store,
            "offer_kind_customer_requested",
        );

        let offer_kind_account_not_matching =
            build_offer_kind_with_error(PocketOfferError::PermanentFailure {
                code: PermanentFailureCode::AccountNotMatching,
            });
        store_payment_with_offer_and_test(
            offer_kind_account_not_matching,
            &mut data_store,
            "offer_kind_account_not_matching",
        );

        let offer_kind_payout_expired =
            build_offer_kind_with_error(PocketOfferError::PermanentFailure {
                code: PermanentFailureCode::PayoutExpired,
            });
        store_payment_with_offer_and_test(
            offer_kind_payout_expired,
            &mut data_store,
            "offer_kind_payout_expired",
        );
    }

    fn build_offer_kind_with_error(error: PocketOfferError) -> OfferKind {
        OfferKind::Pocket {
            id: "id".to_string(),
            exchange_rate: ExchangeRate {
                currency_code: "EUR".to_string(),
                rate: 5123,
                updated_at: SystemTime::now(),
            },
            topup_value_minor_units: 51245,
            exchange_fee_minor_units: 123,
            exchange_fee_rate_permyriad: 50,
            error: Some(error),
        }
    }

    fn store_payment_with_offer_and_test(offer: OfferKind, data_store: &mut DataStore, hash: &str) {
        let user_preferences = UserPreferences {
            fiat_currency: "EUR".to_string(),
            timezone_config: TzConfig {
                timezone_id: "Bern".to_string(),
                timezone_utc_offset_secs: -1234,
            },
        };

        let exchange_rates = vec![
            ExchangeRate {
                currency_code: "EUR".to_string(),
                rate: 123,
                updated_at: SystemTime::now(),
            },
            ExchangeRate {
                currency_code: "USD".to_string(),
                rate: 234,
                updated_at: SystemTime::now(),
            },
        ];

        data_store
            .store_payment_info(
                hash,
                user_preferences.clone(),
                exchange_rates,
                Some(offer.clone()),
            )
            .unwrap();

        assert_eq!(
            data_store
                .retrieve_payment_info(hash)
                .unwrap()
                .unwrap()
                .offer
                .unwrap(),
            offer
        );
    }

    #[test]
    fn test_exchange_rate_storage() {
        let db_name = String::from("rates.db3");
        reset_db(&db_name);
        let mut data_store = DataStore::new(&format!("{TEST_DB_PATH}/{db_name}")).unwrap();

        assert!(data_store.get_all_exchange_rates().unwrap().is_empty());

        data_store
            .update_exchange_rate(
                "USD",
                1234,
                SystemTime::UNIX_EPOCH + Duration::from_secs(10),
            )
            .unwrap();
        let rates = data_store.get_all_exchange_rates().unwrap();
        let usd_rate = rates.iter().find(|r| r.currency_code == "USD").unwrap();
        assert_eq!(usd_rate.rate, 1234);
        assert_eq!(
            usd_rate.updated_at,
            SystemTime::UNIX_EPOCH + Duration::from_secs(10)
        );

        sleep(Duration::from_secs(2));

        data_store
            .update_exchange_rate(
                "EUR",
                5678,
                SystemTime::UNIX_EPOCH + Duration::from_secs(20),
            )
            .unwrap();
        let rates = data_store.get_all_exchange_rates().unwrap();
        let usd_rate = rates.iter().find(|r| r.currency_code == "USD").unwrap();
        let eur_rate = rates.iter().find(|r| r.currency_code == "EUR").unwrap();
        assert_eq!(usd_rate.rate, 1234);
        assert_eq!(
            usd_rate.updated_at,
            SystemTime::UNIX_EPOCH + Duration::from_secs(10)
        );
        assert_eq!(eur_rate.rate, 5678);
        assert_eq!(
            eur_rate.updated_at,
            SystemTime::UNIX_EPOCH + Duration::from_secs(20)
        );

        sleep(Duration::from_secs(2));

        data_store
            .update_exchange_rate(
                "USD",
                4321,
                SystemTime::UNIX_EPOCH + Duration::from_secs(30),
            )
            .unwrap();
        let rates = data_store.get_all_exchange_rates().unwrap();
        let usd_rate = rates.iter().find(|r| r.currency_code == "USD").unwrap();
        let eur_rate = rates.iter().find(|r| r.currency_code == "EUR").unwrap();
        assert_eq!(usd_rate.rate, 4321);
        assert_eq!(
            usd_rate.updated_at,
            SystemTime::UNIX_EPOCH + Duration::from_secs(30)
        );
        assert_eq!(eur_rate.rate, 5678);
        assert_eq!(
            eur_rate.updated_at,
            SystemTime::UNIX_EPOCH + Duration::from_secs(20)
        );
    }

    #[test]
    fn test_persisting_funds_migration_status() {
        let db_name = String::from("funds_migration.db3");
        reset_db(&db_name);
        let mut data_store = DataStore::new(&format!("{TEST_DB_PATH}/{db_name}")).unwrap();

        assert_eq!(
            data_store.retrive_funds_migration_status().unwrap(),
            MigrationStatus::Unknown
        );

        data_store
            .append_funds_migration_status(MigrationStatus::Pending)
            .unwrap();
        assert_eq!(
            data_store.retrive_funds_migration_status().unwrap(),
            MigrationStatus::Pending
        );

        data_store
            .append_funds_migration_status(MigrationStatus::Completed)
            .unwrap();
        assert_eq!(
            data_store.retrive_funds_migration_status().unwrap(),
            MigrationStatus::Completed
        );
    }

    #[test]
    fn test_invoice_persistence() {
        let db_name = String::from("invoice_persistence.db3");
        reset_db(&db_name);
        let mut data_store = DataStore::new(&format!("{TEST_DB_PATH}/{db_name}")).unwrap();

        assert!(data_store.retrieve_created_invoices(5).unwrap().is_empty());

        let invoice1 = CreatedInvoice {
            hash: "hash1".to_string(),
            invoice: "invoice1".to_string(),
            channel_opening_fees: Some(25000000),
        };
        let invoice2 = CreatedInvoice {
            hash: "hash2".to_string(),
            invoice: "invoice2".to_string(),
            channel_opening_fees: None,
        };

        data_store
            .store_created_invoice(
                invoice1.hash.as_str(),
                invoice1.invoice.as_str(),
                &invoice1.channel_opening_fees,
            )
            .unwrap();
        assert_eq!(
            data_store.retrieve_created_invoices(5).unwrap(),
            vec![invoice1.clone()]
        );

        data_store
            .store_created_invoice(
                invoice2.hash.as_str(),
                invoice2.invoice.as_str(),
                &invoice2.channel_opening_fees,
            )
            .unwrap();
        assert_eq!(
            data_store.retrieve_created_invoices(5).unwrap(),
            vec![invoice2.clone(), invoice1.clone()]
        );

        assert_eq!(
            data_store.retrieve_created_invoices(1).unwrap(),
            vec![invoice2.clone()]
        );

        assert!(data_store
            .retrieve_created_invoice_by_hash("hash0")
            .unwrap()
            .is_none());
        assert_eq!(
            data_store
                .retrieve_created_invoice_by_hash(invoice1.hash.as_str())
                .unwrap(),
            Some(invoice1)
        );
        assert_eq!(
            data_store
                .retrieve_created_invoice_by_hash(invoice2.hash.as_str())
                .unwrap(),
            Some(invoice2)
        );
    }

    #[test]
    fn test_fiat_topup_info_persistence() {
        let db_name = String::from("fiat_topup_info_persistence");
        reset_db(&db_name);
        let data_store = DataStore::new(&format!("{TEST_DB_PATH}/{db_name}")).unwrap();

        assert_eq!(data_store.retrieve_latest_fiat_topup_info().unwrap(), None);

        let mut fiat_topup_info = FiatTopupInfo {
            order_id: "961b8ee9-74cc-4844-9fe8-b02ce0702663".to_string(),
            debitor_iban: "CH4889144919566329178".to_string(),
            creditor_reference: "8584-9931-ABCD".to_string(),
            creditor_iban: "DE2089144126222826294".to_string(),
            creditor_bank_name: "Example Bank".to_string(),
            creditor_bank_street: "Bankingstreet 21".to_string(),
            creditor_bank_postal_code: "2121".to_string(),
            creditor_bank_town: "Example Town".to_string(),
            creditor_bank_country: "CH".to_string(),
            creditor_bank_bic: "VA7373".to_string(),
            creditor_name: "John Doe".to_string(),
            creditor_street: "Doestreet 21".to_string(),
            creditor_postal_code: "2112".to_string(),
            creditor_town: "Creditor Town".to_string(),
            creditor_country: "DE".to_string(),
            currency: "EUR".to_string(),
        };

        data_store
            .store_fiat_topup_info(fiat_topup_info.clone())
            .unwrap();
        assert_eq!(
            data_store.retrieve_latest_fiat_topup_info().unwrap(),
            Some(fiat_topup_info.clone())
        );

        fiat_topup_info.order_id = "361dd7f8-c7b7-4871-b906-b67fa3ed9b55".to_string();

        data_store
            .store_fiat_topup_info(fiat_topup_info.clone())
            .unwrap();
        assert_eq!(
            data_store.retrieve_latest_fiat_topup_info().unwrap(),
            Some(fiat_topup_info)
        );
    }

    fn reset_db(db_name: &str) {
        let _ = fs::create_dir(TEST_DB_PATH);
        let _ = fs::remove_file(format!("{TEST_DB_PATH}/{db_name}"));
    }
}
