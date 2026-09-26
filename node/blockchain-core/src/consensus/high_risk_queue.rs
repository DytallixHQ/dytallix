//! High-Risk Transaction Queue System
//!
//! This module implements a queuing system for transactions flagged as high-risk
//! by the AI analysis system. It provides manual review workflow capabilities,
//! notification systems, and bulk approval/rejection functionality.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::consensus::ai_integration::{AIVerificationResult, RiskProcessingDecision};
use crate::consensus::notification_system::{NotificationSystem, NotificationSystemConfig};
use crate::consensus::notification_types::NotificationType;
use crate::types::{Transaction, TxHash};

/// Status of a transaction in the high-risk queue
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReviewStatus {
    /// Transaction is pending manual review
    Pending,
    /// Transaction is currently being reviewed by an officer
    InReview {
        officer_id: String,
        started_at: DateTime<Utc>,
    },
    /// Transaction has been approved for processing
    Approved {
        officer_id: String,
        approved_at: DateTime<Utc>,
        notes: Option<String>,
    },
    /// Transaction has been rejected
    Rejected {
        officer_id: String,
        rejected_at: DateTime<Utc>,
        reason: String,
    },
    /// Transaction was auto-expired due to timeout
    Expired { expired_at: DateTime<Utc> },
}

/// Priority level for high-risk transactions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReviewPriority {
    Low,
    Medium,
    High,
    Critical,
}

/// Information about a transaction queued for review
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedTransaction {
    pub queue_id: Uuid,
    pub transaction: Transaction,
    pub transaction_hash: TxHash,
    pub ai_result: AIVerificationResult,
    pub risk_decision: RiskProcessingDecision,
    pub priority: ReviewPriority,
    pub status: ReviewStatus,
    pub queued_at: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    pub tags: Vec<String>,
    pub compliance_notes: Option<String>,
}

/// Statistics about the review queue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStatistics {
    pub total_pending: usize,
    pub total_in_review: usize,
    pub total_approved_today: usize,
    pub total_rejected_today: usize,
    pub average_review_time_minutes: f64,
    pub oldest_pending_age_hours: f64,
    pub priority_breakdown: HashMap<ReviewPriority, usize>,
}

/// Configuration for the high-risk queue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HighRiskQueueConfig {
    /// Maximum number of active records, including pending and in-review records
    pub max_queue_size: usize,
    /// Maximum time a transaction can stay in queue before auto-expiring (hours)
    pub max_queue_time_hours: u64,
    /// Maximum time a transaction can be "in review" before timing out (hours)
    pub max_review_time_hours: u64,
    /// Enable email notifications to compliance officers
    pub enable_notifications: bool,
    /// Enable automatic prioritization based on risk scores
    pub enable_auto_prioritization: bool,
    /// Minimum risk score to escalate to high priority
    pub high_priority_threshold: f64,
    /// Minimum risk score to escalate to critical priority
    pub critical_priority_threshold: f64,
}

impl Default for HighRiskQueueConfig {
    fn default() -> Self {
        Self {
            max_queue_size: 1000,
            max_queue_time_hours: 72,  // 3 days
            max_review_time_hours: 24, // 1 day
            enable_notifications: true,
            enable_auto_prioritization: true,
            high_priority_threshold: 0.8,
            critical_priority_threshold: 0.9,
        }
    }
}

/// Canonical queue records and their indexes share one lock.
#[derive(Debug, Default)]
struct QueueState {
    pending: VecDeque<Uuid>,
    transactions: HashMap<Uuid, QueuedTransaction>,
    active_hashes: HashMap<TxHash, Uuid>,
}

/// High-risk transaction queue manager.
#[derive(Debug)]
pub struct HighRiskQueue {
    config: HighRiskQueueConfig,
    state: RwLock<QueueState>,
    notification_queue: Arc<Mutex<VecDeque<NotificationType>>>,
    notification_system: Option<Arc<NotificationSystem>>,
}

impl HighRiskQueue {
    /// Create a new high-risk transaction queue
    pub fn new(config: HighRiskQueueConfig) -> Self {
        let notification_system = NotificationSystem::new(NotificationSystemConfig {
            enable_email: true,  // Enable email notifications by default
            enable_in_app: true, // Enable in-app notifications
            ..Default::default()
        });

        Self {
            config,
            state: RwLock::new(QueueState::default()),
            notification_queue: Arc::new(Mutex::new(VecDeque::new())),
            notification_system: Some(Arc::new(notification_system)),
        }
    }

