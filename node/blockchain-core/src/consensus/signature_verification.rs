//! AI Oracle Signature Verification Module
//!
//! This module implements blockchain-side verification of AI Oracle signatures using
//! Post-Quantum Cryptography. It handles oracle public key management, certificate
//! chain validation, and signature verification for AI responses.

use anyhow::Result;
use chrono;
use dytallix_pqc::PQCManager;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::consensus::{OracleIdentity, SignedAIOracleResponse};

/// Errors that can occur during signature verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationError {
    /// Invalid signature format or structure
    InvalidSignature(String),
    /// Oracle not found in registry
    OracleNotFound(String),
    /// Oracle is not trusted (low reputation, inactive, etc.)
    OracleNotTrusted(String),
    /// Certificate validation failed
    CertificateError(String),
    /// Response has expired
    ResponseExpired(String),
    /// Replay attack detected (nonce already used)
    ReplayAttack(String),
    /// Signature verification failed
    SignatureVerificationFailed(String),
    /// Request-response binding failed
    RequestResponseMismatch(String),
    /// Timestamp validation failed
    TimestampError(String),
    /// General verification error
    VerificationFailed(String),
}

impl From<anyhow::Error> for VerificationError {
    fn from(error: anyhow::Error) -> Self {
        VerificationError::VerificationFailed(error.to_string())
    }
}

impl std::fmt::Display for VerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerificationError::InvalidSignature(msg) => write!(f, "Invalid signature: {msg}"),
            VerificationError::OracleNotFound(msg) => write!(f, "Oracle not found: {msg}"),
            VerificationError::OracleNotTrusted(msg) => write!(f, "Oracle not trusted: {msg}"),
            VerificationError::CertificateError(msg) => write!(f, "Certificate error: {msg}"),
            VerificationError::ResponseExpired(msg) => write!(f, "Response expired: {msg}"),
            VerificationError::ReplayAttack(msg) => write!(f, "Replay attack: {msg}"),
            VerificationError::SignatureVerificationFailed(msg) => {
                write!(f, "Signature verification failed: {msg}")
            }
            VerificationError::RequestResponseMismatch(msg) => {
                write!(f, "Request-response mismatch: {msg}")
            }
            VerificationError::TimestampError(msg) => write!(f, "Timestamp error: {msg}"),
            VerificationError::VerificationFailed(msg) => write!(f, "Verification failed: {msg}"),
        }
    }
}

impl std::error::Error for VerificationError {}

/// Configuration for signature verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationConfig {
    /// Minimum oracle reputation score required for trust
    pub min_oracle_reputation: f64,
    /// Maximum allowed signature age in seconds
    pub max_signature_age: u64,
    /// Maximum allowed response age in seconds
    pub max_response_age: u64,
    /// Clock skew tolerance in seconds
    pub clock_skew_tolerance: u64,
    /// Whether to enforce certificate chain validation
    pub enforce_certificate_validation: bool,
    /// Whether to enforce request-response binding
    pub enforce_request_binding: bool,
    /// Maximum nonce cache size
    pub max_nonce_cache_size: usize,
    /// Nonce cache TTL in seconds
    pub nonce_cache_ttl: u64,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            min_oracle_reputation: 0.7,
            max_signature_age: 600,   // 10 minutes
            max_response_age: 300,    // 5 minutes
            clock_skew_tolerance: 30, // 30 seconds
            enforce_certificate_validation: true,
            enforce_request_binding: false, // Optional for now
            max_nonce_cache_size: 100000,
            nonce_cache_ttl: 3600, // 1 hour
        }
    }
}

/// Nonce cache entry for replay protection
#[derive(Debug, Clone)]
struct NonceEntry {
    /// When the nonce was first seen
    first_seen: u64,
    /// Oracle that used this nonce
    oracle_id: String,
}

/// Oracle registry entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleRegistryEntry {
    /// Oracle identity information
    pub identity: OracleIdentity,
    /// When the oracle was registered
    pub registered_at: u64,
    /// Last update timestamp
    pub last_updated: u64,
    /// Whether the oracle is currently active
    pub is_active: bool,
    /// Staking amount (for future use)
    pub stake_amount: u64,
    /// Performance metrics
    pub performance_metrics: OraclePerformanceMetrics,
}

/// Oracle performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OraclePerformanceMetrics {
    /// Total responses verified
    pub total_responses: u64,
    /// Successful verifications
    pub successful_verifications: u64,
    /// Failed verifications
    pub failed_verifications: u64,
    /// Average response time in milliseconds
    pub avg_response_time_ms: f64,
    /// Last verification timestamp
    pub last_verification: u64,
}

