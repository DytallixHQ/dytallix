//! Gas Attack Vector Analyzer
//!
//! This module specializes in detecting gas-related attack vectors including
//! gas griefing, DoS attacks, and gas manipulation exploits.

use super::{SecurityFinding, Severity, VulnerabilityCategory};
use crate::gas_optimizer::{GasOptimizer, OperationComplexity};
use crate::runtime::{ContractCall, ContractDeployment, ExecutionResult};
use serde::{Deserialize, Serialize};

/// Analyzer for gas-related attack vectors
pub struct GasAttackAnalyzer {
    gas_optimizer: GasOptimizer,
    attack_count: u64,
    execution_history: Vec<GasExecutionRecord>,
    thresholds: GasAttackThresholds,
}

/// Record of gas usage for analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GasExecutionRecord {
    contract_address: String,
    method: String,
    estimated_gas: u64,
    actual_gas: u64,
    gas_limit: u64,
    timestamp: u64,
    efficiency: f64,
}

/// Thresholds for detecting gas attacks
#[derive(Debug, Clone)]
struct GasAttackThresholds {
    /// Efficiency below this threshold triggers gas griefing alert
    min_efficiency: f64,
    /// Gas usage spike multiplier for DoS detection
    dos_spike_multiplier: f64,
    /// Maximum allowed gas limit for single operation
    max_single_operation_gas: u64,
    /// Minimum gas needed to trigger exhaustion concern
    exhaustion_threshold: u64,
    /// Maximum allowed complexity score
    max_complexity_score: u32,
}

impl Default for GasAttackThresholds {
    fn default() -> Self {
        Self {
            min_efficiency: 0.6,
            dos_spike_multiplier: 3.0,
            max_single_operation_gas: 5_000_000,
            exhaustion_threshold: 8_000_000,
            max_complexity_score: 100,
        }
    }
}

impl GasAttackAnalyzer {
    pub fn new() -> Self {
        Self {
            gas_optimizer: GasOptimizer::new(),
            attack_count: 0,
            execution_history: Vec::new(),
            thresholds: GasAttackThresholds::default(),
        }
    }
}

impl Default for GasAttackAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl GasAttackAnalyzer {
    /// Analyze a contract deployment for gas attack vectors
    pub async fn analyze_deployment(
        &mut self,
        deployment: &ContractDeployment,
    ) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        // 1. Analyze gas limit requirements
        findings.extend(self.analyze_gas_limits(deployment));

        // 2. Estimate complexity and gas requirements
        findings.extend(self.analyze_complexity_patterns(deployment).await);

        // 3. Check for gas griefing patterns in bytecode
        findings.extend(self.detect_griefing_patterns(deployment));

