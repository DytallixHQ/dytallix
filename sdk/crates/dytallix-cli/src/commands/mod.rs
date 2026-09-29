//! Command modules and shared CLI helpers.

pub mod balance;
pub mod config;
pub mod consensus;
pub mod crypto;
pub mod governance;
pub mod ordinary;
pub(crate) mod passphrase;
pub mod recovery;
pub mod send;
pub mod stake;
pub mod wallet;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use dytallix_core::keypair::DytallixKeypair;
use dytallix_sdk::error::SdkError;
use dytallix_sdk::keystore::Keystore;
use dytallix_sdk::KeystoreEntry;

/// A file without a `version` field is version 1 (interfaces v1).
pub(crate) fn first_version() -> u32 {
    1
}

pub(crate) fn ensure_cli_dir() -> Result<PathBuf> {
    let dir = home_dir().join(".dytallix");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// `~/.dytallix`, which holds `chain.json` and `keystore.json`.
pub(crate) fn cli_dir() -> PathBuf {
    home_dir().join(".dytallix")
}

pub(crate) fn load_keystore() -> Result<Keystore> {
    Keystore::open(Keystore::default_path()).map_err(map_keystore_error)
}

/// The keystore for adding keys: an existing one unlocked, or a new
/// encrypted one under a new passphrase (E04 gap 16).
pub(crate) fn load_or_create_keystore() -> Result<Keystore> {
    let path = Keystore::default_path();
    if path.exists() {
        let mut keystore = load_keystore()?;
        unlock_keystore(&mut keystore)?;
        Ok(keystore)
    } else {
        let passphrase = passphrase::new()?;
        Keystore::create(path, &passphrase).map_err(map_keystore_error)
    }
}

/// Unlock an encrypted keystore with its passphrase. A version 1 keystore
/// holds plaintext keys and must be migrated first.
pub(crate) fn unlock_keystore(keystore: &mut Keystore) -> Result<()> {
    if keystore.version() == 1 {
        return Err(anyhow!(MIGRATE_MESSAGE));
    }
    if !keystore.is_unlocked() {
        keystore
            .unlock(&passphrase::existing()?)
            .map_err(humanize_sdk_error)?;
    }
    Ok(())
}

/// A named keypair, asking for the passphrase when the keystore is locked.
pub(crate) fn keypair_named(keystore: &Keystore, name: &str) -> Result<DytallixKeypair> {
    if keystore.version() == 1 {
        return Err(anyhow!(MIGRATE_MESSAGE));
    }
    if keystore.is_unlocked() {
        keystore.get_keypair(name)
    } else {
        keystore.open_keypair(name, &passphrase::existing()?)
    }
    .map_err(humanize_sdk_error)
}

pub(crate) const MIGRATE_MESSAGE: &str = "This keystore holds plaintext private keys (version 1). Run `dytallix wallet migrate` to encrypt it before using its keys.";

pub(crate) fn active_entry(keystore: &Keystore) -> Result<&KeystoreEntry> {
    keystore.active().ok_or_else(|| {
		anyhow!(
			"No active wallet. Run `dytallix wallet create` to create one, or `dytallix wallet switch NAME` to activate an existing wallet."
		)
	})
}

pub(crate) fn active_keypair(keystore: &Keystore) -> Result<DytallixKeypair> {
    let entry = active_entry(keystore)?;
    keypair_named(keystore, &entry.name)
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
            token,
            required,
            available,
        } => anyhow!(
            "Insufficient {token} balance. Required: {} {token}. Available: {} {token}.",
            format_number(required),
            format_number(available)
        ),
        SdkError::KeystoreNotFound(_) => anyhow!(keystore_not_found_message()),
        SdkError::Network(message) => anyhow!("Network error: {message}"),
        SdkError::Io(err) => anyhow!("I/O error: {err}"),
        SdkError::Serialization(message) => anyhow!("Serialization error: {message}"),
        SdkError::TransactionRejected(message) => anyhow!("Transaction rejected: {message}"),
        SdkError::KeystoreCorrupt(message) => anyhow!("Keystore corrupt: {message}"),
        SdkError::KeystoreLocked => anyhow!("The keystore is locked; its passphrase is required."),
        SdkError::KeystorePlaintext => anyhow!(MIGRATE_MESSAGE),
        SdkError::KeystorePassphrase => anyhow!("Wrong keystore passphrase."),
        SdkError::NetworkMismatch(message) => anyhow!("Network mismatch: {message}"),
        SdkError::InsufficientGas { required, provided } => anyhow!(
            "Insufficient gas: required {required} units but only {provided} were provided. Increase the gas limit and try again."
        ),
    }
}

pub(crate) fn map_keystore_error(error: SdkError) -> anyhow::Error {
    match error {
        SdkError::KeystoreNotFound(_) => anyhow!(keystore_not_found_message()),
        other => humanize_sdk_error(other),
    }
}

pub(crate) fn keystore_not_found_message() -> &'static str {
    "No keystore found at ~/.dytallix/keystore.json. Run dytallix wallet create to create one."
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
