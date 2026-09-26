//! AI Integration Module for Blockchain Consensus
//!
//! This module integrates AI Oracle signature verification into the blockchain
//! consensus and transaction validation pipeline. It provides high-level APIs
//! for validating AI responses and managing oracle interactions.

use anyhow::Result;
use chrono;
use log;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::consensus::{
    replay_protection::{ReplayProtectionConfig, ReplayProtectionManager},
    signature_verification::{OracleRegistryEntry, SignatureVerifier, VerificationConfig},
    AIOracleClient, AIResponsePayload, AIServiceConfig, SignedAIOracleResponse,
};
use crate::types::Amount as Tokens;

/// Risk-based processing decision
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RiskProcessingDecision {
    /// Auto-approve the transaction
    AutoApprove,
    /// Flag for manual review
    RequireReview { reason: String },
    /// Auto-reject the transaction
    AutoReject { reason: String },
}

/// Risk thresholds for different transaction types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskThresholds {
    /// Transfer transaction thresholds
    pub transfer: TransactionRiskThresholds,
    /// Contract deployment thresholds
    pub deploy: TransactionRiskThresholds,
    /// Contract call thresholds
    pub call: TransactionRiskThresholds,
    /// Staking transaction thresholds
    pub stake: TransactionRiskThresholds,
    /// AI request transaction thresholds
    pub ai_request: TransactionRiskThresholds,
}

/// Risk thresholds for a specific transaction type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRiskThresholds {
    /// Risk score below this threshold will auto-approve (0.0-1.0)
    pub auto_approve_threshold: f64,
    /// Risk score above this threshold will auto-reject (0.0-1.0)
    pub auto_reject_threshold: f64,
    /// Amount threshold that requires review regardless of risk score
    pub amount_review_threshold: Option<u128>,
    /// Fraud probability threshold for auto-rejection (0.0-1.0)
    pub fraud_reject_threshold: f64,
    /// Minimum confidence required for AI decision (0.0-1.0)
    pub min_confidence_threshold: f64,
}

impl Default for TransactionRiskThresholds {
    fn default() -> Self {
        Self {
            auto_approve_threshold: 0.3,   // Low risk auto-approve
            auto_reject_threshold: 0.8,    // High risk auto-reject
            amount_review_threshold: None, // No amount-based review by default
            fraud_reject_threshold: 0.7,   // High fraud probability rejection
            min_confidence_threshold: 0.6, // Minimum AI confidence required
        }
    }
}

impl Default for RiskThresholds {
    fn default() -> Self {
        Self {
            transfer: TransactionRiskThresholds {
                auto_approve_threshold: 0.2,
                auto_reject_threshold: 0.8,
                amount_review_threshold: Some(1_000_000), // Review large transfers
                fraud_reject_threshold: 0.6,
                min_confidence_threshold: 0.7,
            },
            deploy: TransactionRiskThresholds {
                auto_approve_threshold: 0.1, // More strict for contract deployment
                auto_reject_threshold: 0.7,
                amount_review_threshold: None,
                fraud_reject_threshold: 0.5,
                min_confidence_threshold: 0.8,
            },
            call: TransactionRiskThresholds {
                auto_approve_threshold: 0.3,
                auto_reject_threshold: 0.8,
                amount_review_threshold: Some(500_000),
                fraud_reject_threshold: 0.7,
                min_confidence_threshold: 0.6,
            },
            stake: TransactionRiskThresholds {
                auto_approve_threshold: 0.4, // Staking is generally safer
                auto_reject_threshold: 0.9,
                amount_review_threshold: Some(10_000_000), // Review very large stakes
                fraud_reject_threshold: 0.8,
                min_confidence_threshold: 0.5,
            },
            ai_request: TransactionRiskThresholds::default(),
        }
    }
}

/// AI integration configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIIntegrationConfig {
    /// Signature verification configuration
    pub verification_config: VerificationConfig,
    /// AI service configuration
    pub ai_service_config: AIServiceConfig,
    /// Replay protection configuration
    pub replay_protection_config: ReplayProtectionConfig,
    /// Risk-based processing thresholds
    pub risk_thresholds: RiskThresholds,
    /// Whether AI verification is required for transactions
    pub require_ai_verification: bool,
    /// Whether to fail transactions if AI service is unavailable
    pub fail_on_ai_unavailable: bool,
    /// Maximum time to wait for AI response (milliseconds)
    pub ai_timeout_ms: u64,
    /// Whether to cache AI responses
    pub enable_response_caching: bool,
    /// Response cache TTL in seconds
    pub response_cache_ttl: u64,
    /// Whether to enable risk-based processing
    pub enable_risk_based_processing: bool,
    /// Whether to log all risk-based decisions for audit
    pub log_risk_decisions: bool,
}

