pub mod channel_closes;
pub mod reverse_swap;
pub mod swap;

use crate::amount::{AsSats, Msats, Sats, ToAmount};
use crate::errors::Result;
use crate::onchain::channel_closes::ChannelClose;
use crate::onchain::reverse_swap::ReverseSwap;
use crate::onchain::swap::Swap;
use crate::support::Support;
use crate::{OnchainResolvingFees, RuntimeErrorCode, SwapToLightningFees};
use breez_sdk_core::ReceiveOnchainRequest;
use log::error;
use perro::MapToError;
use std::sync::Arc;

pub struct Onchain {
    swap: Arc<Swap>,
    reverse_swap: Arc<ReverseSwap>,
    channel_close: Arc<ChannelClose>,
}

impl Onchain {
    pub(crate) fn new(support: Arc<Support>) -> Self {
        let swap = Arc::new(Swap::new(Arc::clone(&support)));
        let reverse_swap = Arc::new(ReverseSwap::new(Arc::clone(&support)));
        let channel_close = Arc::new(ChannelClose::new(Arc::clone(&support), Arc::clone(&swap)));
        Self {
            swap,
            reverse_swap,
            channel_close,
        }
    }

    pub fn swap(&self) -> Arc<Swap> {
        Arc::clone(&self.swap)
    }

    pub fn reverse_swap(&self) -> Arc<ReverseSwap> {
        Arc::clone(&self.reverse_swap)
    }

    pub fn channel_close(&self) -> Arc<ChannelClose> {
        Arc::clone(&self.channel_close)
    }
}

fn get_onchain_resolving_fees<F>(
    support: &Support,
    swap: &Swap,
    amount: Msats,
    prepare_onchain_tx: F,
) -> Result<Option<OnchainResolvingFees>>
where
    F: FnOnce(String) -> Result<(Sats, Sats, u32)>,
{
    let rate = support.get_exchange_rate();
    let lsp_fees = swap.calculate_lsp_fee_for_amount(amount.msats)?;

    let swap_info = support
        .rt
        .handle()
        .block_on(support.sdk.receive_onchain(ReceiveOnchainRequest {
            opening_fee_params: Some(lsp_fees.lsp_fee_params),
        }))
        .ok();

    let (sent_amount, onchain_fee, sats_per_vbyte) = match prepare_onchain_tx(
        swap_info
            .clone()
            .map(|s| s.bitcoin_address)
            .unwrap_or("1BitcoinEaterAddressDontSendf59kuE".to_string()),
    ) {
        Ok(t) => t,
        // TODO: expose distinction between insufficient funds failure and other failures
        //  -> requires that the SDK exposes an error when preparing for resolving failed swaps
        //  for now, it only does for preparing to resolve onchain funds from channel closes.
        Err(e) => {
            error!("Failed to prepare onchain tx due to {e}");
            return Ok(None);
        }
    };

    // Require onchain fees to be less than half of the onchain balance to leave some leeway
    //  (for now, the onchain fee is just an estimation because the destination address is unknown)
    if onchain_fee.sats * 2 > amount.sats_round_down().sats {
        return Ok(None);
    }

    let lsp_fee_response = swap.calculate_lsp_fee_for_amount(amount.msats)?;

    if swap_info.is_none()
        || sent_amount.sats < (swap_info.clone().unwrap().min_allowed_deposit as u64)
        || sent_amount.sats > (swap_info.clone().unwrap().max_allowed_deposit as u64)
        || sent_amount.sats <= lsp_fee_response.lsp_fee.sats
    {
        return Ok(Some(OnchainResolvingFees {
            swap_fees: None,
            sweep_onchain_fee_estimate: onchain_fee.to_amount_up(&rate),
            sats_per_vbyte,
        }));
    }

    let swap_fee = 0_u64.as_sats();
    let swap_to_lightning_fees = SwapToLightningFees {
        swap_fee: swap_fee.sats.as_sats().to_amount_up(&rate),
        onchain_fee: onchain_fee.to_amount_up(&rate),
        channel_opening_fee: lsp_fee_response.lsp_fee.clone(),
        total_fees: (swap_fee.sats + onchain_fee.sats + lsp_fee_response.lsp_fee.sats)
            .as_sats()
            .to_amount_up(&rate),
        lsp_fee_params: lsp_fee_response.lsp_fee_params,
    };

    Ok(Some(OnchainResolvingFees {
        swap_fees: Some(swap_to_lightning_fees),
        sweep_onchain_fee_estimate: onchain_fee.to_amount_up(&rate),
        sats_per_vbyte,
    }))
}

fn query_onchain_fee_rate(support: &Support) -> Result<u32> {
    let recommended_fees = support
        .rt
        .handle()
        .block_on(support.sdk.recommended_fees())
        .map_to_runtime_error(
            RuntimeErrorCode::NodeUnavailable,
            "Couldn't fetch recommended fees",
        )?;

    Ok(recommended_fees.half_hour_fee as u32)
}
