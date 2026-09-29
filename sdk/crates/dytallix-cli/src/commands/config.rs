//! Configuration command implementation: the pinned consensus chain.

use anyhow::Result;
use clap::{Args, Subcommand};

use crate::commands::consensus::{ChainConfig, Network};
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
    /// Show the pinned chain and its endpoint.
    Show,
    /// Pin the consensus chain and its node for send, stake, balance and
    /// governance. Take the chain ID and genesis digest from a source you
    /// trust, never from the node itself.
    PinChain {
        /// A node you trust: a loopback http://IP:PORT, or a remote
        /// endpoint's pin file (client channel v1). There is no TLS.
        #[arg(long)]
        endpoint: String,
        #[arg(long, value_enum)]
        network: Network,
        #[arg(long)]
        chain_id: String,
        /// Lowercase hexadecimal SHA-256 of the exact genesis file.
        #[arg(long)]
        genesis_digest: String,
        /// Store the pin without asking the node which chain it reports.
        #[arg(long)]
        no_check: bool,
    },
}

/// Runs the `config` command.
pub async fn run(args: ConfigArgs) -> Result<()> {
    match args.command {
        ConfigCommand::Show => show_config(),
        ConfigCommand::PinChain {
            endpoint,
            network,
            chain_id,
            genesis_digest,
            no_check,
        } => {
            pin_chain(
                ChainConfig::new(&endpoint, network, chain_id, genesis_digest)?,
                no_check,
            )
            .await
        }
    }
}

async fn pin_chain(config: ChainConfig, no_check: bool) -> Result<()> {
    let pin = config.pin()?;
    if !no_check {
        let reported = config.client(None)?.query_profile().await?.context;
        pin.check(&reported).map_err(|_| {
            anyhow::anyhow!(
                "the node reports chain {} with genesis digest {}; nothing was pinned",
                reported.chain_id,
                crate::bytes::bytes_to_hex(&reported.genesis_digest)
            )
        })?;
    }
    config.save()?;
    output::success(
        &format!(
            "Pinned chain {} ({:?}) at {}",
            config.chain_id,
            config.network,
            config.endpoint()?.describe()
        ),
        None,
    );
    Ok(())
}

fn show_config() -> Result<()> {
    output::section("CLI configuration");
    match ChainConfig::load() {
        Ok(chain) => {
            println!("Pinned chain: {} ({:?})", chain.chain_id, chain.network);
            println!("Genesis digest: {}", chain.genesis_digest);
            match chain.endpoint() {
                Ok(endpoint) => println!("Chain endpoint: {}", endpoint.describe()),
                Err(error) => println!("Chain endpoint: {} ({error})", chain.endpoint),
            }
        }
        Err(_) => println!("Pinned chain: none"),
    }
    Ok(())
}
