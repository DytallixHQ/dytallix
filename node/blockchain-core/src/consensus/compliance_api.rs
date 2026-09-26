//! Compliance Reporting API Endpoints
//!
//! This module provides REST API endpoints for compliance reporting,
//! audit trail queries, and regulatory data export functionality.

use anyhow::Result;
use chrono::{DateTime, Utc};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::consensus::audit_trail::{
    AuditEntry, AuditStatistics, AuditTrailManager, ComplianceQuery, ComplianceReportSummary,
    ComplianceStatus, ExportFormat,
};
use crate::consensus::notification_types::ReviewPriority;

/// API request for compliance report generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceReportRequest {
    /// Start date for the report (ISO 8601 format)
    pub start_date: Option<String>,
    /// End date for the report (ISO 8601 format)
    pub end_date: Option<String>,
    /// Filter by compliance status
    pub status_filters: Option<Vec<String>>,
    /// Filter by risk priority levels
    pub priority_filters: Option<Vec<String>>,
    /// Filter by transaction types
    pub transaction_types: Option<Vec<String>>,
    /// Filter by oracle IDs
    pub oracle_ids: Option<Vec<String>>,
    /// Minimum transaction amount
    pub min_amount: Option<u64>,
    /// Maximum transaction amount
    pub max_amount: Option<u64>,
    /// Include archived/deleted entries
    pub include_archived: Option<bool>,
    /// Page number for pagination (starts at 1)
    pub page: Option<usize>,
    /// Number of entries per page
    pub page_size: Option<usize>,
}

/// API response for compliance reports
#[derive(Debug, Serialize, Deserialize)]
pub struct ComplianceReportResponse {
    /// Query execution status
    pub success: bool,
    /// Error message if query failed
    pub error: Option<String>,
    /// Report summary statistics
    pub summary: Option<ComplianceReportSummary>,
    /// Paginated audit entries
    pub entries: Vec<AuditEntry>,
    /// Pagination information
    pub pagination: PaginationInfo,
    /// Report generation metadata
    pub metadata: ReportMetadata,
}

/// Pagination information for API responses
#[derive(Debug, Serialize, Deserialize)]
pub struct PaginationInfo {
    /// Current page number
    pub current_page: usize,
    /// Total number of pages
    pub total_pages: usize,
    /// Number of entries per page
    pub page_size: usize,
    /// Total number of entries matching query
    pub total_entries: usize,
    /// Whether there are more pages available
    pub has_next_page: bool,
    /// Whether there are previous pages available
    pub has_previous_page: bool,
}

/// Report generation metadata
#[derive(Debug, Serialize, Deserialize)]
pub struct ReportMetadata {
    /// Timestamp when report was generated
    pub generated_at: DateTime<Utc>,
    /// Time taken to generate report (milliseconds)
    pub generation_time_ms: u64,
    /// Version of the audit system
    pub audit_version: String,
    /// Unique report ID for tracking
    pub report_id: String,
    /// User/system that requested the report
    pub requested_by: Option<String>,
}

/// Request for exporting compliance data
#[derive(Debug, Serialize, Deserialize)]
pub struct ExportRequest {
    /// Export format (json, csv)
    pub format: String,
    /// Same filtering options as compliance report
    #[serde(flatten)]
    pub filters: ComplianceReportRequest,
    /// Include sensitive data (requires elevated permissions)
    pub include_sensitive: Option<bool>,
    /// Compress the export file
    pub compress: Option<bool>,
}

/// Response for export requests
#[derive(Debug, Serialize, Deserialize)]
pub struct ExportResponse {
    /// Export operation status
    pub success: bool,
    /// Error message if export failed
    pub error: Option<String>,
    /// Download URL or file ID
    pub download_url: Option<String>,
    /// Export file size in bytes
    pub file_size: Option<u64>,
    /// Export metadata
    pub metadata: ReportMetadata,
    /// Expiration time for download link
    pub expires_at: Option<DateTime<Utc>>,
}

/// Request for updating compliance status
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateComplianceStatusRequest {
    /// Audit entry ID to update
    pub audit_id: String,
    /// New compliance status
    pub status: String,
    /// Officer ID making the update
    pub officer_id: String,
    /// Optional notes for the status change
    pub notes: Option<String>,
    /// Reason for the status change
    pub reason: Option<String>,
}

