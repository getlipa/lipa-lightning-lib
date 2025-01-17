use crate::analytics::AnalyticsConfig;
use crate::errors::Result;
use crate::migrations::migrate;
use crate::{EnableStatus, ExchangeRate, Offer, PocketOfferError, TzConfig, UserPreferences};

use chrono::{DateTime, Utc};
use crow::FiatTopupSetupInfo;
use crow::{PermanentFailureCode, TemporaryFailureCode};
use log::debug;
use perro::MapToError;
use rusqlite::{backup, params, Connection, OptionalExtension, Params, Row};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) const BACKUP_DB_FILENAME_SUFFIX: &str = ".backup";

#[derive(PartialEq, Debug, Clone)]
pub(crate) struct LocalPaymentData {
    pub user_preferences: Option<UserPreferences>,
    pub exchange_rate: Option<ExchangeRate>,
    pub offer: Option<Offer>,
    pub personal_note: Option<String>,
    pub received_on: Option<String>,
    pub received_lnurl_comment: Option<String>,
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
        offer: Option<Offer>,
        received_on: Option<String>,
        received_lnurl_comment: Option<String>,
    ) -> Result<()> {
        self.backup_status = BackupStatus::WaitingForBackup;
        let tx = self
            .conn
            .transaction()
            .map_to_permanent_failure("Failed to begin SQL transaction")?;

        let snapshot_id = insert_exchange_rate_snapshot(&tx, exchange_rates)?;

        tx.execute(
            "\
            INSERT INTO payments (hash, timezone_id, timezone_utc_offset_secs, fiat_currency, \
            exchange_rates_history_snapshot_id, received_on, received_lnurl_comment)\
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)\
            ",
            (
                payment_hash,
                &user_preferences.timezone_config.timezone_id,
                user_preferences.timezone_config.timezone_utc_offset_secs,
                user_preferences.fiat_currency,
                snapshot_id,
                received_on,
                received_lnurl_comment,
            ),
        )
        .map_to_permanent_failure("Failed to add payment info to db")?;

        if let Some(Offer {
            id: pocket_id,
            exchange_rate:
                ExchangeRate {
                    currency_code,
                    rate,
                    updated_at,
                },
            topup_value_minor_units,
            topup_value_sats,
            exchange_fee_minor_units,
            exchange_fee_rate_permyriad,
            error,
            ..
        }) = offer
        {
            let exchanged_at: DateTime<Utc> = updated_at.into();
            tx.execute(
            "\
                INSERT INTO offers (payment_hash, pocket_id, fiat_currency, rate, exchanged_at, topup_value_minor_units, exchange_fee_minor_units, exchange_fee_rate_permyriad, error, topup_value_sats)\
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)\
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
                    from_offer_error(error),
                    topup_value_sats,
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
            o.exchange_fee_minor_units, o.exchange_fee_rate_permyriad, o.error, o.topup_value_sats, \
            payments.personal_note, payments.received_on, payments.received_lnurl_comment \
            FROM payments \
            LEFT JOIN exchange_rates_history h on payments.exchange_rates_history_snapshot_id=h.snapshot_id \
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
        invoice_expiry_timestamp: u64,
    ) -> Result<()> {
        self.backup_status = BackupStatus::WaitingForBackup;
        self.conn
            .execute(
                "\
            INSERT INTO created_invoices (hash, invoice, channel_opening_fees, invoice_expiry_timestamp)\
            VALUES (?1, ?2, ?3, ?4)\
            ",
                params![hash, invoice, channel_opening_fees, invoice_expiry_timestamp],
            )
            .map_to_permanent_failure("Failed to store created invoice to local db")?;
        Ok(())
    }

    /// Returns all pending and `number_of_expired_invoices` expired invoices.
    pub fn retrieve_created_invoices(
        &self,
        number_of_expired_invoices: u32,
    ) -> Result<Vec<CreatedInvoice>> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_to_permanent_failure("Time went backwards")?
            .as_secs();

        self.conn
            .prepare(
                "\
            SELECT * FROM ( \
                SELECT hash, invoice, channel_opening_fees \
                FROM created_invoices \
                WHERE invoice_expiry_timestamp >= ?1) \
            UNION \
            SELECT * FROM ( \
                SELECT hash, invoice, channel_opening_fees \
                FROM created_invoices \
                WHERE invoice_expiry_timestamp < ?1 \
                ORDER BY id DESC \
                LIMIT ?2);
            ",
            )
            .map_to_permanent_failure("Failed to retrieve created invoice from local db")?
            .query_map([now, number_of_expired_invoices as u64], |r| {
                Ok(CreatedInvoice {
                    hash: r.get(0)?,
                    invoice: r.get(1)?,
                    channel_opening_fees: r.get(2)?,
                })
            })
            .map_to_permanent_failure("Failed to bind parameter to prepared SQL query")?
            .map(|r| r.map_to_permanent_failure("Corrupted db"))
            .collect()
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

    pub fn update_personal_note(
        &mut self,
        payment_hash: &str,
        personal_note: Option<&str>,
    ) -> Result<()> {
        self.backup_status = BackupStatus::WaitingForBackup;
        let number_of_rows = self
            .conn
            .execute(
                "
                UPDATE payments \
                SET personal_note = ?1 \
                WHERE hash=?2",
                params![personal_note, payment_hash],
            )
            .map_to_permanent_failure("Failed to store personal note in local db")?;

        if number_of_rows == 0 {
            debug!(
                "Payment with hash [{}] not found in local db, inserting new row.",
                payment_hash
            );
            self.conn
                .execute(
                    "
                INSERT INTO payments (personal_note, hash) \
                    VALUES (?1, ?2);",
                    params![personal_note, payment_hash],
                )
                .map_to_permanent_failure(
                    "Failed to insert new payment to store personal note in local db",
                )?;
        }

        Ok(())
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
        self.conn
            .prepare(
                " \
            SELECT fiat_currency, rate, updated_at \
            FROM exchange_rates \
            ",
            )
            .map_to_permanent_failure("Failed to prepare SQL query")?
            .query_map([], exchange_rate_from_row)
            .map_to_permanent_failure("Failed to bind parameter to prepared SQL query")?
            .map(|r| r.map_to_permanent_failure("Corrupted db"))
            .collect()
    }

    pub fn store_fiat_topup_info(&self, fiat_topup_info: FiatTopupSetupInfo) -> Result<()> {
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

    pub fn retrieve_latest_fiat_topup_info(&self) -> Result<Option<FiatTopupSetupInfo>> {
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

    pub fn append_analytics_config(&mut self, status: AnalyticsConfig) -> Result<()> {
        self.backup_status = BackupStatus::WaitingForBackup;
        self.conn
            .execute(
                "INSERT INTO analytics_config (status) VALUES (?1)",
                (status as u8,),
            )
            .map_to_permanent_failure("Failed to add analytics config to db")?;
        Ok(())
    }

    pub fn retrieve_analytics_config(&self) -> Result<AnalyticsConfig> {
        let status_from_row = |row: &Row| {
            let status: u8 = row.get(0)?;
            AnalyticsConfig::try_from(status).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Integer,
                    Box::new(e),
                )
            })
        };
        self.conn
            .query_row(
                "SELECT status, updated_at FROM analytics_config ORDER BY id DESC LIMIT 1",
                (),
                status_from_row,
            )
            .map_to_permanent_failure("Failed to query analytics config")
    }

    pub fn store_lightning_address(&mut self, lightning_address: &str) -> Result<()> {
        self
            .conn
            .execute(
                "INSERT OR REPLACE INTO lightning_addresses (address, enable_status) VALUES (?1, ?2)",
                (lightning_address, EnableStatus::Enabled as u8),
            )
            .map_to_permanent_failure("Failed to add lightning address to db")?;
        self.backup_status = BackupStatus::WaitingForBackup;
        Ok(())
    }

    pub fn update_lightning_address(
        &mut self,
        lightning_address: &str,
        enable_status: EnableStatus,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE lightning_addresses SET enable_status = ?1 WHERE address = ?2",
                (enable_status as u8, lightning_address),
            )
            .map_to_permanent_failure("Failed to update lightning address in db")?;
        self.backup_status = BackupStatus::WaitingForBackup;
        Ok(())
    }

    pub fn retrieve_lightning_addresses(&self) -> Result<Vec<(String, EnableStatus)>> {
        self.query_map(
            "SELECT address, enable_status FROM lightning_addresses ORDER BY registered_at",
            [],
            lightning_address_from_row,
        )
        .map_to_permanent_failure("Failed to query lightning addresses")
    }

    fn query_map<T, P, F>(
        &self,
        statement: &str,
        params: P,
        from_row: F,
    ) -> rusqlite::Result<Vec<T>>
    where
        P: Params,
        F: Fn(&Row) -> rusqlite::Result<T>,
    {
        self.conn
            .prepare(statement)?
            .query_map(params, from_row)?
            .collect()
    }

    pub fn store_hidden_channel_close_onchain_funds_amount_sat(
        &mut self,
        amount_sat: u64,
    ) -> Result<()> {
        self.backup_status = BackupStatus::WaitingForBackup;
        self.conn
            .execute(
                "INSERT INTO hidden_channel_close_amount (amount_sat) VALUES (?1)",
                params![amount_sat],
            )
            .map_to_permanent_failure("Failed to store hidden channel close amount in db")?;
        Ok(())
    }

    pub fn retrieve_hidden_channel_close_onchain_funds_amount_sat(
        &mut self,
    ) -> Result<Option<u64>> {
        self.conn
            .query_row(
                "SELECT amount_sat FROM hidden_channel_close_amount ORDER BY id DESC LIMIT 1",
                (),
                |r| r.get(0),
            )
            .optional()
            .map_to_permanent_failure("Failed to query last hidden channel close amount")
    }

    pub fn store_hidden_unresolved_failed_swap(&mut self, swap_address: &str) -> Result<()> {
        self.backup_status = BackupStatus::WaitingForBackup;
        self.conn
            .execute(
                "INSERT INTO hidden_failed_swaps (swap_address) VALUES (?1)",
                params![swap_address],
            )
            .map_to_permanent_failure("Failed to store hidden failed swap in db")?;
        Ok(())
    }

    pub fn retrieve_hidden_unresolved_failed_swaps(&self) -> Result<Vec<String>> {
        Ok(self
            .conn
            .prepare("SELECT swap_address FROM hidden_failed_swaps")
            .map_to_permanent_failure("Failed to prepare query for hidden failed swaps")?
            .query_map([], |row| row.get(0))
            .map_to_permanent_failure("Failed to query for hidden failed swaps")?
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>())
    }

    pub fn store_selected_fiat_currency(&mut self, fiat_currency: &str) -> Result<()> {
        self.backup_status = BackupStatus::WaitingForBackup;
        self.conn
            .execute(
                "INSERT INTO fiat_currency (fiat_currency) VALUES (?1)",
                params![fiat_currency],
            )
            .map_to_permanent_failure("Failed to store fiat currency in db")?;
        Ok(())
    }

    pub fn retrieve_last_set_fiat_currency(&self) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT fiat_currency FROM fiat_currency ORDER BY id DESC LIMIT 1",
                (),
                |r| r.get(0),
            )
            .optional()
            .map_to_permanent_failure("Failed to query last hidden channel close amount")
    }
}

