//! Command modules and shared CLI helpers.

pub mod balance;
pub mod config;
pub mod consensus;
pub mod crypto;
pub mod governance;
pub mod ordinary;
pub mod send;
pub mod stake;
pub mod wallet;

// Legacy testnet REST commands and their helpers (clients v1, decision 1).
#[cfg(feature = "legacy-network")]
pub mod chain;
#[cfg(feature = "legacy-network")]
pub mod contract;
#[cfg(feature = "legacy-network")]
pub mod dev;
#[cfg(feature = "legacy-network")]
pub mod faucet;
#[cfg(feature = "legacy-network")]
pub mod init;
#[cfg(feature = "legacy-network")]
pub mod legacy;
#[cfg(feature = "legacy-network")]
pub mod node;
#[cfg(feature = "legacy-network")]
mod rest;
#[cfg(feature = "legacy-network")]
pub(crate) use rest::*;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use dytallix_core::keypair::DytallixKeypair;
use dytallix_sdk::error::SdkError;
use dytallix_sdk::keystore::Keystore;
use dytallix_sdk::{KeystoreEntry, Token};
use serde::{Deserialize, Serialize};

/// Version of `config.json` (interfaces v1); a file without one is version 1.
pub(crate) const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CliConfig {
    #[serde(default = "first_version")]
    pub(crate) version: u32,
    pub(crate) network: NetworkProfile,
    pub(crate) values: BTreeMap<String, String>,
}
impl Default for CliConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            network: NetworkProfile::default(),
            values: BTreeMap::new(),
        }
    }
}
pub(crate) fn first_version() -> u32 {
    1
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub(crate) enum NetworkProfile {
    #[default]
    Testnet,
    Mainnet,
    Local,
}

impl std::fmt::Display for NetworkProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Testnet => f.write_str("testnet"),
            Self::Mainnet => f.write_str("mainnet"),
            Self::Local => f.write_str("local"),
        }
    }
}

pub(crate) fn ensure_cli_dir() -> Result<PathBuf> {
    let dir = home_dir().join(".dytallix");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub(crate) fn config_path() -> PathBuf {
    home_dir().join(".dytallix").join("config.json")
}

pub(crate) fn load_config() -> Result<CliConfig> {
    let path = config_path();
    if !path.exists() {
        return Ok(CliConfig::default());
    }

    let contents = fs::read_to_string(&path)?;
    let config: CliConfig = serde_json::from_str(&contents)
        .map_err(|err| anyhow!("Invalid CLI config at {}: {err}", display_path(&path)))?;
    if config.version != CONFIG_VERSION {
        return Err(anyhow!(
            "Unsupported CLI config version {} at {}",
            config.version,
            display_path(&path)
        ));
    }
    Ok(config)
}

pub(crate) fn save_config(config: &CliConfig) -> Result<()> {
    let path = config_path();
    ensure_cli_dir()?;
    let json = serde_json::to_string_pretty(config)?;
    fs::write(path, json)?;
    Ok(())
}

pub(crate) fn load_keystore() -> Result<Keystore> {
    Keystore::open(Keystore::default_path()).map_err(map_keystore_error)
}

pub(crate) fn load_or_create_keystore() -> Result<Keystore> {
    Keystore::open_or_create(Keystore::default_path()).map_err(map_keystore_error)
}

pub(crate) fn active_entry(keystore: &Keystore) -> Result<&KeystoreEntry> {
    keystore.active().ok_or_else(|| {
		anyhow!(
			"No active wallet. Run `dytallix init` to create one, or `dytallix wallet switch NAME` to activate an existing wallet."
		)
	})
}

pub(crate) fn active_keypair(keystore: &Keystore) -> Result<DytallixKeypair> {
    let entry = active_entry(keystore)?;
    keystore
        .get_keypair(&entry.name)
        .map_err(humanize_sdk_error)
}

pub(crate) fn format_number(value: u128) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    let chars = digits.chars().rev().collect::<Vec<char>>();
    for (index, ch) in chars.iter().enumerate() {
        if index > 0 && index % 3 == 0 {
            out.push(',');
        }
        out.push(*ch);
    }
    out.chars().rev().collect()
}

/// Formats a micro-denominated token amount using up to 6 decimal places.
#[cfg(feature = "legacy-network")]
pub(crate) fn format_micro_amount(value: u128) -> String {
    let whole = value / 1_000_000;
    let fractional = value % 1_000_000;

    if fractional == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{fractional:06}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    }
}

pub(crate) fn display_path(path: &Path) -> String {
    let home = home_dir();
    if let Ok(stripped) = path.strip_prefix(&home) {
        if stripped.as_os_str().is_empty() {
            "~".to_owned()
        } else {
            format!("~/{}", stripped.display())
        }
    } else {
        path.display().to_string()
    }
}

pub(crate) fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("Failed to read {}", display_path(path)))
}

pub(crate) use crate::bytes::bytes_to_hex;