/// API for compliance reporting and audit trail management
pub struct ComplianceAPI {
    audit_manager: Arc<AuditTrailManager>,
}

impl ComplianceAPI {
    /// Create a new compliance API instance
    pub fn new(audit_manager: Arc<AuditTrailManager>) -> Self {
        Self { audit_manager }
    }

    /// Generate a compliance report based on the provided parameters
    pub async fn generate_compliance_report(
        &self,
        request: ComplianceReportRequest,
    ) -> Result<ComplianceReportResponse> {
        let start_time = std::time::Instant::now();
        let report_id = uuid::Uuid::new_v4().to_string();

        info!("Generating compliance report with ID: {report_id}");

        // Convert API request to internal query format
        let query = self.convert_request_to_query(request.clone())?;

        // Execute the query
        let (entries, summary) = match self.audit_manager.query_audit_entries(query.clone()).await {
            Ok(result) => result,
            Err(e) => {
                error!("Failed to generate compliance report: {e}");
                return Ok(ComplianceReportResponse {
                    success: false,
                    error: Some(e.to_string()),
                    summary: None,
                    entries: Vec::new(),
                    pagination: PaginationInfo {
                        current_page: 1,
                        total_pages: 0,
                        page_size: query.limit,
                        total_entries: 0,
                        has_next_page: false,
                        has_previous_page: false,
                    },
                    metadata: ReportMetadata {
                        generated_at: Utc::now(),
                        generation_time_ms: start_time.elapsed().as_millis() as u64,
                        audit_version: "1.0.0".to_string(),
                        report_id,
                        requested_by: None,
                    },
                });
            }
        };

        // Calculate pagination info
        let total_entries = summary.total_entries;
        let page_size = query.limit;
        let current_page = (query.offset / page_size) + 1;
        let total_pages = total_entries.div_ceil(page_size);

        let pagination = PaginationInfo {
            current_page,
            total_pages,
            page_size,
            total_entries,
            has_next_page: current_page < total_pages,
            has_previous_page: current_page > 1,
        };

        let metadata = ReportMetadata {
            generated_at: Utc::now(),
            generation_time_ms: start_time.elapsed().as_millis() as u64,
            audit_version: "1.0.0".to_string(),
            report_id,
            requested_by: None, // Would be populated from authentication context
        };

        info!(
            "Generated compliance report {} with {} entries in {}ms",
            metadata.report_id,
            entries.len(),
            metadata.generation_time_ms
        );

        Ok(ComplianceReportResponse {
            success: true,
            error: None,
            summary: Some(summary),
            entries,
            pagination,
            metadata,
        })
    }

    /// Export compliance data in the requested format
    pub async fn export_compliance_data(&self, request: ExportRequest) -> Result<ExportResponse> {
        let start_time = std::time::Instant::now();
        let report_id = uuid::Uuid::new_v4().to_string();

        info!("Exporting compliance data with format: {}", request.format);

        // Parse export format
        let format = match request.format.to_lowercase().as_str() {
            "json" => ExportFormat::Json,
            "csv" => ExportFormat::Csv,
            _ => {
                return Ok(ExportResponse {
                    success: false,
                    error: Some(format!("Unsupported export format: {}", request.format)),
                    download_url: None,
                    file_size: None,
                    metadata: ReportMetadata {
                        generated_at: Utc::now(),
                        generation_time_ms: start_time.elapsed().as_millis() as u64,
                        audit_version: "1.0.0".to_string(),
                        report_id,
                        requested_by: None,
                    },
                    expires_at: None,
                });
            }
        };

        // Convert filters to query
        let query = self.convert_request_to_query(request.filters)?;

        // Export the data
        let export_data = match self
            .audit_manager
            .export_compliance_data(query, format)
            .await
        {
            Ok(data) => data,
            Err(e) => {
                error!("Failed to export compliance data: {e}");
                return Ok(ExportResponse {
                    success: false,
                    error: Some(e.to_string()),
                    download_url: None,
                    file_size: None,
                    metadata: ReportMetadata {
                        generated_at: Utc::now(),
                        generation_time_ms: start_time.elapsed().as_millis() as u64,
                        audit_version: "1.0.0".to_string(),
                        report_id,
                        requested_by: None,
                    },
                    expires_at: None,
                });
            }
        };

        let file_size = export_data.len() as u64;

        // In a real implementation, you would:
        // 1. Save the export data to a secure storage location
        // 2. Generate a signed download URL
        // 3. Set appropriate expiration times
        // 4. Apply compression if requested
        // 5. Handle access controls and permissions

        let download_url = format!("/api/compliance/export/download/{report_id}");
        let expires_at = Utc::now() + chrono::Duration::hours(24); // 24 hour expiration

        let metadata = ReportMetadata {
            generated_at: Utc::now(),
            generation_time_ms: start_time.elapsed().as_millis() as u64,
            audit_version: "1.0.0".to_string(),
            report_id,
            requested_by: None,
        };

        info!(
            "Exported compliance data {} ({} bytes) in {}ms",
            metadata.report_id, file_size, metadata.generation_time_ms
        );

        Ok(ExportResponse {
            success: true,
            error: None,
            download_url: Some(download_url),
            file_size: Some(file_size),
            metadata,
            expires_at: Some(expires_at),
        })
    }

