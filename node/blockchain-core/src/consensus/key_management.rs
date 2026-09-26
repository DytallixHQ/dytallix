//! Key Management Module
//!
//! This module handles Post-Quantum Cryptography (PQC) key generation, storage,
//! and management for the consensus engine.

use anyhow::{anyhow, Result};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::crypto::PQCManager;

/// PQC Key Information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PQCKeyInfo {
    pub algorithm: String,
    pub public_key: Vec<u8>,
    pub private_key: Vec<u8>,
    pub key_id: String,
    pub created_at: u64,
    pub expires_at: Option<u64>,
}

/// Node Key Store for multiple PQC algorithms
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeKeyStore {
    pub dilithium: PQCKeyInfo,
    pub falcon: PQCKeyInfo,
    pub sphincs: PQCKeyInfo,
    pub node_id: String,
    pub created_at: u64,
    pub version: String,
}

/// Key Management System
#[derive(Debug)]
pub struct KeyManager {
    key_store: Option<NodeKeyStore>,
    key_file_path: String,
    _pqc_manager: std::sync::Arc<PQCManager>, // underscore
}

impl KeyManager {
    /// Create new key manager
    pub fn new(key_file_path: String, pqc_manager: std::sync::Arc<PQCManager>) -> Self {
        Self {
            key_store: None,
            key_file_path,
            _pqc_manager: pqc_manager,
        }
    }

    /// Initialize key management system
    pub fn initialize(&mut self) -> Result<()> {
        let key_file = Path::new(&self.key_file_path);

        if key_file.exists() {
            self.load_keys()?;
        } else {
            self.generate_and_store_keys()?;
        }

        Ok(())
    }

    /// Load existing keys from file
    pub fn load_keys(&mut self) -> Result<()> {
        let key_file = Path::new(&self.key_file_path);

        match fs::read_to_string(key_file) {
            Ok(data) => {
                if let Ok(store) = serde_json::from_str::<NodeKeyStore>(&data) {
                    info!("Loaded PQC keys from {}", key_file.display());
                    info!(
                        "Available algorithms: {}, {}, {}",
                        store.dilithium.algorithm, store.falcon.algorithm, store.sphincs.algorithm
                    );
                    self.key_store = Some(store);
                    Ok(())
                } else {
                    warn!("Failed to parse PQC key store, generating new keys");
                    self.generate_and_store_keys()
                }
            }
            Err(e) => {
                error!("Error reading key file: {e}");
                warn!("Generating new PQC keys");
                self.generate_and_store_keys()
            }
        }
    }

    /// Generate new keys and store them
    pub fn generate_and_store_keys(&mut self) -> Result<()> {
        let key_file = Path::new(&self.key_file_path);

        // Ensure directory exists
        if let Some(parent) = key_file.parent() {
            fs::create_dir_all(parent)?;
        }

        // Generate keys for each algorithm
        let dilithium_keys = self.generate_algorithm_keys("Dilithium3")?;
        let falcon_keys = self.generate_algorithm_keys("Falcon-512")?;
        let sphincs_keys = self.generate_algorithm_keys("SPHINCS+-SHA256-128f")?;

        let node_id = uuid::Uuid::new_v4().to_string();
        let created_at = chrono::Utc::now().timestamp() as u64;

        let key_store = NodeKeyStore {
            dilithium: dilithium_keys,
            falcon: falcon_keys,
            sphincs: sphincs_keys,
            node_id,
            created_at,
            version: "1.0.0".to_string(),
        };

        // Save to file
        let json_data = serde_json::to_string_pretty(&key_store)?;
        fs::write(key_file, json_data)?;

        info!(
            "Generated and stored new PQC keys to {}",
            key_file.display()
        );
        self.key_store = Some(key_store);

        Ok(())
    }

