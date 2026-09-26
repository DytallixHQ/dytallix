//! Oracle Registry and Reputation Management System
//!
//! This module implements a comprehensive oracle registry with staking requirements,
//! reputation scoring, slashing mechanisms, and performance monitoring for AI oracles
//! in the Dytallix blockchain network.

use anyhow::Result;
use chrono;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::types::{Address, Amount, Timestamp};
use dytallix_pqc::PQCManager;

/// Oracle registry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleRegistryConfig {
    /// Minimum stake required to register as an oracle
    pub min_stake_amount: Amount,
    /// Maximum number of oracles that can be registered
    pub max_oracle_count: usize,
    /// Minimum reputation score to remain active
    pub min_reputation_threshold: f64,
    /// Reputation decay factor per day
    pub reputation_decay_factor: f64,
    /// Slashing percentage for malicious behavior
    pub slashing_percentage: f64,
    /// Grace period before slashing takes effect (seconds)
    pub slashing_grace_period: u64,
    /// Performance monitoring window (seconds)
    pub performance_window: u64,
    /// Maximum allowed response time for reputation calculation
    pub max_response_time_ms: u64,
    /// Minimum accuracy required for positive reputation
    pub min_accuracy_threshold: f64,
}

impl Default for OracleRegistryConfig {
    fn default() -> Self {
        Self {
            min_stake_amount: 1000000000, // 10 DYTX (assuming 8 decimal places)
            max_oracle_count: 100,
            min_reputation_threshold: 0.7,
            reputation_decay_factor: 0.99, // 1% decay per day
            slashing_percentage: 0.1,      // 10% slashing
            slashing_grace_period: 86400,  // 24 hours
            performance_window: 604800,    // 7 days
            max_response_time_ms: 5000,    // 5 seconds
            min_accuracy_threshold: 0.8,   // 80% accuracy
        }
    }
}

/// Oracle status in the registry
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OracleStatus {
    /// Oracle is pending activation
    Pending,
    /// Oracle is active and can provide responses
    Active,
    /// Oracle is temporarily suspended
    Suspended,
    /// Oracle is permanently slashed
    Slashed,
    /// Oracle has voluntarily withdrawn
    Withdrawn,
}

/// Oracle staking information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleStake {
    /// Total amount staked
    pub total_amount: Amount,
    /// Amount currently locked due to slashing
    pub locked_amount: Amount,
    /// Stake creation timestamp
    pub staked_at: Timestamp,
    /// Last stake update timestamp
    pub last_updated: Timestamp,
    /// Pending slashing amount
    pub pending_slash: Amount,
    /// Slashing grace period end time
    pub slash_grace_end: Option<Timestamp>,
}

/// Oracle reputation metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleReputation {
    /// Current reputation score (0.0 to 1.0)
    pub current_score: f64,
    /// Historical maximum reputation
    pub max_score: f64,
    /// Total responses submitted
    pub total_responses: u64,
    /// Accurate responses (verified correct)
    pub accurate_responses: u64,
    /// Inaccurate responses (verified incorrect)
    pub inaccurate_responses: u64,
    /// Responses with invalid signatures
    pub invalid_signature_responses: u64,
    /// Average response time in milliseconds
    pub avg_response_time_ms: f64,
    /// Last reputation update
    pub last_updated: Timestamp,
    /// Reputation history (last 30 days)
    pub daily_scores: Vec<(Timestamp, f64)>,
}

impl Default for OracleReputation {
    fn default() -> Self {
        Self {
            current_score: 1.0, // Start with perfect reputation
            max_score: 1.0,
            total_responses: 0,
            accurate_responses: 0,
            inaccurate_responses: 0,
            invalid_signature_responses: 0,
            avg_response_time_ms: 0.0,
            last_updated: chrono::Utc::now().timestamp() as u64,
            daily_scores: Vec::new(),
        }
    }
}

