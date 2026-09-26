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
//! ```rust
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
pub mod consensus_engine;
pub mod transaction_validation;
pub mod block_processing;
pub mod key_management;

// Additional AI integration modules
pub mod signature_verification;
pub mod ai_integration;
pub mod oracle_registry;
pub mod enhanced_ai_integration;
pub mod replay_protection;
pub mod high_risk_queue;
pub mod review_api;
pub mod notification_system;
pub mod notification_types;
pub mod audit_trail;
pub mod compliance_api;
pub mod performance_optimizer;

// Legacy module - to be fully refactored
pub mod mod_clean;

// Re-export main types and components for convenience
pub use types::*;
pub use consensus_engine::ConsensusEngine;

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
    use std::time::Duration;
    use serde_json::json;

    #[test]
    fn test_ai_oracle_client_creation() {
        let client = ai_oracle_client::AIOracleClient::new("http://localhost:8080".to_string());
        assert!(client.is_ok());

        let client = client.unwrap();
        assert_eq!(client.base_url(), "http://localhost:8080");
        assert_eq!(client.timeout(), Duration::from_secs(30));
    }

    #[test]
    fn test_ai_response_payload_creation() {
        let payload = AIResponsePayload::success(
            "req_123".to_string(),
            AIServiceType::FraudDetection,
            json!({"result": "clean"})
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
            json!({"risk_score": 0.1})
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
            error
        );

        assert!(!payload.is_successful());
        assert!(payload.is_failure());
        assert_eq!(payload.status, ResponseStatus::Failure);
    }

    #[test]
    fn test_ai_response_payload_timeout() {
        let payload = AIResponsePayload::timeout(
            "req_123".to_string(),
            AIServiceType::PatternAnalysis
        );

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
        // Note: supports_service requires setting supported_services field
        // For this test, we'll create a new oracle with services
        let mut oracle_with_services = identity.clone();
        oracle_with_services.supported_services = Some(vec![AIServiceType::FraudDetection, AIServiceType::RiskScoring]);

        assert!(oracle_with_services.supports_service(&AIServiceType::FraudDetection));
        assert!(!oracle_with_services.supports_service(&AIServiceType::KYC));
    }

    #[test]
    fn test_ai_service_load_calculations() {
        let mut load = AIServiceLoad {
            current_requests: 75,
            max_capacity: 100,
            avg_response_time_ms: 500,
            cpu_usage: 60.0,
            memory_usage: 45.0,
        };

        assert_eq!(load.load_percentage(), 0.75);
        assert!(!load.is_overloaded());
        assert!(load.is_high_load());
        assert!(load.is_available());
        assert_eq!(load.available_slots(), 25);

        load.update_load(95, 800);
        assert!(load.is_overloaded());
        assert!(!load.is_available());
    }

    #[test]
    fn test_circuit_breaker_functionality() {
        let mut circuit = CircuitBreakerContext::new(3, 60000);

        // Initially closed
        assert!(circuit.is_closed());
        assert!(circuit.should_allow_request());

        // Record failures
        circuit.record_failure();
        circuit.record_failure();
        assert!(circuit.is_closed());

        // Third failure should open circuit
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
        let load = AIServiceLoad::default();
        let health = AIHealthCheckResponse::healthy(
            AIServiceType::FraudDetection,
            load
        );

        assert!(health.is_healthy());
        assert!(health.is_available());
        assert_eq!(health.service_type, AIServiceType::FraudDetection);
    }

    #[test]
    fn test_signed_ai_oracle_response() {
        let payload = AIResponsePayload::success(
            "req_123".to_string(),
            AIServiceType::RiskScoring,
            json!({"risk_score": 0.2})
        );

        let signature = AIResponseSignature::new(
            SignatureAlgorithm::Dilithium5,
            vec![1, 2, 3, 4],
            vec![5, 6, 7, 8]
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
            oracle_identity
        );

        assert!(!signed_response.is_verified());
        assert_eq!(signed_response.oracle_identity.oracle_id, "oracle_1");
    }

    #[test]
    fn test_ai_response_payload_serialization() {
        let payload = AIResponsePayload::success(
            "req_123".to_string(),
            AIServiceType::ContractAnalysis,
            json!({"analysis": "safe"})
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
            json!({"reputation": "good"})
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
        assert_eq!(AIServiceStatus::Unavailable.to_string(), "Unavailable");
        assert_eq!(AIServiceStatus::Failed.to_string(), "Failed");
    }

    #[test]
    fn test_error_category_display() {
        assert_eq!(ErrorCategory::ValidationError.to_string(), "ValidationError");
        assert_eq!(ErrorCategory::NetworkError.to_string(), "NetworkError");
        assert_eq!(ErrorCategory::AuthenticationError.to_string(), "AuthenticationError");
    }

    #[test]
    fn test_ai_response_age_calculation() {
        let mut payload = AIResponsePayload::success(
            "req_123".to_string(),
            AIServiceType::ThreatDetection,
            json!({"threat_level": "low"})
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
            vec![5, 6, 7, 8]
        );

        // Set timestamp to 10 minutes ago
        signature.signature_timestamp = (chrono::Utc::now().timestamp() - 600) as u64;

        assert_eq!(signature.age_seconds(), 600);
        assert!(!signature.is_fresh());
    }

    #[test]
    fn test_verification_data_handling() {
        let verification_data = VerificationData {
            verified_at: chrono::Utc::now().timestamp() as u64,
            is_valid: true,
            errors: Vec::new(),
            warnings: vec!["Low confidence score".to_string()],
            metadata: Some(json!({"verifier": "test"})),
        };

        assert!(verification_data.is_valid);
        assert_eq!(verification_data.errors.len(), 0);
        assert_eq!(verification_data.warnings.len(), 1);
    }

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
        assert_eq!(circuit.stats().successful_requests, 3);
        assert_eq!(circuit.stats().failed_requests, 2);
    }

    #[test]
    fn test_ai_service_load_resource_updates() {
        let mut load = AIServiceLoad::default();

        load.update_resources(85.5, 67.2);
        assert_eq!(load.cpu_usage, 85.5);
        assert_eq!(load.memory_usage, 67.2);

        // Test clamping
        load.update_resources(150.0, -10.0);
        assert_eq!(load.cpu_usage, 100.0);
        assert_eq!(load.memory_usage, 0.0);
    }

    #[test]
    fn test_health_check_response_timing() {
        let load = AIServiceLoad::default();
        let mut health = AIHealthCheckResponse::healthy(
            AIServiceType::CreditAssessment,
            load
        );

        health.mark_success();
        assert!(health.time_since_last_success().unwrap() < 5); // Should be very recent

        health.mark_failure("Test failure".to_string());
        assert!(health.time_since_last_failure().unwrap() < 5); // Should be very recent
        assert_eq!(health.error_message.as_ref().unwrap(), "Test failure");
    }
}
