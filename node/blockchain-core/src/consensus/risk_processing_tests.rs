//! Tests for risk-based processing functionality
//!
//! This module contains comprehensive tests for Task 3.2: Risk-Based Processing Rules

#[cfg(test)]
mod tests {
    use super::super::ai_integration::*;
    use serde_json::json;

    /// Create a test AI integration config with custom risk thresholds
    fn create_test_config_with_risk_thresholds() -> AIIntegrationConfig {
        AIIntegrationConfig {
            risk_thresholds: RiskThresholds {
                transfer: TransactionRiskThresholds {
                    auto_approve_threshold: 0.3,
                    auto_reject_threshold: 0.8,
                    amount_review_threshold: Some(1_000_000),
                    fraud_reject_threshold: 0.7,
                    min_confidence_threshold: 0.6,
                },
                deploy: TransactionRiskThresholds {
                    auto_approve_threshold: 0.2,
                    auto_reject_threshold: 0.7,
                    amount_review_threshold: None,
                    fraud_reject_threshold: 0.5,
                    min_confidence_threshold: 0.8,
                },
                ..Default::default()
            },
            enable_risk_based_processing: true,
            log_risk_decisions: true,
            ..Default::default()
        }
    }

    #[test]
    fn test_risk_thresholds_creation() {
        let thresholds = RiskThresholds::default();

        // Test default values
        assert_eq!(thresholds.transfer.auto_approve_threshold, 0.2);
        assert_eq!(thresholds.transfer.auto_reject_threshold, 0.8);
        assert_eq!(thresholds.transfer.amount_review_threshold, Some(1_000_000));

        assert_eq!(thresholds.deploy.auto_approve_threshold, 0.1);
        assert_eq!(thresholds.deploy.auto_reject_threshold, 0.7);
        assert_eq!(thresholds.deploy.min_confidence_threshold, 0.8);
    }

    #[test]
    fn test_transaction_risk_thresholds_default() {
        let thresholds = TransactionRiskThresholds::default();

        assert_eq!(thresholds.auto_approve_threshold, 0.3);
        assert_eq!(thresholds.auto_reject_threshold, 0.8);
        assert_eq!(thresholds.fraud_reject_threshold, 0.7);
        assert_eq!(thresholds.min_confidence_threshold, 0.6);
        assert_eq!(thresholds.amount_review_threshold, None);
    }

    #[tokio::test]
    async fn test_risk_processing_auto_approve_low_risk() {
        let config = create_test_config_with_risk_thresholds();
        let ai_manager = AIIntegrationManager::new(config).await.unwrap();

        // Test low risk score -> auto approve
        let decision = ai_manager.make_risk_processing_decision(
            "transfer",
            0.1, // Low risk score
            0.2, // Low fraud probability
            0.9, // High confidence
            Some(100_000), // Normal amount
        );

        assert_eq!(decision, RiskProcessingDecision::AutoApprove);
    }

    #[tokio::test]
    async fn test_risk_processing_auto_reject_high_risk() {
        let config = create_test_config_with_risk_thresholds();
        let ai_manager = AIIntegrationManager::new(config).await.unwrap();

        // Test high risk score -> auto reject
        let decision = ai_manager.make_risk_processing_decision(
            "transfer",
            0.9, // High risk score
            0.3, // Normal fraud probability
            0.8, // Good confidence
            Some(100_000), // Normal amount
        );

        match decision {
            RiskProcessingDecision::AutoReject { reason } => {
                assert!(reason.contains("High risk score"));
            }
            _ => panic!("Expected AutoReject decision"),
        }
    }

    #[tokio::test]
    async fn test_risk_processing_auto_reject_high_fraud() {
        let config = create_test_config_with_risk_thresholds();
        let ai_manager = AIIntegrationManager::new(config).await.unwrap();

        // Test high fraud probability -> auto reject
        let decision = ai_manager.make_risk_processing_decision(
            "transfer",
            0.5, // Medium risk score
            0.8, // High fraud probability
            0.8, // Good confidence
            Some(100_000), // Normal amount
        );

        match decision {
            RiskProcessingDecision::AutoReject { reason } => {
                assert!(reason.contains("High fraud probability"));
            }
            _ => panic!("Expected AutoReject decision"),
        }
    }

    #[tokio::test]
    async fn test_risk_processing_require_review_medium_risk() {
        let config = create_test_config_with_risk_thresholds();
        let ai_manager = AIIntegrationManager::new(config).await.unwrap();

        // Test medium risk score -> require review
        let decision = ai_manager.make_risk_processing_decision(
            "transfer",
            0.5, // Medium risk score
            0.3, // Normal fraud probability
            0.8, // Good confidence
            Some(100_000), // Normal amount
        );

        match decision {
            RiskProcessingDecision::RequireReview { reason } => {
                assert!(reason.contains("Medium risk score"));
            }
            _ => panic!("Expected RequireReview decision"),
        }
    }

