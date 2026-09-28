//! `dytallix balance` on the consensus chain: an account's liquid balances
//! from its native state record (clients v1, K-c), with the record's state
//! proof checked against the application hash in the pinned node's next
//! block header (K-d). Header signatures are not checked.
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use clap::Args;
use dytallix_sdk::ordinary_v2::AccountAddress;
use serde_json::json;

use super::bytes_to_hex;
use super::consensus::{self, ChainConfig};
use super::ordinary::{client, print_json};

#[derive(Debug, Clone, Args)]
pub struct BalanceArgs {
    /// Account address on the pinned network; defaults to the signer's.
    address: Option<String>,
    /// Comet RPC endpoint; defaults to the pinned chain's endpoint.
    #[arg(long)]
    endpoint: Option<String>,
    /// Keystore wallet whose address to read; defaults to the active wallet.
    #[arg(long, conflicts_with_all = ["key_file", "address"])]
    wallet: Option<String>,
    #[arg(long, conflicts_with = "address")]
    key_file: Option<PathBuf>,
}

pub async fn run(args: BalanceArgs) -> Result<()> {
    let config = ChainConfig::load()?;
    let pin = config.pin()?;
    let address = match &args.address {
        Some(raw) => AccountAddress::decode(pin.network, raw)
            .map_err(|_| anyhow!("address is not on the pinned network"))?,
        None => {
            let key = consensus::load_signer(args.wallet.as_deref(), args.key_file.as_deref())?;
            pin.origin_address(&consensus::key_identity(&key)?)?
        }
    };
    let client = client(args.endpoint.as_deref().unwrap_or(&config.endpoint))?;
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