/// Oracle performance metrics for monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OraclePerformanceMetrics {
    /// Responses in the last 24 hours
    pub responses_24h: u64,
    /// Responses in the last 7 days
    pub responses_7d: u64,
    /// Responses in the last 30 days
    pub responses_30d: u64,
    /// Average accuracy in the last 7 days
    pub accuracy_7d: f64,
    /// Average response time in the last 7 days
    pub response_time_7d: f64,
    /// Uptime percentage in the last 7 days
    pub uptime_7d: f64,
    /// Last response timestamp
    pub last_response: Timestamp,
    /// Consecutive failed responses
    pub consecutive_failures: u32,
    /// Performance degradation alerts
    pub alerts: Vec<String>,
}

impl Default for OraclePerformanceMetrics {
    fn default() -> Self {
        Self {
            responses_24h: 0,
            responses_7d: 0,
            responses_30d: 0,
            accuracy_7d: 1.0,
            response_time_7d: 0.0,
            uptime_7d: 1.0,
            last_response: 0,
            consecutive_failures: 0,
            alerts: Vec::new(),
        }
    }
}

/// Complete oracle registry entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleRegistryEntry {
    /// Oracle address/identity
    pub oracle_address: Address,
    /// Human-readable oracle name
    pub oracle_name: String,
    /// Oracle description
    pub description: String,
    /// Oracle public key for signature verification
    pub public_key: Vec<u8>,
    /// Oracle status
    pub status: OracleStatus,
    /// Staking information
    pub stake: OracleStake,
    /// Reputation metrics
    pub reputation: OracleReputation,
    /// Performance metrics
    pub performance: OraclePerformanceMetrics,
    /// Registration timestamp
    pub registered_at: Timestamp,
    /// Last activity timestamp
    pub last_activity: Timestamp,
    /// Contact information (optional)
    pub contact_info: Option<String>,
    /// Oracle version/type information
    pub oracle_version: String,
    /// Supported AI service types
    pub supported_services: Vec<String>,
}

/// Oracle whitelist/blacklist management
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OracleAccessControl {
    /// Whitelisted oracle addresses (if empty, no whitelist)
    pub whitelist: Vec<Address>,
    /// Blacklisted oracle addresses
    pub blacklist: Vec<Address>,
    /// Temporary suspensions with end times
    pub temporary_suspensions: HashMap<Address, Timestamp>,
    /// Administrative notes for access control decisions
    pub access_notes: HashMap<Address, String>,
}

/// Arguments for registering a new oracle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterOracleArgs {
    pub oracle_address: Address,
    pub oracle_name: String,
    pub description: String,
    pub public_key: Vec<u8>,
    pub stake_amount: Amount,
    pub oracle_version: String,
    pub supported_services: Vec<String>,
    pub contact_info: Option<String>,
}

/// Oracle registry and reputation management system
pub struct OracleRegistry {
    /// Registry configuration
    config: OracleRegistryConfig,
    /// Oracle entries
    oracles: Arc<RwLock<HashMap<Address, OracleRegistryEntry>>>,
    /// Access control lists
    access_control: Arc<RwLock<OracleAccessControl>>,
    /// PQC manager for cryptographic operations
    _pqc_manager: Arc<PQCManager>,
    /// Registry statistics
    stats: Arc<RwLock<RegistryStatistics>>,
}

/// Registry statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryStatistics {
    /// Total oracles ever registered
    pub total_registered: u64,
    /// Currently active oracles
    pub active_count: u64,
    /// Slashed oracles count
    pub slashed_count: u64,
    /// Total stake amount across all oracles
    pub total_stake: Amount,
    /// Average reputation across active oracles
    pub avg_reputation: f64,
    /// Total responses processed
    pub total_responses: u64,
    /// Overall accuracy rate
    pub overall_accuracy: f64,
    /// Registry start time
    pub registry_started: Timestamp,
    /// Last statistics update
    pub last_updated: Timestamp,
}

impl Default for RegistryStatistics {
    fn default() -> Self {
        Self {
            total_registered: 0,
            active_count: 0,
            slashed_count: 0,
            total_stake: 0,
            avg_reputation: 0.0,
            total_responses: 0,
            overall_accuracy: 0.0,
            registry_started: chrono::Utc::now().timestamp() as u64,
            last_updated: chrono::Utc::now().timestamp() as u64,
        }
    }
}

