//! Notification System for High-Risk Transaction Queue
//!
//! This module implements a notification system to alert compliance officers
//! about high-risk transactions that require manual review. It supports
//! multiple notification channels and can be extended with email, SMS,
//! or webhook integrations.

use anyhow::Result;
use chrono::{DateTime, Utc};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::consensus::notification_types::{
    NotificationChannel as TypesNotificationChannel, NotificationType, ReviewPriority,
};

/// Configuration alias for backward compatibility
pub type NotificationSystemConfig = NotificationConfig;

/// Configuration for the notification system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    /// Enable email notifications
    pub enable_email: bool,
    /// Email server configuration
    pub email_smtp_server: Option<String>,
    pub email_smtp_port: Option<u16>,
    pub email_username: Option<String>,
    pub email_password: Option<String>,
    /// List of compliance officer email addresses
    pub officer_emails: Vec<String>,
    /// Enable webhook notifications
    pub enable_webhooks: bool,
    /// Webhook URL for notifications
    pub webhook_url: Option<String>,
    /// Enable in-app notifications
    pub enable_in_app: bool,
    /// Notification retention period (hours)
    pub retention_hours: u64,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            enable_email: false, // Disabled by default for demo
            email_smtp_server: None,
            email_smtp_port: None,
            email_username: None,
            email_password: None,
            officer_emails: vec![
                "compliance@dytallix.com".to_string(),
                "security@dytallix.com".to_string(),
            ],
            enable_webhooks: false,
            webhook_url: None,
            enable_in_app: true,
            retention_hours: 168, // 7 days
        }
    }
}

/// A notification message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: Uuid,
    pub notification_type: NotificationType,
    pub title: String,
    pub message: String,
    pub priority: NotificationPriority,
    pub created_at: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub acknowledged_at: Option<DateTime<Utc>>,
    pub acknowledged_by: Option<String>,
    pub channels_sent: Vec<TypesNotificationChannel>,
    pub retry_count: u32,
}

/// Priority level for notifications
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NotificationPriority {
    Low,
    Medium,
    High,
    Critical,
}

/// Notification delivery status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryStatus {
    pub channel: TypesNotificationChannel,
    pub delivered: bool,
    pub delivered_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
    pub retry_count: u32,
}

/// Notification statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationStats {
    pub total_sent: usize,
    pub total_delivered: usize,
    pub total_acknowledged: usize,
    pub total_failed: usize,
    pub average_delivery_time_seconds: f64,
    pub by_priority: std::collections::HashMap<NotificationPriority, usize>,
    pub by_channel: std::collections::HashMap<TypesNotificationChannel, usize>,
}

/// Main notification system
#[derive(Debug)]
pub struct NotificationSystem {
    config: NotificationConfig,
    notifications: Arc<Mutex<Vec<Notification>>>,
    // Email client would go here in a real implementation
    // webhook_client: Arc<reqwest::Client>,
}

impl NotificationSystem {
    /// Create a new notification system
    pub fn new(config: NotificationConfig) -> Self {
        Self {
            config,
            notifications: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Send a notification
    pub async fn send_notification(&self, notification_type: NotificationType) -> Result<Uuid> {
        let notification = self.create_notification(notification_type).await;
        let notification_id = notification.id;

        // Store the notification
        {
            let mut notifications = self.notifications.lock().await;
            notifications.push(notification.clone());
        }

        // Send through configured channels
        self.deliver_notification(notification).await?;

        Ok(notification_id)
    }

    /// Get all notifications for a user/officer
    pub async fn get_notifications(&self, _officer_id: Option<String>) -> Vec<Notification> {
        let notifications = self.notifications.lock().await;
        // For now, return all notifications. In a real implementation,
        // this would filter by officer_id and permissions
        notifications.clone()
    }

    /// Get unacknowledged notifications
    pub async fn get_unacknowledged_notifications(&self) -> Vec<Notification> {
        let notifications = self.notifications.lock().await;
        notifications
            .iter()
            .filter(|n| n.acknowledged_at.is_none())
            .cloned()
            .collect()
    }

    /// Acknowledge a notification
    pub async fn acknowledge_notification(
        &self,
        notification_id: Uuid,
        officer_id: String,
    ) -> Result<()> {
        let mut notifications = self.notifications.lock().await;

        if let Some(notification) = notifications.iter_mut().find(|n| n.id == notification_id) {
            notification.acknowledged_at = Some(Utc::now());
            notification.acknowledged_by = Some(officer_id.clone());

            info!("Notification {notification_id} acknowledged by {officer_id}");
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Notification not found: {}",
                notification_id
            ))
        }
    }

