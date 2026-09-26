//! In-memory audit records, staging batches, and filtered reports.
//! Accepted records remain visible before and after staging completion.
//! Durable storage, encryption, and archive delivery require a separate backend.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::consensus::ai_integration::{AIVerificationResult, RiskProcessingDecision};
use crate::consensus::notification_types::ReviewPriority;
use crate::types::{Address, Transaction, TxHash};

/// Audit record for an AI decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Unique identifier for this audit entry
    pub audit_id: Uuid,
    /// Transaction hash this audit relates to
    pub transaction_hash: TxHash,
    /// Block number where transaction was processed
    pub block_number: Option<u64>,
    /// Timestamp when AI decision was made
    pub timestamp: DateTime<Utc>,
    /// AI verification result and risk assessment
    pub ai_result: AIVerificationResult,
    /// Risk processing decision made
    pub risk_decision: RiskProcessingDecision,
    /// Risk priority assigned to transaction
    pub risk_priority: ReviewPriority,
    /// Oracle ID that provided the AI decision
    pub oracle_id: String,
    /// Request ID for tracing AI service calls
    pub request_id: String,
    /// Transaction metadata for compliance
    pub transaction_metadata: TransactionMetadata,
    /// Compliance status and notes
    pub compliance_status: ComplianceStatus,
    /// Data retention information
    pub retention_info: RetentionInfo,
}

/// Transaction metadata extracted for compliance reporting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionMetadata {
    /// Transaction type (Transfer, Deploy, etc.)
    pub transaction_type: String,
    /// From address
    pub from_address: Option<Address>,
    /// To address
    pub to_address: Option<Address>,
    /// Transaction amount
    pub amount: Option<u128>,
    /// Transaction fee
    pub fee: Option<u128>,
    /// Transaction timestamp
    pub transaction_timestamp: u64,
    /// Additional contextual data
    pub additional_data: HashMap<String, String>,
}

/// Compliance status tracking
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ComplianceStatus {
    /// Pending compliance review
    Pending,
    /// Passed automated compliance checks
    AutoApproved,
    /// Requires manual compliance review
    ManualReviewRequired,
    /// Approved after manual review
    ManualApproved { officer_id: String, notes: String },
    /// Failed compliance checks
    Failed { reason: String },
    /// Flagged for investigation
    Flagged {
        reason: String,
        investigator: Option<String>,
    },
}

/// Data retention policy information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionInfo {
    /// Data classification level
    pub classification: DataClassification,
    /// Retention period in days
    pub retention_days: u32,
    /// Archive date when data should be archived
    pub archive_date: DateTime<Utc>,
    /// Deletion date when data should be purged
    pub deletion_date: DateTime<Utc>,
    /// Whether this entry is subject to legal hold
    pub legal_hold: bool,
}

/// Data classification levels for retention policies
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DataClassification {
    /// Standard business data (7 years retention)
    Standard,
    /// Financial compliance data (10 years retention)
    Financial,
    /// High-risk/suspicious activity (indefinite retention)
    HighRisk,
    /// Legal investigation data (indefinite retention until resolved)
    Legal,
}

/// Compliance report filtering and query parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceQuery {
    /// Date range for the report
    pub date_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
    /// Filter by compliance status
    pub status_filter: Option<Vec<ComplianceStatus>>,
    /// Filter by risk priority
    pub priority_filter: Option<Vec<ReviewPriority>>,
    /// Filter by transaction type
    pub transaction_type_filter: Option<Vec<String>>,
    /// Filter by oracle ID
    pub oracle_filter: Option<Vec<String>>,
    /// Filter by address (from/to)
    pub address_filter: Option<Vec<Address>>,
    /// Minimum amount threshold
    pub min_amount: Option<u128>,
    /// Maximum amount threshold
    pub max_amount: Option<u128>,
    /// Include deleted/archived entries
    pub include_archived: bool,
    /// Pagination offset
    pub offset: usize,
    /// Pagination limit
    pub limit: usize,
}

