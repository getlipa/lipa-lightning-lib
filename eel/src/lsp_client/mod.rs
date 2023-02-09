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
use tonic::service::interceptor::InterceptedService;
use tonic::service::Interceptor;
use tonic::transport::channel::Endpoint;
use tonic::transport::Channel;
use tonic::{Request, Status};

pub struct LspClient {
    endpoint: Endpoint,
    interceptor: AuthInterceptor,
    rt: Runtime,
}

impl LspClient {
    pub fn new(address: String, auth_token: String) -> Result<Self> {
        let bearer = format!("Bearer {auth_token}")
            .parse()
            .map_to_invalid_input("Invalid LSP auth token")?;
        let interceptor = AuthInterceptor::new(bearer);

        let endpoint = Channel::from_shared(address)
            .map_to_invalid_input("Invalid gRPC URL")?
            .connect_timeout(std::time::Duration::from_secs(30))
            .timeout(std::time::Duration::from_secs(30));

        let rt = Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_to_permanent_failure("Failed to build a tokio runtime")?;

        Ok(Self {
            endpoint,
            interceptor,
            rt,
        })
    }

    // The function must be async because `Endpoint::connect_lazy()` accesses
    // the async runtime.
    async fn build_client(
        &self,
    ) -> ChannelOpenerClient<InterceptedService<Channel, AuthInterceptor>> {
        let channel = self.endpoint.connect_lazy();
        ChannelOpenerClient::with_interceptor(channel, self.interceptor.clone())
    }
}

impl Lsp for LspClient {
    fn channel_information(&self) -> Result<Vec<u8>> {
        let request = Request::new(ChannelInformationRequest {
            pubkey: "".to_string(),
        });
        Ok(self
            .rt
            .block_on(async {
                let mut client = self.build_client().await;
                client.channel_information(request).await
            })
            .map_to_runtime_error(
                RuntimeErrorCode::LspServiceUnavailable,
                "LSP channel information request failed",
            )?
            .into_inner()
            .encode_to_vec())
    }

    fn register_payment(&self, blob: Vec<u8>) -> Result<()> {
        let request = Request::new(RegisterPaymentRequest { blob });
        self.rt
            .block_on(async {
                let mut client = self.build_client().await;
                client.register_payment(request).await
            })
            .map_to_runtime_error(
                RuntimeErrorCode::LspServiceUnavailable,
                "LSP register payment request failed",
            )?;
        Ok(())
    }
}

#[derive(Clone)]
struct AuthInterceptor {
    bearer: MetadataValue<Ascii>,
}

impl AuthInterceptor {
    fn new(bearer: MetadataValue<Ascii>) -> Self {
        Self { bearer }
    }
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, mut request: Request<()>) -> std::result::Result<Request<()>, Status> {
        request
            .metadata_mut()
            .insert("authorization", self.bearer.clone());
        Ok(request)
    }
}