    /// Add a transaction to the high-risk queue
    pub async fn enqueue_transaction(
        &self,
        transaction: Transaction,
        transaction_hash: TxHash,
        ai_result: AIVerificationResult,
        risk_decision: RiskProcessingDecision,
    ) -> Result<Uuid> {
        let queue_id = Uuid::new_v4();
        let now = Utc::now();

        // Determine priority based on AI result
        let priority = self.calculate_priority(&ai_result);

        // Create tags based on the risk decision and AI result
        let tags = self.generate_tags(&risk_decision, &ai_result);

        let queued_transaction = QueuedTransaction {
            queue_id,
            transaction,
            transaction_hash: transaction_hash.clone(),
            ai_result: ai_result.clone(),
            risk_decision: risk_decision.clone(),
            priority: priority.clone(),
            status: ReviewStatus::Pending,
            queued_at: now,
            last_updated: now,
            tags,
            compliance_notes: None,
        };

        // Validate and insert without releasing the state lock.
        {
            let mut state = self.state.write().await;
            if state.active_hashes.contains_key(&transaction_hash) {
                return Err(anyhow!("Transaction already in queue: {transaction_hash}"));
            }
            let current_size = state.active_hashes.len();
            if current_size >= self.config.max_queue_size {
                drop(state);
                self.send_notification(NotificationType::QueueCapacityWarning {
                    current_size,
                    max_size: self.config.max_queue_size,
                    warning_level: 3,
                })
                .await;
                return Err(anyhow!(
                    "Queue is at capacity: {}/{}",
                    current_size,
                    self.config.max_queue_size
                ));
            }
            let position = state
                .pending
                .iter()
                .position(|id| state.transactions[id].priority < priority)
                .unwrap_or(state.pending.len());
            state.pending.insert(position, queue_id);
            state.transactions.insert(queue_id, queued_transaction);
            state
                .active_hashes
                .insert(transaction_hash.clone(), queue_id);
        }

        // Send notification for high/critical priority transactions
        if matches!(priority, ReviewPriority::High | ReviewPriority::Critical) {
            let risk_score = match &ai_result {
                AIVerificationResult::Verified {
                    risk_score: Some(score),
                    ..
                } => *score,
                AIVerificationResult::Verified {
                    fraud_probability: Some(prob),
                    ..
                } => *prob,
                _ => 0.5, // Default risk score if not available
            };

            self.send_notification(NotificationType::NewHighRiskTransaction {
                queue_id,
                transaction_hash: hex::encode(&transaction_hash),
                risk_score,
                priority: priority.clone(),
            })
            .await;
        }

        info!(
            "Transaction {} queued for review with priority {:?} (queue ID: {})",
            hex::encode(&transaction_hash),
            priority,
            queue_id
        );

        Ok(queue_id)
    }

    /// Select the next pending record without removing it.
    /// Call start_review to claim this ID, or use claim_next_for_review atomically.
    pub async fn get_next_for_review(&self) -> Option<QueuedTransaction> {
        let state = self.state.read().await;
        state
            .pending
            .front()
            .and_then(|id| state.transactions.get(id))
            .cloned()
    }

    /// Claim the highest-priority pending record in one state transition.
    pub async fn claim_next_for_review(&self, officer_id: String) -> Option<QueuedTransaction> {
        let mut state = self.state.write().await;
        let id = *state.pending.front()?;
        Self::start_review_locked(&mut state, id, officer_id).ok()?;
        state.transactions.get(&id).cloned()
    }

    fn start_review_locked(
        state: &mut QueueState,
        queue_id: Uuid,
        officer_id: String,
    ) -> Result<()> {
        let transaction = state
            .transactions
            .get_mut(&queue_id)
            .ok_or_else(|| anyhow!("Transaction not found in queue: {queue_id}"))?;
        if !matches!(transaction.status, ReviewStatus::Pending) {
            return Err(anyhow!(
                "Transaction is not pending: {:?}",
                transaction.status
            ));
        }
        let now = Utc::now();
        transaction.status = ReviewStatus::InReview {
            officer_id,
            started_at: now,
        };
        transaction.last_updated = now;
        state.pending.retain(|id| *id != queue_id);
        Ok(())
    }

    pub async fn start_review(&self, queue_id: Uuid, officer_id: String) -> Result<()> {
        let mut state = self.state.write().await;
        Self::start_review_locked(&mut state, queue_id, officer_id)
    }

    fn finish_review_locked(
        state: &mut QueueState,
        queue_id: Uuid,
        status: ReviewStatus,
    ) -> Result<QueuedTransaction> {
        let transaction = state
            .transactions
            .get_mut(&queue_id)
            .ok_or_else(|| anyhow!("Transaction not found in queue: {queue_id}"))?;
        if !matches!(
            transaction.status,
            ReviewStatus::Pending | ReviewStatus::InReview { .. }
        ) {
            return Err(anyhow!(
                "Transaction already has a terminal review status: {:?}",
                transaction.status
            ));
        }
        transaction.status = status;
        transaction.last_updated = Utc::now();
        let result = transaction.clone();
        state.active_hashes.remove(&result.transaction_hash);
        state.pending.retain(|id| *id != queue_id);
        Ok(result)
    }