/// Compliance report summary statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceReportSummary {
    /// Total number of entries in query period
    pub total_entries: usize,
    /// Breakdown by compliance status
    pub status_breakdown: HashMap<String, usize>,
    /// Breakdown by risk priority
    pub priority_breakdown: HashMap<ReviewPriority, usize>,
    /// Breakdown by transaction type
    pub transaction_type_breakdown: HashMap<String, usize>,
    /// Average risk score
    pub average_risk_score: f64,
    /// Total transaction volume
    pub total_volume: u128,
    /// Number of manual reviews required
    pub manual_reviews_required: usize,
    /// Number of flagged transactions
    pub flagged_transactions: usize,
}

/// Configuration for audit trail system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditConfig {
    /// Enable audit trail logging
    pub enabled: bool,
    /// Maximum number of audit entries to keep in memory
    pub max_memory_entries: usize,
    /// Batch size for database writes
    pub batch_write_size: usize,
    /// How often to flush audit entries to storage (seconds)
    pub flush_interval_seconds: u64,
    /// Default retention policy
    pub default_retention: RetentionInfo,
    /// Auto-archive old entries
    pub auto_archive: bool,
    /// Compression for archived data
    pub compression_enabled: bool,
    /// Encryption for sensitive audit data
    pub encryption_enabled: bool,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_memory_entries: 10000,
            batch_write_size: 100,
            flush_interval_seconds: 60,
            default_retention: RetentionInfo {
                classification: DataClassification::Standard,
                retention_days: 2557, // 7 years
                archive_date: Utc::now() + Duration::days(2557),
                deletion_date: Utc::now() + Duration::days(2557 + 30), // 30 day grace period
                legal_hold: false,
            },
            auto_archive: true,
            compression_enabled: true,
            encryption_enabled: true,
        }
    }
}

/// Arguments for recording an AI decision
pub struct RecordAiDecisionArgs<'a> {
    pub transaction: &'a Transaction,
    pub transaction_hash: TxHash,
    pub ai_result: AIVerificationResult,
    pub risk_decision: RiskProcessingDecision,
    pub risk_priority: ReviewPriority,
    pub oracle_id: String,
    pub request_id: String,
    pub block_number: Option<u64>,
}

/// Accepted entries, pending IDs, and counters change under one lock.
#[derive(Debug)]
struct AuditState {
    entries: HashMap<Uuid, AuditEntry>,
    pending: Vec<Uuid>,
    stats: AuditStatistics,
}

/// In-memory audit staging. No durable backend is configured by this type.
#[derive(Debug)]
pub struct AuditTrailManager {
    config: AuditConfig,
    state: RwLock<AuditState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditStatistics {
    pub total_entries: usize,
    pub entries_today: usize,
    pub pending_entries: usize,
    pub archived_entries: usize,
    pub last_flush_time: DateTime<Utc>,
    pub flush_failures: usize,
    pub query_count: usize,
    pub last_query_time: Option<DateTime<Utc>>,
}

impl AuditTrailManager {
    /// Create a new audit trail manager
    pub fn new(config: AuditConfig) -> Self {
        Self {
            config,
            state: RwLock::new(AuditState {
                entries: HashMap::new(),
                pending: Vec::new(),
                stats: AuditStatistics {
                    total_entries: 0,
                    entries_today: 0,
                    pending_entries: 0,
                    archived_entries: 0,
                    last_flush_time: Utc::now(),
                    flush_failures: 0,
                    query_count: 0,
                    last_query_time: None,
                },
            }),
        }
    }

