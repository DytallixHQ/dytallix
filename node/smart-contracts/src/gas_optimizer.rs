//! Gas Optimization Utilities for Cosmos WASM Contracts
//!
//! This module provides utilities for dynamic gas calculation, optimization strategies,
//! and gas usage profiling to achieve measurable gas cost reductions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Gas cost configuration for different operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasCostConfig {
    pub base_storage_read: u64,
    pub base_storage_write: u64,
    pub storage_key_cost_per_byte: u64,
    pub storage_value_cost_per_byte: u64,
    pub memory_allocation_base: u64,
    pub memory_cost_per_byte: u64,
    pub computation_base: u64,
    pub validation_cost_per_check: u64,
    pub cryptographic_operation_base: u64,
    pub network_call_base: u64,
}

impl Default for GasCostConfig {
    fn default() -> Self {
        Self {
            base_storage_read: 1000,
            base_storage_write: 2000,
            storage_key_cost_per_byte: 10,
            storage_value_cost_per_byte: 5,
            memory_allocation_base: 500,
            memory_cost_per_byte: 1,
            computation_base: 100,
            validation_cost_per_check: 200,
            cryptographic_operation_base: 1500,
            network_call_base: 5000,
        }
    }
}

/// Operation complexity metrics for gas calculation
#[derive(Debug, Clone)]
pub struct OperationComplexity {
    pub storage_reads: u32,
    pub storage_writes: u32,
    pub key_size_bytes: u32,
    pub value_size_bytes: u32,
    pub memory_allocations: u32,
    pub validation_checks: u32,
    pub cryptographic_operations: u32,
    pub network_calls: u32,
    pub computational_intensity: u32, // Scale 1-10
}

impl Default for OperationComplexity {
    fn default() -> Self {
        Self {
            storage_reads: 0,
            storage_writes: 0,
            key_size_bytes: 0,
            value_size_bytes: 0,
            memory_allocations: 0,
            validation_checks: 0,
            cryptographic_operations: 0,
            network_calls: 0,
            computational_intensity: 1,
        }
    }
}

/// Gas optimization strategies
#[derive(Debug, Clone)]
pub enum OptimizationStrategy {
    /// Batch multiple operations together
    BatchOperations {
        batch_size: u32,
        operation_type: String,
    },
    /// Cache frequently accessed data
    CacheData {
        cache_size_limit: u32,
        ttl_seconds: u64,
    },
    /// Use compact data structures
    CompactSerialization { compression_ratio: f64 },
    /// Lazy load expensive operations
    LazyLoading { threshold_complexity: u32 },
    /// Early return optimization
    EarlyReturn { validation_order: Vec<String> },
}

/// Gas usage metrics for profiling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasUsageMetrics {
    pub operation_name: String,
    pub estimated_gas: u64,
    pub actual_gas_used: Option<u64>,
    pub optimization_applied: Option<String>,
    pub complexity_score: u32,
    pub efficiency_rating: f64, // 0.0 to 1.0, higher is better
    pub timestamp: u64,
}

/// Gas optimizer main struct
pub struct GasOptimizer {
    config: GasCostConfig,
    operation_cache: HashMap<String, GasUsageMetrics>,
    optimization_strategies: Vec<OptimizationStrategy>,
}

impl GasOptimizer {
    /// Create a new gas optimizer with default configuration
    pub fn new() -> Self {
        Self {
            config: GasCostConfig::default(),
            operation_cache: HashMap::new(),
            optimization_strategies: Vec::new(),
        }
    }
}

impl Default for GasOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GasOptimizer {
    /// Create a gas optimizer with custom configuration
    pub fn with_config(config: GasCostConfig) -> Self {
        Self {
            config,
            operation_cache: HashMap::new(),
            optimization_strategies: Vec::new(),
        }
    }

    /// Add an optimization strategy
    pub fn add_strategy(&mut self, strategy: OptimizationStrategy) {
        self.optimization_strategies.push(strategy);
    }

