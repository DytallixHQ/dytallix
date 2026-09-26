use crate::storage::state::Storage;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Configuration for a user's dead man switch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadManSwitchConfig {
    /// The address that can claim the funds
    pub beneficiary: String,
    /// The inactivity period in blocks after which funds can be claimed
    pub period_blocks: u64,
    /// The block height of the last activity (registration or ping)
    pub last_active_block: u64,
}

/// Dead Man Switch module
pub struct DeadManSwitchModule {
    storage: Arc<Storage>,
}

impl DeadManSwitchModule {
    pub fn new(storage: Arc<Storage>) -> Self {
        Self { storage }
    }

    /// Load the switch configuration for a user
    pub fn load_config(&self, owner: &str) -> Option<DeadManSwitchConfig> {
        let key = format!("dms:config:{}", owner);
        self.storage
            .db
            .get(&key)
            .ok()
            .flatten()
            .and_then(|v| bincode::deserialize::<DeadManSwitchConfig>(&v).ok())
    }

    /// Save the switch configuration for a user
    fn save_config(&self, owner: &str, config: &DeadManSwitchConfig) -> Result<(), String> {
        let key = format!("dms:config:{}", owner);
        self.storage
            .db
            .put(&key, bincode::serialize(config).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }

    /// Register or update a dead man switch
    pub fn register(
        &self,
        owner: &str,
        beneficiary: &str,
        period_blocks: u64,
        current_block: u64,
    ) -> Result<(), String> {
        if period_blocks == 0 {
            return Err("Period must be greater than 0".to_string());
        }
        if owner == beneficiary {
            return Err("Beneficiary cannot be the owner".to_string());
        }

        let config = DeadManSwitchConfig {
            beneficiary: beneficiary.to_string(),
            period_blocks,
            last_active_block: current_block,
        };
        self.save_config(owner, &config)
    }

    /// Reset the inactivity timer
    pub fn ping(&self, owner: &str, current_block: u64) -> Result<(), String> {
        let mut config = self
            .load_config(owner)
            .ok_or("No dead man switch registered")?;
        config.last_active_block = current_block;
        self.save_config(owner, &config)
    }

    /// Validate if a claim is valid. Returns the beneficiary address if valid.
    pub fn validate_claim(
        &self,
        owner: &str,
        caller: &str,
        current_block: u64,
    ) -> Result<String, String> {
        let config = self
            .load_config(owner)
            .ok_or("No dead man switch registered")?;

        if config.beneficiary != caller {
            return Err("Caller is not the beneficiary".to_string());
        }

        if current_block < config.last_active_block + config.period_blocks {
            return Err(format!(
                "Switch has not triggered yet. Remaining blocks: {}",
                (config.last_active_block + config.period_blocks).saturating_sub(current_block)
            ));
        }

        Ok(config.beneficiary)
    }
}