fn lightning_address_from_row(row: &Row) -> rusqlite::Result<(String, EnableStatus)> {
    let address = row.get(0)?;
    let enable_status: u8 = row.get(1)?;
    let enable_status = EnableStatus::try_from(enable_status).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Integer, Box::new(e))
    })?;
    Ok((address, enable_status))
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

fn offer_from_row(row: &Row) -> rusqlite::Result<Option<Offer>> {
    if let Some(id) = row.get(5)? {
        let fiat_currency: String = row.get(6)?;
        let rate: u32 = row.get(7)?;
        let exchanged_at: chrono::DateTime<chrono::Utc> = row.get(8)?;
        let exchanged_at = SystemTime::from(exchanged_at);
        let topup_value_minor_units: u64 = row.get(9)?;
        let exchange_fee_minor_units: u64 = row.get(10)?;
        let exchange_fee_rate_permyriad: u16 = row.get(11)?;
        let topup_value_sats: Option<u64> = row.get(13)?;

        let exchange_rate = ExchangeRate {
            currency_code: fiat_currency,
            rate,
            updated_at: exchanged_at,
        };

        return Ok(Some(Offer {
            id,
            exchange_rate: exchange_rate.clone(),
            topup_value_minor_units,
            topup_value_sats,
            exchange_fee_minor_units,
            exchange_fee_rate_permyriad,
            lightning_payout_fee: None,
            error: to_offer_error(row.get(12)?),
        }));
    }

    Ok(None)
}

