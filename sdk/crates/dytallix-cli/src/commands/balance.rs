//! Balance command implementation.

use anyhow::Result;
use clap::Args;

use crate::commands::{
    active_entry, configured_client, humanize_sdk_error, load_keystore, validate_address,
};
use crate::output;

/// Arguments for the `balance` command.
#[derive(Debug, Clone, Args)]
pub struct BalanceArgs {
    /// Optional address to inspect instead of the active wallet.
    pub address: Option<String>,
}

/// Runs the `balance` command.
pub async fn run(args: BalanceArgs) -> Result<()> {
    let address = match args.address {
        Some(raw) => validate_address(&raw)?,
        None => {
            let keystore = load_keystore()?;
            active_entry(&keystore)?.address.clone()
        }
    };

    let client = configured_client().await?;
    let balance = client
        .get_balance(&address)
        .await
        .map_err(humanize_sdk_error)?;

    output::section("Balances");
    output::balance(balance.dgt, balance.drt);
    Ok(())
}
