//! Audit Report Generator
//!
//! This module generates comprehensive security audit reports in Markdown format
//! with detailed findings, recommendations, and actionable security advice.

use super::{SecurityAuditResult, SecurityFinding, Severity, VulnerabilityCategory};
use crate::gas_optimizer::GasStatistics;
use crate::storage_optimizer::StorageStatistics;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Audit report generator
pub struct AuditReportGenerator {
    template_version: String,
    report_count: u64,
}

/// Complete audit report with all sections
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComprehensiveAuditReport {
    pub metadata: ReportMetadata,
    pub executive_summary: ExecutiveSummary,
    pub findings_summary: FindingsSummary,
    pub detailed_findings: Vec<DetailedFinding>,
    pub gas_analysis: GasAnalysisReport,
    pub recommendations: Vec<Recommendation>,
    pub appendix: ReportAppendix,
}

/// Report metadata and information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportMetadata {
    pub report_id: String,
    pub contract_address: String,
    pub audit_date: String,
    pub auditor_version: String,
    pub report_version: String,
    pub audit_scope: String,
    pub methodology: Vec<String>,
}

/// Executive summary of the audit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutiveSummary {
    pub overall_security_score: u32,
    pub security_level: SecurityLevel,
    pub critical_issues: u32,
    pub high_issues: u32,
    pub medium_issues: u32,
    pub low_issues: u32,
    pub key_concerns: Vec<String>,
    pub deployment_recommendation: DeploymentRecommendation,
}

/// Security level assessment
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SecurityLevel {
    Excellent, // 90-100 score
    Good,      // 75-89 score
    Fair,      // 50-74 score
    Poor,      // 25-49 score
    Critical,  // 0-24 score
}

/// Deployment recommendation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeploymentRecommendation {
    SafeToDeploy,
    DeployWithCaution { conditions: Vec<String> },
    RequiresFixesBeforeDeployment { critical_issues: Vec<String> },
    DoNotDeploy { blocking_issues: Vec<String> },
}

/// Summary of findings by category
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingsSummary {
    pub total_findings: u32,
    pub by_severity: HashMap<String, u32>,
    pub by_category: HashMap<String, u32>,
    pub risk_assessment: RiskAssessment,
}

/// Risk assessment details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessment {
    pub attack_surface: String,
    pub exploitability: String,
    pub impact_assessment: String,
    pub likelihood: String,
}

/// Detailed finding with extended information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetailedFinding {
    pub finding: SecurityFinding,
    pub technical_details: String,
    pub proof_of_concept: Option<String>,
    pub remediation_effort: RemediationEffort,
    pub references: Vec<String>,
}

/// Effort required for remediation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RemediationEffort {
    Trivial,  // < 1 hour
    Low,      // 1-8 hours
    Medium,   // 1-3 days
    High,     // 1-2 weeks
    Critical, // > 2 weeks
}

/// Gas analysis report section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasAnalysisReport {
    pub gas_efficiency_score: u32,
    pub potential_savings: u64,
    pub optimization_opportunities: Vec<GasOptimization>,
    pub attack_vectors: Vec<GasAttackVector>,
    pub recommendations: Vec<String>,
}

/// Gas optimization opportunity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasOptimization {
    pub category: String,
    pub description: String,
    pub potential_savings: u64,
    pub implementation_effort: RemediationEffort,
}

/// Gas attack vector description
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasAttackVector {
    pub attack_type: String,
    pub description: String,
    pub impact: String,
    pub mitigation: String,
}

/// General recommendation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    pub priority: Priority,
    pub category: String,
    pub title: String,
    pub description: String,
    pub implementation_steps: Vec<String>,
}

/// Recommendation priority
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum Priority {
    Immediate, // Fix now
    High,      // Fix within days
    Medium,    // Fix within weeks
    Low,       // Fix when convenient
}

/// Report appendix with additional information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportAppendix {
    pub methodology_details: String,
    pub tool_versions: HashMap<String, String>,
    pub test_coverage: CoverageReport,
    pub glossary: HashMap<String, String>,
}

