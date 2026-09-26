//! File-backed keystore support for Dytallix keypairs.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use dytallix_core::address::DAddr;
use dytallix_core::keypair::DytallixKeypair;

use crate::error::SdkError;
use crate::KeystoreEntry;

/// File-backed keystore for named Dytallix keypairs.
#[derive(Debug, Clone)]
pub struct Keystore {
    path: PathBuf,
    entries: Vec<KeystoreEntry>,
    active_name: Option<String>,
}

impl Keystore {
    /// Creates a new empty keystore at the provided path.
    pub fn new(path: PathBuf) -> Result<Self, SdkError> {
        ensure_parent_dir(&path)?;
        Ok(Self {
            path,
            entries: Vec::new(),
            active_name: None,
        })
    }

    /// Opens an existing keystore from disk.
    pub fn open(path: PathBuf) -> Result<Self, SdkError> {
        if !path.exists() {
            return Err(SdkError::KeystoreNotFound(path));
        }

        let contents = fs::read_to_string(&path)?;
        let file: KeystoreFile = serde_json::from_str(&contents)
            .map_err(|err| SdkError::KeystoreCorrupt(err.to_string()))?;

        Ok(Self {
            path,
            entries: file.entries,
            active_name: file.active,
        })
    }

    /// Opens an existing keystore or creates a new empty one when it does not exist.
    pub fn open_or_create(path: PathBuf) -> Result<Self, SdkError> {
        if path.exists() {
            Self::open(path)
        } else {
            Self::new(path)
        }
    }

    /// Adds or replaces a named keypair entry in the keystore.
    pub fn add_keypair(&mut self, keypair: &DytallixKeypair, name: &str) -> Result<(), SdkError> {
        keypair.scheme().require_production_operational()?;
        let address = DAddr::from_public_key(keypair.public_key())?;
        let entry = KeystoreEntry {
            name: name.to_owned(),
            address,
            public_key: keypair.public_key().to_vec(),
            private_key: keypair.private_key().to_vec(),
            scheme: keypair.scheme(),
            created_at: unix_timestamp(),
        };

        if let Some(existing) = self.entries.iter_mut().find(|item| item.name == name) {
            *existing = entry;
        } else {
            self.entries.push(entry);
        }

        if self.active_name.is_none() {
            self.active_name = Some(name.to_owned());
        }

        Ok(())
    }

    /// Reconstructs a keypair from a named keystore entry.
    pub fn get_keypair(&self, name: &str) -> Result<DytallixKeypair, SdkError> {
        let entry = self
            .entries
            .iter()
            .find(|item| item.name == name)
            .ok_or_else(|| SdkError::KeystoreCorrupt(format!("missing keypair entry: {name}")))?;
        entry.scheme.require_production_operational()?;
        let keypair =
            DytallixKeypair::from_keypair(entry.scheme, &entry.public_key, &entry.private_key)?;

        if DAddr::from_public_key(keypair.public_key())? != entry.address {
            return Err(SdkError::KeystoreCorrupt(format!(
                "address mismatch for entry: {name}"
            )));
        }

        Ok(keypair)
    }

    /// Lists all stored keystore entries.
    pub fn list(&self) -> Vec<&KeystoreEntry> {
        self.entries.iter().collect()
    }

    /// Removes a named keystore entry.
    pub fn remove(&mut self, name: &str) -> Result<(), SdkError> {
        let original_len = self.entries.len();
        self.entries.retain(|entry| entry.name != name);
        if self.entries.len() == original_len {
            return Err(SdkError::KeystoreCorrupt(format!(
                "missing keypair entry: {name}"
            )));
        }

        if self.active_name.as_deref() == Some(name) {
            self.active_name = self.entries.first().map(|entry| entry.name.clone());
        }

        Ok(())
    }

    /// Returns the active keystore entry, if one is set.
    pub fn active(&self) -> Option<&KeystoreEntry> {
        self.active_name
            .as_deref()
            .and_then(|name| self.entries.iter().find(|entry| entry.name == name))
    }

    /// Marks the named keystore entry as active.
    pub fn set_active(&mut self, name: &str) -> Result<(), SdkError> {
        if self.entries.iter().any(|entry| entry.name == name) {
            self.active_name = Some(name.to_owned());
            Ok(())
        } else {
            Err(SdkError::KeystoreCorrupt(format!(
                "missing keypair entry: {name}"
            )))
        }
    }

    /// Saves the current keystore contents to disk.
    pub fn save(&self) -> Result<(), SdkError> {
        ensure_parent_dir(&self.path)?;
        let file = KeystoreFile {
            active: self.active_name.clone(),
            entries: self.entries.clone(),
        };
        let json = serde_json::to_string_pretty(&file)
            .map_err(|err| SdkError::Serialization(err.to_string()))?;
        fs::write(&self.path, json)?;
        Ok(())
    }

    /// Returns the canonical default keystore path.
    pub fn default_path() -> PathBuf {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        home.join(".dytallix").join("keystore.json")
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct KeystoreFile {
    active: Option<String>,
    entries: Vec<KeystoreEntry>,
}

fn ensure_parent_dir(path: &Path) -> Result<(), SdkError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
