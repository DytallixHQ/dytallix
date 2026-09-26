//! Local qualification only. Shares the ordinary command implementation.
mod bytes;
use bytes::bytes_to_hex;
#[path = "commands/ordinary.rs"]
mod ordinary;

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
            serde_json::json!({"status":"error", "message":err.to_string()})
        );
        std::process::exit(1);
    }
    Ok(())
}