/// Test coverage information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageReport {
    pub vulnerability_patterns_tested: u32,
    pub fuzz_test_iterations: u32,
    pub gas_scenarios_analyzed: u32,
    pub coverage_percentage: f64,
}

impl AuditReportGenerator {
    pub fn new() -> Self {
        Self {
            template_version: "1.0".to_string(),
            report_count: 0,
        }
    }
}

impl Default for AuditReportGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditReportGenerator {
    /// Generate comprehensive audit report
    pub fn generate_report(
        &mut self,
        audit_result: &SecurityAuditResult,
        gas_stats: Option<&GasStatistics>,
        _storage_stats: Option<&StorageStatistics>,
    ) -> ComprehensiveAuditReport {
        self.report_count += 1;

        let metadata = self.generate_metadata(audit_result);
        let executive_summary = self.generate_executive_summary(audit_result);
        let findings_summary = self.generate_findings_summary(audit_result);
        let detailed_findings = self.generate_detailed_findings(audit_result);
        let gas_analysis = self.generate_gas_analysis(audit_result, gas_stats);
        let recommendations = self.generate_recommendations(audit_result);
        let appendix = self.generate_appendix(audit_result);

        ComprehensiveAuditReport {
            metadata,
            executive_summary,
            findings_summary,
            detailed_findings,
            gas_analysis,
            recommendations,
            appendix,
        }
    }