    /// Approve a pending or in-review record once. Authorization belongs to the caller.
    pub async fn approve_transaction(
        &self,
        queue_id: Uuid,
        officer_id: String,
        notes: Option<String>,
    ) -> Result<QueuedTransaction> {
        let mut state = self.state.write().await;
        Self::finish_review_locked(
            &mut state,
            queue_id,
            ReviewStatus::Approved {
                officer_id,
                approved_at: Utc::now(),
                notes,
            },
        )
    }

    /// Reject a pending or in-review record once. Terminal records cannot change status.
    pub async fn reject_transaction(
        &self,
        queue_id: Uuid,
        officer_id: String,
        reason: String,
    ) -> Result<()> {
        let mut state = self.state.write().await;
        Self::finish_review_locked(
            &mut state,
            queue_id,
            ReviewStatus::Rejected {
                officer_id,
                rejected_at: Utc::now(),
                reason,
            },
        )?;
        Ok(())
    }

    pub async fn get_pending_transactions(&self) -> Vec<QueuedTransaction> {
        let state = self.state.read().await;
        state
            .pending
            .iter()
            .filter_map(|id| state.transactions.get(id))
            .cloned()
            .collect()
    }

    pub async fn get_transaction(&self, queue_id: Uuid) -> Option<QueuedTransaction> {
        self.state.read().await.transactions.get(&queue_id).cloned()
    }

    /// Calculate statistics from one current snapshot. Reads do not accumulate counts.
    pub async fn get_statistics(&self) -> QueueStatistics {
        let state = self.state.read().await;
        Self::calculate_stats(&state.transactions, Utc::now())
    }

    /// Bulk approve transactions
    pub async fn bulk_approve(
        &self,
        queue_ids: Vec<Uuid>,
        officer_id: String,
    ) -> Result<Vec<QueuedTransaction>> {
        let mut approved = Vec::new();

        for queue_id in queue_ids {
            match self
                .approve_transaction(queue_id, officer_id.clone(), None)
                .await
            {
                Ok(transaction) => approved.push(transaction),
                Err(e) => warn!("Failed to approve transaction {queue_id}: {e}"),
            }
        }

        info!(
            "Officer {} bulk approved {} transactions",
            officer_id,
            approved.len()
        );
        Ok(approved)
    }

    /// Bulk reject transactions
    pub async fn bulk_reject(
        &self,
        queue_ids: Vec<Uuid>,
        officer_id: String,
        reason: String,
    ) -> Result<usize> {
        let mut rejected_count = 0;

        for queue_id in queue_ids {
            match self
                .reject_transaction(queue_id, officer_id.clone(), reason.clone())
                .await
            {
                Ok(_) => rejected_count += 1,
                Err(e) => warn!("Failed to reject transaction {queue_id}: {e}"),
            }
        }

        info!("Officer {officer_id} bulk rejected {rejected_count} transactions");
        Ok(rejected_count)
    }

    /// Expire eligible records under the same lock used by review decisions.
    pub async fn cleanup_expired(&self) -> Result<usize> {
        let duration = |hours: u64| -> Result<chrono::Duration> {
            let hours = i64::try_from(hours)
                .map_err(|_| anyhow!("Queue expiry duration is out of range"))?;
            chrono::Duration::try_hours(hours)
                .ok_or_else(|| anyhow!("Queue expiry duration is out of range"))
        };
        let max_age = duration(self.config.max_queue_time_hours)?;
        let max_review_time = duration(self.config.max_review_time_hours)?;
        let notifications = {
            let mut state = self.state.write().await;
            let now = Utc::now();
            let ids: Vec<_> = state
                .transactions
                .iter()
                .filter_map(|(id, transaction)| {
                    let expired = match transaction.status {
                        ReviewStatus::Pending => now - transaction.queued_at > max_age,
                        ReviewStatus::InReview { started_at, .. } => {
                            now - started_at > max_review_time
                        }
                        _ => false,
                    };
                    expired.then_some(*id)
                })
                .collect();
            let mut notifications = Vec::with_capacity(ids.len());
            for id in ids {
                let transaction = Self::finish_review_locked(
                    &mut state,
                    id,
                    ReviewStatus::Expired { expired_at: now },
                )?;
                notifications.push(NotificationType::TransactionExpired {
                    queue_id: id,
                    transaction_hash: hex::encode(&transaction.transaction_hash),
                    expiry_time: now,
                });
            }
            notifications
        };
        let count = notifications.len();
        // External notification work never holds the queue state lock.
        for notification in notifications {
            self.send_notification(notification).await;
        }
        Ok(count)
    }