impl OracleRegistry {
    /// Create a new oracle registry
    pub fn new(config: OracleRegistryConfig) -> Result<Self> {
        Ok(Self {
            config,
            oracles: Arc::new(RwLock::new(HashMap::new())),
            access_control: Arc::new(RwLock::new(OracleAccessControl::default())),
            _pqc_manager: Arc::new(PQCManager::new()?),
            stats: Arc::new(RwLock::new(RegistryStatistics::default())),
        })
    }

    /// Register a new oracle with stake requirements
    pub async fn register_oracle(&self, args: RegisterOracleArgs) -> Result<()> {
        // Verify stake amount meets minimum requirement
        if args.stake_amount < self.config.min_stake_amount {
            return Err(anyhow::anyhow!(
                "Stake amount {} is below minimum requirement {}",
                args.stake_amount,
                self.config.min_stake_amount
            ));
        }

        // Check if oracle already exists
        let oracles = self.oracles.read().await;
        if oracles.contains_key(&args.oracle_address) {
            return Err(anyhow::anyhow!(
                "Oracle {} already registered",
                args.oracle_address
            ));
        }

        // Check registry capacity
        if oracles.len() >= self.config.max_oracle_count {
            return Err(anyhow::anyhow!("Oracle registry at maximum capacity"));
        }
        drop(oracles);

        // Check access control
        let access_control = self.access_control.read().await;
        if access_control.blacklist.contains(&args.oracle_address) {
            return Err(anyhow::anyhow!(
                "Oracle {} is blacklisted",
                args.oracle_address
            ));
        }

        // If whitelist exists and is not empty, oracle must be whitelisted
        if !access_control.whitelist.is_empty()
            && !access_control.whitelist.contains(&args.oracle_address)
        {
            return Err(anyhow::anyhow!(
                "Oracle {} is not whitelisted",
                args.oracle_address
            ));
        }
        drop(access_control);

        let now = chrono::Utc::now().timestamp() as u64;

        // Create oracle entry
        let oracle_entry = OracleRegistryEntry {
            oracle_address: args.oracle_address.clone(),
            oracle_name: args.oracle_name,
            description: args.description,
            public_key: args.public_key,
            status: OracleStatus::Pending, // Start as pending, admin can activate
            stake: OracleStake {
                total_amount: args.stake_amount,
                locked_amount: 0,
                staked_at: now,
                last_updated: now,
                pending_slash: 0,
                slash_grace_end: None,
            },
            reputation: OracleReputation::default(),
            performance: OraclePerformanceMetrics::default(),
            registered_at: now,
            last_activity: now,
            contact_info: args.contact_info,
            oracle_version: args.oracle_version,
            supported_services: args.supported_services,
        };

        // Add to registry
        let mut oracles = self.oracles.write().await;
        oracles.insert(args.oracle_address.clone(), oracle_entry);
        drop(oracles);

        // Update statistics
        let mut stats = self.stats.write().await;
        stats.total_registered += 1;
        stats.total_stake += args.stake_amount;
        stats.last_updated = now;
        drop(stats);

        info!(
            "Oracle {} registered successfully with stake {}",
            args.oracle_address, args.stake_amount
        );
        Ok(())
    }

    /// Activate an oracle (admin function)
    pub async fn activate_oracle(&self, oracle_address: &Address) -> Result<()> {
        let mut oracles = self.oracles.write().await;
        if let Some(oracle) = oracles.get_mut(oracle_address) {
            match oracle.status {
                OracleStatus::Pending => {
                    oracle.status = OracleStatus::Active;
                    oracle.last_activity = chrono::Utc::now().timestamp() as u64;

                    // Update statistics
                    let mut stats = self.stats.write().await;
                    stats.active_count += 1;
                    stats.last_updated = oracle.last_activity;

                    info!("Oracle {oracle_address} activated");
                    Ok(())
                }
                _ => Err(anyhow::anyhow!(
                    "Oracle {} cannot be activated from status {:?}",
                    oracle_address,
                    oracle.status
                )),
            }
        } else {
            Err(anyhow::anyhow!("Oracle {} not found", oracle_address))
        }
    }

