/*
Fee Burn Mechanism for Dytallix Dual-Token Economy

Implements configurable fee burning to reduce circulating supply of DGT/DRT tokens.
Supports governance-configurable burn rates with transparent accounting.
*/

use crate::state::State;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Fee burn configuration - governable parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeBurnConfig {
    /// Percentage of transaction fees to burn (basis points: 500 = 5%)
    pub burn_rate_bps: u32,
    /// Minimum fee amount required to trigger burning (prevents dust burn)
    pub min_burn_threshold: u128,
    /// Which token to burn fees in ("udgt" or "udrt")
    pub burn_token: String,
    /// Whether fee burning is enabled
    pub enabled: bool,
}

impl Default for FeeBurnConfig {
    fn default() -> Self {
        Self {
            burn_rate_bps: 2500,            // 25% default burn rate
            min_burn_threshold: 1000,       // 1000 micro-tokens minimum
            burn_token: "udgt".to_string(), // Burn DGT fees by default
            enabled: true,
        }
    }
}

/// Fee burn event for accounting and audit trails
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeBurnEvent {
    pub tx_hash: String,
    pub block_height: u64,
    pub timestamp: u64,
    pub fee_paid: u128,
    pub burn_amount: u128,
    pub burn_token: String,
    pub total_burned_after: u128, // Running total
}

/// Fee burn engine with persistent state tracking
#[derive(Debug, Clone)]
pub struct FeeBurnEngine {
    pub config: FeeBurnConfig,
    pub total_burned: HashMap<String, u128>, // token -> total burned
    pub burn_events: Vec<FeeBurnEvent>,
}

impl FeeBurnEngine {
    /// Create new fee burn engine with default configuration
    pub fn new() -> Self {
        Self {
            config: FeeBurnConfig::default(),
            total_burned: HashMap::new(),
            burn_events: Vec::new(),
        }
    }

    /// Create engine with custom configuration
    pub fn with_config(config: FeeBurnConfig) -> Self {
        Self {
            config,
            total_burned: HashMap::new(),
            burn_events: Vec::new(),
        }
    }

    /// Process fee burning for a transaction
    pub fn process_fee_burn(
        &mut self,
        tx_hash: String,
        block_height: u64,
        fee_paid: u128,
        _state: &mut State,
    ) -> Result<Option<FeeBurnEvent>, String> {
        // Check if burning is enabled
        if !self.config.enabled {
            return Ok(None);
        }

        // Check minimum threshold
        if fee_paid < self.config.min_burn_threshold {
            return Ok(None);
        }

        // Calculate burn amount
        if self.config.burn_rate_bps > 10000 {
            return Err("Burn rate cannot exceed 100%".into());
        }
        let rate = u128::from(self.config.burn_rate_bps);
        let burn_amount = (fee_paid / 10000) * rate + (fee_paid % 10000) * rate / 10000;

        if burn_amount == 0 {
            return Ok(None);
        }

        // Get current timestamp
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| format!("Time error: {}", e))?
            .as_secs();

        // Update total burned for this token
        let token = &self.config.burn_token;
        let current_total = self.total_burned.get(token).unwrap_or(&0);
        let new_total = current_total
            .checked_add(burn_amount)
            .ok_or("Burn diagnostic total exceeds u128")?;
        self.total_burned.insert(token.clone(), new_total);

        // Note: In a real implementation, we would burn tokens from the treasury/fee pool
        // For now, we just track the burn amount for accounting

        // Create burn event
        let burn_event = FeeBurnEvent {
            tx_hash,
            block_height,
            timestamp,
            fee_paid,
            burn_amount,
            burn_token: token.clone(),
            total_burned_after: new_total,
        };

        // Store event
        self.burn_events.push(burn_event.clone());

        // Keep only last 1000 events to prevent unbounded growth
        if self.burn_events.len() > 1000 {
            self.burn_events.remove(0);
        }

        Ok(Some(burn_event))
    }

    /// Update configuration (governance function)
    pub fn update_config(&mut self, new_config: FeeBurnConfig) -> Result<(), String> {
        // Validate configuration
        if new_config.burn_rate_bps > 10000 {
            return Err("Burn rate cannot exceed 100%".to_string());
        }

        if new_config.burn_token != "udgt" && new_config.burn_token != "udrt" {
            return Err("Burn token must be 'udgt' or 'udrt'".to_string());
        }

        self.config = new_config;
        Ok(())
    }

    /// Get total burned amount for a token
    pub fn get_total_burned(&self, token: &str) -> u128 {
        *self.total_burned.get(token).unwrap_or(&0)
    }

    /// Get recent burn events (last N events)
    pub fn get_recent_events(&self, limit: usize) -> Vec<FeeBurnEvent> {
        let start_idx = if self.burn_events.len() > limit {
            self.burn_events.len() - limit
        } else {
            0
        };
        self.burn_events[start_idx..].to_vec()
    }

    /// Get burn statistics
    pub fn get_burn_stats(&self) -> BurnStats {
        let total_events = self.burn_events.len();
        let total_udgt_burned = self.get_total_burned("udgt");
        let total_udrt_burned = self.get_total_burned("udrt");

        let total_fees_processed = self.burn_events.iter().map(|e| e.fee_paid).sum::<u128>();

        let total_burned_all = self.burn_events.iter().map(|e| e.burn_amount).sum::<u128>();

        let effective_burn_rate = if total_fees_processed > 0 {
            (total_burned_all * 10000) / total_fees_processed
        } else {
            0
        };

        BurnStats {
            total_events,
            total_fees_processed,
            total_burned_all,
            total_udgt_burned,
            total_udrt_burned,
            effective_burn_rate_bps: effective_burn_rate as u32,
            current_config: self.config.clone(),
        }
    }
}