    /// Generate keys for a specific algorithm
    fn generate_algorithm_keys(&self, algorithm: &str) -> Result<PQCKeyInfo> {
        // For now, generate placeholder keys
        // In a real implementation, you would use the PQC manager
        let key_id = uuid::Uuid::new_v4().to_string();
        let created_at = chrono::Utc::now().timestamp() as u64;

        Ok(PQCKeyInfo {
            algorithm: algorithm.to_string(),
            public_key: vec![0u8; 32],  // Placeholder
            private_key: vec![0u8; 64], // Placeholder
            key_id,
            created_at,
            expires_at: None,
        })
    }

    /// Get the current key store
    pub fn get_key_store(&self) -> Option<&NodeKeyStore> {
        self.key_store.as_ref()
    }

    /// Get public key for a specific algorithm
    pub fn get_public_key(&self, algorithm: &str) -> Option<&[u8]> {
        if let Some(store) = &self.key_store {
            match algorithm {
                "Dilithium3" => Some(&store.dilithium.public_key),
                "Falcon-512" => Some(&store.falcon.public_key),
                "SPHINCS+-SHA256-128f" => Some(&store.sphincs.public_key),
                _ => None,
            }
        } else {
            None
        }
    }

    /// Get private key for a specific algorithm
    pub fn get_private_key(&self, algorithm: &str) -> Option<&[u8]> {
        if let Some(store) = &self.key_store {
            match algorithm {
                "Dilithium3" => Some(&store.dilithium.private_key),
                "Falcon-512" => Some(&store.falcon.private_key),
                "SPHINCS+-SHA256-128f" => Some(&store.sphincs.private_key),
                _ => None,
            }
        } else {
            None
        }
    }

    /// Get node ID
    pub fn get_node_id(&self) -> Option<&str> {
        self.key_store.as_ref().map(|s| s.node_id.as_str())
    }

    /// Check if keys need rotation (based on age or expiration)
    pub fn needs_key_rotation(&self) -> bool {
        if let Some(store) = &self.key_store {
            let current_time = chrono::Utc::now().timestamp() as u64;
            let key_age = current_time - store.created_at;

            // Rotate keys if they're older than 30 days
            key_age > 30 * 24 * 60 * 60
        } else {
            true
        }
    }

    /// Rotate keys if needed
    pub fn rotate_keys_if_needed(&mut self) -> Result<bool> {
        if self.needs_key_rotation() {
            info!("Rotating PQC keys due to age");
            self.generate_and_store_keys()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get key information for all algorithms
    pub fn get_all_key_info(&self) -> Option<Vec<(&str, &PQCKeyInfo)>> {
        if let Some(store) = &self.key_store {
            Some(vec![
                ("Dilithium3", &store.dilithium),
                ("Falcon-512", &store.falcon),
                ("SPHINCS+-SHA256-128f", &store.sphincs),
            ])
        } else {
            None
        }
    }

    /// Backup keys to a different location
    pub fn backup_keys(&self, backup_path: &str) -> Result<()> {
        if let Some(store) = &self.key_store {
            let backup_file = Path::new(backup_path);

            // Ensure backup directory exists
            if let Some(parent) = backup_file.parent() {
                fs::create_dir_all(parent)?;
            }

            let json_data = serde_json::to_string_pretty(store)?;
            fs::write(backup_file, json_data)?;

            info!("Backed up PQC keys to {backup_path}");
            Ok(())
        } else {
            Err(anyhow!("No keys to backup"))
        }
    }

    /// Restore keys from backup
    pub fn restore_from_backup(&mut self, backup_path: &str) -> Result<()> {
        let backup_file = Path::new(backup_path);

        if !backup_file.exists() {
            return Err(anyhow!("Backup file does not exist: {}", backup_path));
        }

        let data = fs::read_to_string(backup_file)?;
        let store: NodeKeyStore = serde_json::from_str(&data)?;

        // Save to main location
        let json_data = serde_json::to_string_pretty(&store)?;
        fs::write(&self.key_file_path, json_data)?;

        self.key_store = Some(store);
        info!("Restored PQC keys from backup: {backup_path}");

        Ok(())
    }
}
