//! Legacy testnet commands over the REST API, built with the
//! `legacy-network` feature until that testnet retires (clients v1,
//! decision 1). The consensus chain refuses all of them.
use anyhow::Result;
use clap::{Args, Subcommand};

pub mod balance;
pub mod governance;
pub mod send;
pub mod stake;

#[derive(Debug, Clone, Args)]
pub struct LegacyArgs {
    #[command(subcommand)]
    pub command: LegacyCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum LegacyCommand {
    /// Show DGT and DRT balances through the testnet REST API.
    Balance(balance::BalanceArgs),
    /// Send DGT or DRT through the testnet REST API.
    Send(send::SendArgs),
    /// Staking status and direct-node staking writes.
    Stake(stake::StakeArgs),
    /// Governance reads and direct-node governance writes.
    Governance(governance::LegacyArgs),
}

pub async fn run(args: LegacyArgs) -> Result<()> {
    match args.command {
        LegacyCommand::Balance(args) => balance::run(args).await,
        LegacyCommand::Send(args) => send::run(args).await,
        LegacyCommand::Stake(args) => stake::run(args).await,
        LegacyCommand::Governance(args) => governance::run(args).await,
    }
}