    /// Get notification statistics
    pub async fn get_statistics(&self) -> NotificationStats {
        let notifications = self.notifications.lock().await;

        let mut stats = NotificationStats {
            total_sent: notifications.len(),
            total_delivered: 0,
            total_acknowledged: 0,
            total_failed: 0,
            average_delivery_time_seconds: 0.0,
            by_priority: std::collections::HashMap::new(),
            by_channel: std::collections::HashMap::new(),
        };

        let mut delivery_times = Vec::new();

        for notification in notifications.iter() {
            // Count acknowledged
            if notification.acknowledged_at.is_some() {
                stats.total_acknowledged += 1;
            }

            // Count delivered
            if notification.delivered_at.is_some() {
                stats.total_delivered += 1;

                if let Some(delivered_at) = notification.delivered_at {
                    let delivery_time =
                        (delivered_at - notification.created_at).num_seconds() as f64;
                    delivery_times.push(delivery_time);
                }
            }

            // Count by priority
            *stats
                .by_priority
                .entry(notification.priority.clone())
                .or_insert(0) += 1;

            // Count by channels
            for channel in &notification.channels_sent {
                *stats.by_channel.entry(channel.clone()).or_insert(0) += 1;
            }
        }

        // Calculate average delivery time
        if !delivery_times.is_empty() {
            stats.average_delivery_time_seconds =
                delivery_times.iter().sum::<f64>() / delivery_times.len() as f64;
        }

        stats.total_failed = stats.total_sent - stats.total_delivered;

        stats
    }

    /// Clean up old notifications
    pub async fn cleanup_old_notifications(&self) -> Result<usize> {
        let cutoff_time = Utc::now() - chrono::Duration::hours(self.config.retention_hours as i64);
        let mut notifications = self.notifications.lock().await;

        let initial_count = notifications.len();
        notifications.retain(|n| n.created_at > cutoff_time);
        let removed_count = initial_count - notifications.len();

        if removed_count > 0 {
            info!("Cleaned up {removed_count} old notifications");
        }

        Ok(removed_count)
    }

