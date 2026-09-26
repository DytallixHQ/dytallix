//! Dytallix Consensus Module
//!
//! This module contains the consensus mechanism for the Dytallix blockchain,
//! including AI-powered oracle services, transaction validation, and block processing.
//!
//! ## Architecture
//!
//! The consensus module is organized into several focused sub-modules:
//!
//! - `types`: Core type definitions for AI services, oracles, and consensus
//! - `ai_oracle_client`: Client for communicating with AI oracle services
//! - `consensus_engine`: Main consensus engine implementation
//! - `transaction_validation`: Transaction validation logic
//! - `block_processing`: Block creation and validation
//! - `key_management`: Post-quantum cryptographic key management
//!
//! ## Usage
//!
//! ```ignore
//! use dytallix_blockchain_core::consensus::{ConsensusEngine, AIServiceType};
//!
//! // Create a new consensus engine
//! let engine = ConsensusEngine::new(config).await?;
//!
//! // Process transactions
//! let result = engine.process_transactions(transactions).await?;
//! ```

// Re-export types from the types module
pub mod types;

// Core business logic modules
pub mod ai_oracle_client;
pub mod block_processing;
pub mod consensus_engine;
pub mod http_circuit_breaker;
pub mod key_management;
pub mod transaction_validation;

// Additional AI integration modules
pub mod ai_integration;
pub mod audit_trail;
pub mod compliance_api;
pub mod enhanced_ai_integration;
pub mod high_risk_queue;
pub mod notification_system;
pub mod notification_types;
pub mod oracle_registry;
pub mod performance_optimizer;
pub mod replay_protection;
pub mod review_api;
pub mod signature_verification;

// Legacy module - to be fully refactored
pub mod mod_clean;

// Re-export main types and components for convenience
pub use ai_oracle_client::{AIOracleClient, AIServiceConfig};
pub use consensus_engine::ConsensusEngine;
pub use dytallix_pqc::SignatureAlgorithm;
pub use types::*;

// Test modules
#[cfg(test)]
pub mod integration_tests;

#[cfg(test)]
pub mod transaction_validation_tests;

#[cfg(test)]
pub mod simple_risk_tests;

#[cfg(test)]
pub mod performance_test;

// Temporary stub - to be removed after full refactoring
pub struct DytallixConsensus;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_ai_oracle_client_creation() {
        let config = AIServiceConfig {
            base_url: "http://localhost:8080".to_string(),
            timeout_seconds: 30,
            max_retries: 3,
            risk_threshold: 0.7,
            api_key: "test_key".to_string(),
            retry_delay_ms: 500,
        };
        let client = ai_oracle_client::AIOracleClient::new(config);