impl Default for AIIntegrationConfig {
    fn default() -> Self {
        Self {
            verification_config: VerificationConfig::default(),
            ai_service_config: AIServiceConfig::default(),
            replay_protection_config: ReplayProtectionConfig::default(),
            risk_thresholds: RiskThresholds::default(),
            require_ai_verification: false, // Start with optional verification
            fail_on_ai_unavailable: false,  // Graceful degradation by default
            ai_timeout_ms: 5000,            // 5 second timeout
            enable_response_caching: true,
            response_cache_ttl: 300,            // 5 minutes
            enable_risk_based_processing: true, // Enable by default
            log_risk_decisions: true,           // Enable audit logging by default
        }
    }
}

/// AI verification result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AIVerificationResult {
    /// Verification passed
    Verified {
        oracle_id: String,
        response_id: String,
        risk_score: Option<f64>,
        confidence: Option<f64>,
        processing_decision: RiskProcessingDecision,
        fraud_probability: Option<f64>,
    },
    /// Verification failed
    Failed {
        error: String,
        oracle_id: Option<String>,
        response_id: Option<String>,
    },
    /// AI service unavailable
    Unavailable {
        error: String,
        fallback_allowed: bool,
    },
    /// Verification skipped (if not required)
    Skipped { reason: String },
}

/// Cached AI response
#[derive(Debug, Clone)]
struct CachedResponse {
    response: SignedAIOracleResponse,
    cached_at: u64,
    verification_result: AIVerificationResult,
}

/// AI Integration Manager
#[derive(Debug)]
pub struct AIIntegrationManager {
    /// Configuration
    config: AIIntegrationConfig,
    /// Signature verifier
    verifier: Arc<SignatureVerifier>,
    /// AI oracle client
    ai_client: Arc<AIOracleClient>,
    /// Replay protection manager
    replay_protection: Arc<ReplayProtectionManager>,
    /// Response cache
    response_cache: Arc<RwLock<std::collections::HashMap<String, CachedResponse>>>,
    /// Statistics
    stats: Arc<RwLock<AIIntegrationStats>>,
}

/// AI integration statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIIntegrationStats {
    /// Total AI verification requests
    pub total_requests: u64,
    /// Successful verifications
    pub successful_verifications: u64,
    /// Failed verifications
    pub failed_verifications: u64,
    /// AI service unavailable count
    pub service_unavailable_count: u64,
    /// Cache hits
    pub cache_hits: u64,
    /// Cache misses
    pub cache_misses: u64,
    /// Average verification time (milliseconds)
    pub avg_verification_time_ms: f64,
    /// Last update timestamp
    pub last_updated: u64,
}

impl Default for AIIntegrationStats {
    fn default() -> Self {
        Self {
            total_requests: 0,
            successful_verifications: 0,
            failed_verifications: 0,
            service_unavailable_count: 0,
            cache_hits: 0,
            cache_misses: 0,
            avg_verification_time_ms: 0.0,
            last_updated: chrono::Utc::now().timestamp() as u64,
        }
    }
}

impl AIIntegrationManager {
    /// Create a new AI integration manager
    pub async fn new(config: AIIntegrationConfig) -> Result<Self> {
        let verifier = Arc::new(SignatureVerifier::new(config.verification_config.clone())?);
        let ai_client = Arc::new(AIOracleClient::new(config.ai_service_config.clone()));
        let replay_protection = Arc::new(ReplayProtectionManager::new(
            config.replay_protection_config.clone(),
        ));

        Ok(Self {
            config,
            verifier,
            ai_client,
            replay_protection,
            response_cache: Arc::new(RwLock::new(std::collections::HashMap::new())),
            stats: Arc::new(RwLock::new(AIIntegrationStats::default())),
        })
    }