    /// Update compliance status for an audit entry
    pub async fn update_compliance_status(
        &self,
        request: UpdateComplianceStatusRequest,
    ) -> Result<serde_json::Value> {
        info!(
            "Updating compliance status for audit ID: {}",
            request.audit_id
        );

        // Parse audit ID
        let audit_id = uuid::Uuid::parse_str(&request.audit_id)
            .map_err(|e| anyhow::anyhow!("Invalid audit ID format: {}", e))?;

        // Parse compliance status
        let new_status = self.parse_compliance_status(&request.status, &request)?;

        // Update the status
        match self
            .audit_manager
            .update_compliance_status(audit_id, new_status)
            .await
        {
            Ok(_) => {
                info!(
                    "Successfully updated compliance status for {} by officer {}",
                    request.audit_id, request.officer_id
                );

                Ok(serde_json::json!({
                    "success": true,
                    "message": "Compliance status updated successfully",
                    "audit_id": request.audit_id,
                    "updated_by": request.officer_id,
                    "updated_at": Utc::now()
                }))
            }
            Err(e) => {
                error!("Failed to update compliance status: {e}");
                Ok(serde_json::json!({
                    "success": false,
                    "error": e.to_string(),
                    "audit_id": request.audit_id
                }))
            }
        }
    }

    /// Get audit trail for a specific transaction
    pub async fn get_transaction_audit_trail(
        &self,
        transaction_hash: &str,
    ) -> Result<serde_json::Value> {
        info!("Getting audit trail for transaction: {transaction_hash}");

        let entries = self
            .audit_manager
            .get_transaction_audit_trail(&transaction_hash.to_string())
            .await;

        Ok(serde_json::json!({
            "success": true,
            "transaction_hash": transaction_hash,
            "audit_entries": entries,
            "entry_count": entries.len(),
            "retrieved_at": Utc::now()
        }))
    }

    /// Get audit system statistics
    pub async fn get_audit_statistics(&self) -> Result<AuditStatistics> {
        let stats = self.audit_manager.get_statistics().await;
        Ok(stats)
    }

    /// Trigger manual archival of old audit entries
    pub async fn trigger_archival(&self) -> Result<serde_json::Value> {
        info!("Triggering manual archival of old audit entries");

        match self.audit_manager.archive_old_entries().await {
            Ok(archived_count) => {
                info!("Successfully archived {archived_count} entries");
                Ok(serde_json::json!({
                    "success": true,
                    "archived_count": archived_count,
                    "archived_at": Utc::now()
                }))
            }
            Err(e) => {
                error!("Failed to archive entries: {e}");
                Ok(serde_json::json!({
                    "success": false,
                    "error": e.to_string()
                }))
            }
        }
    }

    // Helper methods

