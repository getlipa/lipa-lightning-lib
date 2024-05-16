use crate::errors::Result;
use breez_sdk_core::{EnvironmentType, Network};
use hex::FromHex;
use perro::MapToError;

/// A code of the environment for the node to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvironmentCode {
    Local,
    Dev,
    Stage,
    Prod,
}

// TODO remove dead code after breez sdk implementation and fix implementation (e.g. currently it selects mainnet when Local or Dev codes are provided)
#[allow(dead_code)]
pub(crate) struct Environment {
    pub network: Network,
    pub environment_type: EnvironmentType,
    pub backend_url: String,
    pub backend_health_url: String,
    pub pocket_url: String,
    pub notification_webhook_base_url: String,
    pub notification_webhook_secret: [u8; 32],
    pub lipa_lightning_domain: String,
}

impl Environment {
    pub fn load(environment: EnvironmentCode) -> Result<Self> {
        let base_url = get_backend_base_url(environment);
        let backend_url = format!("{base_url}/v1/graphql");
        let backend_health_url = format!("{base_url}/healthz");

        let notification_webhook_base_url =
            get_notification_webhook_base_url(environment).to_string();
        let notification_webhook_secret = get_notification_webhook_secret(environment)?;
        let lipa_lightning_domain = get_lipa_lightning_domain(environment).to_string();

        Ok(match environment {
            EnvironmentCode::Local => Self {
                network: Network::Bitcoin,
                environment_type: EnvironmentType::Production,
                backend_url,
                backend_health_url,
                pocket_url: env!("POCKET_URL_LOCAL").to_string(),
                notification_webhook_base_url,
                notification_webhook_secret,
                lipa_lightning_domain,
            },
            EnvironmentCode::Dev => Self {
                network: Network::Bitcoin,
                environment_type: EnvironmentType::Production,
                backend_url,
                backend_health_url,
                pocket_url: env!("POCKET_URL_DEV").to_string(),
                notification_webhook_base_url,
                notification_webhook_secret,
                lipa_lightning_domain,
            },
            EnvironmentCode::Stage => Self {
                network: Network::Bitcoin,
                environment_type: EnvironmentType::Production,
                backend_url,
                backend_health_url,
                pocket_url: env!("POCKET_URL_STAGE").to_string(),
                notification_webhook_base_url,
                notification_webhook_secret,
                lipa_lightning_domain,
            },
            EnvironmentCode::Prod => Self {
                network: Network::Bitcoin,
                environment_type: EnvironmentType::Production,
                backend_url,
                backend_health_url,
                pocket_url: env!("POCKET_URL_PROD").to_string(),
                notification_webhook_base_url,
                notification_webhook_secret,
                lipa_lightning_domain,
            },
        })
    }
}

fn get_backend_base_url(environment: EnvironmentCode) -> &'static str {
    match environment {
        EnvironmentCode::Local => env!("BACKEND_URL_LOCAL"),
        EnvironmentCode::Dev => env!("BACKEND_URL_DEV"),
        EnvironmentCode::Stage => env!("BACKEND_URL_STAGE"),
        EnvironmentCode::Prod => env!("BACKEND_URL_PROD"),
    }
}

fn get_notification_webhook_base_url(environment_code: EnvironmentCode) -> &'static str {
    match environment_code {
        EnvironmentCode::Local => env!("NOTIFICATION_WEBHOOK_URL_LOCAL"),
        EnvironmentCode::Dev => env!("NOTIFICATION_WEBHOOK_URL_DEV"),
        EnvironmentCode::Stage => env!("NOTIFICATION_WEBHOOK_URL_STAGE"),
        EnvironmentCode::Prod => env!("NOTIFICATION_WEBHOOK_URL_PROD"),
    }
}

fn get_notification_webhook_secret(environment_code: EnvironmentCode) -> Result<[u8; 32]> {
    let secret_hex = match environment_code {
        EnvironmentCode::Local => env!("NOTIFICATION_WEBHOOK_SECRET_LOCAL"),
        EnvironmentCode::Dev => env!("NOTIFICATION_WEBHOOK_SECRET_DEV"),
        EnvironmentCode::Stage => env!("NOTIFICATION_WEBHOOK_SECRET_STAGE"),
        EnvironmentCode::Prod => env!("NOTIFICATION_WEBHOOK_SECRET_PROD"),
    };

    <[u8; 32]>::from_hex(secret_hex)
        .map_to_permanent_failure("Failed to parse embedded notification webhook secret")
}

fn get_lipa_lightning_domain(environment_code: EnvironmentCode) -> &'static str {
    match environment_code {
        EnvironmentCode::Local => env!("LIPA_LIGHTNING_DOMAIN_LOCAL"),
        EnvironmentCode::Dev => env!("LIPA_LIGHTNING_DOMAIN_DEV"),
        EnvironmentCode::Stage => env!("LIPA_LIGHTNING_DOMAIN_STAGE"),
        EnvironmentCode::Prod => env!("LIPA_LIGHTNING_DOMAIN_PROD"),
    }
}