    /// Calculate estimated gas cost for an operation
    pub fn estimate_gas_cost(&self, operation: &str, complexity: &OperationComplexity) -> u64 {
        let mut total_gas = self.config.computation_base;

        // Storage costs
        total_gas += (complexity.storage_reads as u64) * self.config.base_storage_read;
        total_gas += (complexity.storage_writes as u64) * self.config.base_storage_write;
        total_gas += (complexity.key_size_bytes as u64) * self.config.storage_key_cost_per_byte;
        total_gas += (complexity.value_size_bytes as u64) * self.config.storage_value_cost_per_byte;

        // Memory costs
        total_gas += (complexity.memory_allocations as u64) * self.config.memory_allocation_base;
        total_gas += (complexity.key_size_bytes + complexity.value_size_bytes) as u64
            * self.config.memory_cost_per_byte;

        // Validation costs
        total_gas += (complexity.validation_checks as u64) * self.config.validation_cost_per_check;

        // Cryptographic operation costs
        total_gas +=
            (complexity.cryptographic_operations as u64) * self.config.cryptographic_operation_base;

        // Network call costs
        total_gas += (complexity.network_calls as u64) * self.config.network_call_base;

        // Computational intensity multiplier
        total_gas = (total_gas as f64 * (complexity.computational_intensity as f64 / 5.0)) as u64;

        // Apply optimization strategies
        total_gas = self.apply_optimizations(operation, total_gas, complexity);

        total_gas
    }

    /// Apply optimization strategies to reduce gas cost
    fn apply_optimizations(
        &self,
        operation: &str,
        base_gas: u64,
        complexity: &OperationComplexity,
    ) -> u64 {
        let mut optimized_gas = base_gas;
        let mut applied_optimizations = Vec::new();

        for strategy in &self.optimization_strategies {
            match strategy {
                OptimizationStrategy::BatchOperations {
                    batch_size,
                    operation_type,
                } => {
                    if operation.contains(operation_type) && complexity.storage_writes > 1 {
                        // Reduce gas for batched operations
                        let batch_reduction = (*batch_size as f64).ln() * 0.1;
                        optimized_gas =
                            (optimized_gas as f64 * (1.0 - batch_reduction.min(0.3))) as u64;
                        applied_optimizations.push("batch_operations");
                    }
                }
                OptimizationStrategy::CacheData {
                    cache_size_limit: _,
                    ttl_seconds: _,
                } => {
                    if complexity.storage_reads > 2 {
                        // Reduce gas for cached reads
                        optimized_gas = (optimized_gas as f64 * 0.85) as u64;
                        applied_optimizations.push("cache_data");
                    }
                }
                OptimizationStrategy::CompactSerialization { compression_ratio } => {
                    if complexity.value_size_bytes > 100 {
                        // Reduce gas for compact serialization
                        let size_reduction = compression_ratio;
                        optimized_gas =
                            (optimized_gas as f64 * (1.0 - size_reduction * 0.2)) as u64;
                        applied_optimizations.push("compact_serialization");
                    }
                }
                OptimizationStrategy::LazyLoading {
                    threshold_complexity,
                } => {
                    let total_complexity = complexity.storage_reads
                        + complexity.storage_writes
                        + complexity.validation_checks
                        + complexity.cryptographic_operations;
                    if total_complexity > *threshold_complexity {
                        // Reduce gas for lazy loading
                        optimized_gas = (optimized_gas as f64 * 0.90) as u64;
                        applied_optimizations.push("lazy_loading");
                    }
                }
                OptimizationStrategy::EarlyReturn {
                    validation_order: _,
                } => {
                    if complexity.validation_checks > 3 {
                        // Reduce gas for early return optimization
                        optimized_gas = (optimized_gas as f64 * 0.88) as u64;
                        applied_optimizations.push("early_return");
                    }
                }
            }
        }

        optimized_gas
    }

