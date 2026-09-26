//! Transaction Review Dashboard API
//!
//! This module provides HTTP API endpoints for compliance officers to review
//! high-risk transactions in the queue. It includes endpoints for listing
//! pending transactions, approving/rejecting transactions, and viewing statistics.

use anyhow::Result;
use log::info;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::consensus::high_risk_queue::{
    HighRiskQueue, QueueStatistics, QueuedTransaction, ReviewPriority,
};

// Helper function to convert tokens (u128) to gas-sized value (u64) if it fits
fn tokens_to_gas(tokens: u128) -> Option<u64> {
    if tokens <= u64::MAX as u128 {
        Some(tokens as u64)
    } else {
        None
    }
}

/// Request to approve a transaction
#[derive(Debug, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub officer_id: String,
    pub notes: Option<String>,
}

/// Request to reject a transaction
#[derive(Debug, Serialize, Deserialize)]
pub struct RejectionRequest {
    pub officer_id: String,
    pub reason: String,
}

/// Request for bulk operations
#[derive(Debug, Serialize, Deserialize)]
pub struct BulkRequest {
    pub transaction_ids: Vec<Uuid>,
    pub officer_id: String,
    pub reason: Option<String>, // For rejections
}

/// Response for API operations
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
    pub message: Option<String>,
}

/// Detailed view of a queued transaction for the dashboard
#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionReviewView {
    pub queue_id: Uuid,
    pub transaction_hash: String,
    pub transaction_type: String,
    pub amount: Option<u64>,
    pub from_address: Option<String>,
    pub to_address: Option<String>,
    pub risk_score: f64,
    pub fraud_probability: Option<f64>,
    pub confidence: Option<f64>,
    pub priority: ReviewPriority,
    pub queued_at: chrono::DateTime<chrono::Utc>,
    pub last_updated: chrono::DateTime<chrono::Utc>,
    pub tags: Vec<String>,
    pub ai_decision_reason: String,
    pub compliance_notes: Option<String>,
}

/// Filtered list parameters
#[derive(Debug, Serialize, Deserialize)]
pub struct ListFilters {
    pub priority: Option<ReviewPriority>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub tags: Option<Vec<String>>,
}

/// Transaction Review API
pub struct TransactionReviewApi {
    queue: Arc<HighRiskQueue>,
}

impl TransactionReviewApi {
    /// Create a new transaction review API instance
    pub fn new(queue: Arc<HighRiskQueue>) -> Self {
        Self { queue }
    }

    /// Get all pending transactions with optional filtering
    pub async fn get_pending_transactions(
        &self,
        filters: Option<ListFilters>,
    ) -> Result<Vec<TransactionReviewView>> {
        let pending = self.queue.get_pending_transactions().await;
        let mut transactions: Vec<TransactionReviewView> = pending
            .into_iter()
            .map(|tx| self.convert_to_review_view(tx))
            .collect();

        // Apply filters
        if let Some(filters) = filters {
            if let Some(priority) = filters.priority {
                transactions.retain(|tx| tx.priority == priority);
            }

            if let Some(tags) = filters.tags {
                transactions.retain(|tx| tags.iter().any(|tag| tx.tags.contains(tag)));
            }

            // Apply pagination
            if let Some(offset) = filters.offset {
                if offset < transactions.len() {
                    transactions = transactions.into_iter().skip(offset).collect();
                } else {
                    transactions.clear();
                }
            }

            if let Some(limit) = filters.limit {
                transactions.truncate(limit);
            }
        }

        Ok(transactions)
    }

    /// Get a specific transaction by queue ID
    pub async fn get_transaction(&self, queue_id: Uuid) -> Result<Option<TransactionReviewView>> {
        match self.queue.get_transaction(queue_id).await {
            Some(tx) => Ok(Some(self.convert_to_review_view(tx))),
            None => Ok(None),
        }
    }

    /// Start reviewing a transaction
    pub async fn start_review(&self, queue_id: Uuid, officer_id: String) -> Result<()> {
        self.queue.start_review(queue_id, officer_id).await
    }

