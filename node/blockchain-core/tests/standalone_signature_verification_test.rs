//! Standalone integration test for signature verification functionality.
//!
//! This test creates a minimal version of the signature verification components
//! to test the integration flow without depending on the main consensus module.
use anyhow::Result;
use chrono;
use dytallix_pqc::{Signature, SignatureAlgorithm};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

// Minimal AI response structures for testing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestAIResponsePayload {
    pub id: String,
    pub request_id: String,
    pub response_data: serde_json::Value,
    pub timestamp: u64,
    pub status: String,
    pub oracle_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSignedAIOracleResponse {
    pub response: TestAIResponsePayload,
    pub signature: Signature,
    pub oracle_public_key: Vec<u8>,
    pub oracle_id: String,
    pub nonce: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestOracleRegistryEntry {
    pub oracle_id: String,
    pub public_key: Vec<u8>,
    pub reputation_score: f64,
    pub last_activity: u64,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub is_active: bool,
}

#[derive(Debug, Clone)]
pub struct TestSignatureVerifier {
    pub oracle_registry: Arc<RwLock<HashMap<String, TestOracleRegistryEntry>>>,
    pub nonce_cache: Arc<RwLock<HashMap<String, u64>>>,
}

impl TestSignatureVerifier {
    pub fn new() -> Self {
        Self {
            oracle_registry: Arc::new(RwLock::new(HashMap::new())),
            nonce_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register_oracle(&self, oracle: TestOracleRegistryEntry) -> Result<()> {
        let mut registry = self.oracle_registry.write().await;
        registry.insert(oracle.oracle_id.clone(), oracle);
        Ok(())
    }

    pub async fn verify_signed_response(
        &self,
        response: &TestSignedAIOracleResponse,
    ) -> Result<bool> {
        // Check if oracle is registered
        let registry = self.oracle_registry.read().await;
        if !registry.contains_key(&response.oracle_id) {
            return Ok(false);
        }

        // Check nonce for replay protection
        let mut nonce_cache = self.nonce_cache.write().await;
        let cache_key = format!("{}:{}", response.oracle_id, response.nonce);
        if nonce_cache.contains_key(&cache_key) {
            return Ok(false); // Nonce already used
        }
        nonce_cache.insert(cache_key, response.nonce);

        // In a real implementation, this would verify the PQC signature
        // For the test, we'll just return true to test the flow
        Ok(true)
    }

    pub async fn get_oracle_count(&self) -> usize {
        let registry = self.oracle_registry.read().await;
        registry.len()
    }
}

#[derive(Debug, Clone)]
pub struct TestAIIntegrationManager {
    pub signature_verifier: Arc<TestSignatureVerifier>,
    pub require_ai_verification: bool,
}

impl TestAIIntegrationManager {
    pub fn new(require_ai_verification: bool) -> Self {
        Self {
            signature_verifier: Arc::new(TestSignatureVerifier::new()),
            require_ai_verification,
        }
    }

    pub async fn register_oracle(&self, oracle: TestOracleRegistryEntry) -> Result<()> {
        self.signature_verifier.register_oracle(oracle).await
    }

    pub async fn verify_ai_response(&self, response: &TestSignedAIOracleResponse) -> Result<bool> {
        self.signature_verifier
            .verify_signed_response(response)
            .await
    }

    pub async fn get_oracle_count(&self) -> usize {
        self.signature_verifier.get_oracle_count().await
    }
}

// Test cases
#[tokio::test]
async fn test_signature_verification_basic_flow() -> Result<()> {
    println!("Testing basic signature verification flow...");

    let verifier = TestSignatureVerifier::new();

    // Register a test oracle
    let oracle = TestOracleRegistryEntry {
        oracle_id: "test-oracle-1".to_string(),
        public_key: vec![1, 2, 3, 4],
        reputation_score: 0.9,
        last_activity: chrono::Utc::now().timestamp() as u64,
        total_requests: 10,
        successful_requests: 9,
        failed_requests: 1,
        is_active: true,
    };

    verifier.register_oracle(oracle).await?;
    assert_eq!(verifier.get_oracle_count().await, 1);

    // Create a test signed response
    let response = TestSignedAIOracleResponse {
        response: TestAIResponsePayload {
            id: "test-response-1".to_string(),
            request_id: "test-request-1".to_string(),
            response_data: serde_json::json!({
                "risk_score": 0.3,
                "confidence": 0.95
            }),
            timestamp: chrono::Utc::now().timestamp() as u64,
            status: "success".to_string(),
            oracle_id: Some("test-oracle-1".to_string()),
        },
        signature: Signature {
            data: vec![9, 10, 11, 12],
            algorithm: SignatureAlgorithm::Dilithium5,
        },
        oracle_public_key: vec![1, 2, 3, 4],
        oracle_id: "test-oracle-1".to_string(),
        nonce: 12345,
    };

    // Verify the response
    let is_valid = verifier.verify_signed_response(&response).await?;
    assert!(is_valid, "Response should be valid");

    // Test replay protection
    let is_valid_replay = verifier.verify_signed_response(&response).await?;
    assert!(!is_valid_replay, "Replay should be rejected");

    println!("✓ Basic signature verification test passed");
    Ok(())
}

#[tokio::test]
async fn test_ai_integration_manager() -> Result<()> {
    println!("Testing AI integration manager...");

    let manager = TestAIIntegrationManager::new(true);

    // Register oracle
    let oracle = TestOracleRegistryEntry {
        oracle_id: "test-oracle-2".to_string(),
        public_key: vec![5, 6, 7, 8],
        reputation_score: 0.95,
        last_activity: chrono::Utc::now().timestamp() as u64,
        total_requests: 100,
        successful_requests: 95,
        failed_requests: 5,
        is_active: true,
    };

    manager.register_oracle(oracle).await?;
    assert_eq!(manager.get_oracle_count().await, 1);

    // Create signed response
    let response = TestSignedAIOracleResponse {
        response: TestAIResponsePayload {
            id: "test-response-2".to_string(),
            request_id: "test-request-2".to_string(),
            response_data: serde_json::json!({
                "risk_score": 0.7,
                "confidence": 0.88,
                "factors": ["high_amount", "new_recipient"]
            }),
            timestamp: chrono::Utc::now().timestamp() as u64,
            status: "success".to_string(),
            oracle_id: Some("test-oracle-2".to_string()),
        },
        signature: Signature {
            data: vec![13, 14, 15, 16],
            algorithm: SignatureAlgorithm::Dilithium5,
        },
        oracle_public_key: vec![5, 6, 7, 8],
        oracle_id: "test-oracle-2".to_string(),
        nonce: 67890,
    };

    // Verify response
    let is_valid = manager.verify_ai_response(&response).await?;
    assert!(is_valid, "Response should be valid");

    println!("✓ AI integration manager test passed");
    Ok(())
}

#[tokio::test]
async fn test_unregistered_oracle_rejection() -> Result<()> {
    println!("Testing unregistered oracle rejection...");

    let verifier = TestSignatureVerifier::new();

    // Try to verify response from unregistered oracle
    let response = TestSignedAIOracleResponse {
        response: TestAIResponsePayload {
            id: "test-response-3".to_string(),
            request_id: "test-request-3".to_string(),
            response_data: serde_json::json!({"risk_score": 0.5}),
            timestamp: chrono::Utc::now().timestamp() as u64,
            status: "success".to_string(),
            oracle_id: Some("unregistered-oracle".to_string()),
        },
        signature: Signature {
            data: vec![17, 18, 19, 20],
            algorithm: SignatureAlgorithm::Dilithium5,
        },
        oracle_public_key: vec![9, 10, 11, 12],
        oracle_id: "unregistered-oracle".to_string(),
        nonce: 11111,
    };

    let is_valid = verifier.verify_signed_response(&response).await?;
    assert!(!is_valid, "Unregistered oracle should be rejected");

    println!("✓ Unregistered oracle rejection test passed");
    Ok(())
}

#[tokio::test]
async fn test_nonce_replay_protection() -> Result<()> {
    println!("Testing nonce replay protection...");

    let verifier = TestSignatureVerifier::new();

    // Register oracle
    let oracle = TestOracleRegistryEntry {
        oracle_id: "test-oracle-nonce".to_string(),
        public_key: vec![13, 14, 15, 16],
        reputation_score: 0.8,
        last_activity: chrono::Utc::now().timestamp() as u64,
        total_requests: 50,
        successful_requests: 40,
        failed_requests: 10,
        is_active: true,
    };

    verifier.register_oracle(oracle).await?;

    // Create response with specific nonce
    let response = TestSignedAIOracleResponse {
        response: TestAIResponsePayload {
            id: "test-response-nonce".to_string(),
            request_id: "test-request-nonce".to_string(),
            response_data: serde_json::json!({"risk_score": 0.2}),
            timestamp: chrono::Utc::now().timestamp() as u64,
            status: "success".to_string(),
            oracle_id: Some("test-oracle-nonce".to_string()),
        },
        signature: Signature {
            data: vec![21, 22, 23, 24],
            algorithm: SignatureAlgorithm::Dilithium5,
        },
        oracle_public_key: vec![13, 14, 15, 16],
        oracle_id: "test-oracle-nonce".to_string(),
        nonce: 99999,
    };

    // First verification should succeed
    let is_valid_first = verifier.verify_signed_response(&response).await?;
    assert!(is_valid_first, "First verification should succeed");

    // Second verification with same nonce should fail
    let is_valid_second = verifier.verify_signed_response(&response).await?;
    assert!(
        !is_valid_second,
        "Second verification should fail due to nonce reuse"
    );

    println!("✓ Nonce replay protection test passed");
    Ok(())
}

#[tokio::test]
async fn test_multiple_oracles() -> Result<()> {
    println!("Testing multiple oracle management...");

    let manager = TestAIIntegrationManager::new(true);

    // Register multiple oracles
    for i in 1..=5 {
        let oracle = TestOracleRegistryEntry {
            oracle_id: format!("oracle-{}", i),
            public_key: vec![i as u8; 4],
            reputation_score: 0.8 + (i as f64 * 0.02),
            last_activity: chrono::Utc::now().timestamp() as u64,
            total_requests: i * 10,
            successful_requests: i * 9,
            failed_requests: i * 1,
            is_active: true,
        };

        manager.register_oracle(oracle).await?;
    }

    assert_eq!(manager.get_oracle_count().await, 5);

    // Test responses from different oracles
    for i in 1..=3 {
        let response = TestSignedAIOracleResponse {
            response: TestAIResponsePayload {
                id: format!("response-{}", i),
                request_id: format!("request-{}", i),
                response_data: serde_json::json!({"risk_score": 0.1 * i as f64}),
                timestamp: chrono::Utc::now().timestamp() as u64,
                status: "success".to_string(),
                oracle_id: Some(format!("oracle-{}", i)),
            },
            signature: Signature {
                data: vec![i as u8; 4],
                algorithm: SignatureAlgorithm::Dilithium5,
            },
            oracle_public_key: vec![i as u8; 4],
            oracle_id: format!("oracle-{}", i),
            nonce: 10000 + i,
        };

        let is_valid = manager.verify_ai_response(&response).await?;
        assert!(is_valid, "Response from oracle-{} should be valid", i);
    }

    println!("✓ Multiple oracle management test passed");
    Ok(())
}

#[tokio::test]
async fn test_integration_comprehensive() -> Result<()> {
    println!("Running comprehensive integration test...");

    let manager = TestAIIntegrationManager::new(true);

    // Test scenario: complete oracle lifecycle
    let oracle = TestOracleRegistryEntry {
        oracle_id: "comprehensive-oracle".to_string(),
        public_key: vec![100, 101, 102, 103],
        reputation_score: 0.9,
        last_activity: chrono::Utc::now().timestamp() as u64,
        total_requests: 1000,
        successful_requests: 900,
        failed_requests: 100,
        is_active: true,
    };

    manager.register_oracle(oracle).await?;

    // Process multiple requests
    for i in 1..=10 {
        let response = TestSignedAIOracleResponse {
            response: TestAIResponsePayload {
                id: format!("comprehensive-response-{}", i),
                request_id: format!("comprehensive-request-{}", i),
                response_data: serde_json::json!({
                    "risk_score": 0.1 * i as f64,
                    "confidence": 0.9,
                    "transaction_id": format!("tx-{}", i)
                }),
                timestamp: chrono::Utc::now().timestamp() as u64,
                status: "success".to_string(),
                oracle_id: Some("comprehensive-oracle".to_string()),
            },
            signature: Signature {
                data: vec![i as u8; 4],
                algorithm: SignatureAlgorithm::Dilithium5,
            },
            oracle_public_key: vec![100, 101, 102, 103],
            oracle_id: "comprehensive-oracle".to_string(),
            nonce: 50000 + i,
        };

        let is_valid = manager.verify_ai_response(&response).await?;
        assert!(is_valid, "Response {} should be valid", i);
    }

    // Test replay protection works across multiple requests
    let duplicate_response = TestSignedAIOracleResponse {
        response: TestAIResponsePayload {
            id: "duplicate-response".to_string(),
            request_id: "duplicate-request".to_string(),
            response_data: serde_json::json!({"risk_score": 0.5}),
            timestamp: chrono::Utc::now().timestamp() as u64,
            status: "success".to_string(),
            oracle_id: Some("comprehensive-oracle".to_string()),
        },
        signature: Signature {
            data: vec![99, 98, 97, 96],
            algorithm: SignatureAlgorithm::Dilithium5,
        },
        oracle_public_key: vec![100, 101, 102, 103],
        oracle_id: "comprehensive-oracle".to_string(),
        nonce: 50005, // Reuse nonce from request 5
    };

    let is_valid_duplicate = manager.verify_ai_response(&duplicate_response).await?;
    assert!(!is_valid_duplicate, "Duplicate nonce should be rejected");

    println!("✓ Comprehensive integration test passed");
    Ok(())
}

// The Rust test harness executes the six tests above. Do not call the
// synchronous wrappers generated by #[tokio::test] from another async runtime.
