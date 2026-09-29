//! Local qualification only. Shares the ordinary command implementation.
mod bytes;
use bytes::bytes_to_hex;
#[path = "commands/ordinary.rs"]
mod ordinary;
// Shared with the main CLI; this binary reads existing passphrases only.
#[allow(dead_code)]
#[path = "commands/passphrase.rs"]
mod passphrase;

use dytallix_core::keypair::DytallixKeypair;
use dytallix_sdk::keystore::Keystore;

/// The keystore, for `--wallet` (E04 gap 16).
fn load_keystore() -> anyhow::Result<Keystore> {
    Keystore::open(Keystore::default_path()).map_err(|e| anyhow::anyhow!("{e}"))
}

/// A version 2 keystore's key, after its passphrase. A version 1 keystore
/// holds plaintext keys and must be migrated with the main CLI first.
fn keypair_named(keystore: &Keystore, name: &str) -> anyhow::Result<DytallixKeypair> {
    anyhow::ensure!(
        keystore.version() != 1,
        "This keystore holds plaintext private keys (version 1). Run `dytallix wallet migrate` to encrypt it before using its keys."
    );
    keystore
        .open_keypair(name, &passphrase::existing()?)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "dytallix-ordinary-local",
    version,
    about = "Ordinary-v2 local qualification CLI. HTTP literal loopback only. No production qualification."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Prepare, sign, inspect, and submit ordinary-v2 transactions.
    Ordinary(ordinary::OrdinaryArgs),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let Cli {
        command: Commands::Ordinary(args),
    } = Cli::parse();
    if let Err(err) = ordinary::run(args).await {
        eprintln!(
            "{}",
            serde_json::json!({"status":"error", "message":err.to_string(),
                "output_version": ordinary::OUTPUT_VERSION})
        );
        std::process::exit(1);
    }
    Ok(())
}