    /// Approve a transaction
    pub async fn approve_transaction(
        &self,
        queue_id: Uuid,
        request: ApprovalRequest,
    ) -> Result<()> {
        self.queue
            .approve_transaction(queue_id, request.officer_id, request.notes)
            .await?;
        info!("Transaction {queue_id} approved via API");
        Ok(())
    }

    /// Reject a transaction
    pub async fn reject_transaction(
        &self,
        queue_id: Uuid,
        request: RejectionRequest,
    ) -> Result<()> {
        self.queue
            .reject_transaction(queue_id, request.officer_id, request.reason)
            .await?;
        info!("Transaction {queue_id} rejected via API");
        Ok(())
    }

    /// Bulk approve transactions
    pub async fn bulk_approve(&self, request: BulkRequest) -> Result<usize> {
        let approved = self
            .queue
            .bulk_approve(request.transaction_ids, request.officer_id)
            .await?;
        let count = approved.len();
        info!("Bulk approved {count} transactions via API");
        Ok(count)
    }

    /// Bulk reject transactions
    pub async fn bulk_reject(&self, request: BulkRequest) -> Result<usize> {
        let reason = request
            .reason
            .unwrap_or_else(|| "Bulk rejection".to_string());
        let count = self
            .queue
            .bulk_reject(request.transaction_ids, request.officer_id, reason)
            .await?;
        info!("Bulk rejected {count} transactions via API");
        Ok(count)
    }

    /// Get queue statistics
    pub async fn get_statistics(&self) -> QueueStatistics {
        self.queue.get_statistics().await
    }

    /// Get transactions grouped by priority
    pub async fn get_priority_summary(
        &self,
    ) -> Result<std::collections::HashMap<ReviewPriority, Vec<TransactionReviewView>>> {
        let pending = self.queue.get_pending_transactions().await;
        let mut summary = std::collections::HashMap::new();

        for tx in pending {
            let view = self.convert_to_review_view(tx.clone());
            summary
                .entry(tx.priority)
                .or_insert_with(Vec::new)
                .push(view);
        }

        Ok(summary)
    }

    /// Convert QueuedTransaction to TransactionReviewView
    fn convert_to_review_view(&self, tx: QueuedTransaction) -> TransactionReviewView {
        let (transaction_type, amount, from_address, to_address) = match &tx.transaction {
            crate::types::Transaction::Transfer(transfer) => (
                "Transfer".to_string(),
                tokens_to_gas(transfer.amount),
                Some(transfer.from.clone()),
                Some(transfer.to.clone()),
            ),
            crate::types::Transaction::Deploy(_deploy) => ("Deploy".to_string(), None, None, None),
            crate::types::Transaction::Call(call) => (
                "Call".to_string(),
                None,
                Some(call.from.clone()),
                Some(call.to.clone()),
            ),
            crate::types::Transaction::Stake(stake) => (
                "Stake".to_string(),
                tokens_to_gas(stake.amount),
                Some(stake.validator.clone()),
                None,
            ),
            crate::types::Transaction::AIRequest(ai_req) => (
                "AIRequest".to_string(),
                None,
                Some(ai_req.from.clone()),
                None,
            ),
        };

        let (risk_score, fraud_probability, confidence) = match &tx.ai_result {
            crate::consensus::ai_integration::AIVerificationResult::Verified {
                risk_score,
                fraud_probability,
                confidence,
                ..
            } => (*risk_score, *fraud_probability, *confidence),
            crate::consensus::ai_integration::AIVerificationResult::Failed { .. } => {
                (Some(1.0), Some(1.0), Some(0.0))
            }
            crate::consensus::ai_integration::AIVerificationResult::Unavailable { .. } => {
                (Some(0.5), None, None)
            }
            crate::consensus::ai_integration::AIVerificationResult::Skipped { .. } => {
                (Some(0.3), None, Some(1.0))
            }
        };

        let ai_decision_reason = match &tx.risk_decision {
            crate::consensus::ai_integration::RiskProcessingDecision::RequireReview { reason } => {
                reason.clone()
            }
            crate::consensus::ai_integration::RiskProcessingDecision::AutoApprove => {
                "Auto-approve (should not be in queue)".to_string()
            }
            crate::consensus::ai_integration::RiskProcessingDecision::AutoReject { reason } => {
                format!("Auto-reject: {reason}")
            }
        };

        TransactionReviewView {
            queue_id: tx.queue_id,
            transaction_hash: hex::encode(&tx.transaction_hash),
            transaction_type,
            amount,
            from_address,
            to_address,
            risk_score: risk_score.unwrap_or(0.5),
            fraud_probability,
            confidence,
            priority: tx.priority,
            queued_at: tx.queued_at,
            last_updated: tx.last_updated,
            tags: tx.tags,
            ai_decision_reason,
            compliance_notes: tx.compliance_notes,
        }
    }
}