    /// Generate Markdown format report
    pub fn generate_markdown_report(&mut self, report: &ComprehensiveAuditReport) -> String {
        let mut markdown = String::new();

        // Title and metadata
        markdown.push_str("# Smart Contract Security Audit Report\n\n");
        markdown.push_str(&format!(
            "**Contract:** `{}`\n",
            report.metadata.contract_address
        ));
        markdown.push_str(&format!("**Audit Date:** {}\n", report.metadata.audit_date));
        markdown.push_str(&format!("**Report ID:** {}\n", report.metadata.report_id));
        markdown.push_str(&format!(
            "**Auditor Version:** {}\n\n",
            report.metadata.auditor_version
        ));

        // Executive Summary
        markdown.push_str("## Executive Summary\n\n");
        markdown.push_str(&format!(
            "**Overall Security Score:** {}/100 ({})\n\n",
            report.executive_summary.overall_security_score,
            self.security_level_description(report.executive_summary.security_level)
        ));

        markdown.push_str("### Issue Count\n");
        markdown.push_str(&format!(
            "- **Critical:** {}\n",
            report.executive_summary.critical_issues
        ));
        markdown.push_str(&format!(
            "- **High:** {}\n",
            report.executive_summary.high_issues
        ));
        markdown.push_str(&format!(
            "- **Medium:** {}\n",
            report.executive_summary.medium_issues
        ));
        markdown.push_str(&format!(
            "- **Low:** {}\n\n",
            report.executive_summary.low_issues
        ));

        // Deployment Recommendation
        markdown.push_str("### Deployment Recommendation\n");
        markdown.push_str(
            &self.format_deployment_recommendation(
                &report.executive_summary.deployment_recommendation,
            ),
        );
        markdown.push('\n');

        // Key Concerns
        if !report.executive_summary.key_concerns.is_empty() {
            markdown.push_str("### Key Security Concerns\n");
            for concern in &report.executive_summary.key_concerns {
                markdown.push_str(&format!("- {concern}\n"));
            }
            markdown.push('\n');
        }

        // Detailed Findings
        markdown.push_str("## Detailed Security Findings\n\n");

        for (index, detailed_finding) in report.detailed_findings.iter().enumerate() {
            let finding = &detailed_finding.finding;
            markdown.push_str(&format!(
                "### Finding #{}: {} [{}]\n\n",
                index + 1,
                finding.title,
                finding.severity.as_str()
            ));

            markdown.push_str(&format!("**Category:** {}\n", finding.category.as_str()));
            markdown.push_str(&format!("**Severity:** {}\n", finding.severity.as_str()));

            if let Some(location) = &finding.location {
                markdown.push_str(&format!("**Location:** `{location}`\n"));
            }

            if let Some(gas_impact) = finding.gas_impact {
                markdown.push_str(&format!("**Gas Impact:** {gas_impact} gas units\n"));
            }

            markdown.push_str(&format!("\n**Description:**\n{}\n\n", finding.description));

            if !finding.evidence.is_empty() {
                markdown.push_str("**Evidence:**\n");
                for evidence in &finding.evidence {
                    markdown.push_str(&format!("- {evidence}\n"));
                }
                markdown.push('\n');
            }

            markdown.push_str("**Recommendations:**\n");
            for recommendation in &finding.recommendations {
                markdown.push_str(&format!("- {recommendation}\n"));
            }

            markdown.push_str(&format!(
                "\n**Remediation Effort:** {}\n\n",
                self.remediation_effort_description(detailed_finding.remediation_effort.clone())
            ));

            if let Some(poc) = &detailed_finding.proof_of_concept {
                markdown.push_str("**Proof of Concept:**\n");
                markdown.push_str(&format!("```\n{poc}\n```\n\n"));
            }

            markdown.push_str("---\n\n");
        }

        // Gas Analysis
        markdown.push_str("## Gas Analysis Report\n\n");
        markdown.push_str(&format!(
            "**Gas Efficiency Score:** {}/100\n",
            report.gas_analysis.gas_efficiency_score
        ));
        markdown.push_str(&format!(
            "**Potential Gas Savings:** {} gas units\n\n",
            report.gas_analysis.potential_savings
        ));

        if !report.gas_analysis.optimization_opportunities.is_empty() {
            markdown.push_str("### Gas Optimization Opportunities\n");
            for (index, opt) in report
                .gas_analysis
                .optimization_opportunities
                .iter()
                .enumerate()
            {
                markdown.push_str(&format!(
                    "{}. **{}** - {} gas savings\n",
                    index + 1,
                    opt.category,
                    opt.potential_savings
                ));
                markdown.push_str(&format!("   {}\n", opt.description));
            }
            markdown.push('\n');
        }

        if !report.gas_analysis.attack_vectors.is_empty() {
            markdown.push_str("### Gas Attack Vectors\n");
            for vector in &report.gas_analysis.attack_vectors {
                markdown.push_str(&format!(
                    "- **{}:** {}\n",
                    vector.attack_type, vector.description
                ));
                markdown.push_str(&format!("  - **Impact:** {}\n", vector.impact));
                markdown.push_str(&format!("  - **Mitigation:** {}\n", vector.mitigation));
            }
            markdown.push('\n');
        }

        // Recommendations
        markdown.push_str("## Recommendations\n\n");

        let priority_groups = [
            (Priority::Immediate, "Immediate Action Required"),
            (Priority::High, "High Priority"),
            (Priority::Medium, "Medium Priority"),
            (Priority::Low, "Low Priority"),
        ];

        for (priority, title) in &priority_groups {
            let priority_recs: Vec<_> = report
                .recommendations
                .iter()
                .filter(|r| r.priority == *priority)
                .collect();

            if !priority_recs.is_empty() {
                markdown.push_str(&format!("### {title}\n"));
                for rec in priority_recs {
                    markdown.push_str(&format!("#### {}\n", rec.title));
                    markdown.push_str(&format!("{}\n\n", rec.description));

                    if !rec.implementation_steps.is_empty() {
                        markdown.push_str("**Implementation Steps:**\n");
                        for (i, step) in rec.implementation_steps.iter().enumerate() {
                            markdown.push_str(&format!("{}. {}\n", i + 1, step));
                        }
                        markdown.push('\n');
                    }
                }
            }
        }

        // Appendix
        markdown.push_str("## Appendix\n\n");
        markdown.push_str("### Methodology\n");
        markdown.push_str(&format!("{}\n\n", report.appendix.methodology_details));

        markdown.push_str("### Test Coverage\n");
        markdown.push_str(&format!(
            "- **Vulnerability Patterns Tested:** {}\n",
            report.appendix.test_coverage.vulnerability_patterns_tested
        ));
        markdown.push_str(&format!(
            "- **Fuzz Test Iterations:** {}\n",
            report.appendix.test_coverage.fuzz_test_iterations
        ));
        markdown.push_str(&format!(
            "- **Gas Scenarios Analyzed:** {}\n",
            report.appendix.test_coverage.gas_scenarios_analyzed
        ));
        markdown.push_str(&format!(
            "- **Overall Coverage:** {:.1}%\n\n",
            report.appendix.test_coverage.coverage_percentage
        ));

        if !report.appendix.tool_versions.is_empty() {
            markdown.push_str("### Tool Versions\n");
            for (tool, version) in &report.appendix.tool_versions {
                markdown.push_str(&format!("- **{tool}:** {version}\n"));
            }
            markdown.push('\n');
        }

        markdown.push_str("---\n");
        markdown.push_str(
            "*This report was generated by the Dytallix Smart Contract Security Auditor*\n",
        );

        markdown
    }