    /// Update oracle reputation based on response accuracy
    pub async fn update_reputation(
        &self,
        oracle_address: &Address,
        response_time_ms: u64,
        is_accurate: bool,
        signature_valid: bool,
    ) -> Result<()> {
        let mut oracles = self.oracles.write().await;
        if let Some(oracle) = oracles.get_mut(oracle_address) {
            let now = chrono::Utc::now().timestamp() as u64;
            let reputation = &mut oracle.reputation;
            let performance = &mut oracle.performance;

            // Update counters
            reputation.total_responses += 1;
            if signature_valid {
                if is_accurate {
                    reputation.accurate_responses += 1;
                } else {
                    reputation.inaccurate_responses += 1;
                }
            } else {
                reputation.invalid_signature_responses += 1;
            }

            // Update response time
            let total_time =
                reputation.avg_response_time_ms * (reputation.total_responses - 1) as f64;
            reputation.avg_response_time_ms =
                (total_time + response_time_ms as f64) / reputation.total_responses as f64;

            // Calculate new reputation score
            let accuracy_score = if reputation.total_responses > 0 {
                reputation.accurate_responses as f64 / reputation.total_responses as f64
            } else {
                1.0
            };

            let signature_score = if reputation.total_responses > 0 {
                (reputation.total_responses - reputation.invalid_signature_responses) as f64
                    / reputation.total_responses as f64
            } else {
                1.0
            };

            let response_time_score = if response_time_ms <= self.config.max_response_time_ms {
                1.0
            } else {
                (self.config.max_response_time_ms as f64 / response_time_ms as f64).max(0.1)
            };

            // Combined reputation score (weighted average)
            let new_score =
                (accuracy_score * 0.5) + (signature_score * 0.3) + (response_time_score * 0.2);
            reputation.current_score = new_score.clamp(0.0, 1.0);
            reputation.max_score = reputation.max_score.max(reputation.current_score);
            reputation.last_updated = now;

            // Add to daily scores (keep last 30 days)
            reputation
                .daily_scores
                .push((now, reputation.current_score));
            reputation
                .daily_scores
                .retain(|(timestamp, _)| now - timestamp <= 30 * 24 * 3600);

            // Update performance metrics
            performance.last_response = now;
            if !signature_valid || !is_accurate {
                performance.consecutive_failures += 1;
            } else {
                performance.consecutive_failures = 0;
            }

            // Update performance counters (simplified - in practice would use time windows)
            performance.responses_24h += 1;
            performance.responses_7d += 1;
            performance.responses_30d += 1;

            oracle.last_activity = now;

            // Check if oracle should be suspended due to low reputation
            if reputation.current_score < self.config.min_reputation_threshold {
                oracle.status = OracleStatus::Suspended;
                warn!(
                    "Oracle {} suspended due to low reputation: {}",
                    oracle_address, reputation.current_score
                );
            }

            info!(
                "Updated reputation for oracle {}: score={:.3}, accuracy={:.3}",
                oracle_address, reputation.current_score, accuracy_score
            );

            Ok(())
        } else {
            Err(anyhow::anyhow!("Oracle {} not found", oracle_address))
        }
    }

    /// Slash an oracle for malicious behavior
    pub async fn slash_oracle(
        &self,
        oracle_address: &Address,
        slash_reason: String,
        immediate: bool,
    ) -> Result<()> {
        let mut oracles = self.oracles.write().await;
        if let Some(oracle) = oracles.get_mut(oracle_address) {
            let now = chrono::Utc::now().timestamp() as u64;
            let slash_amount =
                (oracle.stake.total_amount as f64 * self.config.slashing_percentage) as Amount;

            if immediate {
                // Immediate slashing
                oracle.stake.locked_amount += slash_amount;
                oracle.status = OracleStatus::Slashed;
                oracle.last_activity = now;

                // Update statistics
                let mut stats = self.stats.write().await;
                stats.slashed_count += 1;
                if oracle.status == OracleStatus::Active {
                    stats.active_count -= 1;
                }
                stats.last_updated = now;

                error!(
                    "Oracle {oracle_address} immediately slashed for: {slash_reason}. Amount: {slash_amount}"
                );
            } else {
                // Grace period slashing
                oracle.stake.pending_slash = slash_amount;
                oracle.stake.slash_grace_end = Some(now + self.config.slashing_grace_period);
                oracle.status = OracleStatus::Suspended;
                oracle.last_activity = now;

                warn!(
                    "Oracle {oracle_address} scheduled for slashing after grace period. Reason: {slash_reason}. Amount: {slash_amount}"
                );
            }

            Ok(())
        } else {
            Err(anyhow::anyhow!("Oracle {} not found", oracle_address))
        }
    }

