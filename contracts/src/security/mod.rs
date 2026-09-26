//! Smart Contract Security Auditing Module
//!
//! This module provides comprehensive security auditing capabilities for WASM-based
//! smart contracts, including vulnerability detection, automated testing, and report generation.

pub mod audit_report;
pub mod fuzz_tester;
pub mod gas_attack_analyzer;
pub mod vulnerability_scanner;

pub use audit_report::*;
pub use fuzz_tester::*;
pub use gas_attack_analyzer::*;
pub use vulnerability_scanner::*;

use crate::runtime::{ContractCall, ContractDeployment, ExecutionResult};
use serde::{Deserialize, Serialize};

/// Security audit severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Critical => "Critical",
            Severity::High => "High",
            Severity::Medium => "Medium",
            Severity::Low => "Low",
        }
    }

    pub fn priority_score(&self) -> u32 {
        match self {
            Severity::Critical => 100,
            Severity::High => 75,
            Severity::Medium => 50,
            Severity::Low => 25,
        }
    }
}

/// Represents a security finding from the audit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityFinding {
    pub id: String,
    pub title: String,
    pub description: String,
    pub severity: Severity,
    pub category: VulnerabilityCategory,
    pub location: Option<String>,
    pub evidence: Vec<String>,
    pub recommendations: Vec<String>,
    pub gas_impact: Option<u64>,
}

/// Categories of vulnerabilities that can be detected
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VulnerabilityCategory {
    Reentrancy,
    IntegerOverflow,
    AccessControl,
    UncheckedExternalCall,
    GasGriefing,
    StateManipulation,
    DoS,
    LogicFlaws,
    GasOptimization,
}

impl VulnerabilityCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            VulnerabilityCategory::Reentrancy => "Reentrancy",
            VulnerabilityCategory::IntegerOverflow => "Integer Overflow/Underflow",
            VulnerabilityCategory::AccessControl => "Access Control",
            VulnerabilityCategory::UncheckedExternalCall => "Unchecked External Call",
            VulnerabilityCategory::GasGriefing => "Gas Griefing",
            VulnerabilityCategory::StateManipulation => "State Manipulation",
            VulnerabilityCategory::DoS => "Denial of Service",
            VulnerabilityCategory::LogicFlaws => "Logic Flaws",
            VulnerabilityCategory::GasOptimization => "Gas Optimization",
        }
    }
}

/// Main security auditor struct that coordinates all security analysis
pub struct SecurityAuditor {
    vulnerability_scanner: VulnerabilityScanner,
    gas_analyzer: GasAttackAnalyzer,
    fuzz_tester: FuzzTester,
}

impl SecurityAuditor {
    /// Create a new security auditor with default configuration
    pub fn new() -> Self {
        Self {
            vulnerability_scanner: VulnerabilityScanner::new(),
            gas_analyzer: GasAttackAnalyzer::new(),
            fuzz_tester: FuzzTester::new(),
        }
    }
}

impl Default for SecurityAuditor {
    fn default() -> Self {
        Self::new()
    }
}

impl SecurityAuditor {
    /// Perform a comprehensive security audit of a smart contract
    pub async fn audit_contract(&mut self, deployment: &ContractDeployment) -> SecurityAuditResult {
        let mut findings = Vec::new();
        let start_time = std::time::Instant::now();

        // 1. Static analysis for vulnerabilities
        let vuln_findings = self.vulnerability_scanner.scan_deployment(deployment);
        findings.extend(vuln_findings);

        // 2. Gas attack vector analysis
        let gas_findings = self.gas_analyzer.analyze_deployment(deployment).await;
        findings.extend(gas_findings);

        // 3. Fuzz testing for edge cases
        let fuzz_findings = self.fuzz_tester.test_deployment(deployment).await;
        findings.extend(fuzz_findings);

        let audit_duration = start_time.elapsed();

        SecurityAuditResult {
            contract_address: deployment.address.clone(),
            findings,
            audit_duration_ms: audit_duration.as_millis() as u64,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            auditor_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Perform runtime security analysis during contract execution
    pub async fn analyze_execution(
        &mut self,
        call: &ContractCall,
        result: &ExecutionResult,
    ) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        // Analyze gas usage patterns for attacks
        let gas_findings = self.gas_analyzer.analyze_execution(call, result).await;
        findings.extend(gas_findings);

        // Check for reentrancy patterns in execution
        let reentrancy_findings = self
            .vulnerability_scanner
            .check_reentrancy_execution(call, result);
        findings.extend(reentrancy_findings);

        findings
    }

    /// Get security statistics for the auditor
    pub fn get_statistics(&self) -> SecurityAuditorStats {
        SecurityAuditorStats {
            contracts_audited: self.vulnerability_scanner.get_scan_count(),
            vulnerabilities_found: self.vulnerability_scanner.get_total_findings(),
            gas_attacks_detected: self.gas_analyzer.get_attack_count(),
            fuzz_tests_run: self.fuzz_tester.get_test_count(),
        }
    }
}

/// Result of a complete security audit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityAuditResult {
    pub contract_address: String,
    pub findings: Vec<SecurityFinding>,
    pub audit_duration_ms: u64,
    pub timestamp: u64,
    pub auditor_version: String,
}