impl Default for OraclePerformanceMetrics {
    fn default() -> Self {
        Self {
            total_responses: 0,
            successful_verifications: 0,
            failed_verifications: 0,
            avg_response_time_ms: 0.0,
            last_verification: 0,
        }
    }
}

/// AI Oracle Signature Verifier
pub struct SignatureVerifier {
    /// Verification configuration
    config: VerificationConfig,
    /// PQC manager for cryptographic operations
    pqc_manager: Arc<PQCManager>,
    /// Oracle registry
    oracle_registry: Arc<RwLock<HashMap<String, OracleRegistryEntry>>>,
    /// Nonce cache for replay protection
    nonce_cache: Arc<RwLock<HashMap<u64, NonceEntry>>>,
    /// Request hash cache for request-response binding
    request_cache: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl SignatureVerifier {
    /// Create a new signature verifier
    pub fn new(config: VerificationConfig) -> Result<Self> {
        let pqc_manager = Arc::new(PQCManager::new()?);

        Ok(Self {
            config,
            pqc_manager,
            oracle_registry: Arc::new(RwLock::new(HashMap::new())),
            nonce_cache: Arc::new(RwLock::new(HashMap::new())),
            request_cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Register an oracle in the registry
    pub fn register_oracle(
        &self,
        oracle_identity: OracleIdentity,
        stake_amount: u64,
    ) -> Result<()> {
        let mut registry = self.oracle_registry.write().unwrap();

        let entry = OracleRegistryEntry {
            identity: oracle_identity.clone(),
            registered_at: chrono::Utc::now().timestamp() as u64,
            last_updated: chrono::Utc::now().timestamp() as u64,
            is_active: oracle_identity.is_active,
            stake_amount,
            performance_metrics: OraclePerformanceMetrics::default(),
        };

        registry.insert(oracle_identity.oracle_id.clone(), entry);
        Ok(())
    }

    /// Update oracle reputation score
    pub fn update_oracle_reputation(&self, oracle_id: &str, new_reputation: f64) -> Result<()> {
        let mut registry = self.oracle_registry.write().unwrap();

        if let Some(entry) = registry.get_mut(oracle_id) {
            entry.identity.reputation_score = new_reputation.clamp(0.0, 1.0);
            entry.last_updated = chrono::Utc::now().timestamp() as u64;
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Oracle {} not found in registry",
                oracle_id
            ))
        }
    }

    /// Deactivate an oracle
    pub fn deactivate_oracle(&self, oracle_id: &str) -> Result<()> {
        let mut registry = self.oracle_registry.write().unwrap();

        if let Some(entry) = registry.get_mut(oracle_id) {
            entry.is_active = false;
            entry.identity.is_active = false;
            entry.last_updated = chrono::Utc::now().timestamp() as u64;
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Oracle {} not found in registry",
                oracle_id
            ))
        }
    }

    /// Get oracle information
    pub fn get_oracle(&self, oracle_id: &str) -> Option<OracleRegistryEntry> {
        let registry = self.oracle_registry.read().unwrap();
        registry.get(oracle_id).cloned()
    }

    /// List all registered oracles
    pub fn list_oracles(&self) -> Vec<OracleRegistryEntry> {
        let registry = self.oracle_registry.read().unwrap();
        registry.values().cloned().collect()
    }

    /// Verify a signed AI oracle response
    pub fn verify_signed_response(
        &self,
        signed_response: &SignedAIOracleResponse,
        request_hash: Option<&[u8]>,
    ) -> Result<(), VerificationError> {
        // 1. Basic freshness validation
        self.validate_response_freshness(signed_response)?;

        // 2. Oracle validation
        let _oracle_entry = self.validate_oracle(&signed_response.oracle_identity)?;

        // 3. Nonce validation (replay protection)
        self.validate_nonce(
            signed_response.nonce,
            &signed_response.oracle_identity.oracle_id,
        )?;

        // 4. Certificate chain validation (if enabled)
        if self.config.enforce_certificate_validation {
            self.validate_certificate_chain(&signed_response.oracle_identity)?;
        }

        // 5. Request-response binding (if enabled and provided)
        if self.config.enforce_request_binding && request_hash.is_some() {
            self.validate_request_binding(signed_response, request_hash.unwrap())?;
        }

        // 6. Signature verification
        self.verify_signature(signed_response)?;

        // 7. Update oracle metrics
        self.update_oracle_metrics(&signed_response.oracle_identity.oracle_id, true)?;

        Ok(())
    }

