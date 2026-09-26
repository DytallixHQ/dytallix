//! Transaction Validation Module
//!
//! This module handles all transaction validation logic including AI-enhanced
//! validation, signature verification, and compliance checking.

use anyhow::{anyhow, Result};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use crate::consensus::ai_integration::{
    AIIntegrationManager, AIVerificationResult, RiskProcessingDecision,
};
use crate::consensus::ai_oracle_client::AIOracleClient;
use crate::consensus::audit_trail::AuditTrailManager;
use crate::consensus::high_risk_queue::{HighRiskQueue, ReviewPriority};
use crate::consensus::performance_optimizer::PerformanceOptimizer;
use crate::consensus::types::AIServiceType;
use crate::consensus::SignedAIOracleResponse;
use crate::policy::PolicyManager;
use crate::types::{AIRequestTransaction, Transaction, TransferTransaction};

/// Transaction validation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub confidence_score: f64,
    pub risk_score: f64,
    pub fraud_probability: f64,
    pub validation_time_ms: u64,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub ai_analysis: Option<SignedAIOracleResponse>,
}

impl ValidationResult {
    /// Create a successful validation result
    pub fn success() -> Self {
        Self {
            is_valid: true,
            confidence_score: 1.0,
            risk_score: 0.0,
            fraud_probability: 0.0,
            validation_time_ms: 0,
            errors: Vec::new(),
            warnings: Vec::new(),
            ai_analysis: None,
        }
    }

    /// Create a failed validation result
    pub fn failure(error: String) -> Self {
        Self {
            is_valid: false,
            confidence_score: 0.0,
            risk_score: 1.0,
            fraud_probability: 1.0,
            validation_time_ms: 0,
            errors: vec![error],
            warnings: Vec::new(),
            ai_analysis: None,
        }
    }

    /// Add an error to the validation result
    pub fn add_error(&mut self, error: String) {
        self.errors.push(error);
        self.is_valid = false;
    }

    /// Add a warning to the validation result
    pub fn add_warning(&mut self, warning: String) {
        self.warnings.push(warning);
    }

    /// Set AI analysis result
    pub fn with_ai_analysis(mut self, analysis: SignedAIOracleResponse) -> Self {
        // Extract confidence score from metadata
        self.confidence_score = analysis
            .response
            .metadata
            .as_ref()
            .and_then(|m| m.confidence_score)
            .unwrap_or(0.0);

        // Parse AI analysis result from response data
        if let Ok(ai_result) = serde_json::from_value::<crate::consensus::types::AIAnalysisResult>(
            analysis.response.response_data.clone(),
        ) {
            self.risk_score = ai_result.risk_score;
            self.fraud_probability = ai_result.fraud_probability;
        } else {
            // Fallback values if parsing fails
            self.risk_score = 0.0;
            self.fraud_probability = 0.0;
        }

        self.ai_analysis = Some(analysis);
        self
    }
}

/// Transaction Validator
#[derive(Debug)]
pub struct TransactionValidator {
    ai_client: Arc<AIOracleClient>,
    ai_integration: Option<Arc<AIIntegrationManager>>,
    high_risk_queue: Arc<HighRiskQueue>,
    audit_trail: Arc<AuditTrailManager>,
    performance_optimizer: Arc<PerformanceOptimizer>,
    policy_manager: Arc<PolicyManager>,
}

impl TransactionValidator {
    /// Create new transaction validator
    pub fn new(
        ai_client: Arc<AIOracleClient>,
        ai_integration: Option<Arc<AIIntegrationManager>>,
        high_risk_queue: Arc<HighRiskQueue>,
        audit_trail: Arc<AuditTrailManager>,
        performance_optimizer: Arc<PerformanceOptimizer>,
        policy_manager: Arc<PolicyManager>,
    ) -> Self {
        Self {
            ai_client,
            ai_integration,
            high_risk_queue,
            audit_trail,
            performance_optimizer,
            policy_manager,
        }
    }