    /// Create a notification from a notification type
    async fn create_notification(&self, notification_type: NotificationType) -> Notification {
        let (title, message, priority) = match &notification_type {
            NotificationType::NewHighRiskTransaction {
                queue_id,
                transaction_hash,
                risk_score,
                priority: tx_priority,
            } => {
                let title = format!("🚨 New {tx_priority:?} Priority Transaction");
                let message = format!(
                    "A new {tx_priority:?} priority transaction has been queued for manual review. Queue ID: {queue_id}, Transaction: {transaction_hash}, Risk Score: {risk_score:.2}"
                );
                let notification_priority = match tx_priority {
                    ReviewPriority::Critical => NotificationPriority::Critical,
                    ReviewPriority::High => NotificationPriority::High,
                    ReviewPriority::Medium => NotificationPriority::Medium,
                    ReviewPriority::Low => NotificationPriority::Low,
                };
                (title, message, notification_priority)
            }
            NotificationType::TransactionExpired {
                queue_id,
                transaction_hash,
                expiry_time: _,
            } => {
                let title = "⏰ Transaction Expired".to_string();
                let message = format!(
                    "Transaction {transaction_hash} (Queue ID: {queue_id}) has expired in the review queue without being processed."
                );
                (title, message, NotificationPriority::Medium)
            }
            NotificationType::ReviewTimeout {
                queue_id,
                officer_id,
                assigned_time: _,
            } => {
                let title = "⏰ Review Timeout".to_string();
                let message = format!(
                    "Transaction {queue_id} has exceeded the maximum review time. Officer: {officer_id}"
                );
                (title, message, NotificationPriority::High)
            }
            NotificationType::QueueCapacityWarning {
                current_size,
                max_size,
                warning_level: _,
            } => {
                let title = "⚠️ Queue Capacity Warning".to_string();
                let message = format!(
                    "The high-risk transaction queue is approaching capacity: {current_size}/{max_size} transactions"
                );
                (title, message, NotificationPriority::High)
            }
            NotificationType::TransactionApproved {
                queue_id,
                transaction_hash,
                officer_id,
            } => {
                let title = "✅ Transaction Approved".to_string();
                let message = format!(
                    "Transaction {transaction_hash} (Queue ID: {queue_id}) has been approved by officer {officer_id}"
                );
                (title, message, NotificationPriority::Low)
            }
            NotificationType::TransactionRejected {
                queue_id,
                transaction_hash,
                officer_id,
                reason,
            } => {
                let title = "❌ Transaction Rejected".to_string();
                let message = format!(
                    "Transaction {transaction_hash} (Queue ID: {queue_id}) has been rejected by officer {officer_id}. Reason: {reason}"
                );
                (title, message, NotificationPriority::Low)
            }
            NotificationType::ManualReviewAssigned {
                queue_id,
                officer_id,
                transaction_hash,
            } => {
                let title = "👨‍💼 Manual Review Assigned".to_string();
                let message = format!(
                    "Transaction {transaction_hash} (Queue ID: {queue_id}) has been assigned to officer {officer_id} for manual review"
                );
                (title, message, NotificationPriority::Medium)
            }
            NotificationType::SystemAlert { message, severity } => {
                let title = format!("🚨 System Alert ({severity:?})");
                let priority = match severity {
                    crate::consensus::notification_types::AlertSeverity::Critical => {
                        NotificationPriority::Critical
                    }
                    crate::consensus::notification_types::AlertSeverity::High => {
                        NotificationPriority::High
                    }
                    crate::consensus::notification_types::AlertSeverity::Medium => {
                        NotificationPriority::Medium
                    }
                    crate::consensus::notification_types::AlertSeverity::Low => {
                        NotificationPriority::Low
                    }
                };
                (title, message.clone(), priority)
            }
        };

        Notification {
            id: Uuid::new_v4(),
            notification_type,
            title,
            message,
            priority,
            created_at: Utc::now(),
            delivered_at: None,
            acknowledged_at: None,
            acknowledged_by: None,
            channels_sent: Vec::new(),
            retry_count: 0,
        }
    }

    /// Deliver notification through configured channels
    async fn deliver_notification(&self, mut notification: Notification) -> Result<()> {
        let mut delivered = false;

        // In-app notifications (always enabled for now)
        if self.config.enable_in_app {
            info!("📱 {}: {}", notification.title, notification.message);
            notification
                .channels_sent
                .push(TypesNotificationChannel::InApp);
            delivered = true;
        }

        // Email notifications (placeholder implementation)
        if self.config.enable_email && !self.config.officer_emails.is_empty() {
            match self.send_email_notification(&notification).await {
                Ok(_) => {
                    info!("📧 Email notification sent for: {}", notification.title);
                    notification
                        .channels_sent
                        .push(TypesNotificationChannel::Email);
                    delivered = true;
                }
                Err(e) => {
                    warn!("Failed to send email notification: {e}");
                }
            }
        }

        // Webhook notifications (placeholder implementation)
        if self.config.enable_webhooks {
            match self.send_webhook_notification(&notification).await {
                Ok(_) => {
                    info!("🔗 Webhook notification sent for: {}", notification.title);
                    notification
                        .channels_sent
                        .push(TypesNotificationChannel::Webhook);
                    delivered = true;
                }
                Err(e) => {
                    warn!("Failed to send webhook notification: {e}");
                }
            }
        }

        if delivered {
            // Update the stored notification with delivery status
            let mut notifications = self.notifications.lock().await;
            if let Some(stored_notification) =
                notifications.iter_mut().find(|n| n.id == notification.id)
            {
                stored_notification.delivered_at = Some(Utc::now());
                stored_notification.channels_sent = notification.channels_sent;
            }
        }

        Ok(())
    }