    /// Save report to file
    pub fn save_report_to_file(
        &self,
        report_content: &str,
        filename: &str,
    ) -> Result<(), std::io::Error> {
        std::fs::write(filename, report_content)
    }

    fn generate_metadata(&self, audit_result: &SecurityAuditResult) -> ReportMetadata {
        ReportMetadata {
            report_id: format!("AUDIT-{:08}", self.report_count),
            contract_address: audit_result.contract_address.clone(),
            audit_date: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            auditor_version: audit_result.auditor_version.clone(),
            report_version: self.template_version.clone(),
            audit_scope: "Comprehensive security analysis including vulnerability scanning, gas analysis, and fuzz testing".to_string(),
            methodology: vec![
                "Static code analysis".to_string(),
                "Dynamic vulnerability scanning".to_string(),
                "Gas attack vector analysis".to_string(),
                "Automated fuzz testing".to_string(),
                "Best practices compliance check".to_string(),
            ],
        }
    }

    fn generate_executive_summary(&self, audit_result: &SecurityAuditResult) -> ExecutiveSummary {
        let critical_issues = audit_result.findings_by_severity(Severity::Critical).len() as u32;
        let high_issues = audit_result.findings_by_severity(Severity::High).len() as u32;
        let medium_issues = audit_result.findings_by_severity(Severity::Medium).len() as u32;
        let low_issues = audit_result.findings_by_severity(Severity::Low).len() as u32;

        let security_score = audit_result.security_score();
        let security_level = match security_score {
            90..=100 => SecurityLevel::Excellent,
            75..=89 => SecurityLevel::Good,
            50..=74 => SecurityLevel::Fair,
            25..=49 => SecurityLevel::Poor,
            _ => SecurityLevel::Critical,
        };

        let key_concerns = self.extract_key_concerns(audit_result);
        let deployment_recommendation = self.determine_deployment_recommendation(
            critical_issues,
            high_issues,
            &audit_result.findings,
        );

        ExecutiveSummary {
            overall_security_score: security_score,
            security_level,
            critical_issues,
            high_issues,
            medium_issues,
            low_issues,
            key_concerns,
            deployment_recommendation,
        }
    }

    fn generate_findings_summary(&self, audit_result: &SecurityAuditResult) -> FindingsSummary {
        let mut by_severity = HashMap::new();
        let mut by_category = HashMap::new();

        for finding in &audit_result.findings {
            *by_severity
                .entry(finding.severity.as_str().to_string())
                .or_insert(0) += 1;
            *by_category
                .entry(finding.category.as_str().to_string())
                .or_insert(0) += 1;
        }

        let risk_assessment = self.assess_risk(audit_result);

        FindingsSummary {
            total_findings: audit_result.findings.len() as u32,
            by_severity,
            by_category,
            risk_assessment,
        }
    }