    fn convert_request_to_query(
        &self,
        request: ComplianceReportRequest,
    ) -> Result<ComplianceQuery> {
        // Parse date range
        let date_range = if let (Some(start), Some(end)) = (request.start_date, request.end_date) {
            let start_date = DateTime::parse_from_rfc3339(&start)?.with_timezone(&Utc);
            let end_date = DateTime::parse_from_rfc3339(&end)?.with_timezone(&Utc);
            Some((start_date, end_date))
        } else {
            None
        };

        // Parse status filters
        let status_filter = if let Some(statuses) = request.status_filters {
            let parsed_statuses: Result<Vec<ComplianceStatus>, _> = statuses
                .iter()
                .map(|s| self.parse_compliance_status_from_string(s))
                .collect();
            Some(parsed_statuses?)
        } else {
            None
        };

        // Parse priority filters
        let priority_filter = if let Some(priorities) = request.priority_filters {
            let parsed_priorities: Result<Vec<ReviewPriority>, _> = priorities
                .iter()
                .map(|p| self.parse_priority_from_string(p))
                .collect();
            Some(parsed_priorities?)
        } else {
            None
        };

        // Calculate pagination
        let page = request.page.unwrap_or(1);
        let page_size = request.page_size.unwrap_or(50).min(1000); // Max 1000 entries per page
        let offset = (page - 1) * page_size;

        Ok(ComplianceQuery {
            date_range,
            status_filter,
            priority_filter,
            transaction_type_filter: request.transaction_types,
            oracle_filter: request.oracle_ids,
            address_filter: None, // Could be added to API if needed
            min_amount: request.min_amount.map(|a| a as u128),
            max_amount: request.max_amount.map(|a| a as u128),
            include_archived: request.include_archived.unwrap_or(false),
            offset,
            limit: page_size,
        })
    }

    fn parse_compliance_status(
        &self,
        status_str: &str,
        request: &UpdateComplianceStatusRequest,
    ) -> Result<ComplianceStatus> {
        match status_str.to_lowercase().as_str() {
            "pending" => Ok(ComplianceStatus::Pending),
            "auto_approved" => Ok(ComplianceStatus::AutoApproved),
            "manual_review_required" => Ok(ComplianceStatus::ManualReviewRequired),
            "manual_approved" => Ok(ComplianceStatus::ManualApproved {
                officer_id: request.officer_id.clone(),
                notes: request.notes.clone().unwrap_or_default(),
            }),
            "failed" => Ok(ComplianceStatus::Failed {
                reason: request
                    .reason
                    .clone()
                    .unwrap_or("No reason provided".to_string()),
            }),
            "flagged" => Ok(ComplianceStatus::Flagged {
                reason: request
                    .reason
                    .clone()
                    .unwrap_or("No reason provided".to_string()),
                investigator: Some(request.officer_id.clone()),
            }),
            _ => Err(anyhow::anyhow!("Invalid compliance status: {}", status_str)),
        }
    }

    fn parse_compliance_status_from_string(&self, status_str: &str) -> Result<ComplianceStatus> {
        match status_str.to_lowercase().as_str() {
            "pending" => Ok(ComplianceStatus::Pending),
            "auto_approved" => Ok(ComplianceStatus::AutoApproved),
            "manual_review_required" => Ok(ComplianceStatus::ManualReviewRequired),
            "manual_approved" => Ok(ComplianceStatus::ManualApproved {
                officer_id: "unknown".to_string(),
                notes: "".to_string(),
            }),
            "failed" => Ok(ComplianceStatus::Failed {
                reason: "".to_string(),
            }),
            "flagged" => Ok(ComplianceStatus::Flagged {
                reason: "".to_string(),
                investigator: None,
            }),
            _ => Err(anyhow::anyhow!("Invalid compliance status: {}", status_str)),
        }
    }

    fn parse_priority_from_string(&self, priority_str: &str) -> Result<ReviewPriority> {
        match priority_str.to_lowercase().as_str() {
            "low" => Ok(ReviewPriority::Low),
            "medium" => Ok(ReviewPriority::Medium),
            "high" => Ok(ReviewPriority::High),
            "critical" => Ok(ReviewPriority::Critical),
            _ => Err(anyhow::anyhow!("Invalid priority: {}", priority_str)),
        }
    }
}

/// Mock HTTP handlers for compliance API endpoints
/// In production, these would be integrated with your web framework (e.g., warp, axum, actix-web)
pub struct ComplianceHttpHandlers {
    api: Arc<ComplianceAPI>,
}

