//! Developer utility command implementation.

use std::time::Instant;

use anyhow::Result;
use clap::{Args, Subcommand};

use dytallix_core::keypair::DytallixKeypair;
use dytallix_core::signature::verify_mldsa65;

use crate::commands::{
    active_entry, configured_client, configured_faucet, hex_to_bytes, load_keystore, open_url,
    validate_address,
};
use crate::output;
use dytallix_sdk::transaction::TransactionBuilder;
use dytallix_sdk::Token;

/// Arguments for the `dev` command.
#[derive(Debug, Clone, Args)]
pub struct DevArgs {
    /// Developer subcommand.
    #[command(subcommand)]
    pub command: DevCommand,
}

/// Developer utility subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum DevCommand {
    /// Show the configured faucet endpoint.
    FaucetServer,
    /// Open the explorer in a browser.
    Explorer,
    /// Open the documentation in a browser.
    Docs,
    /// Open the Discord server in a browser.
    Discord,
    /// Open the GitHub organization in a browser.
    Github,
    /// Decode a hex string.
    Decode { hex: String },
    /// Hex-encode a string.
    Encode { text: String },
    /// Simulate a transfer transaction.
    SimulateTx { address: String, amount: u128 },
    /// Benchmark ML-DSA-65 sign and verify throughput.
    Benchmark,
}

/// Runs the `dev` command.
pub async fn run(args: DevArgs) -> Result<()> {
    match args.command {
        DevCommand::FaucetServer => faucet_server(),
        DevCommand::Explorer => open_url("https://dytallix.com/build/blockchain"),
        DevCommand::Docs => open_url("https://dytallix.com/docs"),
        DevCommand::Discord => open_url("https://discord.gg/eyVvu5kmPG"),
        DevCommand::Github => open_url("https://github.com/DytallixHQ"),
        DevCommand::Decode { hex } => decode(hex),
        DevCommand::Encode { text } => encode(text),
        DevCommand::SimulateTx { address, amount } => simulate_tx(address, amount).await,
        DevCommand::Benchmark => benchmark(),
    }
}

fn faucet_server() -> Result<()> {
    let faucet = configured_faucet()?;
    output::section("Faucet server");
    println!("Configured faucet client: {faucet:?}");
    Ok(())
}

fn decode(hex: String) -> Result<()> {
    let decoded = hex_to_bytes(&hex)?;
    output::section("Decoded bytes");
    println!("{}", String::from_utf8_lossy(&decoded));
    Ok(())
}

fn encode(text: String) -> Result<()> {
    output::section("Encoded text");
    println!("{}", crate::commands::bytes_to_hex(text.as_bytes()));
    Ok(())
}

async fn simulate_tx(address: String, amount: u128) -> Result<()> {
    let destination = validate_address(&address)?;
    let keystore = load_keystore()?;
    let sender = active_entry(&keystore)?.address.clone();
    let client = configured_client().await?;
    let account = client.get_account(&sender).await?;
    let tx = TransactionBuilder::new()
        .from(sender)
        .to(destination)
        .amount(amount, Token::DRT)
        .nonce(account.nonce)
        .build()?;
    let (_, fee) = tx.with_estimated_fee(&client).await?;
    output::fee_breakdown(&fee);
    Ok(())
}

fn benchmark() -> Result<()> {
    let keypair = DytallixKeypair::generate();
    let message = b"benchmark-dytallix-cli";
    let iterations = 100u32;

    let sign_start = Instant::now();
    let mut signatures = Vec::with_capacity(iterations as usize);
    for _ in 0..iterations {
        signatures.push(keypair.sign(message)?);
    }
    let sign_elapsed = sign_start.elapsed();

    let verify_start = Instant::now();
    for signature in &signatures {
        let valid = verify_mldsa65(keypair.public_key(), message, signature)?;
        if !valid {
            return Err(anyhow::anyhow!(
                "Benchmark verification failed unexpectedly."
            ));
        }
    }
    let verify_elapsed = verify_start.elapsed();

    output::section("Benchmark");
    println!(
        "Sign throughput:   {:.2} ops/s",
        f64::from(iterations) / sign_elapsed.as_secs_f64()
    );
    println!(
        "Verify throughput: {:.2} ops/s",
        f64::from(iterations) / verify_elapsed.as_secs_f64()
    );
    Ok(())
}
