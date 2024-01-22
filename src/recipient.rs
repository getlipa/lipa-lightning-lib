use breez_sdk_core::LNInvoice;

#[derive(Clone, Debug)]
pub enum ServiceKind {
    BusinessWallet,
    ConsumerWallet,
    Exchange,
    Lsp,
    Unknown,
}

#[derive(Clone, Debug)]
pub struct Provider {
    pub service: ServiceKind,
    pub name: String,
    pub node_ids: Vec<String>,
}

impl Provider {
    pub fn new(service: ServiceKind, name: &str, ids: Vec<&str>) -> Provider {
        Provider {
            service,
            name: name.to_string(),
            node_ids: ids.into_iter().map(String::from).collect(),
        }
    }
}

#[derive(Debug)]
pub enum RecipientNode {
    Custodial { custodian: Provider },
    NonCustodial { id: String, lsp: Provider },
    NonCustodialWrapped { lsp: Provider },
    Unknown,
}

pub(crate) struct RecipientDecoder {
    voltage: Provider,
    custodians: Vec<Provider>,
    lsps: Vec<Provider>,
}

impl RecipientDecoder {
    pub fn new() -> RecipientDecoder {
        let voltage = Provider::new(
            ServiceKind::Lsp,
            "Voltage Flow 2.0",
            vec!["03aefa43fbb4009b21a4129d05953974b7dbabbbfb511921410080860fca8ee1f0"],
        );
        let custodians = vec![
            Provider::new(
                ServiceKind::Exchange,
                "Kraken",
                vec!["02f1a8c87607f415c8f22c00593002775941dea48869ce23096af27b0cfdcc0b69"],
            ),
            Provider::new(
                ServiceKind::Exchange,
                "Bitstamp",
                vec!["02a04446caa81636d60d63b066f2814cbd3a6b5c258e3172cbdded7a16e2cfff4c"],
            ),
            Provider::new(
                ServiceKind::Exchange,
                "Okcoin",
                vec!["036b53093df5a932deac828cca6d663472dbc88322b05eec1d42b26ab9b16caa1c"],
            ),
            Provider::new(
                ServiceKind::Exchange,
                "OKX",
                vec!["0294ac3e099def03c12a37e30fe5364b1223fd60069869142ef96580c8439c2e0a"],
            ),
            Provider::new(
                ServiceKind::Exchange,
                "Binance",
                vec!["03a1f3afd646d77bdaf545cceaf079bab6057eae52c6319b63b5803d0989d6a72f"],
            ),
            Provider::new(
                ServiceKind::Exchange,
                "Bitfinex",
                vec![
                    "033d8656219478701227199cbd6f670335c8d408a92ae88b962c49d4dc0e83e025",
                    "03cde60a6323f7122d5178255766e38114b4722ede08f7c9e0c5df9b912cc201d6",
                ],
            ),
            Provider::new(
                ServiceKind::ConsumerWallet,
                "River Financial",
                vec![
                    "03037dc08e9ac63b82581f79b662a4d0ceca8a8ca162b1af3551595b8f2d97b70a",
                    "03aab7e9327716ee946b8fbfae039b0db85356549e72c5cca113ea67893d0821e5",
                ],
            ),
            Provider::new(
                ServiceKind::ConsumerWallet,
                "Wallet of Satoshi",
                vec![
                    "0324ba2392e25bff76abd0b1f7e4b53b5f82aa53fddc3419b051b6c801db9e2247",
                    "035e4ff418fc8b5554c5d9eea66396c227bd429a3251c8cbc711002ba215bfc226",
                ],
            ),
            Provider::new(
                ServiceKind::ConsumerWallet,
                "Alby",
                vec![
                    "0265791d3c9e14c69ebc2ea0f2b40c35c1a46b8db3c971bd51d1966206a42215af",
                    "030a58b8653d32b99200a2334cfe913e51dc7d155aa0116c176657a4f1722677a3",
                ],
            ),
            Provider::new(
                ServiceKind::BusinessWallet,
                "lipa for Business",
                vec![
                    "020333076e35e398a0c14c8a0211563bbcdce5087cb300342cba09414e9b5f3605",
                    "02ba3ad33666de22b4c22f5ff9fac0dc5d18ae9b6ce38c0a06d9e171494c39255a",
                ],
            ),
            Provider::new(
                ServiceKind::ConsumerWallet,
                "Strike.me",
                vec![
                    "02535215135eb832df0f9858ff775bd4ae0b8911c59e2828ff7d03b535b333e149",
                    "027cd974e47086291bb8a5b0160a889c738f2712a703b8ea939985fd16f3aae67e",
                    "0335e4265f783f37378e969c6a123557cf5d22cc97ec42ea3abff5dfaa64afea83",
                    "034d7f4bbbd6c1c1d8fbe0a42dd1f59e10b66540c6872dfcaa095d8d5cffebcf46",
                    "03b428ba4b48b524f1fa929203ddc2f0971c2077c2b89bb5b22fd83ed82ac2f7e1",
                ],
            ),
            Provider::new(
                ServiceKind::BusinessWallet,
                "OpenNode.com",
                vec![
                    "0248841dd4a94e902ede85285e67b5527afe5c46d6a3ff27955d63d18c70035757",
                    "028d98b9969fbed53784a36617eb489a59ab6dc9b9d77fcdca9ff55307cd98e3c4",
                    "03abf6f44c355dec0d5aa155bdbdd6e0c8fefe318eff402de65c6eb2e1be55dc3e",
                ],
            ),
            Provider::new(
                ServiceKind::ConsumerWallet,
                "Blink",
                vec![
                    "02fcc5bfc48e83f06c04483a2985e1c390cb0f35058baa875ad2053858b8e80dbd",
                    "0325bb9bda523a85dc834b190289b7e25e8d92615ab2f2abffbe97983f0bb12ffb",
                ],
            ),
            Provider::new(
                ServiceKind::ConsumerWallet,
                "ZEBEDEE",
                vec![
                    "0251fff168b58b74e9b476af5a515b91fe0540a3681bc97fbb65379a807aea5f66",
                    "02c3b0963276dc5f031a9147c3df203d6a03e194aa2934a821fa7709adc926263a",
                    "02c3e01efd4f1944e9a50939f34cb275716fcd438769cbc0126b015677fa3b187e",
                    "033e514ff30be0ea421f9512da0ed1aea52ea541275654d034bde3470a61269285", // klnd1
                    "0349cb2f33d5542432b016405a22dfda18617d87abe4718e61c45909b8a5449329",
                    "03ac0cf6da1916725f86d49ab35275b7b362054845e85c33ac181118aac266ebb7",
                    "03b6f613e88bd874177c28c6ad83b3baba43c4c656f56be1f8df84669556054b79", // klnd0
                    "03bf2ff8699e5528f65d41656d405c4002dd2415e4491e945fd465890bc3a9ce23",
                    "03d506016e3e0e540ac26d557a412520ea24990ca9405d410c24122f648752b830",
                    "03d6b14390cd178d670aa2d57c93d9519feaae7d1e34264d8bbb7932d47b75a50d",
                ],
            ),
            // Cashapp
            // Chivo (River Financial?)
            // Other custodial wallets from https://lightningaddress.com/#providers
        ];
        let lsps = vec![
            Provider::new(
                ServiceKind::ConsumerWallet,
                "lipa", // breez.diem.lsp
                vec!["0264a62a4307d701c04a46994ce5f5323b1ca28c80c66b73c631dbcb0990d6e835"],
            ),
            Provider::new(
                ServiceKind::Lsp,
                "c=",
                vec!["027100442c3b79f606f80f322d98d499eefcb060599efc5d4ecb00209c2cb54190"],
            ),
            Provider::new(
                ServiceKind::ConsumerWallet,
                "Breez",
                vec!["031015a7839468a3c266d662d5bb21ea4cea24226936e2864a7ca4f2c3939836e0"],
            ),
            Provider::new(
                ServiceKind::Lsp,
                "Breez-C",
                vec!["02c811e575be2df47d8b48dab3d3f1c9b0f6e16d0d40b5ed78253308fc2bd7170d"],
            ),
            Provider::new(
                ServiceKind::ConsumerWallet,
                "Zeus",
                vec!["031b301307574bbe9b9ac7b79cbe1700e31e544513eae0b5d7497483083f99e581"],
            ),
            Provider::new(
                ServiceKind::ConsumerWallet,
                "Phoenix",
                vec!["03864ef025fde8fb587d989186ce6a4a186895ee44a926bfc370e2c366597a3f8f"],
            ),
            Provider::new(
                ServiceKind::ConsumerWallet,
                "Bitkit",
                vec!["0296b2db342fcf87ea94d981757fdf4d3e545bd5cef4919f58b5d38dfdd73bf5c9"],
            ),
            Provider::new(
                ServiceKind::ConsumerWallet,
                "Blixt",
                vec!["0230a5bca558e6741460c13dd34e636da28e52afd91cf93db87ed1b0392a7466eb"],
            ),
            voltage.clone(),
        ];
        Self {
            voltage,
            custodians,
            lsps,
        }
    }

