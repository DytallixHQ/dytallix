//! Configuration command implementation.

use std::fs;

use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};

use crate::commands::{
    config_path, display_path, ensure_cli_dir, load_config, save_config, CliConfig, NetworkProfile,
};
use crate::output;

/// Arguments for the `config` command.
#[derive(Debug, Clone, Args)]
pub struct ConfigArgs {
    /// Configuration subcommand.
    #[command(subcommand)]
    pub command: ConfigCommand,
}

/// Configuration subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum ConfigCommand {
    /// Show the current CLI configuration.
    Show,
    /// Set a free-form configuration key.
    Set {
        /// The configuration key.
        key: String,
        /// The configuration value.
        value: String,
    },
    /// Switch the selected network profile.
    Network {
        /// The target network profile.
        network: ConfigNetwork,
    },
    /// Remove the CLI configuration file.
    Reset,
}

/// CLI network selector.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ConfigNetwork {
    /// Dytallix testnet.
    Testnet,
    /// Dytallix mainnet.
    Mainnet,
    /// Local development network.
    Local,
}

impl From<ConfigNetwork> for NetworkProfile {
    fn from(value: ConfigNetwork) -> Self {
        match value {
            ConfigNetwork::Testnet => NetworkProfile::Testnet,
            ConfigNetwork::Mainnet => NetworkProfile::Mainnet,
            ConfigNetwork::Local => NetworkProfile::Local,
        }
    }
}

/// Runs the `config` command.
pub async fn run(args: ConfigArgs) -> Result<()> {
    match args.command {
        ConfigCommand::Show => show_config(),
        ConfigCommand::Set { key, value } => set_config(key, value),
        ConfigCommand::Network { network } => set_network(network.into()),
        ConfigCommand::Reset => reset_config(),
    }
}

fn show_config() -> Result<()> {
    let config = load_config()?;
    output::section("CLI configuration");
    println!("Network: {}", config.network);
    for (key, value) in config.values {
        println!("{key}: {value}");
    }
    Ok(())
}

fn set_config(key: String, value: String) -> Result<()> {
    let mut config = load_config()?;
    config.values.insert(key.clone(), value.clone());
    save_config(&config)?;
    output::success(&format!("Config set: {key}={value}"), None);
    Ok(())
}

fn set_network(network: NetworkProfile) -> Result<()> {
    let mut config = load_config()?;
    config.network = network;
    save_config(&config)?;
    output::success(&format!("Active network set to {network}"), None);
    Ok(())
}

fn reset_config() -> Result<()> {
    ensure_cli_dir()?;
    let path = config_path();
    if path.exists() {
        fs::remove_file(&path)?;
    }
    save_config(&CliConfig::default())?;
    output::success(
        &format!("CLI configuration reset at {}", display_path(&path)),
        None,
    );
    Ok(())
}
