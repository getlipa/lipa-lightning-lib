use crate::amount::{AsSats, ToAmount};
use crate::errors::Result;
use crate::fiat_topup::FiatTopup;
use crate::locker::Locker;
use crate::onchain::Onchain;
use crate::support::Support;
use crate::{ActionRequiredItem, FailedSwapInfo, RuntimeErrorCode, CLN_DUST_LIMIT_SAT};
use breez_sdk_core::{BitcoinAddressData, Network};
use perro::ResultTrait;
use std::ops::Not;
use std::sync::Arc;

pub struct ActionsRequired {
    support: Arc<Support>,
    fiat_topup: Arc<FiatTopup>,
    onchain: Arc<Onchain>,
}

impl ActionsRequired {
    pub(crate) fn new(
        support: Arc<Support>,
        fiat_topup: Arc<FiatTopup>,
        onchain: Arc<Onchain>,
    ) -> Self {
        Self {
            support,
            fiat_topup,
            onchain,
        }
    }

    /// List action required items.
    ///
    /// Returns a list of actionable items. They can be:
    /// * Uncompleted offers (either available for collection or failed).
    /// * Unresolved failed swaps.
    /// * Available funds resulting from channel closes.
    ///
    /// Requires network: **yes**
    pub fn list(&self) -> Result<Vec<ActionRequiredItem>> {
        let uncompleted_offers = self.fiat_topup.query_uncompleted_offers()?;

        let hidden_failed_swap_addresses = self
            .support
            .data_store
            .lock_unwrap()
            .retrieve_hidden_unresolved_failed_swaps()?;
        let failed_swaps: Vec<_> = self
            .onchain
            .swap()
            .list_failed_unresolved()?
            .into_iter()
            .filter(|s| {
                hidden_failed_swap_addresses.contains(&s.address).not()
                    || self
                        .onchain
                        .swap()
                        .prepare_sweep(
                            s.clone(),
                            BitcoinAddressData {
                                address: "1BitcoinEaterAddressDontSendf59kuE".to_string(),
                                network: Network::Bitcoin,
                                amount_sat: None,
                                label: None,
                                message: None,
                            },
                        )
                        .is_ok()
            })
            .collect();

        let available_channel_closes_funds = self.support.get_node_info()?.onchain_balance;

        let mut action_required_items: Vec<ActionRequiredItem> = uncompleted_offers
            .into_iter()
            .map(Into::into)
            .chain(failed_swaps.into_iter().map(Into::into))
            .collect();

        // CLN currently forces a min-emergency onchain balance of 546 (the dust limit)
        // TODO: Replace CLN_DUST_LIMIT_SAT with 0 if/when
        //      https://github.com/ElementsProject/lightning/issues/7131 is addressed
        if available_channel_closes_funds.sats > CLN_DUST_LIMIT_SAT {
            let utxos = self.support.get_node_utxos()?;

            // If we already have a 546 sat UTXO, then we hide from the total amount available
            let available_funds_sats = if utxos
                .iter()
                .any(|u| u.amount_millisatoshi == CLN_DUST_LIMIT_SAT * 1_000)
            {
                available_channel_closes_funds.sats
            } else {
                available_channel_closes_funds.sats - CLN_DUST_LIMIT_SAT
            };

            let optional_hidden_amount_sat = self
                .support
                .data_store
                .lock_unwrap()
                .retrieve_hidden_channel_close_onchain_funds_amount_sat()?;

            let include_item_in_list = match optional_hidden_amount_sat {
                Some(amount) if amount == available_channel_closes_funds.sats => self
                    .onchain
                    .channel_close()
                    .determine_resolving_fees()?
                    .is_some(),
                _ => true,
            };

            if include_item_in_list {
                action_required_items.push(ActionRequiredItem::ChannelClosesFundsAvailable {
                    available_funds: available_funds_sats
                        .as_sats()
                        .to_amount_down(&self.support.get_exchange_rate()),
                });
            }
        }

        // TODO: improve ordering of items in the returned vec
        Ok(action_required_items)
    }

    /// Hides the topup with the given id. Can be called on expired topups so that they stop being returned
    /// by [`ActionsRequired::list`].
    ///
    /// Topup id can be obtained from [`OfferKind::Pocket`](crate::OfferKind::Pocket).
    ///
    /// Requires network: **yes**
    pub fn dismiss_topup(&self, id: String) -> Result<()> {
        self.support
            .offer_manager
            .hide_topup(id)
            .map_runtime_error_to(RuntimeErrorCode::OfferServiceUnavailable)
    }

    /// Hides the channel close action required item in case the amount cannot be recovered due
    /// to it being too small. The item will reappear once the amount of funds changes or
    /// onchain-fees go down enough to make the amount recoverable.
    ///
    /// Requires network: **no**
    pub fn hide_unrecoverable_channel_close_funds_item(&self) -> Result<()> {
        let onchain_balance_sat = self.support.get_node_info()?.onchain_balance.sats;
        self.support
            .data_store
            .lock_unwrap()
            .store_hidden_channel_close_onchain_funds_amount_sat(onchain_balance_sat)?;
        Ok(())
    }

    /// Hides the unresolved failed swap action required item in case the amount cannot be
    /// recovered due to it being too small. The item will reappear once the onchain-fees go
    /// down enough to make the amount recoverable.
    ///
    /// Requires network: **no**
    pub fn hide_unrecoverable_failed_swap_item(
        &self,
        failed_swap_info: FailedSwapInfo,
    ) -> Result<()> {
        self.support
            .data_store
            .lock_unwrap()
            .store_hidden_unresolved_failed_swap(&failed_swap_info.address)?;
        Ok(())
    }
}