    /// Process pending slashing (should be called periodically)
    pub async fn process_pending_slashing(&self) -> Result<()> {
        let now = chrono::Utc::now().timestamp() as u64;
        let mut oracles = self.oracles.write().await;
        let mut slashed_count = 0;

        for (address, oracle) in oracles.iter_mut() {
            if let Some(grace_end) = oracle.stake.slash_grace_end {
                if now >= grace_end && oracle.stake.pending_slash > 0 {
                    // Execute pending slash
                    oracle.stake.locked_amount += oracle.stake.pending_slash;
                    oracle.stake.pending_slash = 0;
                    oracle.stake.slash_grace_end = None;
                    oracle.status = OracleStatus::Slashed;
                    oracle.last_activity = now;
                    slashed_count += 1;

                    error!(
                        "Executed pending slash for oracle {}: {}",
                        address, oracle.stake.locked_amount
                    );
                }
            }
        }

        if slashed_count > 0 {
            let mut stats = self.stats.write().await;
            stats.slashed_count += slashed_count;
            stats.last_updated = now;
            info!("Processed {slashed_count} pending slashing operations");
        }

        Ok(())
    }

    /// Add oracle to whitelist
    pub async fn whitelist_oracle(&self, oracle_address: Address) -> Result<()> {
        let mut access_control = self.access_control.write().await;
        if !access_control.whitelist.contains(&oracle_address) {
            access_control.whitelist.push(oracle_address.clone());
            info!("Oracle {oracle_address} added to whitelist");
        }
        Ok(())
    }

    /// Add oracle to blacklist
    pub async fn blacklist_oracle(&self, oracle_address: Address, reason: String) -> Result<()> {
        let mut access_control = self.access_control.write().await;
        if !access_control.blacklist.contains(&oracle_address) {
            access_control.blacklist.push(oracle_address.clone());
            access_control
                .access_notes
                .insert(oracle_address.clone(), reason.clone());
            info!("Oracle {oracle_address} added to blacklist: {reason}");
        }

        // Also suspend the oracle if it's currently registered
        let mut oracles = self.oracles.write().await;
        if let Some(oracle) = oracles.get_mut(&oracle_address) {
            oracle.status = OracleStatus::Suspended;
            oracle.last_activity = chrono::Utc::now().timestamp() as u64;
        }

        Ok(())
    }

    /// Get oracle information
    pub async fn get_oracle(&self, oracle_address: &Address) -> Option<OracleRegistryEntry> {
        let oracles = self.oracles.read().await;
        oracles.get(oracle_address).cloned()
    }

    /// Get all active oracles
    pub async fn get_active_oracles(&self) -> HashMap<Address, OracleRegistryEntry> {
        let oracles = self.oracles.read().await;
        oracles
            .iter()
            .filter(|(_, oracle)| oracle.status == OracleStatus::Active)
            .map(|(addr, oracle)| (addr.clone(), oracle.clone()))
            .collect()
    }

    /// Get oracles by reputation threshold
    pub async fn get_oracles_by_reputation(
        &self,
        min_reputation: f64,
    ) -> HashMap<Address, OracleRegistryEntry> {
        let oracles = self.oracles.read().await;
        oracles
            .iter()
            .filter(|(_, oracle)| {
                oracle.status == OracleStatus::Active
                    && oracle.reputation.current_score >= min_reputation
            })
            .map(|(addr, oracle)| (addr.clone(), oracle.clone()))
            .collect()
    }

    /// Get registry statistics
    pub async fn get_statistics(&self) -> RegistryStatistics {
        let stats = self.stats.read().await;
        stats.clone()
    }