        findings
    }

    /// Analyze execution for gas attack vectors
    pub async fn analyze_execution(
        &mut self,
        call: &ContractCall,
        result: &ExecutionResult,
    ) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        // Record execution for pattern analysis
        let efficiency = if call.gas_limit > 0 {
            result.gas_used as f64 / call.gas_limit as f64
        } else {
            0.0
        };

        let record = GasExecutionRecord {
            contract_address: call.contract_address.clone(),
            method: call.method.clone(),
            estimated_gas: 0, // Will be calculated
            actual_gas: result.gas_used,
            gas_limit: call.gas_limit,
            timestamp: call.timestamp,
            efficiency,
        };

        self.execution_history.push(record);

        // 1. Detect gas griefing attacks
        findings.extend(self.detect_gas_griefing(call, result));

        // 2. Detect DoS through gas exhaustion
        findings.extend(self.detect_gas_exhaustion_dos(call, result));

        // 3. Detect gas limit manipulation
        findings.extend(self.detect_gas_manipulation(call, result));

        // 4. Analyze gas usage patterns
        findings.extend(self.analyze_gas_usage_patterns(call, result).await);

        if !findings.is_empty() {
            self.attack_count += findings.len() as u64;
        }

        findings
    }

    /// Analyze gas limits for potential attacks
    fn analyze_gas_limits(&self, deployment: &ContractDeployment) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        // Check for excessive gas limit requests
        if deployment.gas_limit > self.thresholds.max_single_operation_gas {
            findings.push(SecurityFinding {
                id: format!("GAS-LIMIT-ABUSE-{}", self.generate_id()),
                title: "Excessive Gas Limit Request".to_string(),
                description: "Contract deployment requests an unusually high gas limit that could be used for attacks".to_string(),
                severity: Severity::Medium,
                category: VulnerabilityCategory::GasGriefing,
                location: Some(deployment.address.clone()),
                evidence: vec![
                    format!("Requested gas limit: {}", deployment.gas_limit),
                    format!("Threshold: {}", self.thresholds.max_single_operation_gas),
                ],
                recommendations: vec![
                    "Review gas requirements and optimize contract code".to_string(),
                    "Use reasonable gas limits to prevent griefing".to_string(),
                    "Consider breaking large operations into smaller batches".to_string(),
                ],
                gas_impact: Some(deployment.gas_limit),
            });
        }

        // Check for potential gas exhaustion setup
        if deployment.gas_limit > self.thresholds.exhaustion_threshold {
            findings.push(SecurityFinding {
                id: format!("GAS-EXHAUSTION-{}", self.generate_id()),
                title: "Potential Gas Exhaustion Vector".to_string(),
                description: "High gas limit could be used to exhaust network resources"
                    .to_string(),
                severity: Severity::High,
                category: VulnerabilityCategory::DoS,
                location: Some(deployment.address.clone()),
                evidence: vec![
                    format!("Gas limit: {}", deployment.gas_limit),
                    "Could be used for network DoS".to_string(),
                ],
                recommendations: vec![
                    "Implement stricter gas limits".to_string(),
                    "Add rate limiting for high-gas operations".to_string(),
                ],
                gas_impact: Some(deployment.gas_limit),
            });
        }

        findings
    }

    /// Analyze complexity patterns that could lead to gas attacks
    async fn analyze_complexity_patterns(
        &mut self,
        deployment: &ContractDeployment,
    ) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        // Estimate complexity based on contract size and patterns
        let complexity = self.estimate_contract_complexity(deployment);
        let complexity_score = self.calculate_complexity_score(&complexity);

        if complexity_score > self.thresholds.max_complexity_score {
            findings.push(SecurityFinding {
                id: format!("GAS-COMPLEXITY-{}", self.generate_id()),
                title: "High Computational Complexity".to_string(),
                description:
                    "Contract exhibits high computational complexity that could lead to gas attacks"
                        .to_string(),
                severity: Severity::Medium,
                category: VulnerabilityCategory::GasGriefing,
                location: Some(deployment.address.clone()),
                evidence: vec![
                    format!("Complexity score: {}", complexity_score),
                    format!(
                        "Storage operations: {}",
                        complexity.storage_reads + complexity.storage_writes
                    ),
                    format!(
                        "Computational intensity: {}",
                        complexity.computational_intensity
                    ),
                ],
                recommendations: vec![
                    "Optimize computational complexity".to_string(),
                    "Consider breaking operations into smaller chunks".to_string(),
                    "Use gas-efficient algorithms".to_string(),
                ],
                gas_impact: Some(
                    self.gas_optimizer
                        .estimate_gas_cost("deployment", &complexity),
                ),
            });
        }

        findings
    }

    /// Detect gas griefing patterns in bytecode
    fn detect_griefing_patterns(&self, deployment: &ContractDeployment) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        // Analyze bytecode for griefing patterns
        let griefing_indicators = self.analyze_bytecode_for_griefing(&deployment.code);

        if griefing_indicators.potential_infinite_loops {
            findings.push(SecurityFinding {
                id: format!("GAS-GRIEF-LOOP-{}", self.generate_id()),
                title: "Potential Infinite Loop Gas Griefing".to_string(),
                description:
                    "Contract contains patterns that could create infinite loops for gas griefing"
                        .to_string(),
                severity: Severity::High,
                category: VulnerabilityCategory::GasGriefing,
                location: Some(deployment.address.clone()),
                evidence: vec![
                    "Loop patterns without proper bounds detected".to_string(),
                    format!("Loop instruction count: {}", griefing_indicators.loop_count),
                ],
                recommendations: vec![
                    "Add proper loop bounds and gas checks".to_string(),
                    "Implement circuit breakers for long-running operations".to_string(),
                ],
                gas_impact: None,
            });
        }

        if griefing_indicators.excessive_storage_ops {
            findings.push(SecurityFinding {
                id: format!("GAS-GRIEF-STORAGE-{}", self.generate_id()),
                title: "Excessive Storage Operations".to_string(),
                description: "Contract performs excessive storage operations that could be used for gas griefing".to_string(),
                severity: Severity::Medium,
                category: VulnerabilityCategory::GasGriefing,
                location: Some(deployment.address.clone()),
                evidence: vec![
                    format!("Storage operation count: {}", griefing_indicators.storage_op_count),
                    "Could lead to high gas consumption".to_string(),
                ],
                recommendations: vec![
                    "Optimize storage access patterns".to_string(),
                    "Use batch operations where possible".to_string(),
                    "Implement storage access limits".to_string(),
                ],
                gas_impact: Some(griefing_indicators.storage_op_count as u64 * 5000u64), // Estimated gas per storage op
            });
        }

        findings
    }

    /// Detect gas griefing in execution
    fn detect_gas_griefing(
        &self,
        call: &ContractCall,
        result: &ExecutionResult,
    ) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        let efficiency = if call.gas_limit > 0 {
            result.gas_used as f64 / call.gas_limit as f64
        } else {
            0.0
        };

        // Check for suspiciously low gas efficiency
        if efficiency < self.thresholds.min_efficiency && result.gas_used >= 100_000 {
            findings.push(SecurityFinding {
                id: format!("GAS-GRIEF-EXEC-{}", self.generate_id()),
                title: "Gas Griefing During Execution".to_string(),
                description:
                    "Contract execution shows patterns consistent with gas griefing attacks"
                        .to_string(),
                severity: Severity::High,
                category: VulnerabilityCategory::GasGriefing,
                location: Some(format!("{}::{}", call.contract_address, call.method)),
                evidence: vec![
                    format!("Gas efficiency: {:.2}%", efficiency * 100.0),
                    format!("Gas used: {} / {}", result.gas_used, call.gas_limit),
                    format!("Caller: {}", call.caller),
                ],
                recommendations: vec![
                    "Investigate gas usage patterns".to_string(),
                    "Implement gas usage monitoring".to_string(),
                    "Consider rate limiting for this caller".to_string(),
                ],
                gas_impact: Some(result.gas_used),
            });
        }

        findings
    }

    /// Detect DoS through gas exhaustion
    fn detect_gas_exhaustion_dos(
        &self,
        call: &ContractCall,
        result: &ExecutionResult,
    ) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        // Check for patterns that could exhaust gas
        if result.gas_used > self.thresholds.exhaustion_threshold {
            findings.push(SecurityFinding {
                id: format!("GAS-DOS-EXHAUST-{}", self.generate_id()),
                title: "Gas Exhaustion DoS Attack".to_string(),
                description: "Execution consumed excessive gas that could be used for DoS attacks"
                    .to_string(),
                severity: Severity::Critical,
                category: VulnerabilityCategory::DoS,
                location: Some(format!("{}::{}", call.contract_address, call.method)),
                evidence: vec![
                    format!("Gas consumed: {}", result.gas_used),
                    format!("Gas remaining: {}", result.gas_remaining),
                    "Could exhaust network resources".to_string(),
                ],
                recommendations: vec![
                    "Implement gas usage limits".to_string(),
                    "Add circuit breakers for high-gas operations".to_string(),
                    "Monitor and rate limit this contract".to_string(),
                ],
                gas_impact: Some(result.gas_used),
            });
        }

        findings
    }

    /// Detect gas limit manipulation
    fn detect_gas_manipulation(
        &self,
        call: &ContractCall,
        result: &ExecutionResult,
    ) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        // Look for patterns where gas limit is set very high but actual usage is low
        // This could indicate manipulation attempts
        let gas_ratio = result.gas_used as f64 / call.gas_limit as f64;

        if call.gas_limit > 1_000_000 && gas_ratio < 0.1 {
            findings.push(SecurityFinding {
                id: format!("GAS-MANIPULATION-{}", self.generate_id()),
                title: "Potential Gas Limit Manipulation".to_string(),
                description:
                    "Unusually high gas limit with low actual usage suggests potential manipulation"
                        .to_string(),
                severity: Severity::Medium,
                category: VulnerabilityCategory::GasGriefing,
                location: Some(format!("{}::{}", call.contract_address, call.method)),
                evidence: vec![
                    format!("Gas limit: {}", call.gas_limit),
                    format!("Gas used: {}", result.gas_used),
                    format!("Usage ratio: {:.2}%", gas_ratio * 100.0),
                ],
                recommendations: vec![
                    "Implement dynamic gas limit calculation".to_string(),
                    "Monitor gas usage patterns".to_string(),
                    "Use gas estimation before execution".to_string(),
                ],
                gas_impact: Some(call.gas_limit - result.gas_used),
            });
        }

        findings
    }

    /// Analyze gas usage patterns over time
    async fn analyze_gas_usage_patterns(
        &self,
        call: &ContractCall,
        result: &ExecutionResult,
    ) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        // Analyze historical patterns for this contract/method
        let recent_executions: Vec<_> = self
            .execution_history
            .iter()
            .filter(|record| {
                record.contract_address == call.contract_address
                    && record.method == call.method
                    && call.timestamp - record.timestamp < 3600 // Last hour
            })
            .collect();

        if recent_executions.len() > 10 {
            // Calculate average gas usage
            let avg_gas: f64 = recent_executions
                .iter()
                .map(|r| r.actual_gas as f64)
                .sum::<f64>()
                / recent_executions.len() as f64;

            // Check for sudden spikes
            if result.gas_used as f64 > avg_gas * self.thresholds.dos_spike_multiplier {
                findings.push(SecurityFinding {
                    id: format!("GAS-SPIKE-DOS-{}", self.generate_id()),
                    title: "Gas Usage Spike DoS Attack".to_string(),
                    description: "Sudden spike in gas usage could indicate DoS attack attempt"
                        .to_string(),
                    severity: Severity::High,
                    category: VulnerabilityCategory::DoS,
                    location: Some(format!("{}::{}", call.contract_address, call.method)),
                    evidence: vec![
                        format!("Current gas usage: {}", result.gas_used),
                        format!("Average gas usage: {:.0}", avg_gas),
                        format!("Spike ratio: {:.2}x", result.gas_used as f64 / avg_gas),
                    ],
                    recommendations: vec![
                        "Investigate cause of gas spike".to_string(),
                        "Implement gas usage monitoring and alerts".to_string(),
                        "Consider rate limiting".to_string(),
                    ],
                    gas_impact: Some(result.gas_used),
                });
            }
        }

        findings
    }

    /// Estimate contract complexity based on deployment
    fn estimate_contract_complexity(&self, deployment: &ContractDeployment) -> OperationComplexity {
        // This is a simplified estimation - real implementation would parse WASM more thoroughly
        let code_size = deployment.code.len();
        let state_size = deployment.initial_state.len();

        // Estimate based on size and patterns
        OperationComplexity {
            storage_reads: (code_size / 1000) as u32,
            storage_writes: (state_size / 500) as u32,
            key_size_bytes: 64,
            value_size_bytes: (state_size / 10) as u32,
            memory_allocations: (code_size / 2000) as u32,
            validation_checks: (code_size / 1500) as u32,
            cryptographic_operations: (code_size / 5000) as u32,
            network_calls: 0, // Estimated separately
            computational_intensity: std::cmp::min(10, (code_size / 10000) as u32 + 1),
        }
    }

    /// Calculate complexity score from operation complexity
    fn calculate_complexity_score(&self, complexity: &OperationComplexity) -> u32 {
        let storage_score = (complexity.storage_reads + complexity.storage_writes) * 2;
        let memory_score = complexity.memory_allocations * 3;
        let validation_score = complexity.validation_checks * 2;
        let crypto_score = complexity.cryptographic_operations * 5;
        let intensity_score = complexity.computational_intensity * 10;

        storage_score + memory_score + validation_score + crypto_score + intensity_score
    }

    /// Analyze bytecode for gas griefing patterns
    fn analyze_bytecode_for_griefing(&self, bytecode: &[u8]) -> GriefingIndicators {
        let mut indicators = GriefingIndicators::default();

        if bytecode.len() < 8 {
            return indicators;
        }

        // Count different instruction types
        for window in bytecode.windows(2) {
            match window {
                // Loop-related instructions
                [0x02, _] | [0x03, _] => indicators.loop_count += 1,
                // Storage operations
                [0x28, _] | [0x29, _] | [0x36, _] | [0x37, _] => indicators.storage_op_count += 1,
                // Conditional branches
                [0x04, _] | [0x05, _] | [0x0d, _] | [0x0e, _] => indicators.branch_count += 1,
                _ => {}
            }
        }

        // Heuristics for griefing patterns
        indicators.potential_infinite_loops = indicators.loop_count > 5
            && (indicators.branch_count as f64 / indicators.loop_count as f64) < 0.3;

        indicators.excessive_storage_ops = indicators.storage_op_count > 100;

        indicators
    }

    fn generate_id(&self) -> String {
        format!(
            "{:08x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u32
        )
    }

    pub fn get_attack_count(&self) -> u64 {
        self.attack_count
    }

    /// Get gas optimization recommendations
    pub fn get_gas_optimization_recommendations(&self) -> Vec<SecurityFinding> {
        let mut recommendations = Vec::new();
        let stats = self.gas_optimizer.get_gas_statistics();

        if stats.average_efficiency < 0.8 {
            recommendations.push(SecurityFinding {
                id: format!("GAS-OPT-{}", self.generate_id()),
                title: "Gas Efficiency Optimization Needed".to_string(),
                description: "Overall gas efficiency is below optimal levels".to_string(),
                severity: Severity::Low,
                category: VulnerabilityCategory::GasOptimization,
                location: None,
                evidence: vec![
                    format!(
                        "Average efficiency: {:.2}%",
                        stats.average_efficiency * 100.0
                    ),
                    format!("Total operations: {}", stats.total_operations),
                ],
                recommendations: vec![
                    "Review gas optimization strategies".to_string(),
                    "Implement batch operations where possible".to_string(),
                    "Use gas-efficient data structures".to_string(),
                ],
                gas_impact: Some(stats.total_estimated_gas - stats.total_actual_gas),
            });
        }

        recommendations
    }
}