    /// Record an AI decision in the audit trail
    pub async fn record_ai_decision(&self, args: RecordAiDecisionArgs<'_>) -> Result<Uuid> {
        if !self.config.enabled {
            return Err(anyhow!("Audit recording is disabled"));
        }

        let audit_id = Uuid::new_v4();
        let now = Utc::now();

        // Extract transaction metadata
        let transaction_metadata = self.extract_transaction_metadata(args.transaction);

        // Determine compliance status based on AI result and risk decision
        let compliance_status =
            self.determine_compliance_status(&args.ai_result, &args.risk_decision);

        // Determine retention policy based on transaction characteristics
        let retention_info =
            self.determine_retention_policy(&transaction_metadata, &compliance_status);

        let audit_entry = AuditEntry {
            audit_id,
            transaction_hash: args.transaction_hash.clone(),
            block_number: args.block_number,
            timestamp: now,
            ai_result: args.ai_result,
            risk_decision: args.risk_decision,
            risk_priority: args.risk_priority,
            oracle_id: args.oracle_id,
            request_id: args.request_id,
            transaction_metadata,
            compliance_status,
            retention_info,
        };

        let mut state = self.state.write().await;
        if state.entries.len() >= self.config.max_memory_entries {
            return Err(anyhow!(
                "Audit memory capacity reached; durable storage is required"
            ));
        }
        state.entries.insert(audit_id, audit_entry);
        state.pending.push(audit_id);
        if state.pending.len() >= self.config.batch_write_size {
            Self::finish_memory_batch(&mut state);
        }
        Ok(audit_id)
    }

    fn finish_memory_batch(state: &mut AuditState) -> usize {
        let count = state.pending.len();
        if count > 0 {
            state.pending.clear();
            state.stats.last_flush_time = Utc::now();
        }
        count
    }

    /// Complete an in-memory staging batch. This does not write durable storage.
    pub async fn flush_pending_entries(&self) -> Result<usize> {
        let mut state = self.state.write().await;
        Ok(Self::finish_memory_batch(&mut state))
    }

    /// Query audit entries with filtering and pagination
    pub async fn query_audit_entries(
        &self,
        query: ComplianceQuery,
    ) -> Result<(Vec<AuditEntry>, ComplianceReportSummary)> {
        let mut entries: Vec<_> = {
            let mut state = self.state.write().await;
            state.stats.query_count = state.stats.query_count.saturating_add(1);
            state.stats.last_query_time = Some(Utc::now());
            state.entries.values().cloned().collect()
        };
        entries.sort_by(|a, b| {
            b.timestamp
                .cmp(&a.timestamp)
                .then_with(|| a.audit_id.cmp(&b.audit_id))
        });
        let mut matching_entries = Vec::new();
        let mut summary_stats = HashMap::new();
        let mut total_volume = 0u128;
        let mut risk_scores = Vec::new();

        let mut priorities = HashMap::new();
        let mut transaction_types = HashMap::new();
        let mut manual_reviews = 0;
        let mut flagged = 0;
        for entry in &entries {
            if self.entry_matches_query(entry, &query) {
                matching_entries.push(entry.clone());

                // Update summary statistics
                let status_key = format!("{:?}", entry.compliance_status);
                *summary_stats.entry(status_key).or_insert(0) += 1;

                if let Some(amount) = entry.transaction_metadata.amount {
                    total_volume = total_volume
                        .checked_add(amount)
                        .ok_or_else(|| anyhow!("Audit volume exceeds u128"))?;
                }

                *priorities.entry(entry.risk_priority.clone()).or_insert(0) += 1;
                *transaction_types
                    .entry(entry.transaction_metadata.transaction_type.clone())
                    .or_insert(0) += 1;
                manual_reviews += usize::from(matches!(
                    entry.compliance_status,
                    ComplianceStatus::ManualReviewRequired
                ));
                flagged += usize::from(matches!(
                    entry.compliance_status,
                    ComplianceStatus::Flagged { .. }
                ));

                // Extract risk score for average calculation
                if let AIVerificationResult::Verified {
                    risk_score: Some(score),
                    ..
                } = &entry.ai_result
                {
                    risk_scores.push(*score);
                }
            }
        }

        // Sort by timestamp (newest first) and apply pagination
        matching_entries.sort_by(|a, b| {
            b.timestamp
                .cmp(&a.timestamp)
                .then_with(|| a.audit_id.cmp(&b.audit_id))
        });
        let total_count = matching_entries.len();
        let paginated_entries = matching_entries
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            .collect();

        // Calculate summary statistics
        let average_risk_score = if risk_scores.is_empty() {
            0.0
        } else {
            risk_scores.iter().sum::<f64>() / risk_scores.len() as f64
        };

        let summary = ComplianceReportSummary {
            total_entries: total_count,
            status_breakdown: summary_stats,
            priority_breakdown: priorities,
            transaction_type_breakdown: transaction_types,
            average_risk_score,
            total_volume,
            manual_reviews_required: manual_reviews,
            flagged_transactions: flagged,
        };

        Ok((paginated_entries, summary))
    }

