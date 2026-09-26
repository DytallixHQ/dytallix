//! Chain query command implementation.

use anyhow::{anyhow, Result};
use clap::{Args, Subcommand};

use dytallix_sdk::client::CapabilitiesSource;
use dytallix_sdk::BlockId;

use crate::commands::{configured_client, humanize_sdk_error, raw_get_json};
use crate::output;

/// Arguments for the `chain` command.
#[derive(Debug, Clone, Args)]
pub struct ChainArgs {
    /// Chain subcommand.
    #[command(subcommand)]
    pub command: ChainCommand,
}

/// Chain subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum ChainCommand {
    /// Show chain head status.
    Status,
    /// Show a block by number or hash.
    Block {
        /// Block number, hash, `latest`, or `finalized`.
        id: String,
    },
    /// Show the current epoch and slot.
    Epoch,
    /// Show the machine-readable chain capabilities contract.
    Capabilities {
        /// Fail if the capabilities document cannot be fetched from a live node.
        #[arg(long)]
        require_live: bool,
    },
    /// Show chain parameters.
    Params,
}

/// Runs the `chain` command.
pub async fn run(args: ChainArgs) -> Result<()> {
    match args.command {
        ChainCommand::Status => show_status().await,
        ChainCommand::Block { id } => show_block(&id).await,
        ChainCommand::Epoch => show_epoch().await,
        ChainCommand::Capabilities { require_live } => show_capabilities(require_live).await,
        ChainCommand::Params => show_params().await,
    }
}

async fn show_status() -> Result<()> {
    let client = configured_client().await?;
    let status = client
        .get_chain_status()
        .await
        .map_err(humanize_sdk_error)?;
    output::section("Chain status");
    println!("Block height:          {}", status.block_height);
    println!("Epoch:                 {}", status.epoch);
    println!("Slot:                  {}", status.slot);
    println!("Finalized checkpoint:  {}", status.finalized_checkpoint);
    Ok(())
}

async fn show_block(id: &str) -> Result<()> {
    let client = configured_client().await?;
    let block_id = parse_block_id(id)?;
    let block = client
        .get_block(block_id)
        .await
        .map_err(humanize_sdk_error)?;
    output::section("Block");
    println!("Number:      {}", block.number);
    println!("Hash:        {}", block.hash);
    println!("Parent hash: {}", block.parent_hash);
    println!("Proposer:    {}", block.proposer);
    println!("Slot:        {}", block.slot);
    println!("Epoch:       {}", block.epoch);
    println!("Tx count:    {}", block.tx_count);
    println!("C-Gas used:  {}", block.c_gas_used);
    println!("B-Gas used:  {}", block.b_gas_used);
    println!("Timestamp:   {}", block.timestamp);
    Ok(())
}

async fn show_epoch() -> Result<()> {
    let client = configured_client().await?;
    let status = client
        .get_chain_status()
        .await
        .map_err(humanize_sdk_error)?;
    output::section("Epoch");
    println!("Epoch: {}", status.epoch);
    println!("Slot:  {}", status.slot);
    Ok(())
}

async fn show_capabilities(require_live: bool) -> Result<()> {
    let client = configured_client().await?;
    let (capabilities, source) = client
        .get_capabilities_with_source()
        .await
        .map_err(humanize_sdk_error)?;
    if require_live && source != CapabilitiesSource::LiveNode {
        return Err(anyhow!(
            "Live capabilities endpoint is unavailable at the current node. The SDK fell back to its embedded manifest instead. Remove `--require-live`, or point the CLI at a compatible node that serves /api/capabilities."
        ));
    }
    output::section("Chain capabilities");
    println!("Source: {}", source.as_str());
    println!("{}", serde_json::to_string_pretty(&capabilities)?);
    Ok(())
}

async fn show_params() -> Result<()> {
    let status = raw_get_json("/status").await?;
    let params = serde_json::json!({
        "chain_id": status.get("chain_id").cloned().unwrap_or(serde_json::Value::Null),
        "gas": status.get("gas").cloned().unwrap_or(serde_json::Value::Null),
    });
    output::section("Chain params");
    println!("{}", serde_json::to_string_pretty(&params)?);
    Ok(())
}

fn parse_block_id(id: &str) -> Result<BlockId> {
    if id.eq_ignore_ascii_case("latest") {
        Ok(BlockId::Latest)
    } else if id.eq_ignore_ascii_case("finalized") {
        Ok(BlockId::Finalized)
    } else if let Ok(number) = id.parse::<u64>() {
        Ok(BlockId::Number(number))
    } else if !id.is_empty() {
        Ok(BlockId::Hash(id.to_owned()))
    } else {
        Err(anyhow!(
            "Provide a block number, block hash, `latest`, or `finalized`."
        ))
    }
}