    /// Create a new AI integration manager synchronously
    pub fn new_sync(config: AIIntegrationConfig) -> Result<Self> {
        let verifier = Arc::new(SignatureVerifier::new(config.verification_config.clone())?);
        let ai_client = Arc::new(AIOracleClient::new(config.ai_service_config.clone()));
        let replay_protection = Arc::new(ReplayProtectionManager::new(
            config.replay_protection_config.clone(),
        ));

        Ok(Self {
            config,
            verifier,
            ai_client,
            replay_protection,
            response_cache: Arc::new(RwLock::new(std::collections::HashMap::new())),
            stats: Arc::new(RwLock::new(AIIntegrationStats::default())),
        })
    }

    /// Register an oracle in the verification system
    pub async fn register_oracle(
        &self,
        oracle_identity: crate::consensus::OracleIdentity,
        stake_amount: Tokens,
    ) -> Result<()> {
        self.verifier
            .register_oracle(oracle_identity, stake_amount.try_into().unwrap())
    }

    /// Verify a signed AI response
    pub async fn verify_ai_response(
        &self,
        signed_response: &SignedAIOracleResponse,
        request_hash: Option<&[u8]>,
    ) -> AIVerificationResult {
        let start_time = std::time::Instant::now();
        let mut stats = self.stats.write().await;
        stats.total_requests += 1;
        drop(stats);

        // First, check replay protection
        let request_hash_bytes = match request_hash {
            Some(hash) => hash.to_vec(),
            None => {
                // Generate hash from response data if not provided
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};

                let mut hasher = DefaultHasher::new();
                signed_response.response.id.hash(&mut hasher);
                signed_response
                    .response
                    .response_data
                    .to_string()
                    .hash(&mut hasher);
                hasher.finish().to_be_bytes().to_vec()
            }
        };

        // Check for replay attacks
        if let Err(replay_error) = self.replay_protection.validate_nonce(
            signed_response.response.nonce.parse().unwrap_or(0),
            &signed_response.oracle_identity.oracle_id,
            &hex::encode(&request_hash_bytes),
        ) {
            return AIVerificationResult::Failed {
                error: format!("Replay protection failed: {replay_error}"),
                oracle_id: Some(signed_response.oracle_identity.oracle_id.clone()),
                response_id: Some(signed_response.response.id.clone()),
            };
        }

        // Check cache first (after replay protection)
        if self.config.enable_response_caching {
            if let Some(cached) = self
                .check_response_cache(&signed_response.response.id)
                .await
            {
                let mut stats = self.stats.write().await;
                stats.cache_hits += 1;
                return cached.verification_result;
            } else {
                let mut stats = self.stats.write().await;
                stats.cache_misses += 1;
            }
        }

        // Perform signature verification
        let result = match self
            .verifier
            .verify_signed_response(signed_response, request_hash)
        {
            Ok(()) => {
                // Extract relevant information from the response
                let risk_score = self.extract_risk_score(&signed_response.response);
                let fraud_probability = self.extract_fraud_probability(&signed_response.response);
                let confidence = self.extract_confidence(&signed_response.response);

                // Create a default processing decision for single response verification
                // (Risk-based decisions are made at the transaction level with full context)
                let processing_decision = RiskProcessingDecision::AutoApprove;

                let result = AIVerificationResult::Verified {
                    oracle_id: signed_response.oracle_identity.oracle_id.clone(),
                    response_id: signed_response.response.id.clone(),
                    risk_score,
                    confidence,
                    processing_decision,
                    fraud_probability,
                };

                let mut stats = self.stats.write().await;
                stats.successful_verifications += 1;
                result
            }
            Err(verification_error) => {
                let result = AIVerificationResult::Failed {
                    error: format!("{verification_error:?}"),
                    oracle_id: Some(signed_response.oracle_identity.oracle_id.clone()),
                    response_id: Some(signed_response.response.id.clone()),
                };

                let mut stats = self.stats.write().await;
                stats.failed_verifications += 1;
                result
            }
        };

        // Cache the result
        if self.config.enable_response_caching {
            self.cache_response(signed_response.clone(), result.clone())
                .await;
        }

        // Update timing statistics
        let verification_time = start_time.elapsed().as_millis() as f64;
        let mut stats = self.stats.write().await;
        stats.avg_verification_time_ms = (stats.avg_verification_time_ms
            * (stats.total_requests - 1) as f64
            + verification_time)
            / stats.total_requests as f64;
        stats.last_updated = chrono::Utc::now().timestamp() as u64;

