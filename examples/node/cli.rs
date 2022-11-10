use bitcoin::secp256k1::PublicKey;
use std::io;
use std::io::{BufRead, Write};

use crate::LightningNode;

pub(crate) fn poll_for_user_input(node: &LightningNode, log_file_path: &str) {
    println!("LDK startup successful. To view available commands: \"help\".");
    println!("Detailed logs are available at {}", log_file_path);
    println!("To stop the LDK node, please type \"stop\" for a graceful shutdown.");
    println!(
        "Local Node ID is: {}",
        PublicKey::from_slice(&node.get_node_info().node_pubkey).unwrap()
    );
    let stdin = io::stdin();
    let mut line_reader = stdin.lock().lines();
    loop {
        print!("> ");
        io::stdout().flush().unwrap(); // Without flushing, the `>` doesn't print
        let line = match line_reader.next() {
            Some(l) => l.unwrap(),
            None => break,
        };
        let mut words = line.split_whitespace();
        if let Some(word) = words.next() {
            match word {
                "help" => help(),
                "nodeinfo" => {
                    node_info(node);
                }
                "invoice" => {
                    if let Err(message) = create_invoice(node, &mut words) {
                        println!("Error: {}", message);
                    }
                }
                "stop" => {
                    break;
                }
                _ => println!("Unknown command. See `\"help\" for available commands."),
            }
        }
    }
}

fn help() {
    println!("invoice <amount in millisats> [description]");
    println!("nodeinfo");
    println!("stop");
}

fn node_info(node: &LightningNode) {
    let node_info = node.get_node_info();
    println!(
        "Node PubKey: {}",
        PublicKey::from_slice(&node_info.node_pubkey).unwrap()
    );
    println!("Number of connected peers: {}", node_info.num_peers);
    println!(
        "       Number of channels: {}",
        node_info.channels_info.num_channels
    );
    println!(
        "Number of usable channels: {}",
        node_info.channels_info.num_usable_channels
    );
    println!(
        "    Local balance in msat: {}",
        node_info.channels_info.local_balance_msat
    );
    println!(
        " Inbound capacity in msat: {}",
        node_info.channels_info.inbound_capacity_msat
    );
    println!(
        "Outbound capacity in msat: {}",
        node_info.channels_info.outbound_capacity_msat
    );
}

fn create_invoice<'a>(
    node: &LightningNode,
    words: &mut dyn Iterator<Item = &'a str>,
) -> Result<(), String> {
    let amount = words
        .next()
        .ok_or_else(|| "amount in millisats is required".to_string())?;
    let amount: u64 = amount
        .parse()
        .map_err(|_| "amount should be an integer number".to_string())?;
    let description = words.collect::<Vec<_>>().join(" ");
    let invoice = node
        .create_invoice(amount, description)
        .map_err(|e| e.to_string())?;
    println!("{}", invoice);
    Ok(())
}