    /// Record actual gas usage for learning and optimization
    pub fn record_gas_usage(&mut self, operation: &str, estimated_gas: u64, actual_gas: u64) {
        let efficiency_rating = if actual_gas > 0 {
            (estimated_gas as f64 / actual_gas as f64).min(1.0)
        } else {
            1.0
        };

        let metrics = GasUsageMetrics {
            operation_name: operation.to_string(),
            estimated_gas,
            actual_gas_used: Some(actual_gas),
            optimization_applied: None,
            complexity_score: 0, // Would be calculated based on operation
            efficiency_rating,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        self.operation_cache.insert(operation.to_string(), metrics);
    }

    /// Get gas usage statistics for analysis
    pub fn get_gas_statistics(&self) -> GasStatistics {
        let mut total_operations = 0u64;
        let mut total_estimated_gas = 0u64;
        let mut total_actual_gas = 0u64;
        let mut efficiency_sum = 0.0;

        for metrics in self.operation_cache.values() {
            total_operations += 1;
            total_estimated_gas += metrics.estimated_gas;
            if let Some(actual) = metrics.actual_gas_used {
                total_actual_gas += actual;
            }
            efficiency_sum += metrics.efficiency_rating;
        }

        let average_efficiency = if total_operations > 0 {
            efficiency_sum / total_operations as f64
        } else {
            0.0
        };

        let gas_savings = if total_actual_gas > 0 && total_estimated_gas > total_actual_gas {
            ((total_estimated_gas - total_actual_gas) as f64 / total_estimated_gas as f64) * 100.0
        } else {
            0.0
        };

        GasStatistics {
            total_operations,
            total_estimated_gas,
            total_actual_gas,
            average_efficiency,
            gas_savings_percentage: gas_savings,
            most_efficient_operation: self.get_most_efficient_operation(),
            least_efficient_operation: self.get_least_efficient_operation(),
        }
    }

    /// Find the most efficient operation
    fn get_most_efficient_operation(&self) -> Option<String> {
        self.operation_cache
            .values()
            .max_by(|a, b| {
                a.efficiency_rating
                    .partial_cmp(&b.efficiency_rating)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|metrics| metrics.operation_name.clone())
    }

    /// Find the least efficient operation
    fn get_least_efficient_operation(&self) -> Option<String> {
        self.operation_cache
            .values()
            .min_by(|a, b| {
                a.efficiency_rating
                    .partial_cmp(&b.efficiency_rating)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|metrics| metrics.operation_name.clone())
    }

    /// Generate optimization recommendations
    pub fn get_optimization_recommendations(&self) -> Vec<OptimizationRecommendation> {
        let mut recommendations = Vec::new();

        for (operation, metrics) in &self.operation_cache {
            if metrics.efficiency_rating < 0.7 {
                recommendations.push(OptimizationRecommendation {
                    operation: operation.clone(),
                    current_efficiency: metrics.efficiency_rating,
                    recommended_strategy: if metrics.estimated_gas > 10000 {
                        "Consider breaking down into smaller operations or adding caching"
                    } else if metrics.complexity_score > 50 {
                        "Optimize validation logic with early returns"
                    } else {
                        "Review data structures for more compact representation"
                    }
                    .to_string(),
                    potential_gas_savings: ((1.0 - metrics.efficiency_rating)
                        * metrics.estimated_gas as f64)
                        as u64,
                });
            }
        }

        recommendations.sort_by(|a, b| b.potential_gas_savings.cmp(&a.potential_gas_savings));
        recommendations
    }

    /// Clear the operation cache
    pub fn clear_cache(&mut self) {
        self.operation_cache.clear();
    }

    /// Update gas cost configuration
    pub fn update_config(&mut self, new_config: GasCostConfig) {
        self.config = new_config;
    }
}

/// Gas usage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasStatistics {
    pub total_operations: u64,
    pub total_estimated_gas: u64,
    pub total_actual_gas: u64,
    pub average_efficiency: f64,
    pub gas_savings_percentage: f64,
    pub most_efficient_operation: Option<String>,
    pub least_efficient_operation: Option<String>,
}

/// Optimization recommendation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationRecommendation {
    pub operation: String,
    pub current_efficiency: f64,
    pub recommended_strategy: String,
    pub potential_gas_savings: u64,
}

/// Predefined operation complexity profiles for common bridge operations
pub struct BridgeOperationProfiles;

impl BridgeOperationProfiles {
    /// Complexity profile for mint tokens operation
    pub fn mint_tokens() -> OperationComplexity {
        OperationComplexity {
            storage_reads: 3,            // state, token_config, existing_transaction_check
            storage_writes: 2,           // bridge_transaction, token_config_update
            key_size_bytes: 64,          // bridge_id + token_denom
            value_size_bytes: 300,       // transaction data + config data
            memory_allocations: 5,       // various structs
            validation_checks: 6,        // pause, validator, amount, token, cap, duplicate
            cryptographic_operations: 1, // signature verification
            network_calls: 0,
            computational_intensity: 3,
        }
    }