    #[tokio::test]
    async fn test_risk_processing_require_review_low_confidence() {
        let config = create_test_config_with_risk_thresholds();
        let ai_manager = AIIntegrationManager::new(config).await.unwrap();

        // Test low confidence -> require review
        let decision = ai_manager.make_risk_processing_decision(
            "transfer",
            0.2, // Low risk score
            0.1, // Low fraud probability
            0.5, // Low confidence (below threshold)
            Some(100_000), // Normal amount
        );

        match decision {
            RiskProcessingDecision::RequireReview { reason } => {
                assert!(reason.contains("AI confidence too low"));
            }
            _ => panic!("Expected RequireReview decision"),
        }
    }

    #[tokio::test]
    async fn test_risk_processing_require_review_large_amount() {
        let config = create_test_config_with_risk_thresholds();
        let ai_manager = AIIntegrationManager::new(config).await.unwrap();

        // Test large amount -> require review
        let decision = ai_manager.make_risk_processing_decision(
            "transfer",
            0.2, // Low risk score
            0.1, // Low fraud probability
            0.9, // High confidence
            Some(2_000_000), // Large amount (above threshold)
        );

        match decision {
            RiskProcessingDecision::RequireReview { reason } => {
                assert!(reason.contains("Large transaction amount"));
            }
            _ => panic!("Expected RequireReview decision"),
        }
    }

    #[tokio::test]
    async fn test_risk_processing_different_transaction_types() {
        let config = create_test_config_with_risk_thresholds();
        let ai_manager = AIIntegrationManager::new(config).await.unwrap();

        // Test deploy transaction with stricter thresholds
        let deploy_decision = ai_manager.make_risk_processing_decision(
            "deploy",
            0.25, // Risk score that would be auto-approved for transfer
            0.3,  // Normal fraud probability
            0.9,  // High confidence
            None, // No amount threshold for deploy
        );

        match deploy_decision {
            RiskProcessingDecision::RequireReview { reason } => {
                assert!(reason.contains("Medium risk score"));
            }
            _ => panic!("Expected RequireReview decision for deploy transaction"),
        }

        // Same risk score for transfer should auto-approve
        let transfer_decision = ai_manager.make_risk_processing_decision(
            "transfer",
            0.25, // Same risk score
            0.3,  // Same fraud probability
            0.9,  // Same confidence
            Some(100_000), // Small amount
        );

        assert_eq!(transfer_decision, RiskProcessingDecision::AutoApprove);
    }

    #[tokio::test]
    async fn test_risk_processing_disabled() {
        let mut config = create_test_config_with_risk_thresholds();
        config.enable_risk_based_processing = false;
        let ai_manager = AIIntegrationManager::new(config).await.unwrap();

        // When risk processing is disabled, should always auto-approve
        let decision = ai_manager.make_risk_processing_decision(
            "transfer",
            0.9, // High risk score
            0.9, // High fraud probability
            0.1, // Low confidence
            Some(10_000_000), // Very large amount
        );

        assert_eq!(decision, RiskProcessingDecision::AutoApprove);
    }

    #[tokio::test]
    async fn test_get_risk_thresholds_for_transaction_types() {
        let config = create_test_config_with_risk_thresholds();
        let ai_manager = AIIntegrationManager::new(config).await.unwrap();

        // Test getting thresholds for different transaction types
        let transfer_thresholds = ai_manager.get_risk_thresholds_for_transaction_type("transfer");
        assert_eq!(transfer_thresholds.auto_approve_threshold, 0.3);

        let deploy_thresholds = ai_manager.get_risk_thresholds_for_transaction_type("deploy");
        assert_eq!(deploy_thresholds.auto_approve_threshold, 0.2);

        let contract_deploy_thresholds = ai_manager.get_risk_thresholds_for_transaction_type("contract_deploy");
        assert_eq!(contract_deploy_thresholds.auto_approve_threshold, 0.2); // Same as deploy

        // Unknown transaction type should default to transfer
        let unknown_thresholds = ai_manager.get_risk_thresholds_for_transaction_type("unknown");
        assert_eq!(unknown_thresholds.auto_approve_threshold, 0.3);
    }

    #[tokio::test]
    async fn test_update_risk_thresholds() {
        let config = create_test_config_with_risk_thresholds();
        let mut ai_manager = AIIntegrationManager::new(config).await.unwrap();

        // Create new thresholds
        let new_thresholds = TransactionRiskThresholds {
            auto_approve_threshold: 0.4,
            auto_reject_threshold: 0.9,
            amount_review_threshold: Some(500_000),
            fraud_reject_threshold: 0.8,
            min_confidence_threshold: 0.7,
        };

        // Update thresholds
        ai_manager.update_risk_thresholds("transfer", new_thresholds.clone());

        // Verify update
        let updated_thresholds = ai_manager.get_risk_thresholds_for_transaction_type("transfer");
        assert_eq!(updated_thresholds.auto_approve_threshold, 0.4);
        assert_eq!(updated_thresholds.auto_reject_threshold, 0.9);
        assert_eq!(updated_thresholds.amount_review_threshold, Some(500_000));
    }