impl Default for FeeBurnEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Burn statistics for monitoring and governance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnStats {
    pub total_events: usize,
    pub total_fees_processed: u128,
    pub total_burned_all: u128,
    pub total_udgt_burned: u128,
    pub total_udrt_burned: u128,
    pub effective_burn_rate_bps: u32,
    pub current_config: FeeBurnConfig,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::state::Storage;
    use std::sync::Arc;
    use tempfile;

    fn create_test_state() -> State {
        let dir = tempfile::tempdir().expect("tempdir");
        let storage = Arc::new(Storage::open(dir.path().join("state.db")).unwrap());
        State::new(storage)
    }

    #[test]
    fn test_fee_burn_basic() {
        let mut engine = FeeBurnEngine::new();
        let mut state = create_test_state();

        let result = engine
            .process_fee_burn(
                "test_tx_1".to_string(),
                100,
                10000, // 10,000 micro-tokens fee
                &mut state,
            )
            .unwrap();

        assert!(result.is_some());
        let event = result.unwrap();
        assert_eq!(event.burn_amount, 2500); // 25% of 10,000
        assert_eq!(event.burn_token, "udgt");
        assert_eq!(engine.get_total_burned("udgt"), 2500);
    }

    #[test]
    fn test_fee_burn_below_threshold() {
        let mut engine = FeeBurnEngine::new();
        let mut state = create_test_state();

        let result = engine
            .process_fee_burn(
                "test_tx_1".to_string(),
                100,
                500, // Below default threshold of 1000
                &mut state,
            )
            .unwrap();

        assert!(result.is_none());
        assert_eq!(engine.get_total_burned("udgt"), 0);
    }

    #[test]
    fn test_fee_burn_disabled() {
        let config = FeeBurnConfig {
            enabled: false,
            ..Default::default()
        };
        let mut engine = FeeBurnEngine::with_config(config);
        let mut state = create_test_state();

        let result = engine
            .process_fee_burn("test_tx_1".to_string(), 100, 10000, &mut state)
            .unwrap();

        assert!(result.is_none());
        assert_eq!(engine.get_total_burned("udgt"), 0);
    }

    #[test]
    fn test_config_update() {
        let mut engine = FeeBurnEngine::new();

        let new_config = FeeBurnConfig {
            burn_rate_bps: 5000, // 50%
            burn_token: "udrt".to_string(),
            ..Default::default()
        };

        assert!(engine.update_config(new_config).is_ok());
        assert_eq!(engine.config.burn_rate_bps, 5000);
        assert_eq!(engine.config.burn_token, "udrt");
    }

    #[test]
    fn test_invalid_config_update() {
        let mut engine = FeeBurnEngine::new();

        let invalid_config = FeeBurnConfig {
            burn_rate_bps: 15000, // >100%
            ..Default::default()
        };

        assert!(engine.update_config(invalid_config).is_err());
    }

    #[test]
    fn test_burn_stats() {
        let mut engine = FeeBurnEngine::new();
        let mut state = create_test_state();

        // Process a few burns
        engine
            .process_fee_burn("tx1".to_string(), 100, 10000, &mut state)
            .unwrap();
        engine
            .process_fee_burn("tx2".to_string(), 101, 20000, &mut state)
            .unwrap();

        let stats = engine.get_burn_stats();
        assert_eq!(stats.total_events, 2);
        assert_eq!(stats.total_fees_processed, 30000);
        assert_eq!(stats.total_burned_all, 7500); // 25% of 30,000
        assert_eq!(stats.total_udgt_burned, 7500);
        assert_eq!(stats.effective_burn_rate_bps, 2500);
    }
    #[test]
    fn full_width_burn_diagnostic_is_exact_and_checked() {
        let mut state = create_test_state();
        let mut engine = FeeBurnEngine::with_config(FeeBurnConfig {
            burn_rate_bps: 10000,
            min_burn_threshold: 0,
            ..Default::default()
        });
        let event = engine
            .process_fee_burn("max".into(), 1, u128::MAX, &mut state)
            .unwrap()
            .unwrap();
        assert_eq!(event.burn_amount, u128::MAX);
        assert!(engine
            .process_fee_burn("overflow".into(), 2, 1, &mut state)
            .is_err());
        assert_eq!(engine.get_total_burned("udgt"), u128::MAX);
        assert_eq!(engine.burn_events.len(), 1);
    }
}
