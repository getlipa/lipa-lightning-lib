mod lspd {
    tonic::include_proto!("lspd");
}

use crate::errors::{Result, RuntimeErrorCode};
use crate::interfaces::Lsp;
use lspd::channel_opener_client::ChannelOpenerClient;
use lspd::{ChannelInformationRequest, RegisterPaymentRequest};
use perro::MapToError;
use prost::Message;
use tokio::runtime::{Builder, Runtime};
use tonic::metadata::{Ascii, MetadataValue};
use tonic::Request;

pub struct LspClient {
    address: String,
    bearer: MetadataValue<Ascii>,
    rt: Runtime,
}

impl LspClient {
    pub fn new(address: String, auth_token: String) -> Result<Self> {
        let bearer = format!("Bearer {auth_token}")
            .parse()
            .map_to_invalid_input("Invalid LSP auth token")?;
        let rt = Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_to_permanent_failure("Failed to build a tokio runtime")?;
        Ok(Self {
            address,
            bearer,
            rt,
        })
    }

    fn wrap_request<T>(&self, request: T) -> Request<T> {
        let mut request = Request::new(request);
        request
            .metadata_mut()
            .insert("authorization", self.bearer.clone());
        request
    }
}

impl Lsp for LspClient {
    fn channel_information(&self) -> Result<Vec<u8>> {
        let request = self.wrap_request(ChannelInformationRequest {
            pubkey: "".to_string(),
        });
        let mut client = self
            .rt
            .block_on(ChannelOpenerClient::connect(self.address.clone()))
            .map_to_runtime_error(
                RuntimeErrorCode::LspServiceUnavailable,
                "Failed to contact LSP",
            )?;
        Ok(self
            .rt
            .block_on(client.channel_information(request))
            .map_to_runtime_error(
                RuntimeErrorCode::LspServiceUnavailable,
                "LSP channel information request failed",
            )?
            .into_inner()
            .encode_to_vec())
    }

    fn register_payment(&self, blob: Vec<u8>) -> Result<()> {
        let request = self.wrap_request(RegisterPaymentRequest { blob });
        let mut client = self
            .rt
            .block_on(ChannelOpenerClient::connect(self.address.clone()))
            .map_to_runtime_error(
                RuntimeErrorCode::LspServiceUnavailable,
                "Failed to contact LSP",
            )?;
        self.rt
            .block_on(client.register_payment(request))
            .map_to_runtime_error(
                RuntimeErrorCode::LspServiceUnavailable,
                "LSP register payment request failed",
            )?;
        Ok(())
    }
}