fn local_payment_data_from_row(row: &Row) -> rusqlite::Result<LocalPaymentData> {
    let user_preferences = user_preferences_from_row(row)?;
    let exchange_rate = payment_exchange_rate_from_row(row)?;
    let offer = offer_from_row(row)?;
    let personal_note = row.get(14)?;
    let received_on = row.get(15)?;
    let received_lnurl_comment = row.get(16)?;

    Ok(LocalPaymentData {
        user_preferences,
        exchange_rate,
        offer,
        personal_note,
        received_on,
        received_lnurl_comment,
    })
}

fn user_preferences_from_row(row: &Row) -> rusqlite::Result<Option<UserPreferences>> {
    let timezone_id: Option<String> = row.get(0)?;
    let timezone_utc_offset_secs: Option<i32> = row.get(1)?;
    let fiat_currency = row.get(2)?;

    match (timezone_id, timezone_utc_offset_secs, fiat_currency) {
        (Some(timezone_id), Some(timezone_utc_offset_secs), Some(fiat_currency)) => {
            Ok(Some(UserPreferences {
                fiat_currency,
                timezone_config: TzConfig {
                    timezone_id,
                    timezone_utc_offset_secs,
                },
            }))
        }
        _ => Ok(None),
    }
}

fn payment_exchange_rate_from_row(row: &Row) -> rusqlite::Result<Option<ExchangeRate>> {
    let currency_code: Option<String> = row.get(2)?;
    let rate: Option<u32> = row.get(3)?;
    let updated_at: Option<DateTime<Utc>> = row.get(4)?;

    match (currency_code, rate, updated_at) {
        (Some(currency_code), Some(rate), Some(updated_at)) => Ok(Some(ExchangeRate {
            currency_code,
            rate,
            updated_at: SystemTime::from(updated_at),
        })),
        _ => Ok(None),
    }
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

fn fiat_topup_info_from_row(row: &Row) -> rusqlite::Result<Option<FiatTopupSetupInfo>> {
    Ok(Some(FiatTopupSetupInfo {
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
    use crate::data_store::{CreatedInvoice, DataStore, LocalPaymentData};
    use crate::node_config::TzConfig;
    use crate::{EnableStatus, ExchangeRate, Offer, PocketOfferError, UserPreferences};

    use crate::analytics::AnalyticsConfig;
    use crow::FiatTopupSetupInfo;
    use crow::TopupError::TemporaryFailure;
    use crow::{PermanentFailureCode, TemporaryFailureCode};
    use std::fs;
    use std::thread::sleep;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
        let exchange_rate = ExchangeRate {
            currency_code: "EUR".to_string(),
            rate: 5123,
            updated_at: SystemTime::now(),
        };
        let offer = Offer {
            id: "id".to_string(),
            exchange_rate: exchange_rate.clone(),
            topup_value_minor_units: 51245,
            topup_value_sats: Some(2625281),
            exchange_fee_minor_units: 123,
            exchange_fee_rate_permyriad: 50,
            lightning_payout_fee: None,
            error: Some(TemporaryFailure {
                code: TemporaryFailureCode::NoRoute,
            }),
        };

        let exchange_rate = ExchangeRate {
            currency_code: "EUR".to_string(),
            rate: 5123,
            updated_at: SystemTime::now(),
        };
        let offer_no_error = Offer {
            id: "id".to_string(),
            exchange_rate: exchange_rate.clone(),
            topup_value_minor_units: 51245,
            topup_value_sats: Some(2625281),
            exchange_fee_minor_units: 123,
            exchange_fee_rate_permyriad: 50,
            lightning_payout_fee: None,
            error: None,
        };

        data_store
            .store_payment_info(
                "hash",
                user_preferences.clone(),
                Vec::new(),
                None,
                None,
                None,
            )
            .unwrap();

        // The second call will not fail.
        data_store
            .store_payment_info(
                "hash",
                user_preferences.clone(),
                exchange_rates.clone(),
                Some(offer.clone()),
                None,
                None,
            )
            .unwrap();

        data_store
            .store_payment_info(
                "hash - no offer",
                user_preferences.clone(),
                exchange_rates.clone(),
                None,
                None,
                None,
            )
            .unwrap();

        data_store
            .store_payment_info(
                "hash - no error",
                user_preferences.clone(),
                exchange_rates,
                Some(offer_no_error.clone()),
                Some("received_on".to_string()),
                Some("received_lnurl_comment".to_string()),
            )
            .unwrap();

        assert!(data_store
            .retrieve_payment_info("non existent hash")
            .unwrap()
            .is_none());

        let local_payment_data = data_store.retrieve_payment_info("hash").unwrap().unwrap();
        assert_eq!(local_payment_data.offer.unwrap(), offer);
        assert_eq!(
            local_payment_data.user_preferences.as_ref().unwrap(),
            &user_preferences.clone()
        );
        assert_eq!(
            local_payment_data
                .exchange_rate
                .as_ref()
                .unwrap()
                .currency_code,
            user_preferences.fiat_currency
        );
        assert_eq!(
            local_payment_data.exchange_rate.as_ref().unwrap().rate,
            4123
        );

        let local_payment_data = data_store
            .retrieve_payment_info("hash - no offer")
            .unwrap()
            .unwrap();
        assert!(local_payment_data.offer.is_none());
        assert_eq!(
            local_payment_data.user_preferences.as_ref().unwrap(),
            &user_preferences.clone()
        );
        assert_eq!(
            local_payment_data
                .exchange_rate
                .as_ref()
                .unwrap()
                .currency_code,
            user_preferences.fiat_currency
        );
        assert_eq!(
            local_payment_data.exchange_rate.as_ref().unwrap().rate,
            4123
        );

        let local_payment_data = data_store
            .retrieve_payment_info("hash - no error")
            .unwrap()
            .unwrap();
        assert_eq!(local_payment_data.offer.as_ref().unwrap(), &offer_no_error);
        assert_eq!(
            local_payment_data.received_on.as_ref().unwrap(),
            "received_on"
        );
        assert_eq!(
            local_payment_data.received_lnurl_comment.as_ref().unwrap(),
            "received_lnurl_comment"
        );

        let mut local_payment_data_with_note = local_payment_data.clone();
        local_payment_data_with_note.personal_note = Some(String::from("a note"));
        data_store
            .update_personal_note("hash - no error", Some("a note"))
            .unwrap();
        let local_payment_data_with_note_from_store = data_store
            .retrieve_payment_info("hash - no error")
            .unwrap()
            .unwrap();
        assert_eq!(
            local_payment_data_with_note_from_store,
            local_payment_data_with_note
        );

        data_store
            .update_personal_note("hash - no error", None)
            .unwrap();
        let local_payment_data_without_note_from_store = data_store
            .retrieve_payment_info("hash - no error")
            .unwrap()
            .unwrap();
        assert_eq!(
            local_payment_data_without_note_from_store,
            local_payment_data
        );

        data_store
            .update_personal_note("hash - no local data", Some("a note"))
            .unwrap();
        let local_payment_data_with_note_from_store = data_store
            .retrieve_payment_info("hash - no local data")
            .unwrap()
            .unwrap();
        assert_eq!(
            local_payment_data_with_note_from_store,
            LocalPaymentData {
                user_preferences: None,
                exchange_rate: None,
                offer: None,
                personal_note: Some(String::from("a note")),
                received_on: None,
                received_lnurl_comment: None,
            }
        );
    }
    #[test]
    fn test_offer_storage() {
        let db_name = String::from("offers.db3");
        reset_db(&db_name);
        let mut data_store = DataStore::new(&format!("{TEST_DB_PATH}/{db_name}")).unwrap();

        // Temporary failures
        let offer_no_route = build_offer_with_error(PocketOfferError::TemporaryFailure {
            code: TemporaryFailureCode::NoRoute,
        });
        store_payment_with_offer_and_test(offer_no_route, &mut data_store, "offer_no_route");

        let offer_invoice_expired = build_offer_with_error(PocketOfferError::TemporaryFailure {
            code: TemporaryFailureCode::InvoiceExpired,
        });
        store_payment_with_offer_and_test(
            offer_invoice_expired,
            &mut data_store,
            "offer_invoice_expired",
        );

        let offer_unexpected = build_offer_with_error(PocketOfferError::TemporaryFailure {
            code: TemporaryFailureCode::Unexpected,
        });
        store_payment_with_offer_and_test(offer_unexpected, &mut data_store, "offer_unexpected");

        let offer_unknown = build_offer_with_error(PocketOfferError::TemporaryFailure {
            code: TemporaryFailureCode::Unknown { msg: "Test".into() },
        });
        store_payment_with_offer_and_test(offer_unknown, &mut data_store, "offer_unknown");

        // Permanent failures
        let offer_threshold_exceeded = build_offer_with_error(PocketOfferError::PermanentFailure {
            code: PermanentFailureCode::ThresholdExceeded,
        });
        store_payment_with_offer_and_test(
            offer_threshold_exceeded,
            &mut data_store,
            "offer_threshold_exceeded",
        );

        let offer_order_inactive = build_offer_with_error(PocketOfferError::PermanentFailure {
            code: PermanentFailureCode::OrderInactive,
        });
        store_payment_with_offer_and_test(
            offer_order_inactive.clone(),
            &mut data_store,
            "offer_order_inactive",
        );

        let offer_companies_unsupported =
            build_offer_with_error(PocketOfferError::PermanentFailure {
                code: PermanentFailureCode::CompaniesUnsupported,
            });
        store_payment_with_offer_and_test(
            offer_companies_unsupported,
            &mut data_store,
            "offer_companies_unsupported",
        );

        let offer_country_unsuported = build_offer_with_error(PocketOfferError::PermanentFailure {
            code: PermanentFailureCode::CountryUnsupported,
        });
        store_payment_with_offer_and_test(
            offer_country_unsuported,
            &mut data_store,
            "offer_country_unsuported",
        );

        let offer_other_risk_detected =
            build_offer_with_error(PocketOfferError::PermanentFailure {
                code: PermanentFailureCode::OtherRiskDetected,
            });
        store_payment_with_offer_and_test(
            offer_other_risk_detected,
            &mut data_store,
            "offer_other_risk_detected",
        );

        let offer_customer_requested = build_offer_with_error(PocketOfferError::PermanentFailure {
            code: PermanentFailureCode::CustomerRequested,
        });
        store_payment_with_offer_and_test(
            offer_customer_requested,
            &mut data_store,
            "offer_customer_requested",
        );

        let offer_account_not_matching =
            build_offer_with_error(PocketOfferError::PermanentFailure {
                code: PermanentFailureCode::AccountNotMatching,
            });
        store_payment_with_offer_and_test(
            offer_account_not_matching,
            &mut data_store,
            "offer_account_not_matching",
        );

        let offer_payout_expired = build_offer_with_error(PocketOfferError::PermanentFailure {
            code: PermanentFailureCode::PayoutExpired,
        });
        store_payment_with_offer_and_test(
            offer_payout_expired,
            &mut data_store,
            "offer_payout_expired",
        );
    }

    fn build_offer_with_error(error: PocketOfferError) -> Offer {
        let exchange_rate = ExchangeRate {
            currency_code: "EUR".to_string(),
            rate: 5123,
            updated_at: SystemTime::now(),
        };
        Offer {
            id: "id".to_string(),
            exchange_rate: exchange_rate.clone(),
            topup_value_minor_units: 51245,
            topup_value_sats: Some(2625281),
            exchange_fee_minor_units: 123,
            exchange_fee_rate_permyriad: 50,
            lightning_payout_fee: None,
            error: Some(error),
        }
    }

    fn store_payment_with_offer_and_test(offer: Offer, data_store: &mut DataStore, hash: &str) {
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
                None,
                None,
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
    fn test_invoice_persistence() {
        let db_name = String::from("invoice_persistence.db3");
        reset_db(&db_name);
        let mut data_store = DataStore::new(&format!("{TEST_DB_PATH}/{db_name}")).unwrap();

        assert!(data_store.retrieve_created_invoices(5).unwrap().is_empty());

        let future = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 1000;

        let expired_invoice = CreatedInvoice {
            hash: "hash1".to_string(),
            invoice: "invoice1".to_string(),
            channel_opening_fees: Some(25000000),
        };
        let pending_invoice = CreatedInvoice {
            hash: "hash2".to_string(),
            invoice: "invoice2".to_string(),
            channel_opening_fees: None,
        };

        data_store
            .store_created_invoice(
                expired_invoice.hash.as_str(),
                expired_invoice.invoice.as_str(),
                &expired_invoice.channel_opening_fees,
                123,
            )
            .unwrap();
        assert_eq!(
            data_store.retrieve_created_invoices(5).unwrap(),
            vec![expired_invoice.clone()]
        );

        data_store
            .store_created_invoice(
                pending_invoice.hash.as_str(),
                pending_invoice.invoice.as_str(),
                &pending_invoice.channel_opening_fees,
                future,
            )
            .unwrap();
        let mut invoices = data_store.retrieve_created_invoices(5).unwrap();
        invoices.sort_by_key(|i| i.hash.clone());
        assert_eq!(
            invoices,
            vec![expired_invoice.clone(), pending_invoice.clone()]
        );

        let mut invoices = data_store.retrieve_created_invoices(1).unwrap();
        invoices.sort_by_key(|i| i.hash.clone());
        assert_eq!(
            invoices,
            vec![expired_invoice.clone(), pending_invoice.clone()]
        );

        let invoices = data_store.retrieve_created_invoices(0).unwrap();
        assert_eq!(invoices, vec![pending_invoice.clone()]);

        assert!(data_store
            .retrieve_created_invoice_by_hash("hash0")
            .unwrap()
            .is_none());
        assert_eq!(
            data_store
                .retrieve_created_invoice_by_hash(expired_invoice.hash.as_str())
                .unwrap(),
            Some(expired_invoice)
        );
        assert_eq!(
            data_store
                .retrieve_created_invoice_by_hash(pending_invoice.hash.as_str())
                .unwrap(),
            Some(pending_invoice)
        );
    }

    #[test]
    fn test_fiat_topup_info_persistence() {
        let db_name = String::from("fiat_topup_info_persistence");
        reset_db(&db_name);
        let data_store = DataStore::new(&format!("{TEST_DB_PATH}/{db_name}")).unwrap();

        assert_eq!(data_store.retrieve_latest_fiat_topup_info().unwrap(), None);

        let mut fiat_topup_info = FiatTopupSetupInfo {
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

    #[test]
    fn test_persisting_analytics_config() {
        let db_name = String::from("analytics_config.db3");
        reset_db(&db_name);
        let mut data_store = DataStore::new(&format!("{TEST_DB_PATH}/{db_name}")).unwrap();

        assert_eq!(
            data_store.retrieve_analytics_config().unwrap(),
            AnalyticsConfig::Enabled
        );

        data_store
            .append_analytics_config(AnalyticsConfig::Disabled)
            .unwrap();
        assert_eq!(
            data_store.retrieve_analytics_config().unwrap(),
            AnalyticsConfig::Disabled
        );

        data_store
            .append_analytics_config(AnalyticsConfig::Enabled)
            .unwrap();
        assert_eq!(
            data_store.retrieve_analytics_config().unwrap(),
            AnalyticsConfig::Enabled
        );
    }

    #[test]
    fn test_storing_lightning_address() {
        let db_name = String::from("lightning_addresses.db3");
        reset_db(&db_name);
        let mut data_store = DataStore::new(&format!("{TEST_DB_PATH}/{db_name}")).unwrap();
        let addresses = data_store.retrieve_lightning_addresses().unwrap();
        assert!(addresses.is_empty());

        // Store an address.
        data_store
            .store_lightning_address("satoshi@lipa.swiss")
            .unwrap();
        let addresses = data_store.retrieve_lightning_addresses().unwrap();
        assert_eq!(
            addresses,
            vec![("satoshi@lipa.swiss".to_string(), EnableStatus::Enabled)]
        );

        // Storing the same address.
        data_store
            .store_lightning_address("satoshi@lipa.swiss")
            .unwrap();
        let addresses = data_store.retrieve_lightning_addresses().unwrap();
        assert_eq!(
            addresses,
            vec![("satoshi@lipa.swiss".to_string(), EnableStatus::Enabled)]
        );

        // Disabling the address.
        data_store
            .update_lightning_address("satoshi@lipa.swiss", EnableStatus::FeatureDisabled)
            .unwrap();
        let addresses = data_store.retrieve_lightning_addresses().unwrap();
        assert_eq!(
            addresses,
            vec![(
                "satoshi@lipa.swiss".to_string(),
                EnableStatus::FeatureDisabled
            )]
        );

        // Storing the same address enables it back.
        data_store
            .store_lightning_address("satoshi@lipa.swiss")
            .unwrap();
        let addresses = data_store.retrieve_lightning_addresses().unwrap();
        assert_eq!(
            addresses,
            vec![("satoshi@lipa.swiss".to_string(), EnableStatus::Enabled)]
        );

        // Storing another address.
        data_store
            .store_lightning_address("finney@lipa.swiss")
            .unwrap();
        let addresses = data_store.retrieve_lightning_addresses().unwrap();
        assert_eq!(
            addresses,
            vec![
                ("satoshi@lipa.swiss".to_string(), EnableStatus::Enabled),
                ("finney@lipa.swiss".to_string(), EnableStatus::Enabled),
            ]
        );
    }

    #[test]
    fn test_storing_channel_close_hidden_amount() {
        let db_name = String::from("hidden_channel_closes.db3");
        reset_db(&db_name);
        let mut data_store = DataStore::new(&format!("{TEST_DB_PATH}/{db_name}")).unwrap();

        assert_eq!(
            data_store
                .retrieve_hidden_channel_close_onchain_funds_amount_sat()
                .unwrap(),
            None
        );

        data_store
            .store_hidden_channel_close_onchain_funds_amount_sat(50)
            .unwrap();
        assert_eq!(
            data_store
                .retrieve_hidden_channel_close_onchain_funds_amount_sat()
                .unwrap(),
            Some(50)
        );

        data_store
            .store_hidden_channel_close_onchain_funds_amount_sat(2000)
            .unwrap();
        assert_eq!(
            data_store
                .retrieve_hidden_channel_close_onchain_funds_amount_sat()
                .unwrap(),
            Some(2000)
        );
    }

    #[test]
    fn test_storing_hidden_failed_swap() {
        let db_name = String::from("hidden_failed_swaps.db3");
        reset_db(&db_name);
        let mut data_store = DataStore::new(&format!("{TEST_DB_PATH}/{db_name}")).unwrap();

        assert!(data_store
            .retrieve_hidden_unresolved_failed_swaps()
            .unwrap()
            .is_empty());

        data_store
            .store_hidden_unresolved_failed_swap("address1")
            .unwrap();
        assert_eq!(
            data_store
                .retrieve_hidden_unresolved_failed_swaps()
                .unwrap(),
            vec!["address1".to_string()]
        );

        data_store
            .store_hidden_unresolved_failed_swap("address2")
            .unwrap();
        assert_eq!(
            data_store
                .retrieve_hidden_unresolved_failed_swaps()
                .unwrap(),
            vec!["address1".to_string(), "address2".to_string()]
        );
    }

    #[test]
    fn test_storing_fiat_currency() {
        let db_name = String::from("fiat_currency.db3");
        reset_db(&db_name);
        let mut data_store = DataStore::new(&format!("{TEST_DB_PATH}/{db_name}")).unwrap();

        assert!(data_store
            .retrieve_last_set_fiat_currency()
            .unwrap()
            .is_none());

        data_store.store_selected_fiat_currency("EUR").unwrap();
        assert_eq!(
            data_store
                .retrieve_last_set_fiat_currency()
                .unwrap()
                .unwrap(),
            "EUR"
        );

        data_store.store_selected_fiat_currency("CHF").unwrap();
        assert_eq!(
            data_store
                .retrieve_last_set_fiat_currency()
                .unwrap()
                .unwrap(),
            "CHF"
        );
    }

    fn reset_db(db_name: &str) {
        let _ = fs::create_dir(TEST_DB_PATH);
        let _ = fs::remove_file(format!("{TEST_DB_PATH}/{db_name}"));
    }
}
