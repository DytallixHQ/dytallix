//! Simple tests for risk-based processing functionality

#[cfg(test)]
mod tests {
    use crate::consensus::ai_integration::*;

    #[test]
    fn test_risk_thresholds_creation() {
        let thresholds = RiskThresholds::default();

        // Test default values for transfer
        assert_eq!(thresholds.transfer.auto_approve_threshold, 0.2);
        assert_eq!(thresholds.transfer.auto_reject_threshold, 0.8);
        assert_eq!(thresholds.transfer.amount_review_threshold, Some(1_000_000));

        // Test default values for deploy (should be more strict)
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

    #[test]
    fn test_risk_processing_decision_equality() {
        // Test the PartialEq implementation
        let auto_approve1 = RiskProcessingDecision::AutoApprove;
        let auto_approve2 = RiskProcessingDecision::AutoApprove;
        assert_eq!(auto_approve1, auto_approve2);

        let review1 = RiskProcessingDecision::RequireReview {
            reason: "Test reason".to_string(),
        };
        let review2 = RiskProcessingDecision::RequireReview {
            reason: "Test reason".to_string(),
        };
        assert_eq!(review1, review2);

        let reject1 = RiskProcessingDecision::AutoReject {
            reason: "Test reason".to_string(),
        };
        let reject2 = RiskProcessingDecision::AutoReject {
            reason: "Test reason".to_string(),
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
        assert!(
            config.risk_thresholds.transfer.auto_approve_threshold
                < config.risk_thresholds.transfer.auto_reject_threshold
        );
        assert!(
            config.risk_thresholds.deploy.auto_approve_threshold
                < config.risk_thresholds.deploy.auto_reject_threshold
        );

        // Verify deploy is more strict than transfer
        assert!(
            config.risk_thresholds.deploy.auto_approve_threshold
                < config.risk_thresholds.transfer.auto_approve_threshold
        );
        assert!(
            config.risk_thresholds.deploy.min_confidence_threshold
                > config.risk_thresholds.transfer.min_confidence_threshold
        );
    }
}
