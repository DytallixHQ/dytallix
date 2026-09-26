//! Faucet command implementation.

use anyhow::Result;
use clap::Args;

use crate::commands::{
    active_entry, faucet_balance, faucet_request, faucet_status, load_keystore, validate_address,
};
use crate::output;

/// Arguments for the `faucet` command.
#[derive(Debug, Clone, Args)]
pub struct FaucetArgs {
    /// Optional address or the literal `status`.
    pub target: Option<String>,
}

/// Runs the `faucet` command.
pub async fn run(args: FaucetArgs) -> Result<()> {
    match args.target.as_deref() {
        Some("status") => {
            let keystore = load_keystore()?;
            let address = active_entry(&keystore)?.address.clone();
            let status = faucet_status(&address).await?;
            output::section("Faucet status");
            if status.can_request {
                output::success("Faucet request is available", None);
            } else if let Some(retry_after_seconds) = status.retry_after_seconds {
                output::warning(&format!(
                    "Faucet cooldown active. Try again in {retry_after_seconds} seconds."
                ));
            } else {
                output::warning(
                    "Faucet status is degraded. Check your network connection or try again later.",
                );
            }
            println!("Address: {address}");
        }
        Some(raw_address) => {
            let address = validate_address(raw_address)?;
            faucet_request(&address, "both").await?;
            let balance = faucet_balance(&address).await?;
            output::section("Faucet funded");
            output::balance(balance.dgt, balance.drt);
        }
        None => {
            let keystore = load_keystore()?;
            let address = active_entry(&keystore)?.address.clone();
            faucet_request(&address, "both").await?;
            let balance = faucet_balance(&address).await?;
            output::section("Faucet funded");
            output::balance(balance.dgt, balance.drt);
        }
    }

    Ok(())
}