/// Indicators of potential gas griefing patterns
#[derive(Debug, Default)]
struct GriefingIndicators {
    loop_count: u32,
    storage_op_count: u32,
    branch_count: u32,
    potential_infinite_loops: bool,
    excessive_storage_ops: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{ContractCall, ContractDeployment, ExecutionResult};

    #[test]
    fn test_gas_analyzer_creation() {
        let analyzer = GasAttackAnalyzer::new();
        assert_eq!(analyzer.get_attack_count(), 0);
    }

    #[test]
    fn test_excessive_gas_limit_detection() {
        let analyzer = GasAttackAnalyzer::new();

        let deployment = ContractDeployment {
            address: "test".to_string(),
            code: b"\x00asm\x01\x00\x00\x00".to_vec(),
            initial_state: vec![],
            gas_limit: 10_000_000, // Excessive gas limit
            deployer: "deployer".to_string(),
            timestamp: 0,
            ai_audit_score: Some(0.8),
        };

        let findings = analyzer.analyze_gas_limits(&deployment);
        assert!(!findings.is_empty());
        assert!(findings
            .iter()
            .any(|f| f.category == VulnerabilityCategory::GasGriefing));
    }

    #[tokio::test]
    async fn test_gas_griefing_detection() {
        let mut analyzer = GasAttackAnalyzer::new();

        let call = ContractCall {
            contract_address: "test".to_string(),
            caller: "caller".to_string(),
            method: "transfer".to_string(),
            input_data: vec![],
            gas_limit: 1_000_000,
            value: 0,
            timestamp: 0,
        };

        let result = ExecutionResult {
            success: true,
            return_data: vec![],
            gas_used: 100_000, // Very low efficiency
            gas_remaining: 900_000,
            state_changes: vec![],
            events: vec![],
            ai_analysis: None,
        };

        let findings = analyzer.analyze_execution(&call, &result).await;
        assert!(findings.iter().any(|f| f.title.contains("Gas Griefing")));
    }

