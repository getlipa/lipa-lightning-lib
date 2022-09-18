use bitcoin::Network;

const PROTOCOL_HTTP: &str = "http";
const PROTOCOL_HTTP_PORT: u16 = 80;
const PROTOCOL_HTTPS: &str = "https";
const PROTOCOL_HTTPS_PORT: u16 = 443;

/// An object that holds all configuration needed to start a LipaLightning instance
///
/// # Fields:
///
/// * `seed` - the seed as a byte array of length 32
/// * `esplora_api_host` - the host name of the esplora API - e.g. "blockstream.info"
/// * `esplora_api_path` <optional> - the path of the esplora API - e.g. "api"
/// * `esplora_api_port` <optional> - the port of esplora API - e.g. 80
/// * `esplora_over_tls` - whether the esplora API is reachable over http or https
/// * `ldk_peer_listening_port` - the port on which LDK listens for new p2p connections
/// * `network` - the on which to run this LipaLightning instance
pub struct LipaLightningConfig {
    pub seed: Vec<u8>,
    pub esplora_api_host: String,
    pub esplora_api_port: Option<u16>,
    pub esplora_api_path: Option<String>,
    pub esplora_over_tls: bool,
    pub ldk_peer_listening_port: u16,
    pub network: Network,
}

impl LipaLightningConfig {
    pub fn get_esplora_url(&self) -> String {
        format!(
            "{}://{}:{}{}",
            self.get_protocol(),
            self.esplora_api_host,
            self.get_port(),
            self.get_path(),
        )
    }

    fn get_protocol(&self) -> &str {
        if self.esplora_over_tls {
            PROTOCOL_HTTPS
        } else {
            PROTOCOL_HTTP
        }
    }

    fn get_port(&self) -> u16 {
        match self.esplora_api_port {
            Some(port) => port,
            None => {
                if self.esplora_over_tls {
                    PROTOCOL_HTTPS_PORT
                } else {
                    PROTOCOL_HTTP_PORT
                }
            }
        }
    }

    fn get_path(&self) -> String {
        match &self.esplora_api_path {
            Some(path) => {
                if !path.starts_with('/') {
                    return format!("/{}", path);
                }

                path.clone()
            }
            None => "".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_esplora_url_construction() {
        let mut config = LipaLightningConfig {
            seed: vec![0; 32],
            esplora_api_host: "localhost".to_string(),
            esplora_api_port: None,
            esplora_api_path: None,
            esplora_over_tls: false,
            ldk_peer_listening_port: 9732,
            network: Network::Regtest,
        };

        assert_eq!(config.get_esplora_url(), "http://localhost:80".to_string());

        config.esplora_api_host = "localhost".to_string();
        config.esplora_api_port = Some(3000);
        config.esplora_api_path = None;
        config.esplora_over_tls = false;

        assert_eq!(
            config.get_esplora_url(),
            "http://localhost:3000".to_string()
        );

        config.esplora_api_host = "blockstream.info".to_string();
        config.esplora_api_port = None;
        config.esplora_api_path = Some("/api".to_string());
        config.esplora_over_tls = true;

        assert_eq!(
            config.get_esplora_url(),
            "https://blockstream.info:443/api".to_string()
        );

        config.esplora_api_host = "localhost".to_string();
        config.esplora_api_port = Some(1234);
        config.esplora_api_path = Some("api".to_string());
        config.esplora_over_tls = true;

        assert_eq!(
            config.get_esplora_url(),
            "https://localhost:1234/api".to_string()
        );
    }
}
