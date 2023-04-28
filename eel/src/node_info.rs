use crate::rounding::ToSats;

use lightning::ln::channelmanager::ChannelDetails;

#[derive(Debug, PartialEq, Eq)]
pub struct ChannelsInfo {
    pub num_channels: u16,
    pub num_usable_channels: u16,
    pub local_balance_sat: u64,
    pub inbound_capacity_sat: u64,
    pub outbound_capacity_sat: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct NodeInfo {
    pub node_pubkey: Vec<u8>,
    pub num_peers: u16,
    pub channels_info: ChannelsInfo,
}

pub(crate) fn get_channels_info(channels: &[ChannelDetails]) -> ChannelsInfo {
    let usable_channels: Vec<_> = channels.iter().filter(|c| c.is_usable).collect();

    let num_channels = channels.len() as u16;
    let num_usable_channels = usable_channels.len() as u16;
    let local_balance_sat = channels
        .iter()
        .map(|c| c.balance_msat)
        .sum::<u64>()
        .to_sats_down();
    let inbound_capacity_sat = usable_channels
        .iter()
        .map(|c| c.inbound_capacity_msat)
        .sum::<u64>()
        .to_sats_down();
    let outbound_capacity_sat = usable_channels
        .iter()
        .map(|c| c.outbound_capacity_msat)
        .sum::<u64>()
        .to_sats_down();

    ChannelsInfo {
        num_channels,
        num_usable_channels,
        local_balance_sat,
        inbound_capacity_sat,
        outbound_capacity_sat,
    }
}

pub(crate) fn estimate_max_incoming_payment_size(channels_info: &ChannelsInfo) -> u64 {
    // TODO: This estimation is not precise. See a similar issue with outbound capacity:
    //       https://github.com/lightningdevkit/rust-lightning/issues/1126
    channels_info.inbound_capacity_sat
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::channels::channel;

    #[test]
    fn test_get_channels_info() {
        assert_eq!(
            get_channels_info(&Vec::new()),
            ChannelsInfo {
                num_channels: 0,
                num_usable_channels: 0,
                local_balance_sat: 0,
                inbound_capacity_sat: 0,
                outbound_capacity_sat: 0,
            }
        );

        let mut channel1 = channel();
        channel1.is_usable = true;
        channel1.inbound_capacity_msat = 1_111;
        channel1.outbound_capacity_msat = 1_222;
        assert_eq!(
            get_channels_info(&vec![channel1.clone()]),
            ChannelsInfo {
                num_channels: 1,
                num_usable_channels: 1,
                local_balance_sat: 0,
                inbound_capacity_sat: 1,
                outbound_capacity_sat: 1,
            }
        );

        let mut channel2 = channel();
        channel2.is_usable = true;
        channel2.inbound_capacity_msat = 90_000;
        channel2.outbound_capacity_msat = 90_111;
        assert_eq!(
            get_channels_info(&vec![channel2.clone()]),
            ChannelsInfo {
                num_channels: 1,
                num_usable_channels: 1,
                local_balance_sat: 0,
                inbound_capacity_sat: 90,
                outbound_capacity_sat: 90,
            }
        );
        assert_eq!(
            get_channels_info(&vec![channel1.clone(), channel2.clone()]),
            ChannelsInfo {
                num_channels: 2,
                num_usable_channels: 2,
                local_balance_sat: 0,
                inbound_capacity_sat: 91,
                outbound_capacity_sat: 91,
            }
        );

        let mut not_usable_channel = channel();
        not_usable_channel.inbound_capacity_msat = 777_777;
        not_usable_channel.outbound_capacity_msat = 888_888;
        assert_eq!(
            get_channels_info(&vec![not_usable_channel.clone()]),
            ChannelsInfo {
                num_channels: 1,
                num_usable_channels: 0,
                local_balance_sat: 0,
                inbound_capacity_sat: 0,
                outbound_capacity_sat: 0,
            }
        );
        assert_eq!(
            get_channels_info(&vec![
                channel1.clone(),
                channel2.clone(),
                not_usable_channel.clone()
            ]),
            ChannelsInfo {
                num_channels: 3,
                num_usable_channels: 2,
                local_balance_sat: 0,
                inbound_capacity_sat: 91,
                outbound_capacity_sat: 91,
            }
        );
        assert_eq!(
            get_channels_info(&vec![channel1.clone(), channel2.clone()]),
            ChannelsInfo {
                num_channels: 2,
                num_usable_channels: 2,
                local_balance_sat: 0,
                inbound_capacity_sat: 91,
                outbound_capacity_sat: 91,
            }
        );
    }
}
