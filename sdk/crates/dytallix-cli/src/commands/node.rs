//! Node command implementation.

use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use clap::{Args, Subcommand};

use crate::commands::{raw_get_json_at, LOCAL_ENDPOINT};
use crate::output;
use dytallix_sdk::client::DytallixClient;

/// Arguments for the `node` command.
#[derive(Debug, Clone, Args)]
pub struct NodeArgs {
    /// Node subcommand.
    #[command(subcommand)]
    pub command: NodeCommand,
}

/// Node subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum NodeCommand {
    /// Start the local node.
    Start,
    /// Stop the local node.
    Stop,
    /// Show local node status.
    Status,
    /// Show connected peers.
    Peers,
    /// Show recent log output.
    Logs,
}

/// Runs the `node` command.
pub async fn run(args: NodeArgs) -> Result<()> {
    match args.command {
        NodeCommand::Start => run_script("start-local.sh"),
        NodeCommand::Stop => run_script("stop-local.sh"),
        NodeCommand::Status => status().await,
        NodeCommand::Peers => peers().await,
        NodeCommand::Logs => logs(),
    }
}

fn run_script(name: &str) -> Result<()> {
    let script = find_upwards(name).ok_or_else(|| {
        anyhow!(
            "Could not find `{name}` or `scripts/{name}` from the current directory. Run from the dytallix-sdk repo root or create the helper scripts."
        )
    })?;
    let status = std::process::Command::new(&script).status()?;
    if status.success() {
        output::success(&format!("Executed {}", script.display()), None);
        Ok(())
    } else {
        Err(anyhow!(
            "Script {} exited with a non-zero status.",
            script.display()
        ))
    }
}

async fn status() -> Result<()> {
    let client = DytallixClient::local().await?;
    let status = client.get_chain_status().await?;
    output::section("Local node status");
    println!("Block height: {}", status.block_height);
    println!("Epoch:        {}", status.epoch);
    println!("Slot:         {}", status.slot);
    Ok(())
}

async fn peers() -> Result<()> {
    let value = raw_get_json_at(LOCAL_ENDPOINT, "/peers").await?;
    output::section("Peers");
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn logs() -> Result<()> {
    let logs_dir = find_upwards("logs")
        .ok_or_else(|| anyhow!("No logs directory found from the current directory."))?;
    output::section("Logs");
    for entry in fs::read_dir(&logs_dir)? {
        let entry = entry?;
        println!("{}", entry.path().display());
    }
    Ok(())
}

fn find_upwards(name: &str) -> Option<PathBuf> {
    let mut current = std::env::current_dir().ok()?;
    for _ in 0..6 {
        let candidates = [current.join(name), current.join("scripts").join(name)];
        for candidate in candidates {
            if candidate.exists() {
                return Some(candidate);
            }
        }
        if !current.pop() {
            break;
        }
    }
    None
}