    /// Validate response freshness and expiration
    fn validate_response_freshness(
        &self,
        signed_response: &SignedAIOracleResponse,
    ) -> Result<(), VerificationError> {
        let now = chrono::Utc::now().timestamp() as u64;

        // Check if response has expired
        if signed_response.expires_at <= now {
            return Err(VerificationError::ResponseExpired(format!(
                "Response expired at {}, current time {}",
                signed_response.expires_at, now
            )));
        }

        // Check signature age
        let signature_age = now.saturating_sub(signed_response.signature.signature_timestamp);

        if signature_age > self.config.max_signature_age + self.config.clock_skew_tolerance {
            return Err(VerificationError::TimestampError(format!(
                "Signature too old: {signature_age} seconds"
            )));
        }

        // Check response age
        let response_age = now.saturating_sub(signed_response.response.timestamp);

        if response_age > self.config.max_response_age + self.config.clock_skew_tolerance {
            return Err(VerificationError::TimestampError(format!(
                "Response too old: {response_age} seconds"
            )));
        }

        Ok(())
    }

    /// Validate oracle and check trust requirements
    fn validate_oracle(
        &self,
        oracle_identity: &OracleIdentity,
    ) -> Result<OracleRegistryEntry, VerificationError> {
        let registry = self.oracle_registry.read().unwrap();

        // Check if oracle is registered
        let oracle_entry = registry.get(&oracle_identity.oracle_id).ok_or_else(|| {
            VerificationError::OracleNotFound(format!(
                "Oracle {} not found in registry",
                oracle_identity.oracle_id
            ))
        })?;

        // Check if oracle is active
        if !oracle_entry.is_active || !oracle_entry.identity.is_active {
            return Err(VerificationError::OracleNotTrusted(format!(
                "Oracle {} is not active",
                oracle_identity.oracle_id
            )));
        }

        // Check reputation score
        if oracle_entry.identity.reputation_score < self.config.min_oracle_reputation {
            return Err(VerificationError::OracleNotTrusted(format!(
                "Oracle {} reputation score {} below minimum {}",
                oracle_identity.oracle_id,
                oracle_entry.identity.reputation_score,
                self.config.min_oracle_reputation
            )));
        }

        Ok(oracle_entry.clone())
    }

    /// Validate nonce for replay protection
    fn validate_nonce(&self, nonce: u64, oracle_id: &str) -> Result<(), VerificationError> {
        let mut nonce_cache = self.nonce_cache.write().unwrap();

        // Check if nonce was already used
        if let Some(entry) = nonce_cache.get(&nonce) {
            return Err(VerificationError::ReplayAttack(format!(
                "Nonce {} already used by oracle {}",
                nonce, entry.oracle_id
            )));
        }

        // Add nonce to cache
        let now = chrono::Utc::now().timestamp() as u64;
        nonce_cache.insert(
            nonce,
            NonceEntry {
                first_seen: now,
                oracle_id: oracle_id.to_string(),
            },
        );

        // Cleanup old nonces if cache is too large
        if nonce_cache.len() > self.config.max_nonce_cache_size {
            let cutoff_time = now - self.config.nonce_cache_ttl;
            nonce_cache.retain(|_, entry| entry.first_seen > cutoff_time);
        }

        Ok(())
    }

    /// Validate certificate chain
    fn validate_certificate_chain(
        &self,
        oracle_identity: &OracleIdentity,
    ) -> Result<(), VerificationError> {
        // For now, just check that certificates exist and are not expired
        if oracle_identity.certificate_chain.is_empty() {
            return Err(VerificationError::CertificateError(
                "No certificates found in chain".to_string(),
            ));
        }

        let now = chrono::Utc::now().timestamp() as u64;

        for (i, cert) in oracle_identity.certificate_chain.iter().enumerate() {
            // Check certificate validity period
            if now < cert.valid_from || now > cert.valid_until {
                return Err(VerificationError::CertificateError(format!(
                    "Certificate {i} in chain is not valid at current time"
                )));
            }

            // Check certificate subject matches oracle
            if cert.subject_oracle_id != oracle_identity.oracle_id {
                return Err(VerificationError::CertificateError(format!(
                    "Certificate {i} subject does not match oracle ID"
                )));
            }
        }

        Ok(())
    }

    /// Validate request-response binding
    fn validate_request_binding(
        &self,
        signed_response: &SignedAIOracleResponse,
        request_hash: &[u8],
    ) -> Result<(), VerificationError> {
        if let Some(verification_data) = &signed_response.verification_data {
            if verification_data.request_hash != request_hash {
                return Err(VerificationError::RequestResponseMismatch(
                    "Request hash does not match verification data".to_string(),
                ));
            }
        } else {
            return Err(VerificationError::RequestResponseMismatch(
                "No verification data provided for request binding".to_string(),
            ));
        }

        Ok(())
    }