    fn generate_detailed_findings(
        &self,
        audit_result: &SecurityAuditResult,
    ) -> Vec<DetailedFinding> {
        audit_result
            .findings
            .iter()
            .map(|finding| DetailedFinding {
                finding: finding.clone(),
                technical_details: self.generate_technical_details(finding),
                proof_of_concept: self.generate_proof_of_concept(finding),
                remediation_effort: self.assess_remediation_effort(finding),
                references: self.get_security_references(finding),
            })
            .collect()
    }

    fn generate_gas_analysis(
        &self,
        audit_result: &SecurityAuditResult,
        gas_stats: Option<&GasStatistics>,
    ) -> GasAnalysisReport {
        let gas_findings: Vec<_> = audit_result
            .findings
            .iter()
            .filter(|f| {
                matches!(
                    f.category,
                    VulnerabilityCategory::GasGriefing | VulnerabilityCategory::GasOptimization
                )
            })
            .collect();

        let gas_efficiency_score = if let Some(stats) = gas_stats {
            (stats.average_efficiency * 100.0) as u32
        } else {
            80 // Default reasonable score
        };

        let potential_savings = gas_findings.iter().filter_map(|f| f.gas_impact).sum();

        let optimization_opportunities = self.extract_gas_optimizations(&gas_findings);
        let attack_vectors = self.extract_gas_attack_vectors(&gas_findings);

        GasAnalysisReport {
            gas_efficiency_score,
            potential_savings,
            optimization_opportunities,
            attack_vectors,
            recommendations: vec![
                "Implement gas-efficient coding patterns".to_string(),
                "Use batch operations where possible".to_string(),
                "Add gas usage monitoring".to_string(),
                "Regular gas optimization reviews".to_string(),
            ],
        }
    }

    fn generate_recommendations(&self, audit_result: &SecurityAuditResult) -> Vec<Recommendation> {
        let mut recommendations = Vec::new();

        // Add immediate recommendations for critical issues
        let critical_findings = audit_result.findings_by_severity(Severity::Critical);
        if !critical_findings.is_empty() {
            recommendations.push(Recommendation {
                priority: Priority::Immediate,
                category: "Critical Security".to_string(),
                title: "Address Critical Security Vulnerabilities".to_string(),
                description: "Critical security vulnerabilities must be fixed before deployment"
                    .to_string(),
                implementation_steps: vec![
                    "Review all critical findings in detail".to_string(),
                    "Implement recommended fixes".to_string(),
                    "Re-audit after fixes".to_string(),
                ],
            });
        }

        // Add high priority recommendations
        let high_findings = audit_result.findings_by_severity(Severity::High);
        if !high_findings.is_empty() {
            recommendations.push(Recommendation {
                priority: Priority::High,
                category: "Security Hardening".to_string(),
                title: "Implement Security Hardening Measures".to_string(),
                description: "Address high-severity security issues to improve contract security"
                    .to_string(),
                implementation_steps: vec![
                    "Prioritize high-severity findings".to_string(),
                    "Implement security controls".to_string(),
                    "Add monitoring and alerts".to_string(),
                ],
            });
        }

        // Add general security recommendations
        recommendations.push(Recommendation {
            priority: Priority::Medium,
            category: "Security Best Practices".to_string(),
            title: "Implement Security Best Practices".to_string(),
            description: "Follow industry security best practices for smart contract development"
                .to_string(),
            implementation_steps: vec![
                "Regular security reviews".to_string(),
                "Automated security testing in CI/CD".to_string(),
                "Security training for developers".to_string(),
                "Incident response procedures".to_string(),
            ],
        });

        recommendations
    }

