use lightning::ln::channelmanager::ChannelDetails;

#[derive(Debug, PartialEq, Eq)]
pub struct ChannelsInfo {
    pub num_channels: u16,
    pub num_usable_channels: u16,
    pub local_balance_msat: u64,
    pub inbound_capacity_msat: u64,
    pub outbound_capacity_msat: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct NodeInfo {
    pub node_pubkey: Vec<u8>,
    pub num_peers: u16,
    pub channels_info: ChannelsInfo,
}

pub(crate) fn get_channels_info(channels: &[ChannelDetails]) -> ChannelsInfo {
    let usable_channels = channels.iter().filter(|c| c.is_usable);

    let num_channels = channels.len() as u16;
    let num_usable_channels = usable_channels.clone().count() as u16;
    let local_balance_msat = channels.iter().map(|c| c.balance_msat).sum();
    let inbound_capacity_msat = usable_channels
        .clone()
        .map(|c| c.inbound_capacity_msat)
        .sum();
    let outbound_capacity_msat = usable_channels
        .clone()
        .map(|c| c.outbound_capacity_msat)
        .sum();

    ChannelsInfo {
        num_channels,
        num_usable_channels,
        local_balance_msat,
        inbound_capacity_msat,
        outbound_capacity_msat,
    }
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
                local_balance_msat: 0,
                inbound_capacity_msat: 0,
                outbound_capacity_msat: 0,
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
                local_balance_msat: 0,
                inbound_capacity_msat: 1_111,
                outbound_capacity_msat: 1_222,
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
                local_balance_msat: 0,
                inbound_capacity_msat: 90_000,
                outbound_capacity_msat: 90_111,
            }
        );
        assert_eq!(
            get_channels_info(&vec![channel1.clone(), channel2.clone()]),
            ChannelsInfo {
                num_channels: 2,
                num_usable_channels: 2,
                local_balance_msat: 0,
                inbound_capacity_msat: 91_111,
                outbound_capacity_msat: 91_333,
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
                local_balance_msat: 0,
                inbound_capacity_msat: 0,
                outbound_capacity_msat: 0,
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
                local_balance_msat: 0,
                inbound_capacity_msat: 91_111,
                outbound_capacity_msat: 91_333,
            }
        );
        assert_eq!(
            get_channels_info(&vec![channel1.clone(), channel2.clone()]),
            ChannelsInfo {
                num_channels: 2,
                num_usable_channels: 2,
                local_balance_msat: 0,
                inbound_capacity_msat: 91_111,
                outbound_capacity_msat: 91_333,
            }
        );
    }
}
