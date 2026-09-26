use crate::{state::State, storage::state::Storage};
use rocksdb::{Direction, IteratorMode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

// Historical emission records and configuration helpers.
// Direct mutation is restricted to explicit archived diagnostic fixtures.

#[derive(Debug, Clone, Serialize)]
pub struct EmissionSnapshot {
    pub height: u64,
    pub pools: std::collections::HashMap<String, u128>,
}

/// Per-block emission event for auditable ledger
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmissionEvent {
    pub height: u64,
    pub timestamp: u64,
    pub total_emitted: u128,
    pub pools: HashMap<String, u128>, // keys: block_rewards, staking_rewards, ai_module_incentives, bridge_operations
    pub reward_index_after: Option<u128>, // scaled (e.g., 1e12)
    pub circulating_supply: u128,     // cumulative newly emitted DRT (excludes initial_supply)
}

/// Phase definition for phased emission schedule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmissionPhase {
    pub start_height: u64,
    pub end_height: Option<u64>, // None means unlimited
    pub per_block_amount: u128,
}

/// Emission schedule modes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EmissionSchedule {
    /// Fixed amount per block
    Static { per_block: u128 },
    /// Time-based phases with different emission rates
    Phased { phases: Vec<EmissionPhase> },
    /// Percentage-based annual inflation (existing implementation)
    Percentage { annual_inflation_rate: u16 }, // basis points (500 = 5%)
}

/// Genesis-based emission configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmissionConfig {
    pub schedule: EmissionSchedule,
    pub initial_supply: u128,
    pub emission_breakdown: EmissionBreakdown,
}

/// Emission distribution breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmissionBreakdown {
    pub block_rewards: u8,        // percentage (60)
    pub staking_rewards: u8,      // percentage (25)
    pub ai_module_incentives: u8, // percentage (10)
    pub bridge_operations: u8,    // percentage (5)
}

impl EmissionBreakdown {
    pub fn is_valid(&self) -> bool {
        self.block_rewards
            + self.staking_rewards
            + self.ai_module_incentives
            + self.bridge_operations
            == 100
    }
}

#[derive(Clone)]
pub struct EmissionEngine {
    pub storage: Arc<Storage>,
    pub state: Arc<Mutex<State>>, // to credit balances
    pub config: EmissionConfig,
    pub circulating_supply: u128, // tracks total DRT emitted
}

impl EmissionEngine {
    pub fn new(storage: Arc<Storage>, state: Arc<Mutex<State>>) -> Self {
        // Historical fixture configuration; these are not production defaults.
        let config = EmissionConfig {
            schedule: EmissionSchedule::Percentage {
                annual_inflation_rate: 500,
            }, // 5% in basis points
            initial_supply: 0, // DRT starts with 0 supply
            emission_breakdown: EmissionBreakdown {
                block_rewards: 60,
                staking_rewards: 25,
                ai_module_incentives: 10,
                bridge_operations: 5,
            },
        };

        // Load existing circulating supply (cumulative emitted) from storage
        let circulating_supply = storage
            .db
            .get("emission:circulating_supply")
            .ok()
            .flatten()
            .and_then(|v| bincode::deserialize::<u128>(&v).ok())
            .unwrap_or(0);

        Self {
            storage,
            state,
            config,
            circulating_supply,
        }
    }

    pub fn new_with_config(
        storage: Arc<Storage>,
        state: Arc<Mutex<State>>,
        config: EmissionConfig,
    ) -> Self {
        // Load existing circulating supply (cumulative emitted) from storage
        let circulating_supply = storage
            .db
            .get("emission:circulating_supply")
            .ok()
            .flatten()
            .and_then(|v| bincode::deserialize::<u128>(&v).ok())
            .unwrap_or(0);

        Self {
            storage,
            state,
            config,
            circulating_supply,
        }
    }

    /// Legacy emission mutation is retired; every call is rejected.
    fn require_legacy_mutation(_storage: &Storage) -> Result<(), String> {
        Err("legacy emission mutation is disabled".into())
    }

    fn pool_key(pool: &str) -> String {
        format!("emission:pool:{pool}")
    }