    #[test]
    fn test_complexity_calculation() {
        let analyzer = GasAttackAnalyzer::new();

        let complexity = OperationComplexity {
            storage_reads: 10,
            storage_writes: 5,
            key_size_bytes: 64,
            value_size_bytes: 200,
            memory_allocations: 3,
            validation_checks: 8,
            cryptographic_operations: 2,
            network_calls: 0,
            computational_intensity: 5,
        };

        let score = analyzer.calculate_complexity_score(&complexity);
        assert!(score > 0);

        // Higher complexity should give higher score
        let high_complexity = OperationComplexity {
            storage_reads: 100,
            storage_writes: 50,
            key_size_bytes: 64,
            value_size_bytes: 200,
            memory_allocations: 30,
            validation_checks: 80,
            cryptographic_operations: 20,
            network_calls: 0,
            computational_intensity: 10,
        };

        let high_score = analyzer.calculate_complexity_score(&high_complexity);
        assert!(high_score > score);
    }

    #[test]
    fn test_griefing_pattern_analysis() {
        let analyzer = GasAttackAnalyzer::new();

        // Create bytecode with many loop instructions
        let bytecode_with_loops = vec![
            0x00, 0x61, 0x73, 0x6d, // WASM magic
            0x01, 0x00, 0x00, 0x00, // WASM version
            0x02, 0x00, 0x03, 0x01, // Many loop instructions
            0x02, 0x00, 0x03, 0x01, 0x02, 0x00, 0x03, 0x01, 0x02, 0x00, 0x03, 0x01, 0x02, 0x00,
            0x03, 0x01, 0x02, 0x00, 0x03, 0x01,
        ];

        let indicators = analyzer.analyze_bytecode_for_griefing(&bytecode_with_loops);
        assert!(indicators.loop_count > 0);
    }
}