    /// Export audit data for regulatory compliance
    pub async fn export_compliance_data(
        &self,
        query: ComplianceQuery,
        format: ExportFormat,
    ) -> Result<Vec<u8>> {
        let (entries, _summary) = self.query_audit_entries(query).await?;

        match format {
            ExportFormat::Json => {
                let json_data = serde_json::to_vec_pretty(&entries)?;
                Ok(json_data)
            }
            ExportFormat::Csv => {
                // Create CSV format for regulatory reporting
                let mut csv_data = Vec::new();
                csv_data.extend_from_slice(b"audit_id,transaction_hash,timestamp,oracle_id,risk_score,compliance_status,amount,from_address,to_address\n");

                for entry in entries {
                    let risk_score = match &entry.ai_result {
                        AIVerificationResult::Verified {
                            risk_score: Some(score),
                            ..
                        } => score.to_string(),
                        _ => "N/A".to_string(),
                    };

                    let line = format!(
                        "{},{},{},{},{},{:?},{},{},{}\n",
                        entry.audit_id,
                        hex::encode(&entry.transaction_hash),
                        entry.timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
                        entry.oracle_id,
                        risk_score,
                        entry.compliance_status,
                        entry.transaction_metadata.amount.unwrap_or(0),
                        entry
                            .transaction_metadata
                            .from_address
                            .as_deref()
                            .unwrap_or("N/A"),
                        entry
                            .transaction_metadata
                            .to_address
                            .as_deref()
                            .unwrap_or("N/A")
                    );
                    csv_data.extend_from_slice(line.as_bytes());
                }

                Ok(csv_data)
            }
        }
    }

    /// Get audit entries for a specific transaction
    pub async fn get_transaction_audit_trail(&self, transaction_hash: &TxHash) -> Vec<AuditEntry> {
        let state = self.state.read().await;
        let mut entries: Vec<_> = state
            .entries
            .values()
            .filter(|entry| &entry.transaction_hash == transaction_hash)
            .cloned()
            .collect();
        entries.sort_by(|a, b| {
            a.timestamp
                .cmp(&b.timestamp)
                .then_with(|| a.audit_id.cmp(&b.audit_id))
        });
        entries
    }

    pub async fn get_statistics(&self) -> AuditStatistics {
        let state = self.state.read().await;
        let mut stats = state.stats.clone();
        stats.total_entries = state.entries.len();
        stats.pending_entries = state.pending.len();
        let today = Utc::now().date_naive();
        stats.entries_today = state
            .entries
            .values()
            .filter(|e| e.timestamp.date_naive() == today)
            .count();
        stats
    }

    /// No archive success is reported without a durable archive implementation.
    pub async fn archive_old_entries(&self) -> Result<usize> {
        if !self.config.auto_archive {
            return Ok(0);
        }
        let state = self.state.read().await;
        let now = Utc::now();
        if state.entries.values().any(|entry| {
            entry.retention_info.archive_date <= now && !entry.retention_info.legal_hold
        }) {
            return Err(anyhow!("Durable audit archive is not configured"));
        }
        Ok(0)
    }

    pub async fn update_compliance_status(
        &self,
        audit_id: Uuid,
        new_status: ComplianceStatus,
    ) -> Result<()> {
        let mut state = self.state.write().await;
        let entry = state
            .entries
            .get_mut(&audit_id)
            .ok_or_else(|| anyhow!("Audit entry not found: {audit_id}"))?;
        entry.compliance_status = new_status;
        Ok(())
    }

