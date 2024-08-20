#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvironmentCode {
    Local,
    Dev,
    Stage,
    Prod,
}

pub(crate) struct Environment {
    pub backend_url: String,
    pub pocket_url: String,
    pub notification_webhook_base_url: String,
    pub notification_webhook_secret_hex: String,
    pub lipa_lightning_domain: String,
}

impl Environment {
    pub fn load(environment: EnvironmentCode) -> Self {
        let backend_url = get_backend_url(environment).to_string();

        let notification_webhook_base_url =
            get_notification_webhook_base_url(environment).to_string();
        let notification_webhook_secret_hex =
            get_notification_webhook_secret_hex(environment).to_string();
        let lipa_lightning_domain = get_lipa_lightning_domain(environment).to_string();

        match environment {
            EnvironmentCode::Local => Self {
                backend_url,
                pocket_url: env!("POCKET_URL_LOCAL").to_string(),
                notification_webhook_base_url,
                notification_webhook_secret_hex,
                lipa_lightning_domain,
            },
            EnvironmentCode::Dev => Self {
                backend_url,
                pocket_url: env!("POCKET_URL_DEV").to_string(),
                notification_webhook_base_url,
                notification_webhook_secret_hex,
                lipa_lightning_domain,
            },
            EnvironmentCode::Stage => Self {
                backend_url,
                pocket_url: env!("POCKET_URL_STAGE").to_string(),
                notification_webhook_base_url,
                notification_webhook_secret_hex,
                lipa_lightning_domain,
            },
            EnvironmentCode::Prod => Self {
                backend_url,
                pocket_url: env!("POCKET_URL_PROD").to_string(),
                notification_webhook_base_url,
                notification_webhook_secret_hex,
                lipa_lightning_domain,
            },
        }
    }
}

fn get_backend_url(environment: EnvironmentCode) -> &'static str {
    match environment {
        EnvironmentCode::Local => env!("BACKEND_COMPLETE_URL_LOCAL"),
        EnvironmentCode::Dev => env!("BACKEND_COMPLETE_URL_DEV"),
        EnvironmentCode::Stage => env!("BACKEND_COMPLETE_URL_STAGE"),
        EnvironmentCode::Prod => env!("BACKEND_COMPLETE_URL_PROD"),
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

fn get_notification_webhook_secret_hex(environment_code: EnvironmentCode) -> &'static str {
    match environment_code {
        EnvironmentCode::Local => env!("NOTIFICATION_WEBHOOK_SECRET_LOCAL"),
        EnvironmentCode::Dev => env!("NOTIFICATION_WEBHOOK_SECRET_DEV"),
        EnvironmentCode::Stage => env!("NOTIFICATION_WEBHOOK_SECRET_STAGE"),
        EnvironmentCode::Prod => env!("NOTIFICATION_WEBHOOK_SECRET_PROD"),
    }
}

fn get_lipa_lightning_domain(environment_code: EnvironmentCode) -> &'static str {
    match environment_code {
        EnvironmentCode::Local => env!("LIPA_LIGHTNING_DOMAIN_LOCAL"),
        EnvironmentCode::Dev => env!("LIPA_LIGHTNING_DOMAIN_DEV"),
        EnvironmentCode::Stage => env!("LIPA_LIGHTNING_DOMAIN_STAGE"),
        EnvironmentCode::Prod => env!("LIPA_LIGHTNING_DOMAIN_PROD"),
    }
}