    fn height_key() -> &'static str {
        "emission:last_height"
    }

    fn event_key(height: u64) -> String {
        format!("emission:event:{height}")
    }

    fn circulating_supply_key() -> &'static str {
        "emission:circulating_supply"
    }

    /// Calculate per-block emission based on the emission schedule
    fn calculate_per_block_emission(&self, _current_height: u64) -> u128 {
        match &self.config.schedule {
            EmissionSchedule::Static { per_block } => *per_block,

            EmissionSchedule::Phased { phases } => {
                // Find the active phase for current height
                for phase in phases {
                    if _current_height >= phase.start_height
                        && (phase.end_height.is_none()
                            || _current_height <= phase.end_height.unwrap())
                    {
                        return phase.per_block_amount;
                    }
                }
                // No active phase found, return 0
                0
            }

            EmissionSchedule::Percentage {
                annual_inflation_rate,
            } => {
                const BLOCKS_PER_YEAR: u128 = 5_256_000; // ~6 second blocks

                // Total supply considered for inflation = initial + cumulative emitted
                let total_supply = self.config.initial_supply + self.circulating_supply;

                if total_supply == 0 {
                    // Bootstrap emission when supply is 0 - use a small fixed amount
                    return 1_000_000; // 1 DRT in uDRT (micro denomination)
                }

                let annual_emission = (total_supply * (*annual_inflation_rate as u128)) / 10000;
                let per_block = annual_emission / BLOCKS_PER_YEAR;

                // Future-proof floor: ensure non-zero emission when annual_emission > 0
                // to avoid stalling due to integer division rounding to zero.
                if annual_emission > 0 {
                    // Use a minimum per-block emission to ensure distribution pools receive
                    // meaningful allocations even at very low supply levels.
                    // This keeps staking rewards progressing deterministically in tests and CI.
                    per_block.max(100)
                } else {
                    0
                }
            }
        }
    }

    /// Calculate per-block distribution across pools
    fn calculate_pool_distributions(&self, total_emission: u128) -> HashMap<String, u128> {
        let mut pools = HashMap::new();
        let breakdown = &self.config.emission_breakdown;

        // Calculate amounts with proper rounding
        let block_rewards = (total_emission * breakdown.block_rewards as u128) / 100;
        let staking_rewards = (total_emission * breakdown.staking_rewards as u128) / 100;
        let ai_module_incentives = (total_emission * breakdown.ai_module_incentives as u128) / 100;

        // Allocate remainder to bridge_operations to ensure no loss
        let allocated = block_rewards + staking_rewards + ai_module_incentives;
        let bridge_operations = total_emission.saturating_sub(allocated);

        pools.insert("block_rewards".to_string(), block_rewards);
        pools.insert("staking_rewards".to_string(), staking_rewards);
        pools.insert("ai_module_incentives".to_string(), ai_module_incentives);
        pools.insert("bridge_operations".to_string(), bridge_operations);

        pools
    }

    pub fn pool_amount(&self, pool: &str) -> u128 {
        self.storage
            .db
            .get(Self::pool_key(pool))
            .ok()
            .flatten()
            .and_then(|v| bincode::deserialize::<u128>(&v).ok())
            .unwrap_or(0)
    }

    fn set_pool_amount(&self, pool: &str, amt: u128) {
        let _ = self
            .storage
            .db
            .put(Self::pool_key(pool), bincode::serialize(&amt).unwrap());
    }

    fn set_circulating_supply(&self, supply: u128) {
        let _ = self.storage.db.put(
            Self::circulating_supply_key(),
            bincode::serialize(&supply).unwrap(),
        );
    }

    pub fn last_accounted_height(&self) -> u64 {
        self.storage
            .db
            .get(Self::height_key())
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
    }

    fn set_last_height(&self, h: u64) {
        let _ = self.storage.db.put(Self::height_key(), h.to_be_bytes());
    }

    /// Get emission event for a specific height
    pub fn get_event(&self, height: u64) -> Option<EmissionEvent> {
        self.storage
            .db
            .get(Self::event_key(height))
            .ok()
            .flatten()
            .and_then(|v| bincode::deserialize::<EmissionEvent>(&v).ok())
    }

    /// Store emission event
    fn store_event(&self, event: &EmissionEvent) {
        let _ = self.storage.db.put(
            Self::event_key(event.height),
            bincode::serialize(event).unwrap(),
        );
    }

    /// Retired direct catch-up. Only isolated legacy diagnostic fixtures may mutate.
    pub fn apply_until(&mut self, target_height: u64) -> Result<(), String> {
        let storage = self.storage.clone();
        let _guard = storage
            .lock_execution()
            .map_err(|error| error.to_string())?;
        Self::require_legacy_mutation(&storage)?;
        let mut h = self.last_accounted_height();

        while h < target_height {
            h += 1;

            // Calculate emission for this block
            let total_emission = self.calculate_per_block_emission(h);
            let pool_distributions = self.calculate_pool_distributions(total_emission);

            // Update pool amounts
            for (pool, amount) in &pool_distributions {
                let current = self.pool_amount(pool);
                self.set_pool_amount(pool, current.saturating_add(*amount));
            }

            // Update circulating supply
            self.circulating_supply = self.circulating_supply.saturating_add(total_emission);
            self.set_circulating_supply(self.circulating_supply);

            // Create and store emission event
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let event = EmissionEvent {
                height: h,
                timestamp,
                total_emitted: total_emission,
                pools: pool_distributions,
                reward_index_after: None, // Will be set by staking module if needed
                circulating_supply: self.circulating_supply,
            };

            self.store_event(&event);
        }

        if h >= target_height {
            self.set_last_height(target_height);
        }
        Ok(())
    }

    /// Get the staking rewards amount for the latest block
    pub fn get_latest_staking_rewards(&self) -> u128 {
        let latest_height = self.last_accounted_height();
        if let Some(event) = self.get_event(latest_height) {
            event.pools.get("staking_rewards").copied().unwrap_or(0)
        } else {
            0
        }
    }
    /// Retired direct payout. Current rewards settle through committed blocks.
    pub fn claim(&self, pool: &str, amount: u128, to: &str) -> Result<u128, String> {
        // Block callers lock State before the execution mutex. Keep that order.
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Emission state lock poisoned".to_string())?;
        let storage = self.storage.clone();
        let _guard = storage
            .lock_execution()
            .map_err(|error| error.to_string())?;
        Self::require_legacy_mutation(&storage)?;
        let cur = self.pool_amount(pool);
        if amount > cur {
            return Err("InsufficientPool".into());
        }
        let new_amt = cur - amount;
        self.set_pool_amount(pool, new_amt);
        // credit account with DRT tokens (reward token), not DGT (governance token)
        state.credit(to, "udrt", amount);
        Ok(new_amt)
    }
    pub fn snapshot(&self) -> EmissionSnapshot {
        let mut pools = std::collections::HashMap::new();

        // Use current pool names from emission breakdown
        let pool_names = [
            "block_rewards",
            "staking_rewards",
            "ai_module_incentives",
            "bridge_operations",
        ];
        for pool in pool_names.iter() {
            pools.insert(pool.to_string(), self.pool_amount(pool));
        }

        EmissionSnapshot {
            height: self.last_accounted_height(),
            pools,
        }
    }

    /// Archived single-block emission, available only to explicit diagnostics.
    pub fn process_block_emission(
        &mut self,
        height: u64,
        _state: &mut crate::state::State,
    ) -> Result<EmissionEvent, String> {
        let storage = self.storage.clone();
        let _guard = storage
            .lock_execution()
            .map_err(|error| error.to_string())?;
        Self::require_legacy_mutation(&storage)?;
        // Calculate emission for this block
        let total_emission = self.calculate_per_block_emission(height);
        let pool_distributions = self.calculate_pool_distributions(total_emission);

        // Update pool amounts
        for (pool, amount) in &pool_distributions {
            let current = self.pool_amount(pool);
            self.set_pool_amount(pool, current.saturating_add(*amount));
        }

        // Update circulating supply
        self.circulating_supply = self.circulating_supply.saturating_add(total_emission);
        self.set_circulating_supply(self.circulating_supply);

        // Create and store emission event
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let event = EmissionEvent {
            height,
            timestamp,
            total_emitted: total_emission,
            pools: pool_distributions,
            reward_index_after: Some((self.circulating_supply * 1_000_000) / 1_000_000), // Simple reward index
            circulating_supply: self.circulating_supply,
        };

        self.store_event(&event);
        self.set_last_height(height);

        Ok(event)
    }

    /// Archived configuration mutation; this is not a current governance action.
    pub fn update_config(&mut self, config: EmissionConfig) -> Result<(), String> {
        let storage = self.storage.clone();
        let _guard = storage
            .lock_execution()
            .map_err(|error| error.to_string())?;
        Self::require_legacy_mutation(&storage)?;
        self.config = config;
        Ok(())
    }

    /// Get supply information
    pub fn get_supply_info(&self) -> anyhow::Result<SupplyInfo> {
        let supply = crate::supply::inspect(&self.storage)?;
        Ok(SupplyInfo {
            initial_supply: supply.genesis,
            // Legacy field name: cumulative emitted DRT, not liquid circulation.
            circulating_supply: supply.emitted,
            total_supply: supply.total,
            last_updated_height: supply.height,
        })
    }

    /// Get recent emission events
    pub fn get_emission_events(&self, limit: usize) -> Vec<EmissionEvent> {
        let last_height = self.last_accounted_height();
        let start_height = if last_height > limit as u64 {
            last_height - limit as u64 + 1
        } else {
            1
        };

        let mut events = Vec::new();
        for height in start_height..=last_height {
            if let Some(event) = self.get_event(height) {
                events.push(event);
            }
        }
        events
    }
}

/// Supply information structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplyInfo {
    pub initial_supply: u128,
    pub circulating_supply: u128,
    pub total_supply: u128,
    pub last_updated_height: u64,
}

#[cfg(test)]
mod consensus_mode_tests {
    use super::*;
    #[test]
    fn consensus_and_orphan_records_disable_legacy_emission() {
        for key in ["consensus:v1:config", "consensus:unknown:orphan"] {
            let dir = tempfile::tempdir().unwrap();
            let storage = Storage::open(dir.path().join("db")).unwrap();
            storage.db.put(key, b"fixture").unwrap();
            assert!(EmissionEngine::require_legacy_mutation(&storage).is_err());
            assert_eq!(
                storage.db.get(key).unwrap().as_deref(),
                Some(b"fixture".as_slice())
            );
        }
    }
}
