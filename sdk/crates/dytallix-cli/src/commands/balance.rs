//! `dytallix balance` on the consensus chain: an account's liquid balances
//! from its native state record (clients v1, K-c), with the record's state
//! proof checked against the application hash in the pinned node's next
//! block header (K-d). Header signatures are not checked.
use anyhow::Result;
use clap::Args;
use serde_json::json;

use super::bytes_to_hex;
use super::consensus;
use super::ordinary::print_json;

#[derive(Debug, Clone, Args)]
pub struct BalanceArgs {
    #[command(flatten)]
    read: consensus::ReadArgs,
}

pub async fn run(args: BalanceArgs) -> Result<()> {
    let (pin, client, address) = args.read.open()?;
    let reported = client.query_balances(&pin, &address, None).await?;
    let amount = |denomination: &str| {
        reported
            .balances
            .as_ref()
            .and_then(|b| b.get(denomination).copied())
            .unwrap_or(0)
    };
    print_json(&json!({
        "address": address.encode(),
        "height": reported.height,
        "account_exists": reported.balances.is_some(),
        "udgt": amount("udgt").to_string(),
        "udrt": amount("udrt").to_string(),
        "dgt": consensus::tokens(amount("udgt")),
        "drt": consensus::tokens(amount("udrt")),
        "proof_verified": true,
        "app_hash": bytes_to_hex(&reported.app_hash),
        "app_hash_source": reported.source,
    }))
}
