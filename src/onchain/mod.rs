mod channel_closes;
mod swaps;

use crate::amount::{AsSats, Msats, Sats, ToAmount};
use crate::errors::Result;
use crate::onchain::channel_closes::ChannelCloses;
use crate::onchain::swaps::Swaps;
use crate::support::Support;
use crate::{OnchainResolvingFees, SwapToLightningFees};
use breez_sdk_core::ReceiveOnchainRequest;
use log::error;
use std::sync::Arc;

pub struct Onchain {
    swaps: Arc<Swaps>,
    channel_closes: Arc<ChannelCloses>,
}

impl Onchain {
    pub(crate) fn new(support: Arc<Support>) -> Self {
        let swaps = Arc::new(Swaps::new(Arc::clone(&support)));
        let channel_closes = Arc::new(ChannelCloses::new(Arc::clone(&support)));
        Self {
            swaps,
            channel_closes,
        }
    }

    pub fn swaps(&self) -> Arc<Swaps> {
        Arc::clone(&self.swaps)
    }

    pub fn channel_closes(&self) -> Arc<ChannelCloses> {
        Arc::clone(&self.channel_closes)
    }
}

fn get_onchain_resolving_fees<F>(
    support: &Support,
    amount: Msats,
    prepare_onchain_tx: F,
) -> Result<Option<OnchainResolvingFees>>
where
    F: FnOnce(String, u32) -> Result<(Sats, Sats)>,
{
    let rate = support.get_exchange_rate();
    let lsp_fees = support.calculate_lsp_fee_for_amount(amount.msats)?;

    let swap_info = support
        .rt
        .handle()
        .block_on(support.sdk.receive_onchain(ReceiveOnchainRequest {
            opening_fee_params: lsp_fees.lsp_fee_params,
        }))
        .ok();

    let sat_per_vbyte = support.query_onchain_fee_rate()?;

    let (sent_amount, onchain_fee) = match prepare_onchain_tx(
        swap_info
            .clone()
            .map(|s| s.bitcoin_address)
            .unwrap_or("1BitcoinEaterAddressDontSendf59kuE".to_string()),
        sat_per_vbyte,
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

    let lsp_fees = support.calculate_lsp_fee_for_amount(sent_amount.sats)?;

    if swap_info.is_none()
        || sent_amount.sats < (swap_info.clone().unwrap().min_allowed_deposit as u64)
        || sent_amount.sats > (swap_info.clone().unwrap().max_allowed_deposit as u64)
        || sent_amount.sats <= lsp_fees.lsp_fee.sats
    {
        return Ok(Some(OnchainResolvingFees {
            swap_fees: None,
            sweep_onchain_fee_estimate: onchain_fee.to_amount_up(&rate),
            sat_per_vbyte,
        }));
    }

    let swap_fee = 0_u64.as_sats();
    let swap_to_lightning_fees = SwapToLightningFees {
        swap_fee: swap_fee.sats.as_sats().to_amount_up(&rate),
        onchain_fee: onchain_fee.to_amount_up(&rate),
        channel_opening_fee: lsp_fees.lsp_fee.clone(),
        total_fees: (swap_fee.sats + onchain_fee.sats + lsp_fees.lsp_fee.sats)
            .as_sats()
            .to_amount_up(&rate),
        lsp_fee_params: lsp_fees.lsp_fee_params,
    };

    Ok(Some(OnchainResolvingFees {
        swap_fees: Some(swap_to_lightning_fees),
        sweep_onchain_fee_estimate: onchain_fee.to_amount_up(&rate),
        sat_per_vbyte,
    }))
}
