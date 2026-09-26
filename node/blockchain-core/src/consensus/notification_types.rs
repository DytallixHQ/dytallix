// Shared notification types for the high-risk transaction queue system
// These types are used across multiple modules to ensure consistency

use crate::types::TxHash;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// Re-export types that are used by other modules
pub use crate::consensus::high_risk_queue::ReviewPriority;

/// Types of notifications that can be sent to compliance officers
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NotificationType {
    /// New high-risk transaction added to queue
    NewHighRiskTransaction {
        queue_id: Uuid,
        transaction_hash: TxHash,
        risk_score: f64,
        priority: ReviewPriority,
    },
    /// Transaction approved by compliance officer
    TransactionApproved {
        queue_id: Uuid,
        transaction_hash: TxHash,
        officer_id: String,
    },
    /// Transaction rejected by compliance officer
    TransactionRejected {
        queue_id: Uuid,
        transaction_hash: TxHash,
        officer_id: String,
        reason: String,
    },
    /// Transaction expired and was automatically removed
    TransactionExpired {
        queue_id: Uuid,
        transaction_hash: TxHash,
        expiry_time: DateTime<Utc>,
    },
    /// Manual review assignment to specific officer
    ManualReviewAssigned {
        queue_id: Uuid,
        officer_id: String,
        transaction_hash: TxHash,
    },
    /// Review timeout warning
    ReviewTimeout {
        queue_id: Uuid,
        officer_id: String,
        assigned_time: DateTime<Utc>,
    },
    /// Queue capacity warning
    QueueCapacityWarning {
        current_size: usize,
        max_size: usize,
        warning_level: u8, // 1-5 scale
    },
    /// System alerts
    SystemAlert {
        message: String,
        severity: AlertSeverity,
    },
}

/// Alert severity levels
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AlertSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Notification delivery channels
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NotificationChannel {
    InApp,
    Email,
    Webhook,
    SMS, // Future extension
}

/// Notification status tracking
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NotificationStatus {
    Pending,
    Sent,
    Delivered,
    Acknowledged,
    Failed,
    Expired,
}

/// Individual notification record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: Uuid,
    pub notification_type: NotificationType,
    pub channel: NotificationChannel,
    pub recipient: String, // Officer ID or email
    pub status: NotificationStatus,
    pub created_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
    pub acknowledged_at: Option<DateTime<Utc>>,
    pub metadata: HashMap<String, String>,
}

/// Notification delivery configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    pub enabled_channels: Vec<NotificationChannel>,
    pub email_settings: Option<EmailSettings>,
    pub webhook_settings: Option<WebhookSettings>,
    pub retry_attempts: u32,
    pub retry_delay_seconds: u64,
}

/// Email notification settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailSettings {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
    pub password: String, // Should be encrypted in production
    pub from_address: String,
    pub template_dir: Option<String>,
}

/// Webhook notification settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookSettings {
    pub url: String,
    pub secret_key: Option<String>,
    pub headers: HashMap<String, String>,
    pub timeout_seconds: u64,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            enabled_channels: vec![NotificationChannel::InApp],
            email_settings: None,
            webhook_settings: None,
            retry_attempts: 3,
            retry_delay_seconds: 60,
        }
    }
}

impl NotificationType {
    /// Get the priority level for this notification type
    pub fn priority_level(&self) -> u8 {
        match self {
            NotificationType::SystemAlert {
                severity: AlertSeverity::Critical,
                ..
            } => 5,
            NotificationType::QueueCapacityWarning { warning_level, .. } => *warning_level,
            NotificationType::ReviewTimeout { .. } => 4,
            NotificationType::TransactionExpired { .. } => 3,
            NotificationType::NewHighRiskTransaction {
                priority: ReviewPriority::Critical,
                ..
            } => 4,
            NotificationType::NewHighRiskTransaction {
                priority: ReviewPriority::High,
                ..
            } => 3,
            NotificationType::NewHighRiskTransaction { .. } => 2,
            NotificationType::ManualReviewAssigned { .. } => 2,
            NotificationType::TransactionApproved { .. } => 1,
            NotificationType::TransactionRejected { .. } => 1,
            NotificationType::SystemAlert { .. } => 2,
        }
    }

    /// Get a human-readable title for this notification
    pub fn title(&self) -> String {
        match self {
            NotificationType::NewHighRiskTransaction { .. } => {
                "New High-Risk Transaction".to_string()
            }
            NotificationType::TransactionApproved { .. } => "Transaction Approved".to_string(),
            NotificationType::TransactionRejected { .. } => "Transaction Rejected".to_string(),
            NotificationType::TransactionExpired { .. } => "Transaction Expired".to_string(),
            NotificationType::ManualReviewAssigned { .. } => "Manual Review Assigned".to_string(),
            NotificationType::ReviewTimeout { .. } => "Review Timeout Warning".to_string(),
            NotificationType::QueueCapacityWarning { .. } => "Queue Capacity Warning".to_string(),
            NotificationType::SystemAlert { .. } => "System Alert".to_string(),
        }
    }
}