impl SecurityAuditResult {
    /// Get findings by severity level
    pub fn findings_by_severity(&self, severity: Severity) -> Vec<&SecurityFinding> {
        self.findings
            .iter()
            .filter(|f| f.severity == severity)
            .collect()
    }

    /// Get the highest severity level found
    pub fn max_severity(&self) -> Option<Severity> {
        self.findings
            .iter()
            .map(|f| f.severity)
            .max_by_key(|s| s.priority_score())
    }

    /// Calculate overall security score (0-100, higher is better)
    pub fn security_score(&self) -> u32 {
        if self.findings.is_empty() {
            return 100;
        }

        let total_impact: u32 = self
            .findings
            .iter()
            .map(|f| 100 - f.severity.priority_score())
            .sum();

        let max_possible_impact = self.findings.len() as u32 * 100;
        if max_possible_impact == 0 {
            100
        } else {
            ((max_possible_impact - total_impact) * 100) / max_possible_impact
        }
    }

    /// Check if the contract passes basic security requirements
    pub fn passes_security_check(&self) -> bool {
        !self
            .findings
            .iter()
            .any(|f| matches!(f.severity, Severity::Critical))
    }
}

/// Statistics for the security auditor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityAuditorStats {
    pub contracts_audited: u64,
    pub vulnerabilities_found: u64,
    pub gas_attacks_detected: u64,
    pub fuzz_tests_run: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Critical.priority_score() > Severity::High.priority_score());
        assert!(Severity::High.priority_score() > Severity::Medium.priority_score());
        assert!(Severity::Medium.priority_score() > Severity::Low.priority_score());
    }

    #[test]
    fn test_security_score_calculation() {
        let mut result = SecurityAuditResult {
            contract_address: "test".to_string(),
            findings: vec![],
            audit_duration_ms: 100,
            timestamp: 0,
            auditor_version: "test".to_string(),
        };

        // Perfect score with no findings
        assert_eq!(result.security_score(), 100);

        // Add a medium severity finding
        result.findings.push(SecurityFinding {
            id: "test-001".to_string(),
            title: "Test Finding".to_string(),
            description: "Test".to_string(),
            severity: Severity::Medium,
            category: VulnerabilityCategory::LogicFlaws,
            location: None,
            evidence: vec![],
            recommendations: vec![],
            gas_impact: None,
        });

        assert!(result.security_score() < 100);
        assert!(result.passes_security_check()); // Medium is not critical

        // Add a critical finding
        result.findings.push(SecurityFinding {
            id: "test-002".to_string(),
            title: "Critical Finding".to_string(),
            description: "Test".to_string(),
            severity: Severity::Critical,
            category: VulnerabilityCategory::Reentrancy,
            location: None,
            evidence: vec![],
            recommendations: vec![],
            gas_impact: None,
        });

        assert!(!result.passes_security_check()); // Critical finding fails check
        assert_eq!(result.max_severity(), Some(Severity::Critical));
    }

    #[tokio::test]
    async fn test_auditor_creation() {
        let auditor = SecurityAuditor::new();
        let stats = auditor.get_statistics();

        assert_eq!(stats.contracts_audited, 0);
        assert_eq!(stats.vulnerabilities_found, 0);
    }
}
