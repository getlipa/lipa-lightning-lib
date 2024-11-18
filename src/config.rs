use crate::errors::Result;
use crate::locker::Locker;
use crate::support::Support;
use crate::{with_status, AnalyticsConfig, EnableStatus, FeatureFlag, RuntimeErrorCode, TzConfig};
use crow::{CountryCode, LanguageCode};
use log::info;
use perro::{MapToError, ResultTrait};
use std::str::FromStr;
use std::sync::Arc;

pub struct Config {
    support: Arc<Support>,
}

impl Config {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(support: Arc<Support>) -> Self {
        Self { support }
    }

    /// Set the fiat currency (ISO 4217 currency code) - not all are supported
    /// The method [`Config::list_currencies`] can used to list supported codes.
    ///
    /// Requires network: **no**
    pub fn set_fiat_currency(&self, fiat_currency: String) -> Result<()> {
        self.support
            .data_store
            .lock_unwrap()
            .store_selected_fiat_currency(&fiat_currency)?;
        self.support.user_preferences.lock_unwrap().fiat_currency = fiat_currency;
        Ok(())
    }

    /// Set the timezone config.
    ///
    /// Parameters:
    /// * `timezone_config` - the user's current timezone
    ///
    /// Requires network: **no**
    pub fn set_timezone_config(&self, timezone_config: TzConfig) {
        self.support.user_preferences.lock_unwrap().timezone_config = timezone_config;
    }

    /// Set the analytics configuration.
    ///
    /// This can be used to completely prevent any analytics data from being reported.
    ///
    /// Requires network: **no**
    pub fn set_analytics_config(&self, config: AnalyticsConfig) -> Result<()> {
        *self.support.analytics_interceptor.config.lock_unwrap() = config.clone();
        self.support
            .data_store
            .lock_unwrap()
            .append_analytics_config(config)
    }

    /// Get the currently configured analytics configuration.
    ///
    /// Requires network: **no**
    pub fn get_analytics_config(&self) -> Result<AnalyticsConfig> {
        self.support
            .data_store
            .lock_unwrap()
            .retrieve_analytics_config()
    }

    /// Registers a new notification token. If a token has already been registered, it will be updated.
    ///
    /// Requires network: **yes**
    pub fn register_notification_token(
        &self,
        notification_token: String,
        language_iso_639_1: String,
        country_iso_3166_1_alpha_2: String,
    ) -> Result<()> {
        let language = LanguageCode::from_str(&language_iso_639_1.to_lowercase())
            .map_to_invalid_input("Invalid language code")?;
        let country = CountryCode::for_alpha2(&country_iso_3166_1_alpha_2.to_uppercase())
            .map_to_invalid_input("Invalid country code")?;

        self.support
            .offer_manager
            .register_notification_token(notification_token, language, country)
            .map_runtime_error_to(RuntimeErrorCode::OfferServiceUnavailable)
    }

    /// Set value of a feature flag.
    /// The method will report the change to the backend and update the local database.
    ///
    /// Parameters:
    /// * `feature` - feature flag to be set.
    /// * `enable` - enable or disable the feature.
    ///
    /// Requires network: **yes**
    pub fn set_feature_flag(&self, feature: FeatureFlag, flag_enabled: bool) -> Result<()> {
        let kind_of_address = match feature {
            FeatureFlag::LightningAddress => |a: &String| !a.starts_with('-'),
            FeatureFlag::PhoneNumber => |a: &String| a.starts_with('-'),
        };
        let (from_status, to_status) = match flag_enabled {
            true => (EnableStatus::FeatureDisabled, EnableStatus::Enabled),
            false => (EnableStatus::Enabled, EnableStatus::FeatureDisabled),
        };

        let addresses = self
            .support
            .data_store
            .lock_unwrap()
            .retrieve_lightning_addresses()?
            .into_iter()
            .filter_map(with_status(from_status))
            .filter(kind_of_address)
            .collect::<Vec<_>>();

        if addresses.is_empty() {
            info!("No lightning addresses to change the status");
            return Ok(());
        }

        let doing = match flag_enabled {
            true => "Enabling",
            false => "Disabling",
        };
        info!("{doing} {addresses:?} on the backend");

        self.support
            .rt
            .handle()
            .block_on(async {
                if flag_enabled {
                    pigeon::enable_lightning_addresses(
                        &self.support.node_config.remote_services_config.backend_url,
                        &self.support.async_auth,
                        addresses.clone(),
                    )
                    .await
                } else {
                    pigeon::disable_lightning_addresses(
                        &self.support.node_config.remote_services_config.backend_url,
                        &self.support.async_auth,
                        addresses.clone(),
                    )
                    .await
                }
            })
            .map_to_runtime_error(
                RuntimeErrorCode::AuthServiceUnavailable,
                "Failed to enable/disable a lightning address",
            )?;
        let mut data_store = self.support.data_store.lock_unwrap();
        addresses
            .into_iter()
            .try_for_each(|a| data_store.update_lightning_address(&a, to_status))
    }

    /// List codes of supported fiat currencies.
    /// Please keep in mind that this method doesn't make any network calls. It simply retrieves
    /// previously fetched values that are frequently updated by a background task.
    ///
    /// The fetched list will be persisted across restarts to alleviate the consequences of a
    /// slow or unresponsive exchange rate service.
    /// The method will return an empty list if there is nothing persisted yet and
    /// the values are not yet fetched from the service.
    ///
    /// Requires network: **no**
    pub fn list_currencies(&self) -> Vec<String> {
        let rates = self.support.task_manager.lock_unwrap().get_exchange_rates();
        rates.iter().map(|r| r.currency_code.clone()).collect()
    }

    /// Call the method when the app goes to foreground, such that the user can interact with it.
    /// The library starts running the background tasks more frequently to improve user experience.
    ///
    /// Requires network: **no**
    pub fn foreground(&self) {
        self.support.task_manager.lock_unwrap().foreground();
    }

    /// Call the method when the app goes to background, such that the user can not interact with it.
    /// The library stops running some unnecessary tasks and runs necessary tasks less frequently.
    /// It should save battery and internet traffic.
    ///
    /// Requires network: **no**
    pub fn background(&self) {
        self.support.task_manager.lock_unwrap().background();
    }
}