impl ComplianceHttpHandlers {
    pub fn new(api: Arc<ComplianceAPI>) -> Self {
        Self { api }
    }

    /// POST /api/compliance/reports
    pub async fn handle_generate_report(
        &self,
        request: ComplianceReportRequest,
    ) -> Result<ComplianceReportResponse> {
        self.api.generate_compliance_report(request).await
    }

    /// POST /api/compliance/export
    pub async fn handle_export_data(&self, request: ExportRequest) -> Result<ExportResponse> {
        self.api.export_compliance_data(request).await
    }

    /// PUT /api/compliance/status
    pub async fn handle_update_status(
        &self,
        request: UpdateComplianceStatusRequest,
    ) -> Result<serde_json::Value> {
        self.api.update_compliance_status(request).await
    }

    /// GET /api/compliance/audit-trail/{transaction_hash}
    pub async fn handle_get_audit_trail(
        &self,
        transaction_hash: String,
    ) -> Result<serde_json::Value> {
        self.api
            .get_transaction_audit_trail(&transaction_hash)
            .await
    }

    /// GET /api/compliance/statistics
    pub async fn handle_get_statistics(&self) -> Result<AuditStatistics> {
        self.api.get_audit_statistics().await
    }

    /// POST /api/compliance/archive
    pub async fn handle_trigger_archival(&self) -> Result<serde_json::Value> {
        self.api.trigger_archival().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::audit_trail::{AuditConfig, AuditTrailManager};

    #[tokio::test]
    async fn test_compliance_api_report_generation() {
        let audit_manager = Arc::new(AuditTrailManager::new(AuditConfig::default()));
        let api = ComplianceAPI::new(audit_manager);

        let request = ComplianceReportRequest {
            start_date: Some("2024-01-01T00:00:00Z".to_string()),
            end_date: Some("2024-12-31T23:59:59Z".to_string()),
            status_filters: None,
            priority_filters: None,
            transaction_types: None,
            oracle_ids: None,
            min_amount: None,
            max_amount: None,
            include_archived: Some(false),
            page: Some(1),
            page_size: Some(10),
        };

        let started = std::time::Instant::now();
        let response = api.generate_compliance_report(request).await.unwrap();
        assert!(response.success);
        assert!(response.error.is_none());
        // A sub-millisecond report has a valid zero-millisecond duration.
        assert!(u128::from(response.metadata.generation_time_ms) <= started.elapsed().as_millis());
    }

    #[tokio::test]
    async fn test_export_functionality() {
        let audit_manager = Arc::new(AuditTrailManager::new(AuditConfig::default()));
        let api = ComplianceAPI::new(audit_manager);

        let request = ExportRequest {
            format: "json".to_string(),
            filters: ComplianceReportRequest {
                start_date: None,
                end_date: None,
                status_filters: None,
                priority_filters: None,
                transaction_types: None,
                oracle_ids: None,
                min_amount: None,
                max_amount: None,
                include_archived: Some(false),
                page: Some(1),
                page_size: Some(100),
            },
            include_sensitive: Some(false),
            compress: Some(false),
        };

        let response = api.export_compliance_data(request).await.unwrap();
        assert!(response.success);
        assert!(response.download_url.is_some());
        assert!(response.expires_at.is_some());
    }

    #[tokio::test]
    async fn test_invalid_export_format() {
        let audit_manager = Arc::new(AuditTrailManager::new(AuditConfig::default()));
        let api = ComplianceAPI::new(audit_manager);

        let request = ExportRequest {
            format: "invalid_format".to_string(),
            filters: ComplianceReportRequest {
                start_date: None,
                end_date: None,
                status_filters: None,
                priority_filters: None,
                transaction_types: None,
                oracle_ids: None,
                min_amount: None,
                max_amount: None,
                include_archived: Some(false),
                page: Some(1),
                page_size: Some(100),
            },
            include_sensitive: Some(false),
            compress: Some(false),
        };

        let response = api.export_compliance_data(request).await.unwrap();
        assert!(!response.success);
        assert!(response.error.is_some());
        assert!(response
            .error
            .unwrap()
            .contains("Unsupported export format"));
    }
}