    /// Perform daily maintenance tasks
    pub async fn daily_maintenance(&self) -> Result<()> {
        let now = chrono::Utc::now().timestamp() as u64;
        let mut oracles = self.oracles.write().await;

        for (address, oracle) in oracles.iter_mut() {
            // Apply reputation decay
            oracle.reputation.current_score *= self.config.reputation_decay_factor;
            oracle.reputation.last_updated = now;

            // Reset daily counters (simplified)
            oracle.performance.responses_24h = 0;

            debug!("Applied daily maintenance to oracle {address}");
        }

        // Process pending slashing
        drop(oracles);
        self.process_pending_slashing().await?;

        info!("Completed daily maintenance tasks");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_oracle_registration() {
        let config = OracleRegistryConfig::default();
        let registry = OracleRegistry::new(config).unwrap();

        let result = registry
            .register_oracle(RegisterOracleArgs {
                oracle_address: "dyt1oracle1".to_string(),
                oracle_name: "Test Oracle".to_string(),
                description: "Test oracle for unit testing".to_string(),
                public_key: vec![1, 2, 3, 4],
                stake_amount: 2000000000, // 20 DYTX
                oracle_version: "1.0.0".to_string(),
                supported_services: vec!["risk_scoring".to_string()],
                contact_info: Some("test@example.com".to_string()),
            })
            .await;

        assert!(result.is_ok());

        let oracle = registry.get_oracle(&"dyt1oracle1".to_string()).await;
        assert!(oracle.is_some());
        assert_eq!(oracle.unwrap().oracle_name, "Test Oracle");
    }

    #[tokio::test]
    async fn test_reputation_update() {
        let config = OracleRegistryConfig::default();
        let registry = OracleRegistry::new(config).unwrap();

        // Register oracle
        registry
            .register_oracle(RegisterOracleArgs {
                oracle_address: "dyt1oracle2".to_string(),
                oracle_name: "Test Oracle 2".to_string(),
                description: "Test oracle 2".to_string(),
                public_key: vec![5, 6, 7, 8],
                stake_amount: 2000000000,
                oracle_version: "1.0.0".to_string(),
                supported_services: vec!["risk_scoring".to_string()],
                contact_info: None,
            })
            .await
            .unwrap();

        // Activate oracle
        registry
            .activate_oracle(&"dyt1oracle2".to_string())
            .await
            .unwrap();

        // Update reputation
        registry
            .update_reputation(
                &"dyt1oracle2".to_string(),
                1000, // 1 second response time
                true, // accurate
                true, // valid signature
            )
            .await
            .unwrap();

        let oracle = registry
            .get_oracle(&"dyt1oracle2".to_string())
            .await
            .unwrap();
        assert!(oracle.reputation.current_score > 0.9);
        assert_eq!(oracle.reputation.accurate_responses, 1);
    }

    #[tokio::test]
    async fn test_slashing() {
        let config = OracleRegistryConfig::default();
        let registry = OracleRegistry::new(config).unwrap();

        // Register and activate oracle
        registry
            .register_oracle(RegisterOracleArgs {
                oracle_address: "dyt1oracle3".to_string(),
                oracle_name: "Test Oracle 3".to_string(),
                description: "Test oracle 3".to_string(),
                public_key: vec![9, 10, 11, 12],
                stake_amount: 2000000000,
                oracle_version: "1.0.0".to_string(),
                supported_services: vec!["risk_scoring".to_string()],
                contact_info: None,
            })
            .await
            .unwrap();

        registry
            .activate_oracle(&"dyt1oracle3".to_string())
            .await
            .unwrap();

        // Slash oracle
        registry
            .slash_oracle(
                &"dyt1oracle3".to_string(),
                "Malicious behavior detected".to_string(),
                true, // immediate
            )
            .await
            .unwrap();

        let oracle = registry
            .get_oracle(&"dyt1oracle3".to_string())
            .await
            .unwrap();
        assert_eq!(oracle.status, OracleStatus::Slashed);
        assert!(oracle.stake.locked_amount > 0);
    }
}