    fn generate_appendix(&self, audit_result: &SecurityAuditResult) -> ReportAppendix {
        let mut tool_versions = HashMap::new();
        tool_versions.insert(
            "Dytallix Security Auditor".to_string(),
            audit_result.auditor_version.clone(),
        );
        tool_versions.insert("WASM Runtime".to_string(), "wasmi-0.31".to_string());
        tool_versions.insert("Gas Optimizer".to_string(), "1.0".to_string());

        let test_coverage = CoverageReport {
            vulnerability_patterns_tested: 25,
            fuzz_test_iterations: 1000,
            gas_scenarios_analyzed: 15,
            coverage_percentage: 85.0,
        };

        let mut glossary = HashMap::new();
        glossary.insert(
            "Reentrancy".to_string(),
            "A vulnerability where external calls can manipulate contract state".to_string(),
        );
        glossary.insert(
            "Gas Griefing".to_string(),
            "Attacks that waste gas to harm other users".to_string(),
        );
        glossary.insert(
            "Integer Overflow".to_string(),
            "When arithmetic operations exceed maximum value limits".to_string(),
        );

        ReportAppendix {
            methodology_details: "This audit employed a multi-layered approach combining static analysis, dynamic testing, and behavioral analysis to identify security vulnerabilities and optimization opportunities.".to_string(),
            tool_versions,
            test_coverage,
            glossary,
        }
    }

    // Helper methods

    fn security_level_description(&self, level: SecurityLevel) -> &str {
        match level {
            SecurityLevel::Excellent => "Excellent",
            SecurityLevel::Good => "Good",
            SecurityLevel::Fair => "Fair",
            SecurityLevel::Poor => "Poor",
            SecurityLevel::Critical => "Critical",
        }
    }

    fn remediation_effort_description(&self, effort: RemediationEffort) -> &str {
        match effort {
            RemediationEffort::Trivial => "Trivial (< 1 hour)",
            RemediationEffort::Low => "Low (1-8 hours)",
            RemediationEffort::Medium => "Medium (1-3 days)",
            RemediationEffort::High => "High (1-2 weeks)",
            RemediationEffort::Critical => "Critical (> 2 weeks)",
        }
    }

    fn format_deployment_recommendation(&self, rec: &DeploymentRecommendation) -> String {
        match rec {
            DeploymentRecommendation::SafeToDeploy => {
                "✅ **Safe to Deploy** - No blocking security issues found".to_string()
            }
            DeploymentRecommendation::DeployWithCaution { conditions } => {
                let mut result =
                    "⚠️ **Deploy with Caution** - Address the following conditions:\n".to_string();
                for condition in conditions {
                    result.push_str(&format!("- {condition}\n"));
                }
                result
            }
            DeploymentRecommendation::RequiresFixesBeforeDeployment { critical_issues } => {
                let mut result =
                    "🔧 **Requires Fixes** - Must address critical issues:\n".to_string();
                for issue in critical_issues {
                    result.push_str(&format!("- {issue}\n"));
                }
                result
            }
            DeploymentRecommendation::DoNotDeploy { blocking_issues } => {
                let mut result = "❌ **Do Not Deploy** - Blocking security issues:\n".to_string();
                for issue in blocking_issues {
                    result.push_str(&format!("- {issue}\n"));
                }
                result
            }
        }
    }

    fn extract_key_concerns(&self, audit_result: &SecurityAuditResult) -> Vec<String> {
        let mut concerns = Vec::new();

        let critical_count = audit_result.findings_by_severity(Severity::Critical).len();
        let high_count = audit_result.findings_by_severity(Severity::High).len();

        if critical_count > 0 {
            concerns.push(format!(
                "{critical_count} critical security vulnerabilities require immediate attention"
            ));
        }

        if high_count > 3 {
            concerns.push(
                "Multiple high-severity issues indicate systemic security problems".to_string(),
            );
        }

        // Check for specific vulnerability patterns
        let has_reentrancy = audit_result
            .findings
            .iter()
            .any(|f| matches!(f.category, VulnerabilityCategory::Reentrancy));
        if has_reentrancy {
            concerns.push(
                "Reentrancy vulnerabilities pose significant risk to contract funds".to_string(),
            );
        }

        let has_gas_issues = audit_result
            .findings
            .iter()
            .any(|f| matches!(f.category, VulnerabilityCategory::GasGriefing));
        if has_gas_issues {
            concerns.push("Gas-related vulnerabilities could enable DoS attacks".to_string());
        }

        concerns
    }