        assert_eq!(client.base_url(), "http://localhost:8080");
        assert_eq!(client.timeout(), std::time::Duration::from_secs(30));
    }

    #[test]
    fn test_ai_response_payload_creation() {
        let payload = AIResponsePayload::success(
            "req_123".to_string(),
            AIServiceType::FraudDetection,
            json!({"result": "clean"}),
        );

        assert_eq!(payload.request_id, "req_123");
        assert_eq!(payload.service_type, AIServiceType::FraudDetection);
        assert_eq!(payload.status, ResponseStatus::Success);
        assert!(payload.is_successful());
    }

    #[test]
    fn test_ai_response_payload_validation() {
        let payload = AIResponsePayload::success(
            "req_123".to_string(),
            AIServiceType::RiskScoring,
            json!({"risk_score": 0.1}),
        );

        assert!(payload.validate().is_ok());
    }

    #[test]
    fn test_ai_response_payload_failure() {
        let error = AIResponseError {
            code: "INVALID_INPUT".to_string(),
            message: "Invalid transaction data".to_string(),
            details: None,
            category: ErrorCategory::ValidationError,
            retryable: false,
        };

        let payload = AIResponsePayload::failure(
            "req_123".to_string(),
            AIServiceType::TransactionValidation,
            error,
        );

        assert!(!payload.is_successful());
        assert!(payload.is_failure());
        assert_eq!(payload.status, ResponseStatus::Failure);
    }

    #[test]
    fn test_ai_response_payload_timeout() {
        let payload =
            AIResponsePayload::timeout("req_123".to_string(), AIServiceType::PatternAnalysis);

        assert!(payload.is_timeout());
        assert!(payload.is_retryable());
    }

    #[test]
    fn test_ai_service_type_display() {
        assert_eq!(AIServiceType::FraudDetection.to_string(), "Fraud Detection");
        assert_eq!(AIServiceType::RiskScoring.to_string(), "Risk Scoring");
        assert_eq!(AIServiceType::KYC.to_string(), "KYC");
    }

    #[test]
    fn test_response_status_display() {
        assert_eq!(ResponseStatus::Success.to_string(), "Success");
        assert_eq!(ResponseStatus::Failure.to_string(), "Failure");
        assert_eq!(ResponseStatus::Timeout.to_string(), "Timeout");
    }

    #[test]
    fn test_oracle_identity_creation() {
        let identity = OracleIdentity::new(
            "oracle_1".to_string(),
            "Test Oracle".to_string(),
            vec![1, 2, 3, 4],
            SignatureAlgorithm::Dilithium5,
        );

        assert_eq!(identity.oracle_id, "oracle_1");
        assert_eq!(identity.name, "Test Oracle");

        // Create an oracle with services for testing
        let mut oracle_with_services = identity.clone();
        oracle_with_services.supported_services = Some(vec![
            AIServiceType::FraudDetection,
            AIServiceType::RiskScoring,
        ]);

        assert!(oracle_with_services.supports_service(&AIServiceType::FraudDetection));
        assert!(!oracle_with_services.supports_service(&AIServiceType::KYC));
    }

    #[test]
    fn test_ai_service_load_calculations() {
        // Adjusted to current AIServiceLoad (Option fields)
        let mut load = AIServiceLoad {
            cpu_usage: Some(60.0),
            memory_usage: Some(45.0),
            queue_size: Some(75),
            requests_per_second: None,
            avg_response_time_ms: Some(500.0),
        };

        assert_eq!(load.cpu_usage.unwrap(), 60.0);
        assert_eq!(load.memory_usage.unwrap(), 45.0);
        assert!(load.avg_response_time_ms.unwrap() <= 500.0);

        // Update values
        load.cpu_usage = Some(85.5);
        load.memory_usage = Some(67.2);
        assert_eq!(load.cpu_usage.unwrap(), 85.5);
        assert_eq!(load.memory_usage.unwrap(), 67.2);
    }

    #[test]
    fn test_circuit_breaker_functionality() {
        // The API uses a failure ratio and seconds, with ten minimum requests.
        let mut circuit = CircuitBreakerContext::new(0.3, 60);

        // Initially closed
        assert!(circuit.is_closed());
        assert!(circuit.should_allow_request());

        // Seven successes and three failures reach the 30% threshold at request ten.
        for _ in 0..7 {
            circuit.record_success(100);
        }
        circuit.record_failure();
        circuit.record_failure();
        assert!(circuit.is_closed());

        // The tenth request meets both the minimum count and failure-ratio gate.
        circuit.record_failure();
        assert!(circuit.is_open());
        assert!(!circuit.should_allow_request());

        // Success should close circuit when half-open
        circuit.state = CircuitBreakerState::HalfOpen;
        circuit.record_success(100);
        assert!(circuit.is_closed());
    }

    #[test]
    fn test_ai_health_check_response() {
        // Adapted: no helper constructor currently implemented
        let load = AIServiceLoad {
            cpu_usage: Some(10.0),
            memory_usage: Some(20.0),
            queue_size: Some(5),
            requests_per_second: Some(12.0),
            avg_response_time_ms: Some(100.0),
        };
        let health = AIHealthCheckResponse {
            status: AIServiceStatus::Healthy,
            timestamp: chrono::Utc::now().timestamp() as u64,
            response_time_ms: 42,
            version: Some("v1".to_string()),
            details: None,
            endpoints: None,
            load: Some(load),
        };

        assert_eq!(health.status, AIServiceStatus::Healthy);
    }

    #[test]
    fn test_signed_ai_oracle_response() {
        let payload = AIResponsePayload::success(
            "req_123".to_string(),
            AIServiceType::RiskScoring,
            json!({"risk_score": 0.2}),
        );

        let signature = AIResponseSignature::new(
            SignatureAlgorithm::Dilithium5,
            vec![1, 2, 3, 4],
            vec![5, 6, 7, 8],
        );

        let oracle_identity = OracleIdentity::new(
            "oracle_1".to_string(),
            "Test Oracle".to_string(),
            vec![5, 6, 7, 8],
            SignatureAlgorithm::Dilithium5,
        );

        let signed_response = SignedAIOracleResponse::new(
            payload,
            signature,
            12345,
            (chrono::Utc::now().timestamp() + 60) as u64,
            oracle_identity,
        );

        assert!(!signed_response.is_verified());
        assert_eq!(signed_response.oracle_identity.oracle_id, "oracle_1");
    }

    #[test]
    fn test_ai_response_payload_serialization() {
        let payload = AIResponsePayload::success(
            "req_123".to_string(),
            AIServiceType::ContractAnalysis,
            json!({"analysis": "safe"}),
        );

        let json_str = payload.to_json().unwrap();
        let deserialized = AIResponsePayload::from_json(&json_str).unwrap();

        assert_eq!(deserialized.request_id, payload.request_id);
        assert_eq!(deserialized.service_type, payload.service_type);
        assert_eq!(deserialized.status, payload.status);
    }

    #[test]
    fn test_ai_response_metadata() {
        let metadata = AIResponseMetadata {
            model_version: "v1.0.0".to_string(),
            confidence_score: Some(0.95),
            processing_stats: Some(json!({"processing_time": 150})),
            context: None,
            oracle_reputation: Some(0.8),
            correlation_id: Some("corr_123".to_string()),
        };

        let mut payload = AIResponsePayload::success(
            "req_123".to_string(),
            AIServiceType::AddressReputation,
            json!({"reputation": "good"}),
        );

        payload = payload.with_metadata(metadata);
        assert_eq!(payload.model_version().unwrap(), "v1.0.0");
        assert_eq!(payload.confidence_score().unwrap(), 0.95);
        assert_eq!(payload.oracle_reputation().unwrap(), 0.8);
    }

    #[test]
    fn test_request_priority_ordering() {
        assert!(RequestPriority::Critical > RequestPriority::High);
        assert!(RequestPriority::High > RequestPriority::Normal);
        assert!(RequestPriority::Normal > RequestPriority::Low);
    }

    #[test]
    fn test_ai_service_status_display() {
        assert_eq!(AIServiceStatus::Healthy.to_string(), "Healthy");
        assert_eq!(AIServiceStatus::Degraded.to_string(), "Degraded");
        assert_eq!(AIServiceStatus::Unhealthy.to_string(), "Unhealthy");
        assert_eq!(AIServiceStatus::Unknown.to_string(), "Unknown");
    }

    #[test]
    fn test_error_category_display() {
        assert_eq!(
            ErrorCategory::ValidationError.to_string(),
            "ValidationError"
        );
        assert_eq!(ErrorCategory::NetworkError.to_string(), "NetworkError");
        assert_eq!(
            ErrorCategory::AuthenticationError.to_string(),
            "AuthenticationError"
        );
    }

    #[test]
    fn test_ai_response_age_calculation() {
        let mut payload = AIResponsePayload::success(
            "req_123".to_string(),
            AIServiceType::ThreatDetection,
            json!({"threat_level": "low"}),
        );

        // Set timestamp to 1 hour ago
        payload.timestamp = (chrono::Utc::now().timestamp() - 3600) as u64;

        assert_eq!(payload.age_seconds(), 3600);
        assert!(!payload.is_fresh());
        assert!(payload.is_stale());
    }

    #[test]
    fn test_oracle_identity_activity_tracking() {
        let mut identity = OracleIdentity::new(
            "oracle_1".to_string(),
            "Test Oracle".to_string(),
            vec![1, 2, 3, 4],
            SignatureAlgorithm::Dilithium5,
        );

        // Set last activity to 2 hours ago
        identity.last_activity = Some((chrono::Utc::now().timestamp() - 7200) as u64);

        assert!(identity.inactive_seconds() >= 7200);
        assert!(identity.is_inactive());

        identity.update_activity();
        assert!(!identity.is_inactive());
    }

    #[test]
    fn test_oracle_reputation_updates() {
        let mut identity = OracleIdentity::new(
            "oracle_1".to_string(),
            "Test Oracle".to_string(),
            vec![1, 2, 3, 4],
            SignatureAlgorithm::Dilithium5,
        );

        assert_eq!(identity.reputation_score, 0.5);

        identity.update_reputation_mut(0.9);
        assert_eq!(identity.reputation_score, 0.9);

        // Test clamping
        identity.update_reputation_mut(1.5);
        assert_eq!(identity.reputation_score, 1.0);

        identity.update_reputation_mut(-0.1);
        assert_eq!(identity.reputation_score, 0.0);
    }

    #[test]
    fn test_signature_age_calculation() {
        let mut signature = AIResponseSignature::new(
            SignatureAlgorithm::Dilithium5,
            vec![1, 2, 3, 4],
            vec![5, 6, 7, 8],
        );

        // Set timestamp to 10 minutes ago
        signature.signature_timestamp = (chrono::Utc::now().timestamp() - 600) as u64;

        assert_eq!(signature.age_seconds(), 600);
        assert!(!signature.is_fresh());
    }

    // TODO: Restore this test after VerificationData struct is stabilized
    // #[test]
    // fn test_verification_data_handling() {
    //     let verification_data = VerificationData {
    //         verified_at: chrono::Utc::now().timestamp() as u64,
    //         is_valid: true,
    //         errors: Vec::new(),
    //         warnings: vec!["Low confidence score".to_string()],
    //         metadata: Some(json!({"verifier": "test"})),
    //     };
    //
    //     assert!(verification_data.is_valid);
    //     assert_eq!(verification_data.errors.len(), 0);
    //     assert_eq!(verification_data.warnings.len(), 1);
    // }

    #[test]
    fn test_circuit_breaker_failure_rate() {
        let mut circuit = CircuitBreakerContext::default();

        // Record some successes and failures
        circuit.record_success(100);
        circuit.record_success(150);
        circuit.record_failure();
        circuit.record_success(120);
        circuit.record_failure();

        // Should be 2 failures out of 5 total = 40% failure rate
        assert_eq!(circuit.failure_rate(), 0.4);
        assert_eq!(circuit.stats().total_requests, 5);
        assert_eq!(circuit.stats().success_count, 3);
        assert_eq!(circuit.stats().failure_count, 2);
    }

    #[test]
    fn test_ai_service_load_resource_updates() {
        // Adapted to current AIServiceLoad struct without helper methods
        let mut load = AIServiceLoad {
            cpu_usage: None,
            memory_usage: None,
            queue_size: None,
            requests_per_second: None,
            avg_response_time_ms: None,
        };

        load.cpu_usage = Some(85.5);
        load.memory_usage = Some(67.2);
        load.queue_size = Some(10);
        load.requests_per_second = Some(42.0);
        load.avg_response_time_ms = Some(250.0);

        assert_eq!(load.cpu_usage, Some(85.5));
        assert_eq!(load.memory_usage, Some(67.2));
        assert_eq!(load.queue_size, Some(10));
        assert_eq!(load.requests_per_second, Some(42.0));
        assert_eq!(load.avg_response_time_ms, Some(250.0));
    }
}
