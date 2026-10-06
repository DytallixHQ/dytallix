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

#[derive(Parser)]
#[command(
    name = "dytallix",
    about = "Dytallix CLI — early alpha",
    version,
    long_about = "Official CLI for Dytallix.\n\nsend, stake, balance and governance use the consensus chain pinned with `dytallix config pin-chain`. A node on this machine is reached over loopback HTTP, and a remote node over the post-quantum client channel with its endpoint pin file. There is no TLS.\n\nDocumentation: https://dytallix.com/docs\nDiscord: https://discord.gg/eyVvu5kmPG\nGitHub: https://github.com/DytallixHQ"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage wallets and keypairs.
    Wallet(WalletArgs),
    /// Show an account's DGT and DRT balances on the pinned chain.
    Balance(BalanceArgs),
    /// Send DGT or DRT on the pinned chain in one step.
    Send(SendArgs),
    /// Prepare, sign, and submit ordinary-v2 transactions with explicit context.
    Ordinary(OrdinaryArgs),
    /// Account recovery transactions, signed by each party offline.
    Recovery(commands::recovery::RecoveryArgs),
    /// Bond, unbond and claim rewards on the pinned chain in one step.
    Stake(StakeArgs),
    /// Prepare, sign, and submit ordinary-v3 governance transactions.
    Governance(GovernanceArgs),
    /// Cryptographic utilities.
    Crypto(CryptoArgs),
    /// Configuration management.
    Config(ConfigArgs),
    /// Serve a browser wallet on this machine and relay it to the pinned chain.
    Gateway(commands::gateway::GatewayArgs),
    /// Save node status for, dry-run and submit root controls (freeze, resume, upgrade, handover).
    Control(commands::control::ControlArgs),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    // Consensus-chain commands report errors as JSON, like their output.
    let ordinary_command = matches!(
        &cli.command,
        Commands::Ordinary(_)
            | Commands::Recovery(_)
            | Commands::Governance(_)
            | Commands::Send(_)
            | Commands::Stake(_)
            | Commands::Balance(_)
            | Commands::Control(_)
    );
    let result = match cli.command {
        Commands::Wallet(args) => commands::wallet::run(args).await,
        Commands::Balance(args) => commands::balance::run(args).await,
        Commands::Send(args) => commands::send::run(args).await,
        Commands::Ordinary(args) => commands::ordinary::run(args).await,
        Commands::Recovery(args) => commands::recovery::run(args).await,
        Commands::Stake(args) => commands::stake::run(args).await,
        Commands::Governance(args) => commands::governance::run(args).await,
        Commands::Crypto(args) => commands::crypto::run(args).await,
        Commands::Config(args) => commands::config::run(args).await,
        Commands::Gateway(args) => commands::gateway::run(args).await,
        Commands::Control(args) => commands::control::run(args).await,
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
        // The legacy testnet REST commands are gone (E04 gap 19).
        let testnet = Cli::command().get_subcommands().any(|c| {
            [
                "init", "faucet", "contract", "node", "chain", "dev", "legacy",
            ]
            .contains(&c.get_name())
        });
        assert!(!testnet);
    }
}
