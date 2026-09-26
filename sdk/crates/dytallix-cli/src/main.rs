mod bytes;
mod commands;
mod output;

use clap::{Parser, Subcommand};

use commands::balance::BalanceArgs;
use commands::chain::ChainArgs;
use commands::config::ConfigArgs;
use commands::contract::ContractArgs;
use commands::crypto::CryptoArgs;
use commands::dev::DevArgs;
use commands::faucet::FaucetArgs;
use commands::governance::GovernanceArgs;
use commands::node::NodeArgs;
use commands::ordinary::OrdinaryArgs;
use commands::send::SendArgs;
use commands::stake::StakeArgs;
use commands::wallet::WalletArgs;

#[derive(Parser)]
#[command(
    name = "dytallix",
    about = "Dytallix public testnet CLI — early alpha",
    version,
    long_about = "Official CLI for the Dytallix public testnet.\n\nKeypair, faucet, transfer, and basic contract lifecycle are available for experimentation on the public testnet. Staking, governance, and some advanced or operator paths are not yet production-complete.\n\nDocumentation: https://dytallix.com/docs\nDiscord: https://discord.gg/eyVvu5kmPG\nExplorer: https://dytallix.com/build/blockchain\nGitHub: https://github.com/DytallixHQ"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a funded wallet and hit the three developer milestones.
    Init,
    /// Manage wallets and keypairs.
    Wallet(WalletArgs),
    /// Show DGT and DRT balances.
    Balance(BalanceArgs),
    /// Send DGT or DRT to an address.
    Send(SendArgs),
    /// Prepare, sign, and submit ordinary-v2 transactions with explicit context.
    Ordinary(OrdinaryArgs),
    /// Request testnet tokens from the faucet.
    Faucet(FaucetArgs),
    /// Staking status and direct-node staking writes.
    Stake(StakeArgs),
    /// Governance reads and direct-node governance writes.
    Governance(GovernanceArgs),
    /// Deploy and interact with smart contracts.
    Contract(ContractArgs),
    /// Local node operations.
    Node(NodeArgs),
    /// Query chain state.
    Chain(ChainArgs),
    /// Cryptographic utilities.
    Crypto(CryptoArgs),
    /// Developer tools and utilities.
    Dev(DevArgs),
    /// Configuration management.
    Config(ConfigArgs),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let ordinary_command = matches!(&cli.command, Commands::Ordinary(_));
    let result = match cli.command {
        Commands::Init => commands::init::run().await,
        Commands::Wallet(args) => commands::wallet::run(args).await,
        Commands::Balance(args) => commands::balance::run(args).await,
        Commands::Send(args) => commands::send::run(args).await,
        Commands::Ordinary(args) => commands::ordinary::run(args).await,
        Commands::Faucet(args) => commands::faucet::run(args).await,
        Commands::Stake(args) => commands::stake::run(args).await,
        Commands::Governance(args) => commands::governance::run(args).await,
        Commands::Contract(args) => commands::contract::run(args).await,
        Commands::Node(args) => commands::node::run(args).await,
        Commands::Chain(args) => commands::chain::run(args).await,
        Commands::Crypto(args) => commands::crypto::run(args).await,
        Commands::Dev(args) => commands::dev::run(args).await,
        Commands::Config(args) => commands::config::run(args).await,
    };

    if let Err(err) = result {
        if ordinary_command {
            eprintln!(
                "{}",
                serde_json::json!({"status":"error", "message":err.to_string()})
            );
        } else {
            output::error(&err.to_string());
        }
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_structure_help() {
        let mut command = Cli::command();
        let mut buffer = Vec::new();
        command.write_long_help(&mut buffer).unwrap();
        let help = String::from_utf8(buffer).unwrap();

        assert!(help.contains("init"));
        assert!(help.contains("wallet"));
        assert!(help.contains("send"));
        assert!(help.contains("contract"));
        assert!(help.contains("discord.gg/eyVvu5kmPG"));
        assert!(help.contains("github.com/DytallixHQ"));
    }
}
