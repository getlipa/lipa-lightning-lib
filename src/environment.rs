use crate::Network;
use breez_sdk_core::EnvironmentType;

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
    pub lsp_url: String,
    pub lsp_token: String,
    pub esplora_url: String,
    pub rgs_url: String,
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
                lsp_url: env!("LSP_URL_LOCAL").to_string(),
                lsp_token: env!("LSP_TOKEN_LOCAL").to_string(),
                esplora_url: env!("ESPLORA_URL_LOCAL").to_string(),
                rgs_url: env!("RGS_URL_LOCAL").to_string(),
                pocket_url: env!("POCKET_URL_LOCAL").to_string(),
            },
            EnvironmentCode::Dev => Self {
                network: Network::Bitcoin,
                environment_type: EnvironmentType::Production,
                backend_url,
                backend_health_url,
                lsp_url: env!("LSP_URL_DEV").to_string(),
                lsp_token: env!("LSP_TOKEN_DEV").to_string(),
                esplora_url: env!("ESPLORA_URL_DEV").to_string(),
                rgs_url: env!("RGS_URL_DEV").to_string(),
                pocket_url: env!("POCKET_URL_DEV").to_string(),
            },
            EnvironmentCode::Stage => Self {
                network: Network::Bitcoin,
                environment_type: EnvironmentType::Production,
                backend_url,
                backend_health_url,
                lsp_url: env!("LSP_URL_STAGE").to_string(),
                lsp_token: env!("LSP_TOKEN_STAGE").to_string(),
                esplora_url: env!("ESPLORA_URL_STAGE").to_string(),
                rgs_url: env!("RGS_URL_STAGE").to_string(),
                pocket_url: env!("POCKET_URL_STAGE").to_string(),
            },
            EnvironmentCode::Prod => Self {
                network: Network::Bitcoin,
                environment_type: EnvironmentType::Production,
                backend_url,
                backend_health_url,
                lsp_url: env!("LSP_URL_PROD").to_string(),
                lsp_token: env!("LSP_TOKEN_PROD").to_string(),
                esplora_url: env!("ESPLORA_URL_PROD").to_string(),
                rgs_url: env!("RGS_URL_PROD").to_string(),
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
