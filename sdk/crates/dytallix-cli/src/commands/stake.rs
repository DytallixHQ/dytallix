//! Stake command implementation.

use anyhow::{anyhow, Result};
use clap::{Args, Subcommand};

use dytallix_sdk::transaction::TransactionBuilder;
use dytallix_sdk::Token;

use crate::commands::{
    active_entry, active_keypair, configured_client, ensure_public_gateway_write_allowed,
    humanize_sdk_error, load_keystore, raw_get_json, validate_address,
};
use crate::output;

/// Arguments for the `stake` command.
#[derive(Debug, Clone, Args)]
pub struct StakeArgs {
    /// Stake subcommand.
    #[command(subcommand)]
    pub command: StakeCommand,
}

/// Stake subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum StakeCommand {
    /// Delegate DGT to a validator on a local or direct node endpoint.
    Delegate { validator: String, amount: u128 },
    /// Undelegate DGT from a validator on a local or direct node endpoint.
    Undelegate { validator: String, amount: u128 },
    /// Claim unclaimed DRT staking rewards on a local or direct node endpoint.
    Claim,
    /// Show the current public staking balance view.
    Status,
}

/// Runs the `stake` command.
pub async fn run(args: StakeArgs) -> Result<()> {
    match args.command {
        StakeCommand::Delegate { validator, amount } => {
            submit_stake_tx("delegate", &validator, amount).await
        }
        StakeCommand::Undelegate { validator, amount } => {
            submit_stake_tx("undelegate", &validator, amount).await
        }
        StakeCommand::Claim => claim_rewards().await,
        StakeCommand::Status => status().await,
    }
}

async fn submit_stake_tx(operation: &str, validator: &str, amount: u128) -> Result<()> {
    ensure_public_gateway_write_allowed("staking", "stakingWrites", "dytallix stake status")
        .await?;
    let validator = validate_address(validator)?;
    let keystore = load_keystore()?;
    let sender = active_entry(&keystore)?.address.clone();
    let keypair = active_keypair(&keystore)?;
    let client = configured_client().await?;
    let account = client
        .get_account(&sender)
        .await
        .map_err(humanize_sdk_error)?;

    let payload = format!("stake:{operation}:{validator}:{amount}").into_bytes();
    let tx = TransactionBuilder::new()
        .from(sender)
        .to(validator)
        .amount(amount, Token::DGT)
        .nonce(account.nonce)
        .data(payload)
        .build()
        .map_err(|err| anyhow!(err.to_string()))?;
    let (tx, fee) = tx
        .with_estimated_fee(&client)
        .await
        .map_err(humanize_sdk_error)?;
    output::fee_breakdown(&fee);
    let signed = tx.sign(&keypair).map_err(humanize_sdk_error)?;
    let receipt = client
        .submit_transaction(&signed)
        .await
        .map_err(humanize_sdk_error)?;
    output::tx_hash(&receipt.hash);
    output::success(&format!("Stake {operation} submitted"), None);
    Ok(())
}

async fn claim_rewards() -> Result<()> {
    ensure_public_gateway_write_allowed("staking", "stakingWrites", "dytallix stake status")
        .await?;
    let keystore = load_keystore()?;
    let sender = active_entry(&keystore)?.address.clone();
    let keypair = active_keypair(&keystore)?;
    let client = configured_client().await?;
    let account = client
        .get_account(&sender)
        .await
        .map_err(humanize_sdk_error)?;

    let tx = TransactionBuilder::new()
        .from(sender.clone())
        .to(sender)
        .amount(0, Token::DRT)
        .nonce(account.nonce)
        .data(b"stake:claim".to_vec())
        .build()
        .map_err(|err| anyhow!(err.to_string()))?;
    let (tx, fee) = tx
        .with_estimated_fee(&client)
        .await
        .map_err(humanize_sdk_error)?;
    output::fee_breakdown(&fee);
    let signed = tx.sign(&keypair).map_err(humanize_sdk_error)?;
    let receipt = client
        .submit_transaction(&signed)
        .await
        .map_err(humanize_sdk_error)?;
    output::tx_hash(&receipt.hash);
    output::success("Stake claim submitted", None);
    Ok(())
}

async fn status() -> Result<()> {
    let keystore = load_keystore()?;
    let address = active_entry(&keystore)?.address.clone();
    let value = raw_get_json(&format!("/api/staking/balance/{address}")).await?;
    output::section("Staking status");
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