pub(crate) fn hex_to_bytes(raw: &str) -> Result<Vec<u8>> {
    let trimmed = raw.trim();
    if !trimmed.len().is_multiple_of(2) {
        return Err(anyhow!(
            "Invalid hex input. Provide an even number of characters."
        ));
    }

    let mut bytes = Vec::with_capacity(trimmed.len() / 2);
    let chars = trimmed.as_bytes();
    let mut index = 0usize;
    while index < chars.len() {
        let high = decode_hex_nibble(chars[index] as char)?;
        let low = decode_hex_nibble(chars[index + 1] as char)?;
        bytes.push((high << 4) | low);
        index += 2;
    }
    Ok(bytes)
}

pub(crate) fn humanize_sdk_error(error: SdkError) -> anyhow::Error {
    match error {
		SdkError::Core(_) => anyhow!("Invalid address: Bech32m checksum failed — check for typos."),
		SdkError::InsufficientBalance {
			token: Token::DRT,
			required,
			available,
		} => anyhow!(
			"Insufficient DRT balance. Required: {} DRT. Available: {} DRT.",
			format_number(required),
			format_number(available)
		),
		SdkError::InsufficientBalance {
			token: Token::DGT,
			required,
			available,
		} => anyhow!(
			"Insufficient DGT for gas fees. Required: {} DGT. Available: {} DGT. Run dytallix faucet to get more.",
			format_number(required),
			format_number(available)
		),
			SdkError::FaucetRateLimited {
				retry_after_seconds,
            } => anyhow!(
                "Faucet cooldown active. Try again in {retry_after_seconds} seconds. The keystore and wallet are still valid if initialization already created them."
            ),
			SdkError::FaucetUnavailable { endpoint, reason }
            if endpoint.contains("dytallix.com/api/faucet")
                && looks_like_gateway_html(&reason)
                && (reason.contains("502") || reason.contains("503")) => anyhow!(
                "The public faucet gateway is currently unavailable at {endpoint}. The chain RPC may still be healthy. Retry later, or switch to a local node with `dytallix config set endpoint http://localhost:3030` and run `dytallix config network local`."
            ),
			SdkError::FaucetUnavailable { endpoint, .. } => anyhow!(
				"Faucet is not reachable at {endpoint}. Check your network connection or try again later."
			),
		SdkError::NodeUnavailable { endpoint, reason }
            if transaction_api_unavailable(&endpoint, &reason) => anyhow!(
            "The Dytallix testnet transaction API is not available at {endpoint}. Faucet and balance reads may still work, but transaction simulation and submission are not exposed from this endpoint yet."
        ),
		SdkError::NodeUnavailable { endpoint, .. } => anyhow!(
			"Cannot reach the Dytallix testnet at {endpoint}. Check your network connection."
		),
		SdkError::KeystoreNotFound(_) => anyhow!(keystore_not_found_message()),
		SdkError::Network(message) => anyhow!("Network error: {message}"),
		SdkError::Io(err) => anyhow!("I/O error: {err}"),
		SdkError::Serialization(message) => anyhow!("Serialization error: {message}"),
		SdkError::TransactionRejected(message) if looks_like_gateway_html(&message) => anyhow!(
            "The Dytallix testnet transaction API returned a gateway or HTML response instead of transaction JSON. Transaction submission is not usable from the current endpoint."
        ),
		SdkError::TransactionRejected(message) => anyhow!("Transaction rejected: {message}"),
		SdkError::ContractDeployFailed(message) => anyhow!("Contract deployment failed: {message}"),
		SdkError::KeystoreCorrupt(message) => anyhow!("Keystore corrupt: {message}"),
		SdkError::NetworkMismatch(message) => anyhow!("Network mismatch: {message}"),
		SdkError::InsufficientGas { required, provided } => anyhow!(
			"Insufficient gas: required {required} units but only {provided} were provided. Increase the gas limit and try again."
		),
	}
}

fn transaction_api_unavailable(endpoint: &str, reason: &str) -> bool {
    let lower_reason = reason.to_ascii_lowercase();
    (endpoint.contains("/transactions")
        || endpoint.contains("/simulate")
        || endpoint.contains("/api/blockchain/submit"))
        && (lower_reason.contains("405 not allowed")
            || lower_reason.contains("404 not found")
            || lower_reason.contains("cannot post")
            || lower_reason.contains("<html")
            || lower_reason.contains("<!doctype html"))
}

fn looks_like_gateway_html(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("<html") || lower.contains("<!doctype html")
}

pub(crate) fn map_keystore_error(error: SdkError) -> anyhow::Error {
    match error {
        SdkError::KeystoreNotFound(_) => anyhow!(keystore_not_found_message()),
        other => humanize_sdk_error(other),
    }
}

pub(crate) fn keystore_not_found_message() -> &'static str {
    "No keystore found at ~/.dytallix/keystore.json. Run dytallix init to create one."
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn decode_hex_nibble(ch: char) -> Result<u8> {
    match ch {
        '0'..='9' => Ok((ch as u8) - b'0'),
        'a'..='f' => Ok((ch as u8) - b'a' + 10),
        'A'..='F' => Ok((ch as u8) - b'A' + 10),
        _ => Err(anyhow!("Invalid hex input. `{ch}` is not a hex character.")),
    }
}
