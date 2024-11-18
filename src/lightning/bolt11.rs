use crate::amount::AsSats;
use crate::errors::map_send_payment_error;
use crate::lightning::{report_send_payment_issue, store_payment_info};
use crate::locker::Locker;
use crate::support::Support;
use crate::{
    InvoiceCreationMetadata, InvoiceDetails, PayErrorCode, PayResult, PaymentMetadata,
    RuntimeErrorCode,
};
use breez_sdk_core::error::SendPaymentError;
use breez_sdk_core::{OpeningFeeParams, SendPaymentRequest};
use perro::{ensure, runtime_error, MapToError};
use std::sync::Arc;

pub struct Bolt11 {
    support: Arc<Support>,
}

impl Bolt11 {
    pub(crate) fn new(support: Arc<Support>) -> Self {
        Self { support }
    }

    /// Create a bolt11 invoice to receive a payment with.
    ///
    /// Parameters:
    /// * `amount_sat` - the smallest amount of sats required for the node to accept the incoming
    ///   payment (sender will have to pay fees on top of that amount)
    /// * `lsp_fee_params` - the params that will be used to determine the lsp fee.
    ///    Can be obtained from [`Lightning::calculate_lsp_fee_for_amount`](crate::Lightning::calculate_lsp_fee_for_amount)
    ///    to guarantee predicted fees are the ones charged.
    /// * `description` - a description to be embedded into the created invoice
    /// * `metadata` - additional data about the invoice creation used for analytics purposes,
    ///    used to improve the user experience
    ///
    /// Requires network: **yes**
    pub fn create(
        &self,
        amount_sat: u64,
        lsp_fee_params: Option<OpeningFeeParams>,
        description: String,
        metadata: InvoiceCreationMetadata,
    ) -> crate::Result<InvoiceDetails> {
        let response = self
            .support
            .rt
            .handle()
            .block_on(
                self.support
                    .sdk
                    .receive_payment(breez_sdk_core::ReceivePaymentRequest {
                        amount_msat: amount_sat.as_sats().msats,
                        description,
                        preimage: None,
                        opening_fee_params: lsp_fee_params,
                        use_description_hash: None,
                        expiry: None,
                        cltv: None,
                    }),
            )
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to create an invoice",
            )?;

        store_payment_info(&self.support, &response.ln_invoice.payment_hash, None);
        self.support
            .data_store
            .lock_unwrap()
            .store_created_invoice(
                &response.ln_invoice.payment_hash,
                &response.ln_invoice.bolt11,
                &response.opening_fee_msat,
                response.ln_invoice.timestamp + response.ln_invoice.expiry,
            )
            .map_to_permanent_failure("Failed to persist created invoice")?;

        self.support.analytics_interceptor.request_initiated(
            response.clone(),
            self.support.get_exchange_rate(),
            metadata,
        );
        Ok(InvoiceDetails::from_ln_invoice(
            response.ln_invoice,
            &self.support.get_exchange_rate(),
        ))
    }

    /// Start an attempt to pay an invoice. Can immediately fail, meaning that the payment couldn't be started.
    /// If successful, it doesn't mean that the payment itself was successful (funds received by the payee).
    /// After this method returns, the consumer of this library will learn about a successful/failed payment through the
    /// callbacks [`EventsCallback::payment_sent`](crate::EventsCallback::payment_sent) and
    /// [`EventsCallback::payment_failed`](crate::EventsCallback::payment_failed).
    ///
    /// Parameters:
    /// * `invoice_details` - details of an invoice decode by [`LightningNode::decode_data`](crate::LightningNode::decode_data)
    /// * `metadata` - additional meta information about the payment, used by analytics to improve the user experience.
    ///
    /// Requires network: **yes**
    pub fn pay(&self, invoice_details: InvoiceDetails, metadata: PaymentMetadata) -> PayResult<()> {
        self.pay_open_amount(invoice_details, 0, metadata)
    }

    /// Similar to [`Bolt11::pay`] with the difference that the passed in invoice
    /// does not have any payment amount specified, and allows the caller of the method to
    /// specify an amount instead.
    ///
    /// Additional Parameters:
    /// * `amount_sat` - amount in sats to be paid
    ///
    /// Requires network: **yes**
    pub fn pay_open_amount(
        &self,
        invoice_details: InvoiceDetails,
        amount_sat: u64,
        metadata: PaymentMetadata,
    ) -> PayResult<()> {
        let amount_msat = if amount_sat == 0 {
            None
        } else {
            Some(amount_sat.as_sats().msats)
        };
        store_payment_info(&self.support, &invoice_details.payment_hash, None);
        let node_state = self
            .support
            .sdk
            .node_info()
            .map_to_runtime_error(PayErrorCode::NodeUnavailable, "Failed to read node info")?;
        ensure!(
            node_state.id != invoice_details.payee_pub_key,
            runtime_error(
                PayErrorCode::PayingToSelf,
                "A locally issued invoice tried to be paid"
            )
        );

        self.support.analytics_interceptor.pay_initiated(
            invoice_details.clone(),
            metadata,
            amount_msat,
            self.support.get_exchange_rate(),
        );

        let result = self
            .support
            .rt
            .handle()
            .block_on(self.support.sdk.send_payment(SendPaymentRequest {
                bolt11: invoice_details.invoice,
                use_trampoline: true,
                amount_msat,
                label: None,
            }));

        if matches!(
            result,
            Err(SendPaymentError::Generic { .. }
                | SendPaymentError::PaymentFailed { .. }
                | SendPaymentError::PaymentTimeout { .. }
                | SendPaymentError::RouteNotFound { .. }
                | SendPaymentError::RouteTooExpensive { .. }
                | SendPaymentError::ServiceConnectivity { .. })
        ) {
            report_send_payment_issue(&self.support, invoice_details.payment_hash);
        }

        result.map_err(map_send_payment_error)?;
        Ok(())
    }
}