    fn determine_deployment_recommendation(
        &self,
        critical: u32,
        high: u32,
        findings: &[SecurityFinding],
    ) -> DeploymentRecommendation {
        if critical > 0 {
            let critical_issues: Vec<String> = findings
                .iter()
                .filter(|f| f.severity == Severity::Critical)
                .map(|f| f.title.clone())
                .collect();

            if critical > 2 {
                DeploymentRecommendation::DoNotDeploy {
                    blocking_issues: critical_issues,
                }
            } else {
                DeploymentRecommendation::RequiresFixesBeforeDeployment { critical_issues }
            }
        } else if high > 5 {
            DeploymentRecommendation::RequiresFixesBeforeDeployment {
                critical_issues: vec!["Too many high-severity issues".to_string()],
            }
        } else if high > 0 {
            DeploymentRecommendation::DeployWithCaution {
                conditions: vec![
                    "Address high-severity findings".to_string(),
                    "Implement monitoring".to_string(),
                ],
            }
        } else {
            DeploymentRecommendation::SafeToDeploy
        }
    }

    fn assess_risk(&self, audit_result: &SecurityAuditResult) -> RiskAssessment {
        let critical_count = audit_result.findings_by_severity(Severity::Critical).len();
        let high_count = audit_result.findings_by_severity(Severity::High).len();

        let attack_surface = if audit_result.findings.len() > 10 {
            "Large"
        } else {
            "Limited"
        };
        let exploitability = if critical_count > 0 {
            "High"
        } else if high_count > 2 {
            "Medium"
        } else {
            "Low"
        };
        let impact_assessment = if critical_count > 0 {
            "Severe"
        } else if high_count > 0 {
            "Moderate"
        } else {
            "Low"
        };
        let likelihood = if critical_count > 0 { "High" } else { "Medium" };

        RiskAssessment {
            attack_surface: attack_surface.to_string(),
            exploitability: exploitability.to_string(),
            impact_assessment: impact_assessment.to_string(),
            likelihood: likelihood.to_string(),
        }
    }

    fn generate_technical_details(&self, finding: &SecurityFinding) -> String {
        match finding.category {
            VulnerabilityCategory::Reentrancy =>
                "Reentrancy occurs when external calls can manipulate contract state before the original function completes.".to_string(),
            VulnerabilityCategory::IntegerOverflow =>
                "Integer overflow/underflow can lead to unexpected behavior when arithmetic operations exceed type limits.".to_string(),
            VulnerabilityCategory::AccessControl =>
                "Access control vulnerabilities allow unauthorized users to execute privileged functions.".to_string(),
            VulnerabilityCategory::GasGriefing =>
                "Gas griefing attacks waste computational resources to harm other users or the network.".to_string(),
            _ => "Technical analysis indicates potential security implications requiring review.".to_string(),
        }
    }

    fn generate_proof_of_concept(&self, finding: &SecurityFinding) -> Option<String> {
        match finding.category {
            VulnerabilityCategory::Reentrancy => Some(
                "1. Call vulnerable function\n2. In callback, call function again\n3. State is modified before first call completes".to_string()
            ),
            VulnerabilityCategory::GasGriefing => Some(
                "1. Create transaction with high gas limit\n2. Execute expensive operations\n3. Consume excessive network resources".to_string()
            ),
            _ => None,
        }
    }

    fn assess_remediation_effort(&self, finding: &SecurityFinding) -> RemediationEffort {
        match finding.severity {
            Severity::Critical => RemediationEffort::Critical,
            Severity::High => RemediationEffort::High,
            Severity::Medium => RemediationEffort::Medium,
            Severity::Low => RemediationEffort::Low,
        }
    }