    /// Validate a transaction with AI enhancement
    pub async fn validate_transaction(&self, tx: &Transaction) -> Result<ValidationResult> {
        let start_time = std::time::Instant::now();
        let mut result = ValidationResult::success();

        // 1. Basic validation
        if let Err(e) = self.validate_basic_transaction(tx) {
            result.add_error(format!("Basic validation failed: {e}"));
            return Ok(result);
        }

        // 2. AI-enhanced validation (if available)
        if let Some(ai_integration) = &self.ai_integration {
            match self.validate_with_ai(tx, ai_integration.clone()).await {
                Ok(ai_result) => {
                    result = result.with_ai_analysis(ai_result);
                }
                Err(e) => {
                    result.add_warning(format!("AI validation failed: {e}"));
                }
            }
        }

        // 3. High-risk transaction handling
        if result.risk_score > 0.7 || result.fraud_probability > 0.6 {
            let ai_result = AIVerificationResult::Verified {
                oracle_id: "validation_engine".to_string(),
                response_id: tx.hash().clone(),
                risk_score: Some(result.risk_score),
                confidence: Some(result.confidence_score),
                processing_decision: RiskProcessingDecision::RequireReview {
                    reason: "High risk transaction".to_string(),
                },
                fraud_probability: Some(result.fraud_probability),
            };

            let risk_decision = RiskProcessingDecision::RequireReview {
                reason: "High risk transaction".to_string(),
            };

            if let Err(e) = self
                .high_risk_queue
                .enqueue_transaction(tx.clone(), tx.hash(), ai_result, risk_decision)
                .await
            {
                result.add_warning(format!("Failed to add to high-risk queue: {e}"));
            }
        }

        // 4. Record validation time
        result.validation_time_ms = start_time.elapsed().as_millis() as u64;

        // 5. Log to audit trail
        let ai_result = AIVerificationResult::Verified {
            oracle_id: "validation_engine".to_string(),
            response_id: tx.hash().clone(),
            risk_score: Some(result.risk_score),
            confidence: Some(result.confidence_score),
            processing_decision: RiskProcessingDecision::AutoApprove,
            fraud_probability: Some(result.fraud_probability),
        };

        let risk_decision = if result.risk_score > 0.7 {
            RiskProcessingDecision::RequireReview {
                reason: "High risk".to_string(),
            }
        } else {
            RiskProcessingDecision::AutoApprove
        };

        if let Err(e) = self
            .audit_trail
            .record_ai_decision(crate::consensus::audit_trail::RecordAiDecisionArgs {
                transaction: tx,
                transaction_hash: tx.hash(),
                ai_result,
                risk_decision,
                risk_priority: ReviewPriority::Medium,
                oracle_id: "validation_engine".to_string(),
                request_id: "validation_request".to_string(),
                block_number: None,
            })
            .await
        {
            result.add_warning(format!("Failed to log to audit trail: {e}"));
        }

        // 6. Update performance metrics
        self.performance_optimizer
            .record_request_metrics(result.validation_time_ms, result.is_valid)
            .await;

        Ok(result)
    }

    /// Validate transaction with optimized performance
    pub async fn validate_transaction_optimized(
        &self,
        tx: &Transaction,
    ) -> Result<ValidationResult> {
        // Use performance optimizer to determine validation strategy
        let should_use_ai = !self.performance_optimizer.should_degrade().await;

        if should_use_ai {
            self.validate_transaction(tx).await
        } else {
            // Fast path validation without AI
            self.validate_basic_transaction_fast(tx).await
        }
    }

    /// Validate transaction with queue management
    pub async fn validate_transaction_with_queue(
        &self,
        tx: &Transaction,
    ) -> Result<ValidationResult> {
        let result = self.validate_transaction(tx).await?;

        // Determine priority based on AI analysis
        if let Some(ai_analysis) = &result.ai_analysis {
            // Parse AI analysis result from response data
            let (risk_score, fraud_probability) = if let Ok(ai_result) =
                serde_json::from_value::<crate::consensus::types::AIAnalysisResult>(
                    ai_analysis.response.response_data.clone(),
                ) {
                (ai_result.risk_score, ai_result.fraud_probability)
            } else {
                (0.0, 0.0)
            };

            let risk_priority = if risk_score > 0.8 || fraud_probability > 0.7 {
                "HIGH"
            } else if risk_score > 0.5 || fraud_probability > 0.4 {
                "MEDIUM"
            } else {
                "LOW"
            };

            info!(
                "Transaction validation completed: risk={risk_score:.2}, fraud={fraud_probability:.2}, priority={risk_priority}"
            );

            // Add to high-risk queue if needed
            if risk_priority == "HIGH" {
                let ai_result = AIVerificationResult::Verified {
                    oracle_id: "validation_engine".to_string(),
                    response_id: tx.hash().clone(),
                    risk_score: Some(risk_score),
                    confidence: Some(0.8),
                    processing_decision: RiskProcessingDecision::RequireReview {
                        reason: "High risk transaction".to_string(),
                    },
                    fraud_probability: Some(fraud_probability),
                };

                let risk_decision = RiskProcessingDecision::RequireReview {
                    reason: "AI validation flagged as high risk".to_string(),
                };

                if let Err(e) = self
                    .high_risk_queue
                    .enqueue_transaction(tx.clone(), tx.hash(), ai_result, risk_decision)
                    .await
                {
                    warn!("Failed to add high-risk transaction to queue: {e}");
                }
            }
        }

        Ok(result)
    }

    /// Basic transaction validation without AI
    fn validate_basic_transaction(&self, tx: &Transaction) -> Result<()> {
        // 1. Signature policy validation (if enforcement is enabled)
        if self.policy_manager.policy().should_enforce_at_consensus() {
            self.validate_signature_policy(tx)?;
        }

        // 2. Transaction type-specific validation
        match tx {
            Transaction::Transfer(transfer_tx) => self.validate_transfer_transaction(transfer_tx),
            Transaction::AIRequest(ai_tx) => self.validate_ai_request_transaction(ai_tx),
            Transaction::Deploy(_) => {
                // Basic deploy validation
                Ok(())
            }
            Transaction::Call(_) => {
                // Basic call validation
                Ok(())
            }
            Transaction::Stake(_) => {
                // Basic stake validation
                Ok(())
            }
        }
    }