    fn extract_transaction_metadata(&self, transaction: &Transaction) -> TransactionMetadata {
        match transaction {
            Transaction::Transfer(tx) => TransactionMetadata {
                transaction_type: "Transfer".to_string(),
                from_address: Some(tx.from.clone()),
                to_address: Some(tx.to.clone()),
                amount: Some(tx.amount),
                fee: Some(tx.fee),
                transaction_timestamp: tx.timestamp,
                additional_data: HashMap::new(),
            },
            Transaction::Deploy(_) => TransactionMetadata {
                transaction_type: "Deploy".to_string(),
                from_address: None,
                to_address: None,
                amount: None,
                fee: None,
                transaction_timestamp: Utc::now().timestamp() as u64,
                additional_data: HashMap::new(),
            },
            Transaction::Call(_) => TransactionMetadata {
                transaction_type: "Call".to_string(),
                from_address: None,
                to_address: None,
                amount: None,
                fee: None,
                transaction_timestamp: Utc::now().timestamp() as u64,
                additional_data: HashMap::new(),
            },
            Transaction::Stake(_) => TransactionMetadata {
                transaction_type: "Stake".to_string(),
                from_address: None,
                to_address: None,
                amount: None,
                fee: None,
                transaction_timestamp: Utc::now().timestamp() as u64,
                additional_data: HashMap::new(),
            },
            Transaction::AIRequest(_) => TransactionMetadata {
                transaction_type: "AIRequest".to_string(),
                from_address: None,
                to_address: None,
                amount: None,
                fee: None,
                transaction_timestamp: Utc::now().timestamp() as u64,
                additional_data: HashMap::new(),
            },
        }
    }

    fn determine_compliance_status(
        &self,
        _ai_result: &AIVerificationResult,
        risk_decision: &RiskProcessingDecision,
    ) -> ComplianceStatus {
        match risk_decision {
            RiskProcessingDecision::AutoApprove => ComplianceStatus::AutoApproved,
            RiskProcessingDecision::RequireReview { .. } => ComplianceStatus::ManualReviewRequired,
            RiskProcessingDecision::AutoReject { reason } => ComplianceStatus::Failed {
                reason: reason.clone(),
            },
        }
    }

    fn determine_retention_policy(
        &self,
        metadata: &TransactionMetadata,
        status: &ComplianceStatus,
    ) -> RetentionInfo {
        let classification = match status {
            ComplianceStatus::Failed { .. } | ComplianceStatus::Flagged { .. } => {
                DataClassification::HighRisk
            }
            _ => {
                if metadata.amount.unwrap_or(0) > 10000 {
                    DataClassification::Financial
                } else {
                    DataClassification::Standard
                }
            }
        };

        let retention_days = match classification {
            DataClassification::Standard => 2557,  // 7 years
            DataClassification::Financial => 3653, // 10 years
            DataClassification::HighRisk => 7305,  // 20 years
            DataClassification::Legal => 36525,    // 100 years (indefinite)
        };

        RetentionInfo {
            classification: classification.clone(),
            retention_days,
            archive_date: Utc::now() + Duration::days(retention_days as i64),
            deletion_date: Utc::now() + Duration::days(retention_days as i64 + 30),
            legal_hold: matches!(classification, DataClassification::Legal),
        }
    }

    fn entry_matches_query(&self, entry: &AuditEntry, query: &ComplianceQuery) -> bool {
        // Date range filter
        if let Some((start, end)) = &query.date_range {
            if entry.timestamp < *start || entry.timestamp > *end {
                return false;
            }
        }

        // Status filter
        if let Some(statuses) = &query.status_filter {
            if !statuses.contains(&entry.compliance_status) {
                return false;
            }
        }

        // Priority filter
        if let Some(priorities) = &query.priority_filter {
            if !priorities.contains(&entry.risk_priority) {
                return false;
            }
        }

        // Transaction type filter
        if let Some(types) = &query.transaction_type_filter {
            if !types.contains(&entry.transaction_metadata.transaction_type) {
                return false;
            }
        }

        // Oracle filter
        if let Some(oracles) = &query.oracle_filter {
            if !oracles.contains(&entry.oracle_id) {
                return false;
            }
        }

        if let Some(addresses) = &query.address_filter {
            if !addresses.iter().any(|address| {
                entry.transaction_metadata.from_address.as_ref() == Some(address)
                    || entry.transaction_metadata.to_address.as_ref() == Some(address)
            }) {
                return false;
            }
        }
        if query.min_amount.is_some() || query.max_amount.is_some() {
            let Some(amount) = entry.transaction_metadata.amount else {
                return false;
            };
            if query.min_amount.is_some_and(|minimum| amount < minimum)
                || query.max_amount.is_some_and(|maximum| amount > maximum)
            {
                return false;
            }
        }

        true
    }
}

/// Export format options for compliance data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExportFormat {
    Json,
    Csv,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TransferTransaction;