    /// Verify the cryptographic signature
    fn verify_signature(
        &self,
        signed_response: &SignedAIOracleResponse,
    ) -> Result<(), VerificationError> {
        // Get signable data
        let signable_data = signed_response.get_signable_data().map_err(|e| {
            VerificationError::SignatureVerificationFailed(format!(
                "Failed to create signable data: {e}"
            ))
        })?;

        // Verify signature using PQC manager
        let is_valid = self
            .pqc_manager
            .verify(
                &signable_data,
                &dytallix_pqc::Signature {
                    data: signed_response.signature.signature.clone(),
                    algorithm: signed_response.signature.algorithm.clone(),
                },
                &signed_response.signature.public_key,
            )
            .map_err(|e| {
                VerificationError::SignatureVerificationFailed(format!(
                    "PQC verification failed: {e}"
                ))
            })?;

        if !is_valid {
            return Err(VerificationError::SignatureVerificationFailed(
                "Signature verification failed".to_string(),
            ));
        }

        Ok(())
    }

    /// Update oracle performance metrics
    fn update_oracle_metrics(&self, oracle_id: &str, verification_success: bool) -> Result<()> {
        let mut registry = self.oracle_registry.write().unwrap();

        if let Some(entry) = registry.get_mut(oracle_id) {
            entry.performance_metrics.total_responses += 1;

            if verification_success {
                entry.performance_metrics.successful_verifications += 1;
            } else {
                entry.performance_metrics.failed_verifications += 1;
            }

            entry.performance_metrics.last_verification = chrono::Utc::now().timestamp() as u64;

            // Update reputation based on verification success rate
            let success_rate = entry.performance_metrics.successful_verifications as f64
                / entry.performance_metrics.total_responses as f64;

            // Gradually adjust reputation towards success rate
            let current_rep = entry.identity.reputation_score;
            let new_rep = current_rep * 0.95 + success_rate * 0.05;
            entry.identity.reputation_score = new_rep.clamp(0.0, 1.0);
        }

        Ok(())
    }

    /// Clean up expired nonces and old data
    pub fn cleanup(&self) {
        let now = chrono::Utc::now().timestamp() as u64;
        let cutoff_time = now - self.config.nonce_cache_ttl;

        // Clean up nonce cache
        let mut nonce_cache = self.nonce_cache.write().unwrap();
        nonce_cache.retain(|_, entry| entry.first_seen > cutoff_time);

        // Clean up request cache (if needed)
        // This could be implemented based on specific requirements
    }

    /// Get verification statistics
    pub fn get_verification_stats(&self) -> HashMap<String, serde_json::Value> {
        let registry = self.oracle_registry.read().unwrap();
        let nonce_cache = self.nonce_cache.read().unwrap();

        let mut stats = HashMap::new();

        // Oracle statistics
        stats.insert(
            "total_oracles".to_string(),
            serde_json::Value::Number(serde_json::Number::from(registry.len())),
        );

        let active_oracles = registry.values().filter(|entry| entry.is_active).count();
        stats.insert(
            "active_oracles".to_string(),
            serde_json::Value::Number(serde_json::Number::from(active_oracles)),
        );

        // Nonce cache statistics
        stats.insert(
            "nonce_cache_size".to_string(),
            serde_json::Value::Number(serde_json::Number::from(nonce_cache.len())),
        );

        // Aggregate performance metrics
        let total_responses: u64 = registry
            .values()
            .map(|entry| entry.performance_metrics.total_responses)
            .sum();
        let total_successful: u64 = registry
            .values()
            .map(|entry| entry.performance_metrics.successful_verifications)
            .sum();

        stats.insert(
            "total_verifications".to_string(),
            serde_json::Value::Number(serde_json::Number::from(total_responses)),
        );
        stats.insert(
            "successful_verifications".to_string(),
            serde_json::Value::Number(serde_json::Number::from(total_successful)),
        );

        if total_responses > 0 {
            let success_rate = total_successful as f64 / total_responses as f64;
            stats.insert(
                "overall_success_rate".to_string(),
                serde_json::Value::Number(
                    serde_json::Number::from_f64(success_rate)
                        .unwrap_or(serde_json::Number::from(0)),
                ),
            );
        }

        stats
    }
}

impl std::fmt::Debug for SignatureVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignatureVerifier")
            .field("config", &self.config)
            .field("pqc_manager", &"<PQCManager instance>")
            .field("oracle_registry", &self.oracle_registry)
            .field("nonce_cache", &self.nonce_cache)
            .field("request_cache", &self.request_cache)
            .finish()
    }
}
