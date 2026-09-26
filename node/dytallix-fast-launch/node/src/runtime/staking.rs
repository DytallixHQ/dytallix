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
    /// Historical diagnostics never authorize current-profile mutation.
    /// In normal builds all legacy mutation methods are absent.
    pub fn ensure_legacy_fixture_mutation(&self) -> Result<(), String> {
        if !cfg!(feature = "legacy-economic-fixtures") {
            return Err("Legacy staking mutation is retired".into());
        }
        for prefix in [
            b"consensus:".as_slice(),
            b"recovery:",
            b"ordinary:",
            b"rewards:",
            b"issuance:",
            b"adaptive:",
        ] {
            if let Some(entry) = self
                .storage
                .db
                .iterator(rocksdb::IteratorMode::From(
                    prefix,
                    rocksdb::Direction::Forward,
                ))
                .next()
            {
                let (key, _) = entry.map_err(|error| error.to_string())?;
                if key.starts_with(prefix) {
                    return Err("Legacy fixture cannot mutate current-profile state".into());
                }
            }
        }
        for pool in ["validator_rewards", "treasury", "issuance_reserve"] {
            if self
                .storage
                .db
                .get(format!("emission:pool:{pool}"))
                .map_err(|error| error.to_string())?
                .is_some()
            {
                return Err("Legacy fixture cannot mutate current-profile custody".into());
            }
        }
        Ok(())
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

    /// Set new staking reward rate (basis points)
    #[cfg(feature = "legacy-economic-fixtures")]
    pub fn set_reward_rate_bps(&mut self, new_bps: u64) {
        self.ensure_legacy_fixture_mutation()
            .expect("historical staking fixture cannot mutate a current-profile store");
        self.reward_rate_bps = new_bps;
        let _ = self.storage.db.put(
            "staking:reward_rate_bps",
            bincode::serialize(&new_bps).unwrap(),
        );
    }

    /// Get current staking reward rate (bps)
    pub fn get_reward_rate_bps(&self) -> u64 {
        self.reward_rate_bps
    }

    /// Apply external emission from emission engine
    /// If total_stake > 0, update reward_index proportionally
    /// If total_stake == 0, accumulate in pending_staking_emission
    #[cfg(feature = "legacy-economic-fixtures")]
    pub fn apply_external_emission(&mut self, amount: u128) {
        self.ensure_legacy_fixture_mutation()
            .expect("historical staking fixture cannot mutate a current-profile store");
        if self.total_stake > 0 {
            // Distribute current emission using carry-aware division for precision
            let scaled = amount.saturating_mul(REWARD_SCALE);
            let numerator = self.reward_index_residual.saturating_add(scaled);
            let reward_per_unit = numerator / self.total_stake;
            self.reward_index = self.reward_index.saturating_add(reward_per_unit);
            self.reward_index_residual = numerator % self.total_stake;

            // Apply any pending emission too using the same residual carry
            if self.pending_staking_emission > 0 {
                let scaled_pending = self.pending_staking_emission.saturating_mul(REWARD_SCALE);
                let numerator_pending = self.reward_index_residual.saturating_add(scaled_pending);
                let pending_per_unit = numerator_pending / self.total_stake;
                self.reward_index = self.reward_index.saturating_add(pending_per_unit);
                self.reward_index_residual = numerator_pending % self.total_stake;
                self.pending_staking_emission = 0;
                self.save_pending_emission();
            }

            self.save_reward_index();
            self.save_reward_residual();

            // Record reward_index_after in the latest emission event for observability
            let latest_height = {
                // duplicated function logic to avoid a direct dependency on emission.rs
                self.storage
                    .db
                    .get("emission:last_height")
                    .ok()
                    .flatten()
                    .and_then(|v| {
                        if v.len() == 8 {
                            let mut a = [0u8; 8];
                            a.copy_from_slice(&v);
                            Some(u64::from_be_bytes(a))
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0)
            };
            if latest_height > 0 {
                if let Some(mut event) = self
                    .storage
                    .db
                    .get(format!("emission:event:{latest_height}"))
                    .ok()
                    .flatten()
                    .and_then(|v| {
                        bincode::deserialize::<crate::runtime::emission::EmissionEvent>(&v).ok()
                    })
                {
                    event.reward_index_after = Some(self.reward_index);
                    let _ = self.storage.db.put(
                        format!("emission:event:{latest_height}"),
                        bincode::serialize(&event).unwrap(),
                    );
                }
            }
        } else {
            // No stake yet, accumulate for later distribution
            self.pending_staking_emission = self.pending_staking_emission.saturating_add(amount);
            self.save_pending_emission();
        }
    }

    /// Set total stake (called when validators register/delegate)
    #[cfg(feature = "legacy-economic-fixtures")]
    pub fn set_total_stake(&mut self, stake: u128) {
        self.ensure_legacy_fixture_mutation()
            .expect("historical staking fixture cannot mutate a current-profile store");
        self.total_stake = stake;
        self.save_total_stake();

        // If stake becomes > 0 and we have pending emission, apply it using carry-aware division
        if stake > 0 && self.pending_staking_emission > 0 {
            let scaled_pending = self.pending_staking_emission.saturating_mul(REWARD_SCALE);
            let numerator = self.reward_index_residual.saturating_add(scaled_pending);
            let pending_per_unit = numerator / stake;
            self.reward_index = self.reward_index.saturating_add(pending_per_unit);
            self.reward_index_residual = numerator % stake;
            self.pending_staking_emission = 0;
            self.save_reward_index();
            self.save_pending_emission();
            self.save_reward_residual();
        }
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

    /// Save delegator reward record to storage
    #[cfg(feature = "legacy-economic-fixtures")]
    fn save_delegator_record(&self, address: &str, record: &DelegatorRewardRecord) {
        let key = delegator_key(address);
        let _ = self
            .storage
            .db
            .put(&key, bincode::serialize(record).unwrap());
    }

    /// Update stake amount for a delegator (used when delegation changes)
    #[cfg(feature = "legacy-economic-fixtures")]
    pub fn update_delegator_stake(&mut self, address: &str, new_stake: u128) {
        self.ensure_legacy_fixture_mutation()
            .expect("historical staking fixture cannot mutate a current-profile store");
        // First settle any pending rewards before changing stake
        self.settle_delegator_rewards(address);

        let current_reward_index = self.reward_index;
        let mut record = self.load_delegator_record(address);
        if record.last_reward_index == 0 {
            record.last_reward_index = current_reward_index;
        }
        // Update stake without mutating total_stake here to avoid double counting
        record.stake_amount = new_stake;
        self.save_delegator_record(address, &record);
    }

    /// Settle (accrue) rewards for a delegator based on current reward index
    #[cfg(feature = "legacy-economic-fixtures")]
    pub fn settle_delegator_rewards(&mut self, address: &str) -> u128 {
        self.ensure_legacy_fixture_mutation()
            .expect("historical staking fixture cannot mutate a current-profile store");
        let mut record = self.load_delegator_record(address);

        if record.stake_amount > 0 && self.reward_index > record.last_reward_index {
            let delta_index = self.reward_index - record.last_reward_index;
            let newly_accrued = (record.stake_amount * delta_index) / REWARD_SCALE;
            record.accrued_rewards = record.accrued_rewards.saturating_add(newly_accrued);
            record.last_reward_index = self.reward_index;
            self.save_delegator_record(address, &record);
            newly_accrued
        } else {
            // Update index even if no rewards to prevent future issues
            if self.reward_index > record.last_reward_index {
                record.last_reward_index = self.reward_index;
                self.save_delegator_record(address, &record);
            }
            0
        }
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

    /// Claim rewards for a delegator, returning the amount claimed
    /// This performs settlement and transfers rewards to the caller
    #[cfg(feature = "legacy-economic-fixtures")]
    pub fn claim_rewards(&mut self, address: &str) -> u128 {
        self.ensure_legacy_fixture_mutation()
            .expect("historical staking fixture cannot mutate a current-profile store");
        // First settle any pending rewards
        self.settle_delegator_rewards(address);

        let mut record = self.load_delegator_record(address);
        let claimed_amount = record.accrued_rewards;

        if claimed_amount > 0 {
            record.accrued_rewards = 0;
            self.save_delegator_record(address, &record);
        }

        claimed_amount
    }

    // Private storage methods
    #[cfg(feature = "legacy-economic-fixtures")]
    fn save_total_stake(&self) {
        let _ = self.storage.db.put(
            TOTAL_STAKE_KEY,
            bincode::serialize(&self.total_stake).unwrap(),
        );
    }

    #[cfg(feature = "legacy-economic-fixtures")]
    fn save_reward_index(&self) {
        let _ = self.storage.db.put(
            "staking:reward_index",
            bincode::serialize(&self.reward_index).unwrap(),
        );
    }

    #[cfg(feature = "legacy-economic-fixtures")]
    fn save_pending_emission(&self) {
        let _ = self.storage.db.put(
            "staking:pending_emission",
            bincode::serialize(&self.pending_staking_emission).unwrap(),
        );
    }

    #[cfg(feature = "legacy-economic-fixtures")]
    fn save_reward_residual(&self) {
        let _ = self.storage.db.put(
            "staking:reward_residual",
            bincode::serialize(&self.reward_index_residual).unwrap(),
        );
    }

    /// Delegate tokens to a validator
    #[cfg(feature = "legacy-economic-fixtures")]
    pub fn delegate(
        &mut self,
        delegator_addr: &str,
        _validator_addr: &str,
        amount_udgt: u128,
    ) -> Result<(), String> {
        self.ensure_legacy_fixture_mutation()?;
        if amount_udgt == 0 {
            return Err("Cannot delegate zero amount".to_string());
        }

        // Load existing delegator record and settle any pending rewards
        let mut record = self.load_delegator_record(delegator_addr);

        // Settle rewards before changing stake; guard against underflow if last index > current
        if record.stake_amount > 0 {
            let delta_index = self.reward_index.saturating_sub(record.last_reward_index);
            let pending_rewards = (delta_index * record.stake_amount) / REWARD_SCALE;
            record.accrued_rewards = record.accrued_rewards.saturating_add(pending_rewards);
        }

        // Update stake
        record.stake_amount = record.stake_amount.saturating_add(amount_udgt);
        record.last_reward_index = self.reward_index;

        // Save updated record
        self.save_delegator_record(delegator_addr, &record);

        // Update total stake
        self.set_total_stake(self.total_stake.saturating_add(amount_udgt));

        Ok(())
    }

    /// Undelegate tokens from a validator (simplified - immediate unbonding for MVP)
    #[cfg(feature = "legacy-economic-fixtures")]
    pub fn undelegate(
        &mut self,
        delegator_addr: &str,
        _validator_addr: &str,
        amount_udgt: u128,
    ) -> Result<(), String> {
        self.ensure_legacy_fixture_mutation()?;
        if amount_udgt == 0 {
            return Err("Cannot undelegate zero amount".to_string());
        }

        // Load existing delegator record
        let mut record = self.load_delegator_record(delegator_addr);

        if record.stake_amount < amount_udgt {
            return Err("Insufficient delegated amount".to_string());
        }

        // Settle rewards before changing stake; guard against underflow
        if record.stake_amount > 0 {
            let delta_index = self.reward_index.saturating_sub(record.last_reward_index);
            let pending_rewards = (delta_index * record.stake_amount) / REWARD_SCALE;
            record.accrued_rewards = record.accrued_rewards.saturating_add(pending_rewards);
        }

        // Update stake
        record.stake_amount = record.stake_amount.saturating_sub(amount_udgt);
        record.last_reward_index = self.reward_index;

        // Save updated record
        self.save_delegator_record(delegator_addr, &record);

        // Update total stake
        self.set_total_stake(self.total_stake.saturating_sub(amount_udgt));

        Ok(())
    }

    /// Process unbonding entries (simplified for MVP - no unbonding period)
    #[cfg(feature = "legacy-economic-fixtures")]
    pub fn process_unbonding(&mut self, current_height: u64) -> Vec<(String, u128)> {
        self.ensure_legacy_fixture_mutation()
            .expect("historical staking fixture cannot mutate a current-profile store");
        log::info!("Processing unbonding at height {current_height}");

        Vec::new()
    }
}

#[cfg(all(test, feature = "legacy-economic-fixtures"))]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_no_stake_accumulates_pending() {
        let dir = tempdir().unwrap();
        let storage = Arc::new(Storage::open(dir.path().join("test.db")).unwrap());
        let mut staking = StakingModule::new(storage);

        // No stake, should accumulate in pending
        staking.apply_external_emission(1000);
        assert_eq!(staking.pending_staking_emission, 1000);
        assert_eq!(staking.reward_index, 0);

        // Add more emission
        staking.apply_external_emission(500);
        assert_eq!(staking.pending_staking_emission, 1500);
        assert_eq!(staking.reward_index, 0);
    }

    #[test]
    fn test_stake_applies_pending_and_current() {
        let dir = tempdir().unwrap();
        let storage = Arc::new(Storage::open(dir.path().join("test.db")).unwrap());
        let mut staking = StakingModule::new(storage);

        // Accumulate pending emission
        staking.apply_external_emission(1000);
        assert_eq!(staking.pending_staking_emission, 1000);

        // Set stake - should apply pending
        staking.set_total_stake(1_000_000); // 1M uDGT
        assert_eq!(staking.pending_staking_emission, 0);
        let expected_reward_index = (1000 * REWARD_SCALE) / staking.total_stake;
        assert_eq!(staking.reward_index, expected_reward_index);

        // Add new emission with stake
        staking.apply_external_emission(2000);
        let additional_reward = (2000 * REWARD_SCALE) / staking.total_stake;
        assert_eq!(
            staking.reward_index,
            expected_reward_index + additional_reward
        );
    }

    #[test]
    fn test_reward_index_precision() {
        let dir = tempdir().unwrap();
        let storage = Arc::new(Storage::open(dir.path().join("test.db")).unwrap());
        let mut staking = StakingModule::new(storage);

        staking.set_total_stake(1_000_000_000_000); // 1M DGT in uDGT
        staking.apply_external_emission(1_000_000); // 1 DRT in uDRT

        let expected_reward_index = (1_000_000 * REWARD_SCALE) / staking.total_stake;
        assert_eq!(staking.reward_index, expected_reward_index);
        assert_eq!(expected_reward_index, 1_000_000); // Should be 1e6 (1 DRT per 1M DGT)
    }

    #[test]
    fn test_delegator_reward_accrual() {
        let dir = tempdir().unwrap();
        let storage = Arc::new(Storage::open(dir.path().join("test.db")).unwrap());
        let mut staking = StakingModule::new(storage);

        // Setup: Set total stake and a delegator with stake
        staking.set_total_stake(1_000_000_000_000); // 1M DGT in uDGT
        staking.update_delegator_stake("delegator1", 100_000_000_000); // 100k DGT

        // Apply emission which should update reward index
        staking.apply_external_emission(1_000_000); // 1 DRT in uDRT

        // Check that reward index was updated
        let expected_reward_index = (1_000_000 * REWARD_SCALE) / staking.total_stake;
        assert_eq!(staking.reward_index, expected_reward_index);

        // Apply another emission
        staking.apply_external_emission(2_000_000); // 2 DRT in uDRT
        let additional_reward = (2_000_000 * REWARD_SCALE) / staking.total_stake;
        assert_eq!(
            staking.reward_index,
            expected_reward_index + additional_reward
        );

        // Check delegator's accrued rewards
        let accrued = staking.get_accrued_rewards("delegator1");
        // Delegator has 10% of total stake, so should get 10% of total rewards
        let expected_total_rewards = 3_000_000; // 1 + 2 DRT
        let expected_delegator_rewards = expected_total_rewards / 10; // 10% stake
        assert_eq!(accrued, expected_delegator_rewards);

        // Claim rewards
        let claimed = staking.claim_rewards("delegator1");
        assert_eq!(claimed, expected_delegator_rewards);

        // After claiming, accrued should be 0
        let accrued_after = staking.get_accrued_rewards("delegator1");
        assert_eq!(accrued_after, 0);

        // Apply more emission and check accrual again
        staking.apply_external_emission(1_000_000); // 1 more DRT
        let new_accrued = staking.get_accrued_rewards("delegator1");
        let expected_new_rewards = 1_000_000 / 10; // 10% of 1 DRT
        assert_eq!(new_accrued, expected_new_rewards);
    }
}