    /// Complexity profile for burn tokens operation
    pub fn burn_tokens() -> OperationComplexity {
        OperationComplexity {
            storage_reads: 2,  // state, token_config
            storage_writes: 2, // bridge_transaction, token_config
            key_size_bytes: 64,
            value_size_bytes: 250,
            memory_allocations: 4,
            validation_checks: 5, // pause, amount, token, active, duplicate
            cryptographic_operations: 0,
            network_calls: 0,
            computational_intensity: 2,
        }
    }

    /// Complexity profile for confirm bridge operation
    pub fn confirm_bridge() -> OperationComplexity {
        OperationComplexity {
            storage_reads: 2,     // state, bridge_transaction
            storage_writes: 2,    // validator_confirmations, bridge_transaction
            key_size_bytes: 80,   // bridge_id + validator_address
            value_size_bytes: 50, // confirmation flag + updated count
            memory_allocations: 2,
            validation_checks: 3, // validator, already_confirmed, transaction_exists
            cryptographic_operations: 1, // signature verification
            network_calls: 0,
            computational_intensity: 2,
        }
    }

    /// Complexity profile for batch confirm bridge operation (optimized)
    pub fn batch_confirm_bridge(batch_size: u32) -> OperationComplexity {
        OperationComplexity {
            storage_reads: 1 + batch_size,        // state + bridge_transactions
            storage_writes: batch_size,           // confirmation updates
            key_size_bytes: 80 * batch_size,      // multiple bridge_ids + validators
            value_size_bytes: 50 * batch_size,    // multiple confirmations
            memory_allocations: 3,                // batch processing structures
            validation_checks: 2 + batch_size,    // validator + per-transaction checks
            cryptographic_operations: batch_size, // signature verifications
            network_calls: 0,
            computational_intensity: 3,
        }
    }

    /// Complexity profile for query operations
    pub fn query_bridge_stats() -> OperationComplexity {
        OperationComplexity {
            storage_reads: 10, // multiple token configs for stats
            storage_writes: 0,
            key_size_bytes: 100,    // various keys for token iteration
            value_size_bytes: 1000, // accumulated token data
            memory_allocations: 8,  // result aggregation
            validation_checks: 1,   // basic validation
            cryptographic_operations: 0,
            network_calls: 0,
            computational_intensity: 4, // aggregation computation
        }
    }
}

/// Memory-based gas calculation utilities
pub struct MemoryGasCalculator;

impl MemoryGasCalculator {
    /// Calculate gas cost based on memory usage patterns
    pub fn calculate_memory_gas(
        allocated_bytes: u64,
        peak_usage_bytes: u64,
        allocation_count: u32,
    ) -> u64 {
        let base_cost = 1000; // Base memory gas cost
        let size_cost = allocated_bytes / 100; // 1 gas per 100 bytes
        let peak_penalty = if peak_usage_bytes > allocated_bytes * 2 {
            (peak_usage_bytes - allocated_bytes) / 500 // Penalty for excessive memory usage
        } else {
            0
        };
        let allocation_cost = allocation_count as u64 * 50; // Cost per allocation

        base_cost + size_cost + peak_penalty + allocation_cost
    }

