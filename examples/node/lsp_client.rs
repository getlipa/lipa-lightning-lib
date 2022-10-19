pub mod lspd {
    tonic::include_proto!("lspd");
}

use lspd::channel_opener_client::ChannelOpenerClient;
use lspd::{ChannelInformationRequest, RegisterPaymentRequest};
use prost::Message;
use std::cell::RefCell;
use std::sync::Mutex;
use tokio::runtime::{Builder, Runtime};
use tonic::metadata::{Ascii, MetadataValue};
use tonic::Request;
use uniffi_lipalightninglib::callbacks::LspCallback;

pub(crate) struct LspClient {
    bearer: MetadataValue<Ascii>,
    rt: Runtime,
    client: Mutex<RefCell<ChannelOpenerClient<tonic::transport::Channel>>>,
}

impl LspClient {
    pub fn connect(address: String, auth_token: String) -> Self {
        let bearer = format!("Bearer {}", auth_token).parse().unwrap();
        let rt = Builder::new_multi_thread().enable_all().build().unwrap();
        let client = rt.block_on(ChannelOpenerClient::connect(address)).unwrap();
        let client = Mutex::new(RefCell::new(client));
        Self { bearer, rt, client }
    }

    fn wrap_request<T>(&self, request: T) -> Request<T> {
        let mut request = tonic::Request::new(request);
        request
            .metadata_mut()
            .insert("authorization", self.bearer.clone());
        request
    }
}

impl LspCallback for LspClient {
    fn channel_information(&self) -> Vec<u8> {
        let request = self.wrap_request(ChannelInformationRequest {
            pubkey: "".to_string(),
        });
        self.rt
            .block_on(
                self.client
                    .lock()
                    .unwrap()
                    .borrow_mut()
                    .channel_information(request),
            )
            .map_err(|e| e.to_string())
            .unwrap()
            .into_inner()
            .encode_to_vec()
    }

    fn register_payment(&self, blob: Vec<u8>) -> String {
        let request = self.wrap_request(RegisterPaymentRequest { blob });
        self.rt
            .block_on(
                self.client
                    .lock()
                    .unwrap()
                    .borrow_mut()
                    .register_payment(request),
            )
            .map_err(|e| e.to_string())
            .err()
            .unwrap_or_default()
    }
}