/// Mock HTTP endpoints (placeholder for actual web framework integration)
pub mod endpoints {
    use super::*;
    use serde_json::Value;

    /// Mock endpoint handler for getting pending transactions
    pub async fn get_pending_transactions_handler(
        api: Arc<TransactionReviewApi>,
        query_params: Option<Value>,
    ) -> Result<ApiResponse<Vec<TransactionReviewView>>> {
        let filters = if let Some(params) = query_params {
            serde_json::from_value(params).ok()
        } else {
            None
        };

        match api.get_pending_transactions(filters).await {
            Ok(transactions) => Ok(ApiResponse {
                success: true,
                data: Some(transactions),
                error: None,
                message: Some("Pending transactions retrieved successfully".to_string()),
            }),
            Err(e) => Ok(ApiResponse {
                success: false,
                data: None,
                error: Some(e.to_string()),
                message: None,
            }),
        }
    }

    /// Mock endpoint handler for approving a transaction
    pub async fn approve_transaction_handler(
        api: Arc<TransactionReviewApi>,
        queue_id: Uuid,
        request: ApprovalRequest,
    ) -> Result<ApiResponse<()>> {
        match api.approve_transaction(queue_id, request).await {
            Ok(_) => Ok(ApiResponse {
                success: true,
                data: Some(()),
                error: None,
                message: Some("Transaction approved successfully".to_string()),
            }),
            Err(e) => Ok(ApiResponse {
                success: false,
                data: None,
                error: Some(e.to_string()),
                message: None,
            }),
        }
    }

    /// Mock endpoint handler for rejecting a transaction
    pub async fn reject_transaction_handler(
        api: Arc<TransactionReviewApi>,
        queue_id: Uuid,
        request: RejectionRequest,
    ) -> Result<ApiResponse<()>> {
        match api.reject_transaction(queue_id, request).await {
            Ok(_) => Ok(ApiResponse {
                success: true,
                data: Some(()),
                error: None,
                message: Some("Transaction rejected successfully".to_string()),
            }),
            Err(e) => Ok(ApiResponse {
                success: false,
                data: None,
                error: Some(e.to_string()),
                message: None,
            }),
        }
    }

    /// Mock endpoint handler for bulk operations
    pub async fn bulk_approve_handler(
        api: Arc<TransactionReviewApi>,
        request: BulkRequest,
    ) -> Result<ApiResponse<usize>> {
        match api.bulk_approve(request).await {
            Ok(count) => Ok(ApiResponse {
                success: true,
                data: Some(count),
                error: None,
                message: Some(format!("Bulk approved {count} transactions")),
            }),
            Err(e) => Ok(ApiResponse {
                success: false,
                data: None,
                error: Some(e.to_string()),
                message: None,
            }),
        }
    }

    /// Mock endpoint handler for bulk reject
    pub async fn bulk_reject_handler(
        api: Arc<TransactionReviewApi>,
        request: BulkRequest,
    ) -> Result<ApiResponse<usize>> {
        match api.bulk_reject(request).await {
            Ok(count) => Ok(ApiResponse {
                success: true,
                data: Some(count),
                error: None,
                message: Some(format!("Bulk rejected {count} transactions")),
            }),
            Err(e) => Ok(ApiResponse {
                success: false,
                data: None,
                error: Some(e.to_string()),
                message: None,
            }),
        }
    }

