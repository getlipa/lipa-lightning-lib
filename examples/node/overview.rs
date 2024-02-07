use anyhow::{anyhow, Context, Result};
use chrono::offset::FixedOffset;
use chrono::{DateTime, Utc};
use colored::Colorize;
use thousands::Separable;
use uniffi_lipalightninglib::{
    Activity, Amount, BreezHealthCheckStatus, ChannelClose, FiatValue, LightningNode, Payment,
    PaymentState, PaymentType, Recipient,
};

pub fn overview(node: &LightningNode, words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let number_of_activities: u32 = words
        .next()
        .unwrap_or("10")
        .parse()
        .context("Number of activities should be a positive integer number")?;
    let activities = node.get_latest_activities(number_of_activities)?;

    let fun: bool = words
        .next()
        .unwrap_or("false")
        .parse()
        .context("Fun mode should be true of false")?;

    let health_message = match node.get_health_status()? {
        BreezHealthCheckStatus::Operational => None,
        BreezHealthCheckStatus::Maintenance => Some("Maintenance".yellow()),
        BreezHealthCheckStatus::ServiceDisruption => Some("Service Disruption".red().bold()),
    };
    if let Some(health_message) = health_message {
        println!("{health_message}");
    }

    // Balance
    let line = format!("{:^28}", "Balance".bold());
    println!("{}", line.reversed());

    let info = node.get_node_info()?;
    let balance = format_balance(info.channels_info.local_balance);
    let balance = format!("⚡ {balance}");
    println!("{balance:^35}");
    let balance = format_balance(info.onchain_balance);
    let balance = format!("🔗 {balance}");
    println!("{balance:^35}");
    println!();

    if fun {
        let actions = format!(
            "{} {} {}",
            " 💱 Buy ".reversed(),
            " 🔗 Trans ".reversed(),
            " 🌐 Map ".reversed()
        );
        println!("{actions:^28}");
        println!();
    }

    if fun {
        // Fake LNURL Auth.
        println!(" 🔑 Auth @ {:<15}", "bolt.fun");
        println!("    {:<15}", "Dec 6, 20:30".dimmed());

        // Fake pocket topup.
        let title = "Exchange 10 EUR";
        let amount = format!("+{}", 40864.separate_with_commas());
        println!(" 💱 {title:<15} {:>7}", amount.green());
        println!("    {:<15}", "Dec 6, 20:12   9.53 EUR".dimmed());
    }

    let line = format!("{:^28}", "Pending Activities".bold());
    println!("{}", line.reversed());
    for activity in activities.pending_activities {
        print_activity(activity)?;
    }
    println!();

    let line = format!("{:^28}", "Completed Activities".bold());
    println!("{}", line.reversed());
    for activity in activities.completed_activities {
        print_activity(activity)?;
    }

    Ok(())
}

fn print_activity(activity: Activity) -> Result<()> {
    match activity {
        Activity::PaymentActivity { payment } => print_payment(payment),
        Activity::ChannelCloseActivity { channel_close } => print_channel_close(channel_close),
    }
}

fn print_channel_close(channel_close: ChannelClose) -> Result<()> {
    let amount = channel_close.amount.sats.separate_with_commas();
    let amount = format!("−{amount}");
    let fiat = channel_close.amount.fiat.map_or(String::new(), format_fiat);
    println!(" 🛑 {:<15} {amount:>7}", "Channel close");
    if let Some(closed_at) = channel_close.closed_at {
        let timezone = FixedOffset::east_opt(closed_at.timezone_utc_offset_secs)
            .ok_or(anyhow!("east_opt failed"))?;
        let closed_at: DateTime<Utc> = closed_at.time.into();
        let closed_at = closed_at.with_timezone(&timezone);
        let time = closed_at.format("%b %-d, %H:%M");
        let line = format!("    {time:<13} {fiat:>9}").dimmed();
        println!("{line}");
    } else {
        let time = String::new();
        let line = format!("    {time:<13} {fiat:>9}").dimmed();
        println!("{line}");
    }
    Ok(())
}

fn print_payment(payment: Payment) -> Result<()> {
    let (link_1, link_2, lsp_fees) = match payment.lsp_fees {
        Some(lsp_fees) if lsp_fees.sats > 0 && payment.payment_state == PaymentState::Succeeded => {
            let icon = "💧";
            let title = "Liquidity fee";
            let amount = lsp_fees.sats.separate_with_commas();
            let amount = format!("−{amount}");
            let link = "┌".dimmed();
            let created_at: DateTime<Utc> = payment.created_at.time.into();
            let timezone = FixedOffset::east_opt(payment.created_at.timezone_utc_offset_secs)
                .ok_or(anyhow!("east_opt failed"))?;
            let created_at = created_at.with_timezone(&timezone);
            let time = created_at.format("%b %-d, %H:%M");
            let fiat = lsp_fees.fiat.map_or(String::new(), format_fiat);
            let line = format!("│   {time:<13} {fiat:>9}").dimmed();

            let lsp_fees = format!("{link}{icon} {title:<15} {amount:>7}");
            let lsp_fees = format!("{lsp_fees}\n{line}");
            ("└".dimmed(), " ".dimmed(), lsp_fees)
        }
        _ => (" ".normal(), " ".normal(), String::new()),
    };
    if !lsp_fees.is_empty() {
        println!("{lsp_fees}");
    }

    let (icon, title) = match payment.recipient {
        Some(Recipient::LightningAddress { address }) => (" @".bold(), address),
        Some(Recipient::Unknown) | None => ("🧾".normal(), "Invoice".to_string()),
    };

    let amount = payment.requested_amount.sats.separate_with_commas();
    let amount = match payment.payment_type {
        PaymentType::Receiving => format!("+{amount}").green(),
        PaymentType::Sending => format!("−{amount}").normal(),
    };

    let line = format!("{icon} {title:<15} {amount:>7}");
    let line = match payment.payment_state {
        PaymentState::Succeeded => line.normal(),
        PaymentState::Created | PaymentState::Retried => line.italic().dimmed(),
        PaymentState::Failed | PaymentState::InvoiceExpired => line.dimmed().strikethrough(),
    };
    println!("{link_1}{line}");

    // Time and fiat value.
    let created_at: DateTime<Utc> = payment.created_at.time.into();
    let timezone = FixedOffset::east_opt(payment.created_at.timezone_utc_offset_secs)
        .ok_or(anyhow!("east_opt failed"))?;
    let created_at = created_at.with_timezone(&timezone);

    let time = created_at.format("%b %-d, %H:%M");
    let fiat = payment.amount.fiat.map_or(String::new(), format_fiat);
    let line = format!("{link_2}   {time:<13} {fiat:>9}").dimmed();
    println!("{line}");

    if !payment.description.is_empty() {
        println!("{link_2} ↳ {}", payment.description.italic().dimmed());
    }
    Ok(())
}

fn format_balance(amount: Amount) -> String {
    let fiat = amount.fiat.map_or(String::new(), format_fiat);
    let amount = amount.sats.separate_with_commas().bold();
    format!("{amount} sats [{fiat}]")
}

fn format_fiat(fiat: FiatValue) -> String {
    let major = fiat.minor_units / 100;
    let minor = fiat.minor_units - major * 100;
    let major = major.separate_with_commas();
    format!("{major}.{minor:0>2} {}", fiat.currency_code)
}
