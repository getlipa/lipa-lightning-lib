use crate::errors::Result;
use crate::migrations::migrate;
use crate::UserPreferences;
use chrono::{DateTime, Utc};
use std::time::SystemTime;

use crate::ExchangeRate;
use perro::MapToError;
use rusqlite::Connection;
use rusqlite::Row;
use std::sync::{Arc, Mutex};

pub(crate) struct DataStore {
    #[allow(dead_code)]
    user_preferences: Arc<Mutex<UserPreferences>>,
    #[allow(dead_code)]
    conn: Connection,
}

impl DataStore {
    pub fn new(db_path: &str, user_preferences: Arc<Mutex<UserPreferences>>) -> Result<Self> {
        let mut conn = Connection::open(db_path).map_to_invalid_input("Invalid db path")?;
        migrate(&mut conn)?;
        Ok(DataStore {
            user_preferences,
            conn,
        })
    }

    pub fn update_exchange_rate(
        &self,
        currency_code: &str,
        rate: u32,
        updated_at: SystemTime,
    ) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use crate::config::TzConfig;
    use crate::data_store::DataStore;

    use crate::UserPreferences;
    use std::fs;
    use std::sync::{Arc, Mutex};
    use std::thread::sleep;
    use std::time::{Duration, SystemTime};

    const TEST_DB_PATH: &str = ".3l_local_test";
    const TEST_TZ_ID: &str = "test_timezone_id";
    const TEST_TZ_OFFSET: i32 = -1352;

    fn reset_db(db_name: &str) {
        let _ = fs::create_dir(TEST_DB_PATH);
        let _ = fs::remove_file(format!("{TEST_DB_PATH}/{db_name}"));
    }

    #[test]
    fn test_exchange_rate_storage() {
        let db_name = String::from("rates.db3");
        reset_db(&db_name);
        let user_preferences = Arc::new(Mutex::new(UserPreferences {
            fiat_currency: "CHF".to_string(),
            timezone_config: TzConfig {
                timezone_id: String::from(TEST_TZ_ID),
                timezone_utc_offset_secs: TEST_TZ_OFFSET,
            },
        }));
        let data_store =
            DataStore::new(&format!("{TEST_DB_PATH}/{db_name}"), user_preferences).unwrap();

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
}
