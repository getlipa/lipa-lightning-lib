mod lspd {
    tonic::include_proto!("lspd");
}

use eel::errors::LipaResult;
use eel::interfaces::Lsp;
use lspd::channel_opener_client::ChannelOpenerClient;
use lspd::{ChannelInformationRequest, RegisterPaymentRequest};
use prost::Message;
use tokio::runtime::{Builder, Runtime};
use tonic::metadata::{Ascii, MetadataValue};
use tonic::Request;

pub(crate) struct LspClient {
    address: String,
    bearer: MetadataValue<Ascii>,
    rt: Runtime,
}

impl LspClient {
    pub fn build(address: String, auth_token: String) -> Self {
        let bearer = format!("Bearer {}", auth_token).parse().unwrap();
        let rt = Builder::new_multi_thread().enable_all().build().unwrap();
        Self {
            address,
            bearer,
            rt,
        }
    }

    fn wrap_request<T>(&self, request: T) -> Request<T> {
        let mut request = tonic::Request::new(request);
        request
            .metadata_mut()
            .insert("authorization", self.bearer.clone());
        request
    }
}

impl Lsp for LspClient {
    fn channel_information(&self) -> LipaResult<Vec<u8>> {
        let request = self.wrap_request(ChannelInformationRequest {
            pubkey: "".to_string(),
        });
        let mut client = self
            .rt
            .block_on(ChannelOpenerClient::connect(self.address.clone()))
            .unwrap();
        Ok(self
            .rt
            .block_on(client.channel_information(request))
            .unwrap()
            .into_inner()
            .encode_to_vec())
    }

    fn register_payment(&self, blob: Vec<u8>) -> LipaResult<()> {
        let request = self.wrap_request(RegisterPaymentRequest { blob });
        let mut client = self
            .rt
            .block_on(ChannelOpenerClient::connect(self.address.clone()))
            .unwrap();
        self.rt.block_on(client.register_payment(request)).unwrap();
        Ok(())
    }
}