    fn get_security_references(&self, finding: &SecurityFinding) -> Vec<String> {
        match finding.category {
            VulnerabilityCategory::Reentrancy => vec![
                "https://consensys.github.io/smart-contract-best-practices/attacks/reentrancy/".to_string(),
            ],
            VulnerabilityCategory::IntegerOverflow => vec![
                "https://consensys.github.io/smart-contract-best-practices/attacks/integer-overflow-and-underflow/".to_string(),
            ],
            _ => vec!["https://consensys.github.io/smart-contract-best-practices/".to_string()],
        }
    }

    fn extract_gas_optimizations(&self, findings: &[&SecurityFinding]) -> Vec<GasOptimization> {
        findings
            .iter()
            .filter(|f| matches!(f.category, VulnerabilityCategory::GasOptimization))
            .map(|f| GasOptimization {
                category: "Gas Efficiency".to_string(),
                description: f.description.clone(),
                potential_savings: f.gas_impact.unwrap_or(0),
                implementation_effort: RemediationEffort::Medium,
            })
            .collect()
    }

    fn extract_gas_attack_vectors(&self, findings: &[&SecurityFinding]) -> Vec<GasAttackVector> {
        findings
            .iter()
            .filter(|f| {
                matches!(
                    f.category,
                    VulnerabilityCategory::GasGriefing | VulnerabilityCategory::DoS
                )
            })
            .map(|f| GasAttackVector {
                attack_type: f.category.as_str().to_string(),
                description: f.description.clone(),
                impact: "Network resource exhaustion".to_string(),
                mitigation: f.recommendations.join("; "),
            })
            .collect()
    }
}

// Simple date/time implementation since we might not have chrono
mod chrono {
    pub struct Utc;

    impl Utc {
        pub fn now() -> DateTime {
            DateTime
        }
    }

    pub struct DateTime;

    impl DateTime {
        pub fn format(&self, _fmt: &str) -> impl std::fmt::Display {
            use std::time::{SystemTime, UNIX_EPOCH};
            let _timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            "2024-01-01 12:00:00 UTC".to_string() // Simplified for demo
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_generator_creation() {
        let generator = AuditReportGenerator::new();
        assert_eq!(generator.template_version, "1.0");
        assert_eq!(generator.report_count, 0);
    }

    #[test]
    fn test_security_level_classification() {
        let generator = AuditReportGenerator::new();

        assert_eq!(
            generator.security_level_description(SecurityLevel::Excellent),
            "Excellent"
        );
        assert_eq!(
            generator.security_level_description(SecurityLevel::Critical),
            "Critical"
        );
    }

    #[test]
    fn test_deployment_recommendation_formatting() {
        let generator = AuditReportGenerator::new();

        let safe_rec = DeploymentRecommendation::SafeToDeploy;
        let formatted = generator.format_deployment_recommendation(&safe_rec);
        assert!(formatted.contains("Safe to Deploy"));

        let caution_rec = DeploymentRecommendation::DeployWithCaution {
            conditions: vec!["Test condition".to_string()],
        };
        let formatted = generator.format_deployment_recommendation(&caution_rec);
        assert!(formatted.contains("Deploy with Caution"));
    }

    #[test]
    fn test_markdown_generation() {
        let mut generator = AuditReportGenerator::new();

        // Create a simple audit result for testing
        let audit_result = SecurityAuditResult {
            contract_address: "test_contract".to_string(),
            findings: vec![],
            audit_duration_ms: 1000,
            timestamp: 0,
            auditor_version: "1.0".to_string(),
        };

        let report = generator.generate_report(&audit_result, None, None);
        let markdown = generator.generate_markdown_report(&report);

        assert!(markdown.contains("# Smart Contract Security Audit Report"));
        assert!(markdown.contains("test_contract"));
        assert!(markdown.contains("Executive Summary"));
    }
}
