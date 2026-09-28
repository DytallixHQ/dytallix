//! `dytallix stake` on the consensus chain: reward bonds as ordinary-v2
//! transactions signed against the pinned chain (clients v1, K-c), and
//! the account summary and validator set (interfaces v1, decision 2).
use anyhow::{ensure, Result};
use clap::{Args, Subcommand};
use dytallix_sdk::ordinary_v2::{parse_token_units, Action};

use super::consensus::{self, Connection, ReadArgs, WriteArgs};
use super::ordinary::print_json;

#[derive(Debug, Clone, Args)]
pub struct StakeArgs {
    #[command(subcommand)]
    pub command: StakeCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum StakeCommand {
    /// Bond DGT to a validator.
    Bond {
        #[arg(long)]
        validator: String,
        /// DGT, with up to six decimal places.
        #[arg(long)]
        amount: String,
        #[command(flatten)]
        connection: Connection,
        #[command(flatten)]
        write: WriteArgs,
    },
    /// Begin unbonding DGT from a validator.
    Unbond {
        #[arg(long)]
        validator: String,
        /// DGT, with up to six decimal places.
        #[arg(long)]
        amount: String,
        #[command(flatten)]
        connection: Connection,
        #[command(flatten)]
        write: WriteArgs,
    },
    /// Claim accrued DRT staking rewards.
    Claim {
        #[command(flatten)]
        connection: Connection,
        #[command(flatten)]
        write: WriteArgs,
    },
    /// Show an account's balances, bonds, unbonding and claimable rewards.
    /// The node's report; `dytallix balance` proves the balances.
    Status {
        #[command(flatten)]
        read: ReadArgs,
    },
    /// Show the validator set of the next block.
    Validators {
        /// Comet RPC endpoint; defaults to the pinned chain's endpoint.
        #[arg(long)]
        endpoint: Option<String>,
    },
}

fn udgt(amount: &str) -> Result<u128> {
    let amount = parse_token_units(amount)?;
    ensure!(amount > 0, "amount must be positive");
    Ok(amount)
}

pub async fn run(args: StakeArgs) -> Result<()> {
    let (action, connection, write) = match args.command {
        StakeCommand::Status { read } => {
            let (pin, client, address) = read.open()?;
            return print_json(&client.query_account_summary(&pin, &address).await?);
        }
        StakeCommand::Validators { endpoint } => {
            let (pin, client) = consensus::open_chain(endpoint.as_deref())?;
            return print_json(&client.query_validators(&pin).await?);
        }
        StakeCommand::Bond {
            validator,
            amount,
            connection,
            write,
        } => (
            Action::RewardBond {
                validator_id: validator,
                amount_udgt: udgt(&amount)?,
            },
            connection,
            write,
        ),
        StakeCommand::Unbond {
            validator,
            amount,
            connection,
            write,
        } => (
            Action::RewardBeginUnbond {
                validator_id: validator,
                amount_udgt: udgt(&amount)?,
            },
            connection,
            write,
        ),
        StakeCommand::Claim { connection, write } => (Action::RewardClaim, connection, write),
    };
    let session = connection.open()?;
    print_json(&consensus::submit_ordinary(&session, vec![action], write).await?)
}