    /// Calculate priority based on AI result
    fn calculate_priority(&self, ai_result: &AIVerificationResult) -> ReviewPriority {
        if !self.config.enable_auto_prioritization {
            return ReviewPriority::Medium;
        }

        let risk_score = match ai_result {
            AIVerificationResult::Verified { risk_score, .. } => risk_score.unwrap_or(0.5),
            AIVerificationResult::Failed { .. } => 1.0, // Max risk for failed verification
            AIVerificationResult::Unavailable { .. } => 0.7, // High risk for unavailable
            AIVerificationResult::Skipped { .. } => 0.3, // Low risk for skipped
        };

        if risk_score >= self.config.critical_priority_threshold {
            ReviewPriority::Critical
        } else if risk_score >= self.config.high_priority_threshold {
            ReviewPriority::High
        } else if risk_score >= 0.6 {
            ReviewPriority::Medium
        } else {
            ReviewPriority::Low
        }
    }

    /// Generate tags based on risk decision and AI result
    fn generate_tags(
        &self,
        risk_decision: &RiskProcessingDecision,
        ai_result: &AIVerificationResult,
    ) -> Vec<String> {
        let mut tags = Vec::new();

        // Add tag based on decision reason
        if let RiskProcessingDecision::RequireReview { reason } = risk_decision {
            tags.push(format!("review-reason:{reason}"));
        }

        // Add tags based on AI result
        match ai_result {
            AIVerificationResult::Verified {
                risk_score,
                fraud_probability,
                ..
            } => {
                if let Some(score) = risk_score {
                    if *score > 0.8 {
                        tags.push("high-risk".to_string());
                    }
                }
                if let Some(fraud_prob) = fraud_probability {
                    if *fraud_prob > 0.7 {
                        tags.push("fraud-risk".to_string());
                    }
                }
            }
            AIVerificationResult::Failed { .. } => {
                tags.push("verification-failed".to_string());
            }
            AIVerificationResult::Unavailable { .. } => {
                tags.push("ai-unavailable".to_string());
            }
            AIVerificationResult::Skipped { .. } => {
                tags.push("ai-skipped".to_string());
            }
        }

        tags
    }

    /// Send a notification (placeholder implementation)
    async fn send_notification(&self, notification: NotificationType) {
        if !self.config.enable_notifications {
            return;
        }

        let mut queue = self.notification_queue.lock().await;
        queue.push_back(notification.clone());

        // Log the notification for now
        match notification {
            NotificationType::NewHighRiskTransaction {
                queue_id,
                transaction_hash: _,
                risk_score: _,
                ref priority,
            } => {
                warn!("🚨 New {priority:?} priority transaction queued for review: {queue_id}");
            }
            NotificationType::TransactionExpired { queue_id, .. } => {
                warn!("⏰ Transaction expired in queue: {queue_id}");
            }
            NotificationType::ReviewTimeout {
                queue_id,
                ref officer_id,
                ..
            } => {
                warn!("⏰ Review timeout for transaction {queue_id} (officer: {officer_id})");
            }
            NotificationType::QueueCapacityWarning {
                current_size,
                max_size,
                ..
            } => {
                warn!("⚠️ Queue approaching capacity: {current_size}/{max_size}");
            }
            _ => {
                // Handle other notification types generically
                info!("Notification sent: {}", notification.title());
            }
        }

        // Send notification through the notification system
        if let Some(notification_system) = &self.notification_system {
            let _ = notification_system.send_notification(notification).await;
        }
    }

    fn calculate_stats(
        transactions: &HashMap<Uuid, QueuedTransaction>,
        now: DateTime<Utc>,
    ) -> QueueStatistics {
        let mut stats = QueueStatistics {
            total_pending: 0,
            total_in_review: 0,
            total_approved_today: 0,
            total_rejected_today: 0,
            average_review_time_minutes: 0.0,
            oldest_pending_age_hours: 0.0,
            priority_breakdown: HashMap::new(),
        };
        let mut oldest_pending: Option<DateTime<Utc>> = None;
        let mut review_times = Vec::new();

        // Calculate statistics
        for transaction in transactions.values() {
            match &transaction.status {
                ReviewStatus::Pending => {
                    stats.total_pending += 1;
                    if oldest_pending.is_none() || transaction.queued_at < oldest_pending.unwrap() {
                        oldest_pending = Some(transaction.queued_at);
                    }
                    *stats
                        .priority_breakdown
                        .entry(transaction.priority.clone())
                        .or_insert(0) += 1;
                }
                ReviewStatus::InReview { .. } => {
                    stats.total_in_review += 1;
                    *stats
                        .priority_breakdown
                        .entry(transaction.priority.clone())
                        .or_insert(0) += 1;
                }
                ReviewStatus::Approved { approved_at, .. } => {
                    if approved_at.date_naive() == now.date_naive() {
                        stats.total_approved_today += 1;
                    }
                    let review_time = (*approved_at - transaction.queued_at).num_minutes() as f64;
                    review_times.push(review_time);
                }
                ReviewStatus::Rejected { rejected_at, .. } => {
                    if rejected_at.date_naive() == now.date_naive() {
                        stats.total_rejected_today += 1;
                    }
                    let review_time = (*rejected_at - transaction.queued_at).num_minutes() as f64;
                    review_times.push(review_time);
                }
                _ => {}
            }
        }

        // Calculate average review time
        if !review_times.is_empty() {
            stats.average_review_time_minutes =
                review_times.iter().sum::<f64>() / review_times.len() as f64;
        }

        // Calculate oldest pending age
        if let Some(oldest) = oldest_pending {
            stats.oldest_pending_age_hours = (now - oldest).num_hours() as f64;
        }

        stats
    }