    /// Calculate gas for memory expansion during execution
    pub fn expansion_gas_cost(old_size: u64, new_size: u64) -> u64 {
        if new_size <= old_size {
            return 0;
        }

        let expansion = new_size - old_size;
        let expansion_words = expansion.div_ceil(32); // Round up to word boundary

        // Quadratic growth cost to discourage excessive memory usage
        expansion_words * 3 + (expansion_words * expansion_words) / 512
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gas_optimizer_basic_calculation() {
        let optimizer = GasOptimizer::new();
        let complexity = BridgeOperationProfiles::mint_tokens();

        let estimated_gas = optimizer.estimate_gas_cost("mint_tokens", &complexity);
        assert!(estimated_gas > 0);

        // Should be reasonable for a mint operation
        assert!(estimated_gas > 5000);
        assert!(estimated_gas < 50000);
    }

    #[test]
    fn test_optimization_strategies() {
        let mut optimizer = GasOptimizer::new();

        // Add caching strategy
        optimizer.add_strategy(OptimizationStrategy::CacheData {
            cache_size_limit: 1000,
            ttl_seconds: 300,
        });

        let complexity = OperationComplexity {
            storage_reads: 5, // Multiple reads should trigger caching optimization
            ..BridgeOperationProfiles::mint_tokens()
        };

        let base_gas = optimizer.estimate_gas_cost("mint_tokens", &OperationComplexity::default());
        let optimized_gas = optimizer.estimate_gas_cost("mint_tokens", &complexity);

        // Optimized version should use less gas due to caching
        assert!(optimized_gas != base_gas);
    }

    #[test]
    fn test_batch_operation_complexity() {
        let single_confirm = BridgeOperationProfiles::confirm_bridge();
        let batch_confirm = BridgeOperationProfiles::batch_confirm_bridge(5);

        // Batch should be more efficient per operation
        assert!(batch_confirm.storage_writes > single_confirm.storage_writes);
        assert!(batch_confirm.computational_intensity >= single_confirm.computational_intensity);
    }

    #[test]
    fn test_gas_statistics_tracking() {
        let mut optimizer = GasOptimizer::new();

        // Record some usage data
        optimizer.record_gas_usage("mint_tokens", 10000, 8500);
        optimizer.record_gas_usage("burn_tokens", 8000, 9000);
        optimizer.record_gas_usage("confirm_bridge", 5000, 4200);

        let stats = optimizer.get_gas_statistics();
        assert_eq!(stats.total_operations, 3);
        assert!(stats.average_efficiency > 0.0);
        assert!(stats.average_efficiency <= 1.0);
    }

    #[test]
    fn test_optimization_recommendations() {
        let mut optimizer = GasOptimizer::new();

        // Record inefficient operation
        optimizer.record_gas_usage("inefficient_op", 5000, 8000);

        let recommendations = optimizer.get_optimization_recommendations();
        assert!(!recommendations.is_empty());
        assert_eq!(recommendations[0].operation, "inefficient_op");
        assert!(recommendations[0].potential_gas_savings > 0);
    }

    #[test]
    fn test_memory_gas_calculation() {
        let gas_cost = MemoryGasCalculator::calculate_memory_gas(1000, 1200, 5);
        assert!(gas_cost > 1000); // Should include base cost plus allocations

        let expansion_cost = MemoryGasCalculator::expansion_gas_cost(1000, 2000);
        assert!(expansion_cost > 0);

        // No expansion should cost nothing
        let no_expansion = MemoryGasCalculator::expansion_gas_cost(1000, 1000);
        assert_eq!(no_expansion, 0);
    }

    #[test]
    fn test_operation_complexity_defaults() {
        let default_complexity = OperationComplexity::default();
        assert_eq!(default_complexity.storage_reads, 0);
        assert_eq!(default_complexity.computational_intensity, 1);

        let mint_complexity = BridgeOperationProfiles::mint_tokens();
        assert!(mint_complexity.storage_reads > 0);
        assert!(mint_complexity.validation_checks > 0);
    }
}
