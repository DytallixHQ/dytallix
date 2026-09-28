mod bytes;
mod commands;
mod output;

use clap::{Parser, Subcommand};

use commands::balance::BalanceArgs;
use commands::config::ConfigArgs;
use commands::crypto::CryptoArgs;
use commands::governance::GovernanceArgs;
use commands::ordinary::OrdinaryArgs;
use commands::send::SendArgs;
use commands::stake::StakeArgs;
use commands::wallet::WalletArgs;
#[cfg(feature = "legacy-network")]
use commands::{
    chain::ChainArgs, contract::ContractArgs, dev::DevArgs, faucet::FaucetArgs, legacy::LegacyArgs,
    node::NodeArgs,
};

#[derive(Parser)]
#[command(
    name = "dytallix",
    about = "Dytallix CLI — early alpha",
    version,
    long_about = "Official CLI for Dytallix.\n\nsend, stake, balance and governance use the consensus chain pinned with `dytallix config pin-chain`. Builds with the legacy-network feature add the public testnet commands: init, faucet, contract, chain, node, dev and legacy.\n\nDocumentation: https://dytallix.com/docs\nDiscord: https://discord.gg/eyVvu5kmPG\nExplorer: https://dytallix.com/build/blockchain\nGitHub: https://github.com/DytallixHQ"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a funded testnet wallet and hit the three developer milestones.
    #[cfg(feature = "legacy-network")]
    Init,
    /// Manage wallets and keypairs.
    Wallet(WalletArgs),
    /// Show an account's DGT and DRT balances on the pinned chain.
    Balance(BalanceArgs),
    /// Send DGT or DRT on the pinned chain in one step.
    Send(SendArgs),
    /// Prepare, sign, and submit ordinary-v2 transactions with explicit context.
    Ordinary(OrdinaryArgs),
    /// Request testnet tokens from the faucet.
    #[cfg(feature = "legacy-network")]
    Faucet(FaucetArgs),
    /// Bond, unbond and claim rewards on the pinned chain in one step.
    Stake(StakeArgs),
    /// Prepare, sign, and submit ordinary-v3 governance transactions.
    Governance(GovernanceArgs),
    /// Deploy and interact with smart contracts on the testnet.
    #[cfg(feature = "legacy-network")]
    Contract(ContractArgs),
    /// Local testnet node operations.
    #[cfg(feature = "legacy-network")]
    Node(NodeArgs),
    /// Query testnet chain state.
    #[cfg(feature = "legacy-network")]
    Chain(ChainArgs),
    /// Cryptographic utilities.
    Crypto(CryptoArgs),
    /// Testnet developer tools and utilities.
    #[cfg(feature = "legacy-network")]
    Dev(DevArgs),
    /// Configuration management.
    Config(ConfigArgs),
    /// Legacy testnet REST commands. The consensus chain refuses them.
    #[cfg(feature = "legacy-network")]
    Legacy(LegacyArgs),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    // Consensus-chain commands report errors as JSON, like their output.
    let ordinary_command = matches!(
        &cli.command,
        Commands::Ordinary(_)
            | Commands::Governance(_)
            | Commands::Send(_)
            | Commands::Stake(_)
            | Commands::Balance(_)
    );
    let result = match cli.command {
        #[cfg(feature = "legacy-network")]
        Commands::Init => commands::init::run().await,
        Commands::Wallet(args) => commands::wallet::run(args).await,
        Commands::Balance(args) => commands::balance::run(args).await,
        Commands::Send(args) => commands::send::run(args).await,
        Commands::Ordinary(args) => commands::ordinary::run(args).await,
        #[cfg(feature = "legacy-network")]
        Commands::Faucet(args) => commands::faucet::run(args).await,
        Commands::Stake(args) => commands::stake::run(args).await,
        Commands::Governance(args) => commands::governance::run(args).await,
        #[cfg(feature = "legacy-network")]
        Commands::Contract(args) => commands::contract::run(args).await,
        #[cfg(feature = "legacy-network")]
        Commands::Node(args) => commands::node::run(args).await,
        #[cfg(feature = "legacy-network")]
        Commands::Chain(args) => commands::chain::run(args).await,
        Commands::Crypto(args) => commands::crypto::run(args).await,
        #[cfg(feature = "legacy-network")]
        Commands::Dev(args) => commands::dev::run(args).await,
        Commands::Config(args) => commands::config::run(args).await,
        #[cfg(feature = "legacy-network")]
        Commands::Legacy(args) => commands::legacy::run(args).await,
    };

    if let Err(err) = result {
        if ordinary_command {
            eprintln!(
                "{}",
                serde_json::json!({"status":"error", "message":err.to_string(),
                    "output_version": commands::ordinary::OUTPUT_VERSION})
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

        for command in [
            "wallet",
            "send",
            "stake",
            "balance",
            "governance",
            "ordinary",
            "config",
        ] {
            assert!(help.contains(command), "{command}");
        }
        assert!(help.contains("discord.gg/eyVvu5kmPG"));
        assert!(help.contains("github.com/DytallixHQ"));
        // Testnet REST commands exist only in legacy-network builds.
        let testnet = Cli::command()
            .get_subcommands()
            .any(|c| ["init", "faucet", "contract", "legacy"].contains(&c.get_name()));
        assert_eq!(testnet, cfg!(feature = "legacy-network"));
    }
}