    /// Mock endpoint handler for statistics
    pub async fn get_statistics_handler(
        api: Arc<TransactionReviewApi>,
    ) -> Result<ApiResponse<QueueStatistics>> {
        let stats = api.get_statistics().await;
        Ok(ApiResponse {
            success: true,
            data: Some(stats),
            error: None,
            message: Some("Statistics retrieved successfully".to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::ai_integration::{AIVerificationResult, RiskProcessingDecision};
    use crate::consensus::high_risk_queue::{HighRiskQueue, HighRiskQueueConfig};
    use crate::types::{Transaction, TransferTransaction};
    use chrono::Utc;

    fn create_test_queue() -> Arc<HighRiskQueue> {
        let config = HighRiskQueueConfig::default();
        Arc::new(HighRiskQueue::new(config))
    }

    fn create_test_transaction() -> Transaction {
        Transaction::Transfer(TransferTransaction {
            hash: "test_hash".to_string(),
            from: "sender123".to_string(),
            to: "recipient456".to_string(),
            amount: 1000,
            fee: 10,
            nonce: 1,
            timestamp: Utc::now().timestamp() as u64,
            signature: crate::types::PQCTransactionSignature {
                signature: dytallix_pqc::Signature {
                    data: vec![0x01, 0x02, 0x03],
                    algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium5,
                },
                public_key: vec![0x04, 0x05, 0x06],
            },
            ai_risk_score: None,
        })
    }

    fn create_test_ai_result() -> AIVerificationResult {
        AIVerificationResult::Verified {
            risk_score: Some(0.85),
            confidence: Some(0.95),
            oracle_id: "test-oracle".to_string(),
            response_id: "test-response".to_string(),
            fraud_probability: Some(0.7),
            processing_decision: RiskProcessingDecision::RequireReview {
                reason: "High risk score".to_string(),
            },
        }
    }

    #[tokio::test]
    async fn test_api_get_pending_transactions() {
        let queue = create_test_queue();
        let api = TransactionReviewApi::new(queue.clone());

        // Add a test transaction to the queue
        let transaction = create_test_transaction();
        let tx_hash = [0u8; 32];
        let ai_result = create_test_ai_result();
        let risk_decision = RiskProcessingDecision::RequireReview {
            reason: "Test".to_string(),
        };

        queue
            .enqueue_transaction(transaction, hex::encode(tx_hash), ai_result, risk_decision)
            .await
            .unwrap();

        // Test getting pending transactions
        let pending = api.get_pending_transactions(None).await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].transaction_type, "Transfer");
        assert_eq!(pending[0].amount, Some(1000));
    }

    #[tokio::test]
    async fn test_api_approve_transaction() {
        let queue = create_test_queue();
        let api = TransactionReviewApi::new(queue.clone());

        // Add a test transaction to the queue
        let transaction = create_test_transaction();
        let tx_hash = [0u8; 32];
        let ai_result = create_test_ai_result();
        let risk_decision = RiskProcessingDecision::RequireReview {
            reason: "Test".to_string(),
        };

        let queue_id = queue
            .enqueue_transaction(transaction, hex::encode(tx_hash), ai_result, risk_decision)
            .await
            .unwrap();

        // Start review
        api.start_review(queue_id, "officer1".to_string())
            .await
            .unwrap();

        // Approve the transaction
        let approval_request = ApprovalRequest {
            officer_id: "officer1".to_string(),
            notes: Some("Looks good".to_string()),
        };

        api.approve_transaction(queue_id, approval_request)
            .await
            .unwrap();

        // Verify the transaction is approved
        let tx = queue.get_transaction(queue_id).await.unwrap();
        assert!(matches!(
            tx.status,
            crate::consensus::high_risk_queue::ReviewStatus::Approved { .. }
        ));
    }

    #[tokio::test]
    async fn test_api_statistics() {
        let queue = create_test_queue();
        let api = TransactionReviewApi::new(queue.clone());

        // Add a test transaction to the queue
        let transaction = create_test_transaction();
        let tx_hash = [0u8; 32];
        let ai_result = create_test_ai_result();
        let risk_decision = RiskProcessingDecision::RequireReview {
            reason: "Test".to_string(),
        };

        queue
            .enqueue_transaction(transaction, hex::encode(tx_hash), ai_result, risk_decision)
            .await
            .unwrap();

        // Test getting statistics
        let stats = api.get_statistics().await;
        assert_eq!(stats.total_pending, 1);
        assert_eq!(stats.total_in_review, 0);
    }
}
