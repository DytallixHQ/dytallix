//! Wallet command implementation.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use clap::{Args, Subcommand};
use dytallix_core::hash::hash_public_key;
use dytallix_core::keypair::{DytallixKeypair, KeyScheme};

use crate::commands::{
    active_entry, active_keypair, bytes_to_hex, display_path, format_number, hex_to_bytes,
    humanize_sdk_error, load_keystore, load_or_create_keystore, read_bytes,
};
use crate::output;

/// Arguments for the `wallet` command.
#[derive(Debug, Clone, Args)]
pub struct WalletArgs {
    /// Wallet subcommand.
    #[command(subcommand)]
    pub command: WalletCommand,
}

/// Wallet subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum WalletCommand {
    /// Generate a keypair without auto-funding.
    Create {
        /// Optional wallet name.
        #[arg(long)]
        name: Option<String>,
    },
    /// Import a keypair from a file.
    Import {
        /// Path to a raw or hex-encoded private key file.
        #[arg(long = "key-file")]
        key_file: PathBuf,
        /// Optional wallet name.
        #[arg(long)]
        name: Option<String>,
    },
    /// Export the active keypair.
    Export {
        /// Destination path for the exported private key.
        #[arg(long)]
        output: PathBuf,
    },
    /// List known wallets.
    List,
    /// Set the active wallet.
    Switch {
        /// The wallet name to activate.
        name: String,
    },
    /// Rotate the active ML-DSA-65 keypair.
    Rotate,
    /// Show active wallet details.
    Info,
}

/// Runs the `wallet` command.
pub async fn run(args: WalletArgs) -> Result<()> {
    match args.command {
        WalletCommand::Create { name } => create_wallet(name),
        WalletCommand::Import { key_file, name } => import_wallet(key_file, name),
        WalletCommand::Export { output } => export_wallet(output),
        WalletCommand::List => list_wallets(),
        WalletCommand::Switch { name } => switch_wallet(&name),
        WalletCommand::Rotate => rotate_wallet(),
        WalletCommand::Info => wallet_info(),
    }
}

fn create_wallet(name: Option<String>) -> Result<()> {
    let keypair = DytallixKeypair::generate();
    let mut keystore = load_or_create_keystore()?;
    let name = name.unwrap_or_else(default_wallet_name);

    keystore
        .add_keypair(&keypair, &name)
        .map_err(humanize_sdk_error)?;
    keystore.set_active(&name).map_err(humanize_sdk_error)?;
    keystore.save().map_err(humanize_sdk_error)?;

    output::success(&format!("Wallet created: {name}"), None);
    Ok(())
}

fn import_wallet(key_file: PathBuf, name: Option<String>) -> Result<()> {
    let raw = read_bytes(&key_file)?;
    let bytes = match String::from_utf8(raw.clone()) {
        Ok(text)
            if text
                .trim()
                .chars()
                .all(|ch| ch.is_ascii_hexdigit() || ch.is_whitespace()) =>
        {
            hex_to_bytes(text.trim())?
        }
        _ => raw,
    };
    let keypair = DytallixKeypair::from_private_key(&bytes).map_err(|err| {
        anyhow!(
            "Failed to import keypair from {}: {err}",
            display_path(&key_file)
        )
    })?;

    if keypair.scheme() != KeyScheme::MlDsa65 {
        return Err(anyhow!(
			"Only ML-DSA-65 keypairs can be stored in the wallet keystore because D-Addr derivation is ML-DSA-65 only."
		));
    }

    let mut keystore = load_or_create_keystore()?;
    let name = name.unwrap_or_else(|| {
        key_file
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("imported-wallet")
            .to_owned()
    });
    keystore
        .add_keypair(&keypair, &name)
        .map_err(humanize_sdk_error)?;
    keystore.set_active(&name).map_err(humanize_sdk_error)?;
    keystore.save().map_err(humanize_sdk_error)?;

    output::success(&format!("Wallet imported: {name}"), None);
    Ok(())
}

fn export_wallet(output_path: PathBuf) -> Result<()> {
    let keystore = load_keystore()?;
    let keypair = active_keypair(&keystore)?;
    let hex = bytes_to_hex(keypair.private_key());
    fs::write(&output_path, hex)?;
    output::success(
        &format!("Active wallet exported to {}", display_path(&output_path)),
        None,
    );
    Ok(())
}

fn list_wallets() -> Result<()> {
    let keystore = load_keystore()?;
    let active_name = keystore.active().map(|entry| entry.name.clone());
    output::section("Wallets");
    for entry in keystore.list() {
        let marker = if active_name.as_deref() == Some(entry.name.as_str()) {
            "*"
        } else {
            " "
        };
        println!(
            "{marker} {}  {}  {:?}",
            entry.name, entry.address, entry.scheme
        );
    }
    Ok(())
}

fn switch_wallet(name: &str) -> Result<()> {
    let mut keystore = load_keystore()?;
    keystore.set_active(name).map_err(humanize_sdk_error)?;
    keystore.save().map_err(humanize_sdk_error)?;
    output::success(&format!("Active wallet set to {name}"), None);
    Ok(())
}

fn rotate_wallet() -> Result<()> {
    let keystore = load_keystore()?;
    let active = active_entry(&keystore)?;

    Err(anyhow!(
        "Cannot rotate wallet `{}` while preserving {} because the D-Addr is derived from the ML-DSA-65 public key. Use `dytallix wallet create` to make a new wallet or `dytallix wallet export` before replacing this key.",
        active.name,
        active.address
    ))
}

fn wallet_info() -> Result<()> {
    let keystore = load_keystore()?;
    let entry = active_entry(&keystore)?;
    let keypair = active_keypair(&keystore)?;
    let pubkey_hash = hash_public_key(keypair.public_key());

    output::section("Active wallet");
    println!("Name:           {}", entry.name);
    println!("D-Addr:         {}", entry.address);
    println!("Pubkey hash:    {}", bytes_to_hex(&pubkey_hash));
    println!("Scheme:         {:?}", entry.scheme);
    println!(
        "Public key:     {} bytes",
        format_number(keypair.public_key().len() as u128)
    );
    println!(
        "Private key:    {} bytes",
        format_number(keypair.private_key().len() as u128)
    );
    Ok(())
}

fn default_wallet_name() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format!("wallet-{timestamp}")
}