        result
    }

    /// Request AI analysis for a transaction and verify the response
    pub async fn request_and_verify_ai_analysis(
        &self,
        transaction_data: serde_json::Value,
        _analysis_type: &str,
    ) -> AIVerificationResult {
        if !self.config.require_ai_verification {
            return AIVerificationResult::Skipped {
                reason: "AI verification not required".to_string(),
            };
        }

        // Request AI analysis
        let request_payload = crate::consensus::AIRequestPayload {
            id: uuid::Uuid::new_v4().to_string(),
            service_type: crate::consensus::AIServiceType::TransactionValidation,
            request_data: transaction_data,
            timestamp: chrono::Utc::now().timestamp() as u64,
            metadata: None,
            priority: crate::consensus::RequestPriority::Normal,
            timeout_ms: 30000,
            callback_url: None,
            requester_id: "ai_integration".to_string(),
            correlation_id: Some(uuid::Uuid::new_v4().to_string()),
            nonce: uuid::Uuid::new_v4().to_string(),
        };

        // Convert request_data to HashMap<String, Value>
        let mut analysis_data = std::collections::HashMap::new();
        if let serde_json::Value::Object(obj) = request_payload.request_data {
            for (key, value) in obj {
                analysis_data.insert(key, value);
            }
        } else {
            analysis_data.insert("data".to_string(), request_payload.request_data);
        }

        match self
            .ai_client
            .request_ai_analysis(
                crate::consensus::AIServiceType::TransactionValidation,
                analysis_data,
            )
            .await
        {
            Ok(signed_response) => self.verify_ai_response(&signed_response, None).await,
            Err(e) => {
                let mut stats = self.stats.write().await;
                stats.service_unavailable_count += 1;

                AIVerificationResult::Unavailable {
                    error: format!("AI service error: {e}"),
                    fallback_allowed: !self.config.fail_on_ai_unavailable,
                }
            }
        }
    }

    /// Validate a transaction using AI analysis
    pub async fn validate_transaction_with_ai(
        &self,
        transaction_data: serde_json::Value,
    ) -> Result<AIVerificationResult> {
        // Extract transaction information for risk processing
        let transaction_type = transaction_data
            .get("transaction_type")
            .and_then(|t| t.as_str())
            .unwrap_or("unknown")
            .to_string();
        let transaction_amount = transaction_data.get("amount").and_then(|a| {
            // Handle both string and number formats for u128
            match a {
                serde_json::Value::String(s) => s.parse::<u128>().ok(),
                serde_json::Value::Number(n) => n.as_u64().map(|v| v as u128),
                _ => None,
            }
        });
        let transaction_hash = transaction_data
            .get("hash")
            .and_then(|h| h.as_str())
            .unwrap_or("unknown")
            .to_string();

        // Request fraud detection analysis
        let fraud_result = self
            .request_and_verify_ai_analysis(transaction_data.clone(), "fraud_detection")
            .await;

        // Request risk scoring analysis
        let risk_result = self
            .request_and_verify_ai_analysis(transaction_data, "risk_scoring")
            .await;

        // Combine results with risk-based processing
        match (fraud_result, risk_result) {
            (
                AIVerificationResult::Verified {
                    risk_score: fraud_score,
                    confidence: fraud_confidence,
                    ..
                },
                AIVerificationResult::Verified {
                    risk_score,
                    confidence: risk_confidence,
                    oracle_id,
                    response_id,
                    ..
                },
            ) => {
                // Calculate combined risk score and fraud probability
                let combined_risk_score = match (fraud_score, risk_score) {
                    (Some(f), Some(r)) => (f + r) / 2.0,
                    (Some(score), None) | (None, Some(score)) => score,
                    _ => 0.5, // Default medium risk if no scores available
                };

                // Extract fraud probability (fraud score can serve as fraud probability)
                let fraud_probability = fraud_score.unwrap_or(0.0);

                // Calculate combined confidence
                let combined_confidence = match (fraud_confidence, risk_confidence) {
                    (Some(f), Some(r)) => (f + r) / 2.0,
                    (Some(conf), None) | (None, Some(conf)) => conf,
                    _ => 0.5, // Default medium confidence
                };

                // Make risk-based processing decision
                let processing_decision = self.make_risk_processing_decision(
                    &transaction_type,
                    combined_risk_score,
                    fraud_probability,
                    combined_confidence,
                    transaction_amount,
                );

                // Log the decision for audit
                self.log_risk_decision(
                    &transaction_hash,
                    &transaction_type,
                    &processing_decision,
                    combined_risk_score,
                    fraud_probability,
                    combined_confidence,
                );

                Ok(AIVerificationResult::Verified {
                    oracle_id,
                    response_id,
                    risk_score: Some(combined_risk_score),
                    confidence: Some(combined_confidence),
                    processing_decision,
                    fraud_probability: Some(fraud_probability),
                })
            }
            (AIVerificationResult::Failed { error, .. }, _)
            | (_, AIVerificationResult::Failed { error, .. }) => Ok(AIVerificationResult::Failed {
                error: format!("AI verification failed: {error}"),
                oracle_id: None,
                response_id: None,
            }),
            (
                AIVerificationResult::Unavailable {
                    fallback_allowed, ..
                },
                _,
            )
            | (
                _,
                AIVerificationResult::Unavailable {
                    fallback_allowed, ..
                },
            ) => Ok(AIVerificationResult::Unavailable {
                error: "AI service unavailable".to_string(),
                fallback_allowed,
            }),
            _ => Ok(AIVerificationResult::Skipped {
                reason: "AI verification skipped or mixed results".to_string(),
            }),
        }
    }

    /// Check response cache
    async fn check_response_cache(&self, response_id: &str) -> Option<CachedResponse> {
        let cache = self.response_cache.read().await;
        if let Some(cached) = cache.get(response_id) {
            // Touch the response field to avoid unused field warning
            let _ = &cached.response;
            return Some(cached.clone());
        }
        None
    }

    /// Cache a response
    async fn cache_response(&self, response: SignedAIOracleResponse, result: AIVerificationResult) {
        let mut cache = self.response_cache.write().await;

        let cached_response = CachedResponse {
            response: response.clone(),
            cached_at: chrono::Utc::now().timestamp() as u64,
            verification_result: result,
        };

        cache.insert(response.response.id.clone(), cached_response);

        // Clean up old entries if cache is too large
        if cache.len() > 10000 {
            // Configurable limit
            let cutoff_time =
                chrono::Utc::now().timestamp() as u64 - self.config.response_cache_ttl;
            cache.retain(|_, cached| cached.cached_at > cutoff_time);
        }
    }

    /// Extract risk score from AI response
    fn extract_risk_score(&self, response: &AIResponsePayload) -> Option<f64> {
        // Try to extract risk score from response data
        if let Some(risk_score) = response.response_data.get("risk_score") {
            risk_score.as_f64()
        } else if let Some(fraud_score) = response.response_data.get("fraud_score") {
            fraud_score.as_f64()
        } else {
            None
        }
    }

    /// Extract fraud probability from AI response
    fn extract_fraud_probability(&self, response: &AIResponsePayload) -> Option<f64> {
        // Try to extract fraud probability from response data
        if let Some(fraud_prob) = response.response_data.get("fraud_probability") {
            fraud_prob.as_f64()
        } else if let Some(fraud_score) = response.response_data.get("fraud_score") {
            fraud_score.as_f64()
        } else {
            None
        }
    }

    /// Extract confidence from AI response
    fn extract_confidence(&self, response: &AIResponsePayload) -> Option<f64> {
        response
            .response_data
            .get("confidence")
            .and_then(|c| c.as_f64())
    }

    /// Get oracle information
    pub async fn get_oracle(&self, oracle_id: &str) -> Option<OracleRegistryEntry> {
        self.verifier.get_oracle(oracle_id)
    }

    /// List all oracles
    pub async fn list_oracles(&self) -> Vec<OracleRegistryEntry> {
        self.verifier.list_oracles()
    }

    /// Update oracle reputation
    pub async fn update_oracle_reputation(
        &self,
        oracle_id: &str,
        new_reputation: f64,
    ) -> Result<()> {
        self.verifier
            .update_oracle_reputation(oracle_id, new_reputation)
    }

    /// Deactivate an oracle
    pub async fn deactivate_oracle(&self, oracle_id: &str) -> Result<()> {
        self.verifier.deactivate_oracle(oracle_id)
    }

    /// Get integration statistics
    pub async fn get_statistics(&self) -> AIIntegrationStats {
        self.stats.read().await.clone()
    }

    /// Get verification statistics
    pub async fn get_verification_statistics(
        &self,
    ) -> std::collections::HashMap<String, serde_json::Value> {
        self.verifier.get_verification_stats()
    }

    /// Clean up expired cache entries and old data
    pub async fn cleanup(&self) {
        // Clean up signature verifier
        self.verifier.cleanup();

        // Clean up response cache
        let mut cache = self.response_cache.write().await;
        let cutoff_time = chrono::Utc::now().timestamp() as u64 - self.config.response_cache_ttl;
        cache.retain(|_, cached| cached.cached_at > cutoff_time);

        // Clean up replay protection (no direct cleanup method, handled internally)
        // self.replay_protection.cleanup();
    }

    /// Invalidate cache for a specific oracle
    pub async fn invalidate_oracle_cache(&self, oracle_id: &str) {
        self.replay_protection.invalidate_oracle_cache(oracle_id);
    }

    /// Get replay protection statistics
    pub async fn get_replay_protection_stats(&self) -> serde_json::Value {
        let health_metrics = self.replay_protection.get_cache_health();
        serde_json::to_value(health_metrics).unwrap_or(serde_json::Value::Null)
    }

    /// Get cache statistics
    pub async fn get_cache_stats(&self) -> serde_json::Value {
        let cache_size = self.response_cache.read().await.len();
        let replay_health = self.replay_protection.get_cache_health();
        let replay_stats = serde_json::to_value(replay_health).unwrap_or(serde_json::Value::Null);

        serde_json::json!({
            "response_cache_size": cache_size,
            "response_cache_ttl": self.config.response_cache_ttl,
            "replay_protection": replay_stats
        })
    }

    /// Health check for AI integration
    pub async fn health_check(&self) -> Result<serde_json::Value> {
        let ai_health = self.ai_client.health_check().await.is_ok();
        let stats = self.get_statistics().await;
        let verification_stats = self.get_verification_statistics().await;
        let cache_stats = self.get_cache_stats().await;

        Ok(serde_json::json!({
            "ai_service_available": ai_health,
            "total_oracles": self.list_oracles().await.len(),
            "active_oracles": self.list_oracles().await.iter().filter(|o| o.is_active).count(),
            "verification_stats": verification_stats,
            "integration_stats": stats,
            "cache_stats": cache_stats,
            "config": {
                "require_ai_verification": self.config.require_ai_verification,
                "fail_on_ai_unavailable": self.config.fail_on_ai_unavailable,
                "enable_response_caching": self.config.enable_response_caching,
                "replay_protection_enabled": true, // Always enabled in this implementation
            }
        }))
    }

    /// Check if AI verification is required
    pub fn is_ai_verification_required(&self) -> bool {
        self.config.require_ai_verification
    }

    /// Make risk-based processing decision
    pub fn make_risk_processing_decision(
        &self,
        transaction_type: &str,
        risk_score: f64,
        fraud_probability: f64,
        confidence: f64,
        transaction_amount: Option<u128>,
    ) -> RiskProcessingDecision {
        if !self.config.enable_risk_based_processing {
            return RiskProcessingDecision::AutoApprove;
        }

        let thresholds = self.get_risk_thresholds_for_transaction_type(transaction_type);

        // Check minimum confidence requirement
        if confidence < thresholds.min_confidence_threshold {
            return RiskProcessingDecision::RequireReview {
                reason: format!(
                    "AI confidence too low: {:.3} < {:.3}",
                    confidence, thresholds.min_confidence_threshold
                ),
            };
        }

        // Check fraud probability threshold
        if fraud_probability > thresholds.fraud_reject_threshold {
            return RiskProcessingDecision::AutoReject {
                reason: format!(
                    "High fraud probability: {:.3} > {:.3}",
                    fraud_probability, thresholds.fraud_reject_threshold
                ),
            };
        }

        // Check amount-based review requirement
        if let (Some(amount), Some(threshold)) =
            (transaction_amount, thresholds.amount_review_threshold)
        {
            if amount > threshold {
                return RiskProcessingDecision::RequireReview {
                    reason: format!("Large transaction amount: {amount} > {threshold}"),
                };
            }
        }

        // Check risk score thresholds
        if risk_score >= thresholds.auto_reject_threshold {
            RiskProcessingDecision::AutoReject {
                reason: format!(
                    "High risk score: {:.3} >= {:.3}",
                    risk_score, thresholds.auto_reject_threshold
                ),
            }
        } else if risk_score <= thresholds.auto_approve_threshold {
            RiskProcessingDecision::AutoApprove
        } else {
            RiskProcessingDecision::RequireReview {
                reason: format!("Medium risk score: {risk_score:.3} requires manual review"),
            }
        }
    }

    /// Get risk thresholds for a specific transaction type
    pub fn get_risk_thresholds_for_transaction_type(
        &self,
        transaction_type: &str,
    ) -> &TransactionRiskThresholds {
        match transaction_type {
            "transfer" => &self.config.risk_thresholds.transfer,
            "deploy" | "contract_deploy" => &self.config.risk_thresholds.deploy,
            "call" | "contract_call" => &self.config.risk_thresholds.call,
            "stake" => &self.config.risk_thresholds.stake,
            "ai_request" => &self.config.risk_thresholds.ai_request,
            _ => &self.config.risk_thresholds.transfer, // Default fallback
        }
    }

    /// Update risk thresholds for a transaction type
    pub fn update_risk_thresholds(
        &mut self,
        transaction_type: &str,
        thresholds: TransactionRiskThresholds,
    ) {
        match transaction_type {
            "transfer" => self.config.risk_thresholds.transfer = thresholds,
            "deploy" | "contract_deploy" => self.config.risk_thresholds.deploy = thresholds,
            "call" | "contract_call" => self.config.risk_thresholds.call = thresholds,
            "stake" => self.config.risk_thresholds.stake = thresholds,
            "ai_request" => self.config.risk_thresholds.ai_request = thresholds,
            _ => {
                log::warn!(
                    "Unknown transaction type for risk threshold update: {transaction_type}"
                );
            }
        }
    }

    /// Get all risk thresholds
    pub fn get_all_risk_thresholds(&self) -> &RiskThresholds {
        &self.config.risk_thresholds
    }

    /// Check if risk-based processing is enabled
    pub fn is_risk_based_processing_enabled(&self) -> bool {
        self.config.enable_risk_based_processing
    }

    /// Enable or disable risk-based processing
    pub fn set_risk_based_processing_enabled(&mut self, enabled: bool) {
        self.config.enable_risk_based_processing = enabled;
    }

    /// Log a risk-based decision for audit purposes
    pub fn log_risk_decision(
        &self,
        transaction_hash: &str,
        transaction_type: &str,
        decision: &RiskProcessingDecision,
        risk_score: f64,
        fraud_probability: f64,
        confidence: f64,
    ) {
        if self.config.log_risk_decisions {
            match decision {
                RiskProcessingDecision::AutoApprove => {
                    log::info!("RISK_DECISION: AUTO_APPROVE - TX: {transaction_hash} Type: {transaction_type} Risk: {risk_score:.3} Fraud: {fraud_probability:.3} Confidence: {confidence:.3}");
                }
                RiskProcessingDecision::RequireReview { reason } => {
                    log::warn!("RISK_DECISION: REQUIRE_REVIEW - TX: {transaction_hash} Type: {transaction_type} Risk: {risk_score:.3} Fraud: {fraud_probability:.3} Confidence: {confidence:.3} Reason: {reason}");
                }
                RiskProcessingDecision::AutoReject { reason } => {
                    log::error!("RISK_DECISION: AUTO_REJECT - TX: {transaction_hash} Type: {transaction_type} Risk: {risk_score:.3} Fraud: {fraud_probability:.3} Confidence: {confidence:.3} Reason: {reason}");
                }
            }
        }
    }
}

/// Helper function to create default AI integration manager
pub async fn create_default_ai_integration() -> Result<AIIntegrationManager> {
    let config = AIIntegrationConfig::default();
    AIIntegrationManager::new(config).await
}

/// Helper function to create AI integration manager with custom config
pub async fn create_ai_integration_with_config(
    config: AIIntegrationConfig,
) -> Result<AIIntegrationManager> {
    AIIntegrationManager::new(config).await
}