    /// Get pending notifications (for compliance officers)
    pub async fn get_notifications(&self) -> Vec<NotificationType> {
        let mut queue = self.notification_queue.lock().await;
        let notifications: Vec<_> = queue.drain(..).collect();
        notifications
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::ai_integration::RiskProcessingDecision;
    use crate::types::TransferTransaction;

    fn create_test_transaction() -> Transaction {
        Transaction::Transfer(TransferTransaction {
            hash: "test_tx_123".to_string(),
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
            ai_risk_score: Some(0.75),
        })
    }

    fn create_test_ai_result(risk_score: f64) -> AIVerificationResult {
        AIVerificationResult::Verified {
            risk_score: Some(risk_score),
            confidence: Some(0.95),
            oracle_id: "test-oracle".to_string(),
            response_id: "test-response".to_string(),
            fraud_probability: Some(risk_score * 0.8),
            processing_decision: RiskProcessingDecision::RequireReview {
                reason: "High risk score".to_string(),
            },
        }
    }

    #[tokio::test]
    async fn test_enqueue_transaction() {
        let config = HighRiskQueueConfig::default();
        let queue = HighRiskQueue::new(config);

        let transaction = create_test_transaction();
        let tx_hash = [0u8; 32];
        let ai_result = create_test_ai_result(0.85);
        let risk_decision = RiskProcessingDecision::RequireReview {
            reason: "High risk score".to_string(),
        };

        let queue_id = queue
            .enqueue_transaction(transaction, hex::encode(tx_hash), ai_result, risk_decision)
            .await
            .unwrap();

        let stats = queue.get_statistics().await;
        assert_eq!(stats.total_pending, 1);

        let pending = queue.get_pending_transactions().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].queue_id, queue_id);
    }

    #[tokio::test]
    async fn test_priority_ordering() {
        let config = HighRiskQueueConfig::default();
        let queue = HighRiskQueue::new(config);

        // Add transactions with different risk scores
        let tx1 = create_test_transaction();
        let tx2 = create_test_transaction();
        let tx3 = create_test_transaction();

        let low_risk = create_test_ai_result(0.3);
        let high_risk = create_test_ai_result(0.85);
        let critical_risk = create_test_ai_result(0.95);

        let risk_decision = RiskProcessingDecision::RequireReview {
            reason: "Test".to_string(),
        };

        // Add in reverse priority order
        queue
            .enqueue_transaction(tx1, hex::encode([1u8; 32]), low_risk, risk_decision.clone())
            .await
            .unwrap();
        queue
            .enqueue_transaction(
                tx2,
                hex::encode([2u8; 32]),
                high_risk,
                risk_decision.clone(),
            )
            .await
            .unwrap();
        queue
            .enqueue_transaction(tx3, hex::encode([3u8; 32]), critical_risk, risk_decision)
            .await
            .unwrap();

        let pending = queue.get_pending_transactions().await;
        assert_eq!(pending.len(), 3);

        // Should be ordered by priority: Critical, High, Low
        assert_eq!(pending[0].priority, ReviewPriority::Critical);
        assert_eq!(pending[1].priority, ReviewPriority::High);
        assert_eq!(pending[2].priority, ReviewPriority::Low);
    }

    #[tokio::test]
    async fn test_review_workflow() {
        let config = HighRiskQueueConfig::default();
        let queue = HighRiskQueue::new(config);

        let transaction = create_test_transaction();
        let tx_hash = [0u8; 32];
        let ai_result = create_test_ai_result(0.85);
        let risk_decision = RiskProcessingDecision::RequireReview {
            reason: "High risk score".to_string(),
        };

        let queue_id = queue
            .enqueue_transaction(transaction, hex::encode(tx_hash), ai_result, risk_decision)
            .await
            .unwrap();

        // Start review
        queue
            .start_review(queue_id, "officer1".to_string())
            .await
            .unwrap();

        let tx = queue.get_transaction(queue_id).await.unwrap();
        assert!(matches!(tx.status, ReviewStatus::InReview { .. }));

        // Approve transaction
        let approved = queue
            .approve_transaction(
                queue_id,
                "officer1".to_string(),
                Some("Looks good".to_string()),
            )
            .await
            .unwrap();

        assert!(matches!(approved.status, ReviewStatus::Approved { .. }));
    }

    #[tokio::test]
    async fn test_bulk_operations() {
        let config = HighRiskQueueConfig::default();
        let queue = HighRiskQueue::new(config);

        let mut queue_ids = Vec::new();
        let risk_decision = RiskProcessingDecision::RequireReview {
            reason: "Bulk test".to_string(),
        };

        // Add multiple transactions
        for i in 0..5 {
            let transaction = create_test_transaction();
            let mut tx_hash = [0u8; 32];
            tx_hash[0] = i;
            let ai_result = create_test_ai_result(0.75);

            let queue_id = queue
                .enqueue_transaction(
                    transaction,
                    hex::encode(tx_hash),
                    ai_result,
                    risk_decision.clone(),
                )
                .await
                .unwrap();
            queue_ids.push(queue_id);
        }

        // Bulk approve first 3
        let approved = queue
            .bulk_approve(queue_ids[0..3].to_vec(), "officer1".to_string())
            .await
            .unwrap();
        assert_eq!(approved.len(), 3);

        // Bulk reject last 2
        let rejected_count = queue
            .bulk_reject(
                queue_ids[3..5].to_vec(),
                "officer1".to_string(),
                "Bulk rejection test".to_string(),
            )
            .await
            .unwrap();
        assert_eq!(rejected_count, 2);
    }

    #[cfg(test)]
    mod integration_tests {
        use super::*;
        use crate::consensus::ai_integration::{AIVerificationResult, RiskProcessingDecision};
        use crate::types::{PQCTransactionSignature, TransferTransaction};

        #[tokio::test]
        async fn test_end_to_end_queue_workflow() {
            // Create a queue with default config
            let config = HighRiskQueueConfig::default();
            let queue = HighRiskQueue::new(config);

            // Create a sample transaction
            let tx = Transaction::Transfer(TransferTransaction {
                hash: "test_tx_123".to_string(),
                from: "alice".to_string(),
                to: "bob".to_string(),
                amount: 1000,
                fee: 10,
                nonce: 1,
                timestamp: 1234567890,
                signature: PQCTransactionSignature {
                    signature: dytallix_pqc::Signature {
                        data: vec![],
                        algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium5,
                    },
                    public_key: vec![],
                },
                ai_risk_score: Some(0.9),
            });

            // Create AI result indicating high risk requiring review
            let ai_result = AIVerificationResult::Verified {
                risk_score: Some(0.9),
                processing_decision: RiskProcessingDecision::RequireReview {
                    reason: "High fraud probability detected".to_string(),
                },
                fraud_probability: Some(0.85),
                confidence: Some(0.95),
                oracle_id: "test_oracle".to_string(),
                response_id: "test_response".to_string(),
            };

            // Enqueue the transaction
            let queue_id = queue
                .enqueue_transaction(
                    tx,
                    "test_tx_123".to_string(),
                    ai_result,
                    RiskProcessingDecision::RequireReview {
                        reason: "High fraud probability detected".to_string(),
                    },
                )
                .await
                .expect("Failed to enqueue transaction");

            // Verify the transaction is in the queue
            let pending = queue.get_pending_transactions().await;
            assert_eq!(pending.len(), 1);
            assert_eq!(pending[0].queue_id, queue_id);

            // Approve the transaction
            let result = queue
                .approve_transaction(
                    queue_id,
                    "compliance_officer_1".to_string(),
                    Some("Approved after review".to_string()),
                )
                .await;
            assert!(result.is_ok());

            // Verify the transaction is no longer pending
            let pending_after = queue.get_pending_transactions().await;
            assert_eq!(pending_after.len(), 0);

            // Check statistics
            let stats = queue.get_statistics().await;
            assert_eq!(stats.total_approved_today, 1);
            assert_eq!(stats.total_pending, 0);
        }

        #[tokio::test]
        async fn test_notification_integration() {
            let config = HighRiskQueueConfig::default();
            let queue = HighRiskQueue::new(config);

            // Create a transaction that will trigger notifications
            let tx = Transaction::Transfer(TransferTransaction {
                hash: "notification_test_tx".to_string(),
                from: "user1".to_string(),
                to: "user2".to_string(),
                amount: 5000,
                fee: 20,
                nonce: 1,
                timestamp: 1234567890,
                signature: PQCTransactionSignature {
                    signature: dytallix_pqc::Signature {
                        data: vec![],
                        algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium5,
                    },
                    public_key: vec![],
                },
                ai_risk_score: Some(0.95),
            });

            let ai_result = AIVerificationResult::Verified {
                risk_score: Some(0.95),
                processing_decision: RiskProcessingDecision::RequireReview {
                    reason: "Suspicious transaction pattern".to_string(),
                },
                fraud_probability: Some(0.9),
                confidence: Some(0.98),
                oracle_id: "test_oracle".to_string(),
                response_id: "test_response".to_string(),
            };

            // This should trigger a notification
            let _queue_id = queue
                .enqueue_transaction(
                    tx,
                    "notification_test_tx".to_string(),
                    ai_result,
                    RiskProcessingDecision::RequireReview {
                        reason: "Suspicious transaction pattern".to_string(),
                    },
                )
                .await
                .expect("Failed to enqueue transaction");

            // Verify notification was queued (basic check)
            let notification_queue = queue.notification_queue.lock().await;
            assert!(!notification_queue.is_empty());
        }
    }

    fn atomic_queue(capacity: usize) -> Arc<HighRiskQueue> {
        Arc::new(HighRiskQueue::new(HighRiskQueueConfig {
            max_queue_size: capacity,
            enable_notifications: false,
            ..HighRiskQueueConfig::default()
        }))
    }

    async fn add(queue: &HighRiskQueue, hash: &str) -> Result<Uuid> {
        queue
            .enqueue_transaction(
                create_test_transaction(),
                hash.to_owned(),
                create_test_ai_result(0.85),
                RiskProcessingDecision::RequireReview {
                    reason: "fixture".into(),
                },
            )
            .await
    }

    async fn bounded<F: std::future::Future>(future: F) -> F::Output {
        tokio::time::timeout(std::time::Duration::from_secs(5), future)
            .await
            .expect("queue operation did not complete")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_duplicate_admission_accepts_one_record() {
        bounded(async {
            let queue = atomic_queue(32);
            let barrier = Arc::new(tokio::sync::Barrier::new(24));
            let mut tasks = tokio::task::JoinSet::new();
            for _ in 0..24 {
                let queue = queue.clone();
                let barrier = barrier.clone();
                tasks.spawn(async move {
                    barrier.wait().await;
                    add(&queue, "same-hash").await
                });
            }
            let mut accepted = 0;
            while let Some(result) = tasks.join_next().await {
                accepted += usize::from(result.unwrap().is_ok());
            }
            assert_eq!(accepted, 1);
            assert_eq!(queue.get_pending_transactions().await.len(), 1);
            assert_eq!(queue.get_statistics().await.total_pending, 1);
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_admission_obeys_active_capacity() {
        bounded(async {
            let queue = atomic_queue(8);
            let barrier = Arc::new(tokio::sync::Barrier::new(24));
            let mut tasks = tokio::task::JoinSet::new();
            for i in 0..24 {
                let queue = queue.clone();
                let barrier = barrier.clone();
                tasks.spawn(async move {
                    barrier.wait().await;
                    add(&queue, &format!("capacity-{i}")).await
                });
            }
            let mut accepted = 0;
            while let Some(result) = tasks.join_next().await {
                accepted += usize::from(result.unwrap().is_ok());
            }
            assert_eq!(accepted, 8);
            let claimed = queue.claim_next_for_review("officer".into()).await.unwrap();
            assert!(add(&queue, "still-full").await.is_err());
            let stats = queue.get_statistics().await;
            assert_eq!((stats.total_pending, stats.total_in_review), (7, 1));
            queue
                .approve_transaction(claimed.queue_id, "officer".into(), None)
                .await
                .unwrap();
            add(&queue, "freed-slot").await.unwrap();
            assert_eq!(queue.get_pending_transactions().await.len(), 8);
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_claims_return_each_record_once() {
        bounded(async {
            let queue = atomic_queue(16);
            let mut expected = std::collections::HashSet::new();
            for i in 0..16 {
                expected.insert(add(&queue, &format!("claim-{i}")).await.unwrap());
            }
            let mut tasks = tokio::task::JoinSet::new();
            for i in 0..24 {
                let queue = queue.clone();
                tasks.spawn(
                    async move { queue.claim_next_for_review(format!("officer-{i}")).await },
                );
            }
            let mut claimed = std::collections::HashSet::new();
            while let Some(result) = tasks.join_next().await {
                if let Some(record) = result.unwrap() {
                    assert!(matches!(record.status, ReviewStatus::InReview { .. }));
                    assert!(claimed.insert(record.queue_id));
                }
            }
            assert_eq!(claimed, expected);
            assert!(queue.get_pending_transactions().await.is_empty());
            assert_eq!(queue.get_statistics().await.total_in_review, 16);
        })
        .await;
    }

    #[tokio::test]
    async fn selection_preserves_pending_record_until_claim() {
        bounded(async {
            let queue = atomic_queue(2);
            let first = add(&queue, "first").await.unwrap();
            let second = add(&queue, "second").await.unwrap();
            for _ in 0..2 {
                assert_eq!(queue.get_next_for_review().await.unwrap().queue_id, first);
            }
            queue.start_review(first, "officer".into()).await.unwrap();
            assert!(queue.start_review(first, "other".into()).await.is_err());
            assert_eq!(queue.get_next_for_review().await.unwrap().queue_id, second);
            assert_eq!(queue.get_pending_transactions().await.len(), 1);
        })
        .await;
    }

    #[tokio::test]
    async fn terminal_decision_cannot_change_or_remove_new_hash_owner() {
        bounded(async {
            let queue = atomic_queue(2);
            let first = add(&queue, "reused-hash").await.unwrap();
            queue
                .approve_transaction(first, "officer".into(), None)
                .await
                .unwrap();
            let second = add(&queue, "reused-hash").await.unwrap();
            assert!(queue
                .reject_transaction(first, "other".into(), "late".into())
                .await
                .is_err());
            assert!(queue
                .approve_transaction(first, "other".into(), None)
                .await
                .is_err());
            assert!(add(&queue, "reused-hash").await.is_err());
            assert_eq!(queue.get_pending_transactions().await[0].queue_id, second);
            assert!(matches!(
                queue.get_transaction(first).await.unwrap().status,
                ReviewStatus::Approved { .. }
            ));
        })
        .await;
    }

    #[tokio::test]
    async fn statistics_do_not_accumulate_and_empty_queue_clears_age() {
        bounded(async {
            let queue = atomic_queue(3);
            let id = add(&queue, "approved").await.unwrap();
            queue
                .approve_transaction(id, "officer".into(), None)
                .await
                .unwrap();
            let id = add(&queue, "rejected").await.unwrap();
            queue
                .reject_transaction(id, "officer".into(), "fixture".into())
                .await
                .unwrap();
            for _ in 0..3 {
                let stats = queue.get_statistics().await;
                assert_eq!(
                    (
                        stats.total_pending,
                        stats.total_in_review,
                        stats.total_approved_today,
                        stats.total_rejected_today
                    ),
                    (0, 0, 1, 1)
                );
                assert_eq!(stats.oldest_pending_age_hours, 0.0);
                assert!(stats.priority_breakdown.is_empty());
            }
        })
        .await;
    }

    #[tokio::test]
    async fn expiry_and_review_decision_have_one_terminal_outcome() {
        bounded(async {
            let queue = HighRiskQueue::new(HighRiskQueueConfig {
                max_queue_time_hours: 0,
                enable_notifications: false,
                ..HighRiskQueueConfig::default()
            });
            let id = add(&queue, "expiry-race").await.unwrap();
            let (approved, expired) = tokio::join!(
                queue.approve_transaction(id, "officer".into(), None),
                queue.cleanup_expired()
            );
            let expired = expired.unwrap();
            assert_eq!(usize::from(approved.is_ok()) + expired, 1);
            assert!(matches!(
                queue.get_transaction(id).await.unwrap().status,
                ReviewStatus::Approved { .. } | ReviewStatus::Expired { .. }
            ));
            assert!(queue.get_pending_transactions().await.is_empty());
            assert_eq!(queue.cleanup_expired().await.unwrap(), 0);
            assert!(queue
                .reject_transaction(id, "officer".into(), "late".into())
                .await
                .is_err());
        })
        .await;
    }

    #[tokio::test]
    async fn expiry_removes_pending_and_in_review_records() {
        bounded(async {
            let queue = HighRiskQueue::new(HighRiskQueueConfig {
                max_queue_time_hours: 0,
                max_review_time_hours: 0,
                enable_notifications: false,
                ..HighRiskQueueConfig::default()
            });
            let first = add(&queue, "expire-pending").await.unwrap();
            let second = add(&queue, "expire-review").await.unwrap();
            queue.start_review(second, "officer".into()).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            assert_eq!(queue.cleanup_expired().await.unwrap(), 2);
            for id in [first, second] {
                assert!(matches!(
                    queue.get_transaction(id).await.unwrap().status,
                    ReviewStatus::Expired { .. }
                ));
            }
            assert!(queue.get_next_for_review().await.is_none());
            let stats = queue.get_statistics().await;
            assert_eq!((stats.total_pending, stats.total_in_review), (0, 0));
            add(&queue, "expire-pending").await.unwrap();
        })
        .await;
    }

    #[tokio::test]
    async fn invalid_expiry_duration_returns_error_without_state_change() {
        bounded(async {
            let queue = HighRiskQueue::new(HighRiskQueueConfig {
                max_queue_time_hours: u64::MAX,
                enable_notifications: false,
                ..HighRiskQueueConfig::default()
            });
            let id = add(&queue, "invalid-duration").await.unwrap();
            assert!(queue.cleanup_expired().await.is_err());
            assert_eq!(queue.get_pending_transactions().await[0].queue_id, id);
        })
        .await;
    }

    #[tokio::test]
    async fn cancellation_while_waiting_for_state_does_not_insert() {
        bounded(async {
            let queue = atomic_queue(2);
            let guard = queue.state.write().await;
            let shared = queue.clone();
            let task = tokio::spawn(async move { add(&shared, "cancelled").await });
            tokio::task::yield_now().await;
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
            drop(guard);
            assert!(queue.get_pending_transactions().await.is_empty());
            assert_eq!(queue.get_statistics().await.total_pending, 0);
        })
        .await;
    }
}
