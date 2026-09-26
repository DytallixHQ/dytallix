//! Governance command implementation.

use anyhow::{anyhow, Result};
use clap::{Args, Subcommand, ValueEnum};

use dytallix_sdk::transaction::TransactionBuilder;
use dytallix_sdk::Token;

use crate::commands::{
    active_entry, active_keypair, configured_client, ensure_public_gateway_write_allowed,
    humanize_sdk_error, load_keystore, raw_get_json,
};
use crate::output;

/// Arguments for the `governance` command.
#[derive(Debug, Clone, Args)]
pub struct GovernanceArgs {
    /// Governance subcommand.
    #[command(subcommand)]
    pub command: GovernanceCommand,
}

/// Governance subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum GovernanceCommand {
    /// List governance proposals.
    Proposals,
    /// Vote on a governance proposal on a local or direct node endpoint.
    Vote { id: u64, choice: VoteChoice },
    /// Submit a minimal governance proposal transaction on a local or direct node endpoint.
    Propose,
    /// Show a governance proposal status from the public proposals list.
    Status { id: u64 },
}

/// Governance vote choices.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum VoteChoice {
    /// Vote yes.
    Yes,
    /// Vote no.
    No,
    /// Abstain from the proposal.
    Abstain,
}

/// Runs the `governance` command.
pub async fn run(args: GovernanceArgs) -> Result<()> {
    match args.command {
        GovernanceCommand::Proposals => proposals().await,
        GovernanceCommand::Vote { id, choice } => vote(id, choice).await,
        GovernanceCommand::Propose => propose().await,
        GovernanceCommand::Status { id } => status(id).await,
    }
}

async fn proposals() -> Result<()> {
    let value = raw_get_json("/api/governance/proposals").await?;
    output::section("Governance proposals");
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

async fn vote(id: u64, choice: VoteChoice) -> Result<()> {
    ensure_public_gateway_write_allowed(
        "governance",
        "governanceWrites",
        "dytallix governance proposals",
    )
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
        .amount(0, Token::DGT)
        .nonce(account.nonce)
        .data(format!("governance:vote:{id}:{}", vote_choice_label(choice)).into_bytes())
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
    output::success("Governance vote submitted", None);
    Ok(())
}

async fn propose() -> Result<()> {
    ensure_public_gateway_write_allowed(
        "governance",
        "governanceWrites",
        "dytallix governance proposals",
    )
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
        .amount(0, Token::DGT)
        .nonce(account.nonce)
        .data(b"governance:propose".to_vec())
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
    output::success("Governance proposal submitted", None);
    Ok(())
}

async fn status(id: u64) -> Result<()> {
    let value = raw_get_json("/api/governance/proposals").await?;
    let proposal = value
        .get("proposals")
        .and_then(|proposals| proposals.as_array())
        .and_then(|proposals| {
            proposals
                .iter()
                .find(|proposal| proposal.get("id").and_then(|raw| raw.as_u64()) == Some(id))
        })
        .cloned()
        .ok_or_else(|| {
            anyhow!("Governance proposal {id} was not found through the public proposals API.")
        })?;
    output::section("Governance status");
    println!("{}", serde_json::to_string_pretty(&proposal)?);
    Ok(())
}

fn vote_choice_label(choice: VoteChoice) -> &'static str {
    match choice {
        VoteChoice::Yes => "yes",
        VoteChoice::No => "no",
        VoteChoice::Abstain => "abstain",
    }
}