    pub fn decode(&self, invoice: &LNInvoice) -> RecipientNode {
        let id = &invoice.payee_pubkey;
        if invoice.routing_hints.is_empty() {
            for custodian in &self.custodians {
                if custodian.node_ids.contains(id) {
                    return RecipientNode::Custodial {
                        custodian: custodian.clone(),
                    };
                }
            }
            if self.voltage.node_ids.contains(id) {
                return RecipientNode::NonCustodialWrapped {
                    lsp: self.voltage.clone(),
                };
            }
        // TODO: Return node alias.
        // TODO: Compute confidence as amount of sats in days locked in announced channels.
        } else {
            // TODO: Check that the node does not have announced channels.
            for hint in &invoice.routing_hints {
                if hint.hops.len() == 1 {
                    if let Some(hop) = hint.hops.first() {
                        for lsp in &self.lsps {
                            if lsp.node_ids.contains(&hop.src_node_id) {
                                return RecipientNode::NonCustodial {
                                    id: id.clone(),
                                    lsp: lsp.clone(),
                                };
                            }
                        }
                    }
                } else {
                    return RecipientNode::Unknown;
                }
            }
            // TODO: If all hints start from the same node, return node alias.
            // TODO: Compute confidence as amount of sats in days locked in announced channels.
        }

        RecipientNode::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use breez_sdk_core::parse_invoice;

    fn decode(invoice: &str) -> String {
        let decoder = RecipientDecoder::new();
        let invoice = parse_invoice(&invoice).expect("Invalid invoice");
        match decoder.decode(&invoice) {
            RecipientNode::Custodial { custodian } => custodian.name,
            RecipientNode::NonCustodial { id, lsp } => format!("{id}@{}", lsp.name),
            RecipientNode::NonCustodialWrapped { lsp } => format!("wrapped@{}", lsp.name),
            RecipientNode::Unknown => "unknown".to_string(),
        }
    }

    #[test]
    fn test_decode_recipient() {
        let invoice = "lnbc1431800n1pjcgm4epp5hxr22je783fzcr37d4xp0gn5042pnz48u79lnvj76quu36nv0gmshp5fp66r97zwxcrs33jcc8l6rr3803rp8z3h30pevqevt0fqp203p7scqzzsxqyz5vqsp5jg45hhcmchvagsa8fn05nkyptp99cazgtvgchjcs5j3v7xu53rcq9qyyssq6m74fcnv704y0k2e50sqp6wc7wjhxmrhtjndyzutgzw8rplk8w3yg07wdraur2qh37wj67xkcwrv238s965dfdn90vfj75hm65xyf2sppxh5xw";
        assert_eq!(decode(invoice), "Alby");

        let invoice = "lnbc1u1pj62kd6pp557unu8u02cg7nqnsj5rnrgsrzctw7f85g9wr6wu3hhwa5qacmhtqdqqcqzzsxqyz5vqsp5arf47cesn7xyjc7wgq7fl288rczl45j4wql5un4tam8jcuchmh2s9qyyssqmzxkcqk9cpau6fu6zv5n5rz9znuuwwevxz073y8f37yv3qrpp3dpwhruf47206q3rv2st2d7jc2v8nxy7pa6ad7s8rsh9zzq5g33t3qq7d5huu";
        assert_eq!(decode(invoice), "Wallet of Satoshi");

        let invoice = "lnbc11110n1pj6tvs8pp5dxdctpprs7qw82etw4m9ecgjsq9u2ns85uw9vyvqczd7j492wsasdqqcqzzsxqyz5vqsp5y7v5wt5z7f5nk87dzv6vle4xtvkcfu8rlp2ryg3alhhptwwh3uws9qyyssqffdk8ufx8ezvpl292td2tfp9y7r8vyjxmfgdzwqm35gdtd9fyfgqhwz5wftvr6m2fa30a6hvhk4lcduq4wf09h0yxt5u4sucyz6rsegqknr266";
        assert_eq!(decode(invoice), "unknown");

        // Non-custodial.

        let invoice = "lnbc120n1pjcxr98dp923jhxarfdenjqur9dejxjmn8ypcxz7tdv4h8gpp5p0547ufczxajsnzwylyw082p2mz6cwswmr0z0uyhmgpfn06gc7tqxqrrsssp546n87knlt8hedp9cp30rkgtcduw2hrr00ex62msawwzfqszh0k7s9qrsgqcqzysrzjqfj2v2jrqltsrsz2g6v5ee04xga3eg5vsrrxku7xx8dukzvs6m5r2avk07w5uftf4sqqqqlgqqqqqzsqygs6sp6j4mwstpvjd648cmtndazpnfvhnsh9ff8frgrkmx3jarm0vxyqf822a2d9sefxzyqwlm5epvtcyj5rjpu09lsy4jffu7t0a7xxgqpzsw6v";
        assert_eq!(
            decode(invoice),
            "02c4d6599009cfc6a015562252ad7b14b8a4ed2640aeb69b688c215e3b4ceb5a99@lipa"
        );

        let invoice = "lnbc50n1pj62uuqpp5p447yvxk5cjflk685kl53eg3xxz4pp5m5356akn486ez0356p3csdqggfex2et6cqzzsxqrrssrzjqvgptfurj3528snx6e3dtwepafxw5fpzdymw9pj20jj09sunnqmwpapyqqqqqqqltqqqqqlgqqqqqqgq9qsp5jwllzl5nk8q7890qwyyprj9hxgey4hwsph7sq66wdd4p7v7t6pgq9qyyssqypk7z3rar8gnfype6mxsc92ccax49huemm2nnphx3qkhm53hflth6k8t577exmuqsxp5fm7evzpw5v5d3g3004ljh37v58t8wrcchagp063dqm";
        assert_eq!(
            decode(invoice),
            "039fdb76c4ef865649376aeb0b8d1cb71fa12b2712c18eecb7cb03364786f657aa@Breez"
        );

        let invoice = "lnbc5m1pj6tgnhsp5gsfazhx0c5gfcfmxh38ag5lyrshk9h4djzrejldvmfe49vxpyvyqpp59p000w04t5xhc9ch7lj909wtlqmgrcjxymcnnn4gc9xmux7cgnrsdqqnp4qwh05slmksqfkgdyz2wst9fewjmah2amldg3jg2pqzqgvr723mslqxqrrsxcqzzn9qyysgqcd2avdg6gt7j24tjycz0r38xr5r809tczelvyjr52cgy32z7nzs9wsmdxxws4xx8s7s8vv3w5qgfslcg608vj0ys2dqvqg227m75dwcq6z898f";
        assert_eq!(decode(invoice), "wrapped@Voltage Flow 2.0");
    }
}