    fn create_test_transaction() -> Transaction {
        Transaction::Transfer(TransferTransaction {
            hash: "test_tx_123".to_string(),
            from: "alice".to_string(),
            to: "bob".to_string(),
            amount: 1000,
            fee: 10,
            nonce: 1,
            timestamp: 1234567890,
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

    fn create_test_ai_result() -> AIVerificationResult {
        AIVerificationResult::Verified {
            risk_score: Some(0.75),
            confidence: Some(0.95),
            oracle_id: "test-oracle".to_string(),
            response_id: "test-response".to_string(),
            fraud_probability: Some(0.6),
            processing_decision: RiskProcessingDecision::RequireReview {
                reason: "Medium risk score".to_string(),
            },
        }
    }

    #[tokio::test]
    async fn test_audit_entry_recording() {
        let config = AuditConfig::default();
        let audit_manager = AuditTrailManager::new(config);

        let transaction = create_test_transaction();
        let tx_hash = "test_tx_hash".to_string();
        let ai_result = create_test_ai_result();
        let risk_decision = RiskProcessingDecision::RequireReview {
            reason: "Test review".to_string(),
        };

        let audit_id = audit_manager
            .record_ai_decision(RecordAiDecisionArgs {
                transaction: &transaction,
                transaction_hash: tx_hash.clone(),
                ai_result,
                risk_decision,
                risk_priority: ReviewPriority::Medium,
                oracle_id: "test-oracle".to_string(),
                request_id: "test-request".to_string(),
                block_number: Some(12345),
            })
            .await
            .unwrap();

        assert!(!audit_id.is_nil());

        // Verify the entry was recorded
        let trail = audit_manager.get_transaction_audit_trail(&tx_hash).await;
        assert_eq!(trail.len(), 1);
        assert_eq!(trail[0].audit_id, audit_id);
    }

    #[tokio::test]
    async fn test_compliance_query() {
        let config = AuditConfig::default();
        let audit_manager = AuditTrailManager::new(config);

        // Record multiple audit entries
        for i in 0..5 {
            let transaction = create_test_transaction();
            let tx_hash = format!("test_tx_{}", i);
            let ai_result = create_test_ai_result();
            let risk_decision = RiskProcessingDecision::RequireReview {
                reason: format!("Test review {}", i),
            };

            audit_manager
                .record_ai_decision(RecordAiDecisionArgs {
                    transaction: &transaction,
                    transaction_hash: tx_hash,
                    ai_result,
                    risk_decision,
                    risk_priority: ReviewPriority::Medium,
                    oracle_id: "test-oracle".to_string(),
                    request_id: format!("test-request-{}", i),
                    block_number: Some(12345 + i),
                })
                .await
                .unwrap();
        }

        // Flush entries to storage
        audit_manager.flush_pending_entries().await.unwrap();

        // Query all entries
        let query = ComplianceQuery {
            date_range: None,
            status_filter: None,
            priority_filter: None,
            transaction_type_filter: None,
            oracle_filter: None,
            address_filter: None,
            min_amount: None,
            max_amount: None,
            include_archived: false,
            offset: 0,
            limit: 10,
        };

        let (entries, summary) = audit_manager.query_audit_entries(query).await.unwrap();
        assert_eq!(entries.len(), 5);
        assert_eq!(summary.total_entries, 5);
    }

    #[tokio::test]
    async fn test_compliance_data_export() {
        let config = AuditConfig::default();
        let audit_manager = AuditTrailManager::new(config);

        // Record an audit entry
        let transaction = create_test_transaction();
        let tx_hash = "export_test_tx".to_string();
        let ai_result = create_test_ai_result();
        let risk_decision = RiskProcessingDecision::RequireReview {
            reason: "Export test".to_string(),
        };

        audit_manager
            .record_ai_decision(RecordAiDecisionArgs {
                transaction: &transaction,
                transaction_hash: tx_hash,
                ai_result,
                risk_decision,
                risk_priority: ReviewPriority::Medium,
                oracle_id: "test-oracle".to_string(),
                request_id: "export-request".to_string(),
                block_number: Some(12345),
            })
            .await
            .unwrap();

        audit_manager.flush_pending_entries().await.unwrap();

        // Test JSON export
        let query = ComplianceQuery {
            date_range: None,
            status_filter: None,
            priority_filter: None,
            transaction_type_filter: None,
            oracle_filter: None,
            address_filter: None,
            min_amount: None,
            max_amount: None,
            include_archived: false,
            offset: 0,
            limit: 10,
        };

        let json_data = audit_manager
            .export_compliance_data(query.clone(), ExportFormat::Json)
            .await
            .unwrap();
        assert!(!json_data.is_empty());

        // Test CSV export
        let csv_data = audit_manager
            .export_compliance_data(query, ExportFormat::Csv)
            .await
            .unwrap();
        assert!(!csv_data.is_empty());

        // Verify CSV header
        let csv_string = String::from_utf8(csv_data).unwrap();
        assert!(csv_string.starts_with("audit_id,transaction_hash,timestamp"));
    }

    fn all_entries() -> ComplianceQuery {
        ComplianceQuery {
            date_range: None,
            status_filter: None,
            priority_filter: None,
            transaction_type_filter: None,
            oracle_filter: None,
            address_filter: None,
            min_amount: None,
            max_amount: None,
            include_archived: false,
            offset: 0,
            limit: 100,
        }
    }
    async fn record(manager: &AuditTrailManager, hash: &str, amount: u128) -> Result<Uuid> {
        let mut transaction = create_test_transaction();
        if let Transaction::Transfer(tx) = &mut transaction {
            tx.amount = amount;
            tx.from = "sender".into();
            tx.to = "recipient".into();
        }
        manager
            .record_ai_decision(RecordAiDecisionArgs {
                transaction: &transaction,
                transaction_hash: hash.into(),
                ai_result: create_test_ai_result(),
                risk_decision: RiskProcessingDecision::RequireReview {
                    reason: "fixture".into(),
                },
                risk_priority: ReviewPriority::High,
                oracle_id: "fixture-oracle".into(),
                request_id: "fixture-request".into(),
                block_number: None,
            })
            .await
    }

    #[tokio::test]
    async fn accepted_records_are_visible_and_updatable_before_flush() {
        let manager = AuditTrailManager::new(AuditConfig::default());
        let id = record(&manager, "pending", 10).await.unwrap();
        manager
            .update_compliance_status(
                id,
                ComplianceStatus::Flagged {
                    reason: "fixture".into(),
                    investigator: None,
                },
            )
            .await
            .unwrap();
        let (entries, summary) = manager.query_audit_entries(all_entries()).await.unwrap();
        assert_eq!(entries[0].audit_id, id);
        assert_eq!(
            (
                summary.total_entries,
                summary.flagged_transactions,
                summary.total_volume
            ),
            (1, 1, 10)
        );
        assert_eq!(summary.priority_breakdown[&ReviewPriority::High], 1);
        assert_eq!(summary.transaction_type_breakdown["Transfer"], 1);
        assert_eq!(manager.get_statistics().await.pending_entries, 1);
        assert_eq!(manager.flush_pending_entries().await.unwrap(), 1);
        assert_eq!(manager.flush_pending_entries().await.unwrap(), 0);
        let stats = manager.get_statistics().await;
        assert_eq!(
            (
                stats.total_entries,
                stats.entries_today,
                stats.pending_entries
            ),
            (1, 1, 0)
        );
        assert_eq!(
            manager
                .get_transaction_audit_trail(&"pending".into())
                .await
                .len(),
            1
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn recording_and_flushing_preserve_every_accepted_entry() {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let manager = std::sync::Arc::new(AuditTrailManager::new(AuditConfig {
                batch_write_size: 7,
                ..AuditConfig::default()
            }));
            let mut tasks = tokio::task::JoinSet::new();
            for i in 0..32 {
                let manager = manager.clone();
                tasks.spawn(async move {
                    let id = record(&manager, "shared-hash", i).await.unwrap();
                    manager.flush_pending_entries().await.unwrap();
                    id
                });
            }
            let mut ids = std::collections::HashSet::new();
            while let Some(result) = tasks.join_next().await {
                assert!(ids.insert(result.unwrap()));
            }
            let trail = manager
                .get_transaction_audit_trail(&"shared-hash".into())
                .await;
            assert_eq!(
                trail
                    .iter()
                    .map(|e| e.audit_id)
                    .collect::<std::collections::HashSet<_>>(),
                ids
            );
            let (entries, summary) = manager.query_audit_entries(all_entries()).await.unwrap();
            assert_eq!(
                (
                    entries.len(),
                    summary.total_entries,
                    summary.manual_reviews_required
                ),
                (32, 32, 32)
            );
            assert_eq!(summary.total_volume, (0..32u128).sum());
            assert_eq!(manager.get_statistics().await.total_entries, 32);
        })
        .await
        .expect("audit operations did not complete");
    }

    #[tokio::test]
    async fn disabled_or_full_audit_returns_error_without_fake_receipt() {
        let disabled = AuditTrailManager::new(AuditConfig {
            enabled: false,
            ..AuditConfig::default()
        });
        assert!(record(&disabled, "disabled", 1).await.is_err());
        assert_eq!(disabled.get_statistics().await.total_entries, 0);
        let manager = AuditTrailManager::new(AuditConfig {
            max_memory_entries: 1,
            batch_write_size: 1,
            ..AuditConfig::default()
        });
        let id = record(&manager, "accepted", 1).await.unwrap();
        assert!(record(&manager, "rejected", 2).await.is_err());
        assert_eq!(
            manager
                .get_transaction_audit_trail(&"accepted".into())
                .await[0]
                .audit_id,
            id
        );
        assert!(manager
            .get_transaction_audit_trail(&"rejected".into())
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn filters_and_pagination_preserve_full_matching_summary() {
        let manager = AuditTrailManager::new(AuditConfig::default());
        record(&manager, "small", 10).await.unwrap();
        record(&manager, "large", 20).await.unwrap();
        let mut query = all_entries();
        query.address_filter = Some(vec!["unrelated".into()]);
        assert_eq!(
            manager
                .query_audit_entries(query.clone())
                .await
                .unwrap()
                .1
                .total_entries,
            0
        );
        query.address_filter = Some(vec!["recipient".into()]);
        query.limit = 1;
        let (entries, summary) = manager.query_audit_entries(query.clone()).await.unwrap();
        assert_eq!(
            (entries.len(), summary.total_entries, summary.total_volume),
            (1, 2, 30)
        );
        query.min_amount = Some(15);
        let (_, summary) = manager.query_audit_entries(query).await.unwrap();
        assert_eq!((summary.total_entries, summary.total_volume), (1, 20));
    }

    #[tokio::test]
    async fn overflowing_report_volume_returns_error_and_keeps_records() {
        let manager = AuditTrailManager::new(AuditConfig::default());
        record(&manager, "large", u128::MAX).await.unwrap();
        record(&manager, "one", 1).await.unwrap();
        let error = manager
            .query_audit_entries(all_entries())
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "Audit volume exceeds u128");
        assert_eq!(manager.get_statistics().await.total_entries, 2);
    }

    #[tokio::test]
    async fn archive_does_not_report_success_without_storage() {
        let manager = AuditTrailManager::new(AuditConfig::default());
        let id = record(&manager, "archive", 1).await.unwrap();
        {
            let mut state = manager.state.write().await;
            state
                .entries
                .get_mut(&id)
                .unwrap()
                .retention_info
                .archive_date = Utc::now() - Duration::days(1);
        }
        assert!(manager.archive_old_entries().await.is_err());
        assert!(manager.archive_old_entries().await.is_err());
        assert_eq!(manager.get_statistics().await.archived_entries, 0);
        assert_eq!(
            manager
                .get_transaction_audit_trail(&"archive".into())
                .await
                .len(),
            1
        );
    }
}