    /// Send email notification (placeholder implementation)
    async fn send_email_notification(&self, notification: &Notification) -> Result<()> {
        // In a real implementation, this would use an email client library
        // like lettre or sendgrid to send actual emails

        info!(
            "📧 [EMAIL PLACEHOLDER] To: {:?}",
            self.config.officer_emails
        );
        info!("📧 [EMAIL PLACEHOLDER] Subject: {}", notification.title);
        info!("📧 [EMAIL PLACEHOLDER] Body: {}", notification.message);

        // Simulate email sending delay
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        Ok(())
    }

    /// Send webhook notification (placeholder implementation)
    async fn send_webhook_notification(&self, notification: &Notification) -> Result<()> {
        // In a real implementation, this would make an HTTP POST request
        // to the configured webhook URL with the notification data

        if let Some(webhook_url) = &self.config.webhook_url {
            info!("🔗 [WEBHOOK PLACEHOLDER] URL: {webhook_url}");
            info!(
                "🔗 [WEBHOOK PLACEHOLDER] Payload: {}",
                serde_json::to_string(notification)?
            );

            // Simulate webhook sending delay
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }

        Ok(())
    }
}

/// Integration helper for the high-risk queue
pub struct QueueNotificationIntegration {
    notification_system: Arc<NotificationSystem>,
}

impl QueueNotificationIntegration {
    /// Create a new integration
    pub fn new(config: NotificationConfig) -> Self {
        Self {
            notification_system: Arc::new(NotificationSystem::new(config)),
        }
    }

    /// Get the notification system
    pub fn get_notification_system(&self) -> Arc<NotificationSystem> {
        self.notification_system.clone()
    }

    /// Process notifications from the queue
    pub async fn process_queue_notifications(
        &self,
        notifications: Vec<NotificationType>,
    ) -> Result<()> {
        for notification_type in notifications {
            if let Err(e) = self
                .notification_system
                .send_notification(notification_type)
                .await
            {
                error!("Failed to send notification: {e}");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::notification_types::{NotificationType, ReviewPriority};

    #[tokio::test]
    async fn test_notification_creation() {
        let config = NotificationConfig::default();
        let system = NotificationSystem::new(config);

        let notification_type = NotificationType::NewHighRiskTransaction {
            queue_id: Uuid::new_v4(),
            transaction_hash: "test_hash".to_string(),
            risk_score: 0.9,
            priority: ReviewPriority::High,
        };

        let notification_id = system.send_notification(notification_type).await.unwrap();

        let notifications = system.get_notifications(None).await;
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].id, notification_id);
        assert!(notifications[0].title.contains("High Priority"));
    }

    #[tokio::test]
    async fn test_notification_acknowledgment() {
        let config = NotificationConfig::default();
        let system = NotificationSystem::new(config);

        let notification_type = NotificationType::QueueCapacityWarning {
            current_size: 950,
            max_size: 1000,
            warning_level: 3,
        };

        let notification_id = system.send_notification(notification_type).await.unwrap();

        // Acknowledge the notification
        system
            .acknowledge_notification(notification_id, "officer1".to_string())
            .await
            .unwrap();

        let notifications = system.get_notifications(None).await;
        assert!(notifications[0].acknowledged_at.is_some());
        assert_eq!(
            notifications[0].acknowledged_by,
            Some("officer1".to_string())
        );
    }

    #[tokio::test]
    async fn test_notification_statistics() {
        let config = NotificationConfig::default();
        let system = NotificationSystem::new(config);

        // Send multiple notifications
        for i in 0..3 {
            let notification_type = NotificationType::NewHighRiskTransaction {
                queue_id: Uuid::new_v4(),
                transaction_hash: format!("test_hash_{}", i),
                risk_score: 0.9,
                priority: if i == 0 {
                    ReviewPriority::Critical
                } else {
                    ReviewPriority::High
                },
            };
            system.send_notification(notification_type).await.unwrap();
        }

        let stats = system.get_statistics().await;
        assert_eq!(stats.total_sent, 3);
        assert_eq!(stats.total_delivered, 3); // All should be delivered via in-app
        assert!(stats
            .by_priority
            .contains_key(&NotificationPriority::Critical));
        assert!(stats.by_priority.contains_key(&NotificationPriority::High));
    }
}
