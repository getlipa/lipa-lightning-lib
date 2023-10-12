use crate::Network;
use breez_sdk_core::EnvironmentType;

/// A code of the environment for the node to run.
#[derive(Clone, Copy, Debug)]
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
}

impl Environment {
    pub fn load(environment: EnvironmentCode) -> Self {
        let base_url = get_backend_base_url(environment);
        let backend_url = format!("{base_url}/v1/graphql");
        let backend_health_url = format!("{base_url}/healthz");

        match environment {
            EnvironmentCode::Local => Self {
                network: Network::Bitcoin,
                environment_type: EnvironmentType::Production,
                backend_url,
                backend_health_url,
                pocket_url: env!("POCKET_URL_LOCAL").to_string(),
            },
            EnvironmentCode::Dev => Self {
                network: Network::Bitcoin,
                environment_type: EnvironmentType::Production,
                backend_url,
                backend_health_url,
                pocket_url: env!("POCKET_URL_DEV").to_string(),
            },
            EnvironmentCode::Stage => Self {
                network: Network::Bitcoin,
                environment_type: EnvironmentType::Production,
                backend_url,
                backend_health_url,
                pocket_url: env!("POCKET_URL_STAGE").to_string(),
            },
            EnvironmentCode::Prod => Self {
                network: Network::Bitcoin,
                environment_type: EnvironmentType::Production,
                backend_url,
                backend_health_url,
                pocket_url: env!("POCKET_URL_PROD").to_string(),
            },
        }
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