    #[tokio::test]
    async fn test_risk_processing_edge_cases() {
        let config = create_test_config_with_risk_thresholds();
        let ai_manager = AIIntegrationManager::new(config).await.unwrap();

        // Test edge case: risk score exactly at threshold
        let decision_at_threshold = ai_manager.make_risk_processing_decision(
            "transfer",
            0.3, // Exactly at auto_approve_threshold
            0.1,
            0.8,
            Some(100_000),
        );
        assert_eq!(decision_at_threshold, RiskProcessingDecision::AutoApprove);

        // Test edge case: risk score exactly at reject threshold
        let decision_at_reject_threshold = ai_manager.make_risk_processing_decision(
            "transfer",
            0.8, // Exactly at auto_reject_threshold
            0.1,
            0.8,
            Some(100_000),
        );
        match decision_at_reject_threshold {
            RiskProcessingDecision::AutoReject { .. } => {},
            _ => panic!("Expected AutoReject at exact threshold"),
        }

        // Test edge case: amount exactly at threshold
        let decision_at_amount_threshold = ai_manager.make_risk_processing_decision(
            "transfer",
            0.2, // Low risk
            0.1,
            0.8,
            Some(1_000_000), // Exactly at amount threshold
        );
        assert_eq!(decision_at_amount_threshold, RiskProcessingDecision::AutoApprove);

        // Test edge case: amount just above threshold
        let decision_above_amount_threshold = ai_manager.make_risk_processing_decision(
            "transfer",
            0.2, // Low risk
            0.1,
            0.8,
            Some(1_000_001), // Just above amount threshold
        );
        match decision_above_amount_threshold {
            RiskProcessingDecision::RequireReview { .. } => {},
            _ => panic!("Expected RequireReview for amount above threshold"),
        }
    }

    #[tokio::test]
    async fn test_complete_transaction_validation_with_risk_processing() {
        // This test simulates the complete flow from transaction data to risk decision
        let config = create_test_config_with_risk_thresholds();
        let ai_manager = AIIntegrationManager::new(config).await.unwrap();

        // Create mock transaction data
        let transaction_data = json!({
            "transaction_type": "transfer",
            "from": "addr1",
            "to": "addr2",
            "amount": 500_000,
            "hash": "test_hash_123"
        });

        // Extract transaction information like the real validation would
        let transaction_type = transaction_data.get("transaction_type")
            .and_then(|t| t.as_str())
            .unwrap_or("unknown");
        let transaction_amount = transaction_data.get("amount")
            .and_then(|a| a.as_u64());
        let transaction_hash = transaction_data.get("hash")
            .and_then(|h| h.as_str())
            .unwrap_or("unknown");

        // Simulate AI analysis results
        let combined_risk_score = 0.4; // Medium risk
        let fraud_probability = 0.2;   // Low fraud
        let combined_confidence = 0.8;  // High confidence

        // Make risk processing decision
        let decision = ai_manager.make_risk_processing_decision(
            transaction_type,
            combined_risk_score,
            fraud_probability,
            combined_confidence,
            transaction_amount,
        );

        // Log the decision (this tests the logging functionality)
        ai_manager.log_risk_decision(
            transaction_hash,
            transaction_type,
            &decision,
            combined_risk_score,
            fraud_probability,
            combined_confidence,
        );

        // Verify the decision
        match decision {
            RiskProcessingDecision::RequireReview { reason } => {
                assert!(reason.contains("Medium risk score"));
            }
            _ => panic!("Expected RequireReview for medium risk"),
        }
    }

    #[test]
    fn test_risk_processing_decision_equality() {
        // Test the PartialEq implementation
        let auto_approve1 = RiskProcessingDecision::AutoApprove;
        let auto_approve2 = RiskProcessingDecision::AutoApprove;
        assert_eq!(auto_approve1, auto_approve2);

        let review1 = RiskProcessingDecision::RequireReview {
            reason: "Test reason".to_string()
        };
        let review2 = RiskProcessingDecision::RequireReview {
            reason: "Test reason".to_string()
        };
        assert_eq!(review1, review2);

        let reject1 = RiskProcessingDecision::AutoReject {
            reason: "Test reason".to_string()
        };
        let reject2 = RiskProcessingDecision::AutoReject {
            reason: "Test reason".to_string()
        };
        assert_eq!(reject1, reject2);

        // Test inequality
        assert_ne!(auto_approve1, review1);
        assert_ne!(review1, reject1);
    }

    #[test]
    fn test_ai_integration_config_with_risk_thresholds() {
        let config = AIIntegrationConfig::default();

        // Verify risk-based processing is enabled by default
        assert!(config.enable_risk_based_processing);
        assert!(config.log_risk_decisions);

        // Verify default risk thresholds are properly set
        assert!(config.risk_thresholds.transfer.auto_approve_threshold < config.risk_thresholds.transfer.auto_reject_threshold);
        assert!(config.risk_thresholds.deploy.auto_approve_threshold < config.risk_thresholds.deploy.auto_reject_threshold);

        // Verify deploy is more strict than transfer
        assert!(config.risk_thresholds.deploy.auto_approve_threshold < config.risk_thresholds.transfer.auto_approve_threshold);
        assert!(config.risk_thresholds.deploy.min_confidence_threshold > config.risk_thresholds.transfer.min_confidence_threshold);
    }
}
