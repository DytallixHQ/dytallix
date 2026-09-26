use crate::storage::state::Storage;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub(crate) const TOTAL_STAKE_KEY: &str = "staking:total_stake";
pub(crate) fn delegator_key(address: &str) -> String {
    format!("staking:delegator:{address}")
}

/// Fixed-point scale for reward calculations (1e12 for precision)
pub const REWARD_SCALE: u128 = 1_000_000_000_000;

/// Per-delegator reward record for staking rewards
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DelegatorRewardRecord {
    /// Last global reward index when this delegator was updated
    pub last_reward_index: u128,
    /// Accrued but unclaimed rewards (in uDRT base units, unscaled)
    pub accrued_rewards: u128,
    /// Delegator's stake amount (in uDGT)
    pub stake_amount: u128,
}

/// Simplified staking state for lean-launch node
#[derive(Debug, Clone)]
pub struct StakingModule {
    pub storage: Arc<Storage>,
    /// Total stake across all validators (in uDGT)
    pub total_stake: u128,
    /// Global reward index (scaled by REWARD_SCALE)
    pub reward_index: u128,
    /// Pending staking emission when no stake exists
    pub pending_staking_emission: u128,
    /// Carry-over remainder of scaled emission not yet reflected in reward_index
    /// This value is in units of (uDRT * REWARD_SCALE) modulo total_stake at last update,
    /// but can be safely carried across stake changes as a count of leftover scaled units.
    pub reward_index_residual: u128,
    /// Governable per-block staking reward rate in basis points (scaled by 1e4). Example: 500 = 0.05 (5%).
    pub reward_rate_bps: u64,
}

impl StakingModule {
    /// Legacy staking mutation is retired; every call is rejected.
    pub fn ensure_legacy_fixture_mutation(&self) -> Result<(), String> {
        Err("Legacy staking mutation is retired".into())
    }
    pub fn new(storage: Arc<Storage>) -> Self {
        // Load existing state from storage
        let total_stake = storage
            .db
            .get(TOTAL_STAKE_KEY)
            .ok()
            .flatten()
            .and_then(|v| bincode::deserialize::<u128>(&v).ok())
            .unwrap_or(0);

        let reward_index = storage
            .db
            .get("staking:reward_index")
            .ok()
            .flatten()
            .and_then(|v| bincode::deserialize::<u128>(&v).ok())
            .unwrap_or(0);

        let pending_staking_emission = storage
            .db
            .get("staking:pending_emission")
            .ok()
            .flatten()
            .and_then(|v| bincode::deserialize::<u128>(&v).ok())
            .unwrap_or(0);

        let reward_index_residual = storage
            .db
            .get("staking:reward_residual")
            .ok()
            .flatten()
            .and_then(|v| bincode::deserialize::<u128>(&v).ok())
            .unwrap_or(0);
        // Load reward rate (default 0.05 = 500 bps)
        let reward_rate_bps = storage
            .db
            .get("staking:reward_rate_bps")
            .ok()
            .flatten()
            .and_then(|v| bincode::deserialize::<u64>(&v).ok())
            .unwrap_or(500);

        Self {
            storage,
            total_stake,
            reward_index,
            pending_staking_emission,
            reward_index_residual,
            reward_rate_bps,
        }
    }


    /// Get current staking reward rate (bps)
    pub fn get_reward_rate_bps(&self) -> u64 {
        self.reward_rate_bps
    }



    /// Get current reward statistics
    pub fn get_stats(&self) -> (u128, u128, u128) {
        (
            self.total_stake,
            self.reward_index,
            self.pending_staking_emission,
        )
    }

    /// Get total stake for a delegator
    pub fn get_total_stake(&self, address: &str) -> u128 {
        self.load_delegator_record(address).stake_amount
    }

    /// Load delegator reward record from storage
    pub fn load_delegator_record(&self, address: &str) -> DelegatorRewardRecord {
        let key = delegator_key(address);
        self.storage
            .db
            .get(&key)
            .ok()
            .flatten()
            .and_then(|v| bincode::deserialize::<DelegatorRewardRecord>(&v).ok())
            .unwrap_or(DelegatorRewardRecord {
                last_reward_index: self.reward_index,
                ..Default::default()
            })
    }




    /// Get accrued rewards for a delegator (includes pending rewards)
    pub fn get_accrued_rewards(&self, address: &str) -> u128 {
        let record = self.load_delegator_record(address);
        let mut accrued = record.accrued_rewards;

        // Add pending rewards since last settlement
        if record.stake_amount > 0 && self.reward_index > record.last_reward_index {
            let delta_index = self.reward_index - record.last_reward_index;
            let pending = (record.stake_amount * delta_index) / REWARD_SCALE;
            accrued = accrued.saturating_add(pending);
        }

        accrued
    }


    // Private storage methods






}

