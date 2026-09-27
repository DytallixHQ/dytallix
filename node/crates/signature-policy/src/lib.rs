//! Signature Policy Module for PQC Enforcement
//!
//! This module defines configurable policies for signature algorithm validation,
//! ensuring only approved Post-Quantum Cryptography algorithms are accepted
//! across the transaction lifecycle.

use dytallix_protocol_types::signature_algorithm::SignatureAlgorithm;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

/// Policy enforcement error types
#[derive(Debug, Clone, PartialEq)]
pub enum PolicyError {
    /// Algorithm is not in the whitelist
    AlgorithmNotAllowed(SignatureAlgorithm),
    /// Legacy algorithms are explicitly rejected
    LegacyAlgorithmRejected(String),
    /// Unknown or invalid algorithm
    UnknownAlgorithm(String),
    /// Policy not configured
    PolicyNotConfigured,
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyError::AlgorithmNotAllowed(alg) => {
                write!(f, "Algorithm {alg:?} is not in the allowed list")
            }
            PolicyError::LegacyAlgorithmRejected(name) => {
                write!(f, "Legacy algorithm {name} is explicitly rejected")
            }
            PolicyError::UnknownAlgorithm(name) => {
                write!(f, "Unknown algorithm: {name}")
            }
            PolicyError::PolicyNotConfigured => {
                write!(f, "Signature policy not configured")
            }
        }
    }
}

impl std::error::Error for PolicyError {}

/// Signature policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignaturePolicy {
    /// Set of allowed PQC algorithms
    pub allowed_algorithms: HashSet<SignatureAlgorithm>,

    /// Whether to explicitly reject legacy algorithms (ECDSA, RSA, etc.)
    pub reject_legacy: bool,

    /// Whether to enforce policy at mempool level
    pub enforce_at_mempool: bool,

    /// Whether to enforce policy at consensus level
    pub enforce_at_consensus: bool,
}

impl Default for SignaturePolicy {
    fn default() -> Self {
        let mut allowed = HashSet::new();
        // ML-DSA-65 (FIPS 204) is the only approved algorithm.
        allowed.insert(SignatureAlgorithm::MlDsa65);

        Self {
            allowed_algorithms: allowed,
            reject_legacy: true, // Default to strict PQC-only mode
            enforce_at_mempool: true,
            enforce_at_consensus: true,
        }
    }
}

impl SignaturePolicy {
    /// Create a new policy with specific allowed algorithms
    pub fn new(allowed_algorithms: HashSet<SignatureAlgorithm>) -> Self {
        Self {
            allowed_algorithms,
            ..Default::default()
        }
    }

    /// Validate if an algorithm is allowed by this policy
    pub fn validate_algorithm(&self, algorithm: &SignatureAlgorithm) -> Result<(), PolicyError> {
        if !self.allowed_algorithms.contains(algorithm) {
            return Err(PolicyError::AlgorithmNotAllowed(algorithm.clone()));
        }
        Ok(())
    }

    /// Check if algorithm name represents a legacy (non-PQC) algorithm
    pub fn is_legacy_algorithm(algorithm_name: &str) -> bool {
        matches!(
            algorithm_name.to_lowercase().as_str(),
            "ecdsa" | "rsa" | "ed25519" | "secp256k1" | "p256" | "p384" | "p521"
        )
    }

    /// Validate algorithm by name, checking both allowlist and legacy rejection
    pub fn validate_algorithm_name(
        &self,
        algorithm_name: &str,
    ) -> Result<SignatureAlgorithm, PolicyError> {
        // Check for legacy algorithms first if rejection is enabled
        if self.reject_legacy && Self::is_legacy_algorithm(algorithm_name) {
            return Err(PolicyError::LegacyAlgorithmRejected(
                algorithm_name.to_string(),
            ));
        }

        let algorithm = Self::parse_algorithm_name(algorithm_name)?;
        self.validate_algorithm(&algorithm)?;
        Ok(algorithm)
    }

    /// Parse an approved algorithm name without granting permission to use it.
    pub fn parse_algorithm_name(algorithm_name: &str) -> Result<SignatureAlgorithm, PolicyError> {
        match algorithm_name.to_ascii_lowercase().as_str() {
            "mldsa65" | "ml-dsa-65" => Ok(SignatureAlgorithm::MlDsa65),
            _ => Err(PolicyError::UnknownAlgorithm(algorithm_name.to_string())),
        }
    }

    /// Get list of allowed algorithm names for display/config
    pub fn allowed_algorithm_names(&self) -> Vec<String> {
        self.allowed_algorithms
            .iter()
            .map(|alg| format!("{alg:?}"))
            .collect()
    }

    /// Check if policy should be enforced at mempool level
    pub fn should_enforce_at_mempool(&self) -> bool {
        self.enforce_at_mempool
    }

    /// Check if policy should be enforced at consensus level
    pub fn should_enforce_at_consensus(&self) -> bool {
        self.enforce_at_consensus
    }
}

/// Thread-safe policy manager for runtime access
#[derive(Debug, Clone)]
pub struct PolicyManager {
    policy: Arc<SignaturePolicy>,
}

impl Default for PolicyManager {
    fn default() -> Self {
        Self::new(SignaturePolicy::default())
    }
}

impl PolicyManager {
    /// Create a new policy manager with given policy
    pub fn new(policy: SignaturePolicy) -> Self {
        Self {
            policy: Arc::new(policy),
        }
    }

    /// Get the current policy
    pub fn policy(&self) -> &SignaturePolicy {
        &self.policy
    }

    /// Update the policy (creates new Arc)
    pub fn update_policy(&mut self, new_policy: SignaturePolicy) {
        self.policy = Arc::new(new_policy);
    }

    /// Validate a transaction signature algorithm
    pub fn validate_transaction_algorithm(
        &self,
        algorithm: &SignatureAlgorithm,
    ) -> Result<(), PolicyError> {
        self.policy.validate_algorithm(algorithm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_allows_only_mldsa65() {
        let policy = SignaturePolicy::default();
        assert_eq!(policy.allowed_algorithms.len(), 1);
        assert!(policy.validate_algorithm(&SignatureAlgorithm::MlDsa65).is_ok());
        assert_eq!(
            policy.validate_algorithm_name("mldsa65").unwrap(),
            SignatureAlgorithm::MlDsa65
        );
        assert!(policy.reject_legacy && policy.enforce_at_mempool && policy.enforce_at_consensus);
    }

    #[test]
    fn unapproved_pqc_names_are_unknown() {
        let policy = SignaturePolicy::default();
        for name in ["dilithium", "dilithium3", "dilithium5", "falcon", "sphincs+", "mldsa87"] {
            assert!(matches!(
                policy.validate_algorithm_name(name),
                Err(PolicyError::UnknownAlgorithm(_))
            ));
        }
    }

    #[test]
    fn classical_names_are_rejected_as_legacy() {
        let policy = SignaturePolicy::default();
        for name in ["ecdsa", "RSA", "ed25519", "secp256k1"] {
            assert!(SignaturePolicy::is_legacy_algorithm(name));
            assert!(matches!(
                policy.validate_algorithm_name(name),
                Err(PolicyError::LegacyAlgorithmRejected(_))
            ));
        }
        assert!(!SignaturePolicy::is_legacy_algorithm("mldsa65"));
    }

    #[test]
    fn empty_policy_rejects_everything() {
        let manager = PolicyManager::new(SignaturePolicy::new(HashSet::new()));
        assert!(matches!(
            manager.validate_transaction_algorithm(&SignatureAlgorithm::MlDsa65),
            Err(PolicyError::AlgorithmNotAllowed(_))
        ));
    }
}