    /// Fast basic transaction validation
    async fn validate_basic_transaction_fast(&self, tx: &Transaction) -> Result<ValidationResult> {
        let start_time = std::time::Instant::now();
        let mut result = ValidationResult::success();

        if let Err(e) = self.validate_basic_transaction(tx) {
            result.add_error(format!("Basic validation failed: {e}"));
        }

        result.validation_time_ms = start_time.elapsed().as_millis() as u64;
        Ok(result)
    }

    /// Validate transfer transaction
    fn validate_transfer_transaction(&self, tx: &TransferTransaction) -> Result<()> {
        // Basic transfer validation
        if tx.amount == 0 {
            return Err(anyhow!("Transfer amount cannot be zero"));
        }

        if tx.from == tx.to {
            return Err(anyhow!("Cannot transfer to self"));
        }

        // Additional validation logic would go here
        Ok(())
    }

    /// Validate AI request transaction
    fn validate_ai_request_transaction(&self, tx: &AIRequestTransaction) -> Result<()> {
        // Validate AI request structure
        if tx.request_data.is_empty() {
            return Err(anyhow!("AI request data cannot be empty"));
        }

        // Additional AI request validation logic
        Ok(())
    }

    /// Validate transaction signature algorithm against policy
    fn validate_signature_policy(&self, _tx: &Transaction) -> Result<()> {
        // This will need to be implemented based on the actual transaction type structure
        // For now, assume all transactions use Dilithium5 (this should be extracted from actual signature)
        let signature_algorithm = dytallix_pqc::SignatureAlgorithm::Dilithium5;

        self.policy_manager
            .validate_transaction_algorithm(&signature_algorithm)
            .map_err(|e| anyhow!("Signature policy violation: {}", e))?;

        Ok(())
    }

    /// Validate transaction with AI integration
    async fn validate_with_ai(
        &self,
        tx: &Transaction,
        ai_integration: Arc<AIIntegrationManager>,
    ) -> Result<SignedAIOracleResponse> {
        // Prefix unused variable to silence warning if currently unused
        let _ai_integration = ai_integration; // retained for future use
                                              // Prepare data for AI analysis
        let mut analysis_data = HashMap::new();

        match tx {
            Transaction::Transfer(transfer_tx) => {
                analysis_data.insert("from".to_string(), Value::String(transfer_tx.from.clone()));
                analysis_data.insert("to".to_string(), Value::String(transfer_tx.to.clone()));
                analysis_data.insert(
                    "amount".to_string(),
                    Value::String(transfer_tx.amount.to_string()),
                );
                analysis_data.insert("type".to_string(), Value::String("transfer".to_string()));
            }
            Transaction::AIRequest(ai_tx) => {
                analysis_data.insert(
                    "service_type".to_string(),
                    Value::String(format!("{:?}", ai_tx.service_type)),
                );
                analysis_data.insert(
                    "request_data".to_string(),
                    Value::String(String::from_utf8_lossy(&ai_tx.request_data).to_string()),
                );
                analysis_data.insert("type".to_string(), Value::String("ai_request".to_string()));
            }
            _ => {
                analysis_data.insert("type".to_string(), Value::String("other".to_string()));
            }
        }

        // Request AI analysis
        let ai_response = self
            .ai_client
            .request_ai_analysis(AIServiceType::TransactionValidation, analysis_data)
            .await?;

        Ok(ai_response)
    }

    /// Batch validate multiple transactions
    pub async fn batch_validate_transactions(
        &self,
        transactions: Vec<Transaction>,
    ) -> Result<Vec<ValidationResult>> {
        let mut results = Vec::new();

        for tx in transactions {
            let result = self.validate_transaction(&tx).await?;
            results.push(result);
        }

        Ok(results)
    }

    /// Get validation statistics
    pub async fn get_validation_stats(&self) -> HashMap<String, Value> {
        let mut stats = HashMap::new();

        // Get performance stats
        let perf_stats = self.performance_optimizer.get_metrics().await;
        stats.insert(
            "performance".to_string(),
            serde_json::to_value(perf_stats).unwrap_or_default(),
        );

        // Get high-risk queue stats
        let queue_stats = self.high_risk_queue.get_statistics().await;
        stats.insert(
            "high_risk_queue".to_string(),
            serde_json::to_value(queue_stats).unwrap_or_default(),
        );

        stats
    }

    /// Check if AI validation is available
    pub fn has_ai_validation(&self) -> bool {
        self.ai_integration.is_some()
    }

    /// Get AI integration statistics
    pub async fn get_ai_integration_stats(&self) -> Option<Value> {
        if let Some(ai_integration) = &self.ai_integration {
            let stats = ai_integration.get_statistics().await;
            Some(serde_json::to_value(stats).unwrap_or_default())
        } else {
            None
        }
    }
}
