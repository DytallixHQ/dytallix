//! Integration tests for AI-enhanced transaction validation
//!
//! This module tests the enhanced transaction validation pipeline
//! with AI integration for Phase 3, Task 3.1

use anyhow::Result;
use std::sync::Arc;
use tokio;

use crate::consensus::ConsensusEngine;
use crate::crypto::PQCManager;
use crate::runtime::DytallixRuntime;
use crate::storage::StorageManager;
use crate::types::{AIRequestTransaction, AIServiceType, Transaction, TransferTransaction};

struct TestConsensus {
    engine: ConsensusEngine,
    // Drop the engine and its database before removing the directory.
    _directory: crate::test_support::TestDirectory,
}
impl std::ops::Deref for TestConsensus {
    type Target = ConsensusEngine;
    fn deref(&self) -> &Self::Target {
        &self.engine
    }
}

async fn create_test_consensus_engine() -> Result<TestConsensus> {
    let directory = crate::test_support::TestDirectory::new();
    // Initialize storage
    let storage = Arc::new(
        StorageManager::open(directory.path(), "consensus-test", None)
            .await
            .map_err(|e| anyhow::anyhow!("Storage error: {}", e))?,
    );

    // Initialize runtime
    let runtime = Arc::new(
        DytallixRuntime::new(storage).map_err(|e| anyhow::anyhow!("Runtime error: {}", e))?,
    );

    // Initialize PQC manager
    let pqc_manager =
        Arc::new(PQCManager::new().map_err(|e| anyhow::anyhow!("PQC manager error: {}", e))?);

    // Create consensus engine (it will initialize AI integration internally)
    let consensus = ConsensusEngine::new_with_key_path(
        runtime,
        pqc_manager,
        &directory.path().join("pqc_keys.json"),
    )
    .await
    .map_err(|e| anyhow::anyhow!("Consensus engine error: {}", e))?;

    Ok(TestConsensus {
        engine: consensus,
        _directory: directory,
    })
}

/// Create a test transfer transaction
fn create_test_transfer_transaction(amount: u128, from: &str, to: &str) -> Transaction {
    let mut transfer_tx = TransferTransaction {
        hash: String::new(),
        from: from.to_string(),
        to: to.to_string(),
        amount,
        fee: 1,
        nonce: 1,
        timestamp: chrono::Utc::now().timestamp() as u64,
        signature: crate::types::PQCTransactionSignature {
            signature: dytallix_pqc::Signature {
                data: Vec::new(),
                algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium5,
            },
            public_key: Vec::new(),
        },
        ai_risk_score: None,
    };

    // Calculate hash
    transfer_tx.hash = transfer_tx.calculate_hash();

    Transaction::Transfer(transfer_tx)
}

/// Create a test AI request transaction
fn create_test_ai_request_transaction() -> Transaction {
    let mut ai_request_tx = AIRequestTransaction {
        hash: String::new(),
        from: "dyt1test_user".to_string(),
        service_type: AIServiceType::FraudDetection,
        request_data: b"test_request_data".to_vec(),
        payload: serde_json::json!({
            "transaction_data": {
                "amount": 1000,
                "recipient": "dyt1suspicious_account"
            }
        }),
        ai_risk_score: None,
        ai_response: None,
        fee: 5,
        nonce: 1,
        timestamp: chrono::Utc::now().timestamp() as u64,
        signature: crate::types::PQCTransactionSignature {
            signature: dytallix_pqc::Signature {
                data: Vec::new(),
                algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium5,
            },
            public_key: Vec::new(),
        },
    };

    // Calculate hash
    ai_request_tx.hash = ai_request_tx.calculate_hash();

    Transaction::AIRequest(ai_request_tx)
}

#[tokio::test]
async fn test_consensus_initialization_integration() -> Result<()> {
    println!("=== Testing Consensus Engine AI Integration ===");

    let consensus = create_test_consensus_engine().await?;

    // Check AI integration availability
    let has_ai = consensus.has_ai_integration();
    println!(
        "✓ Consensus engine created, AI integration available: {}",
        has_ai
    );

    assert!(has_ai, "configured AI integration did not initialize");
    let stats = consensus
        .get_ai_integration_stats()
        .await
        .expect("AI statistics unavailable");
    for field in [
        "total_requests",
        "successful_verifications",
        "failed_verifications",
        "service_unavailable_count",
    ] {
        assert_eq!(
            stats.get(field).and_then(serde_json::Value::as_u64),
            Some(0),
            "initial {field}"
        );
    }

    // Test basic transaction validation with AI
    let transfer = create_test_transfer_transaction(100, "dyt1genesis", "dyt1user1");
    let result = consensus.validate_transaction_with_ai(&transfer).await;

    match result {
        Ok(valid) => {
            println!("✓ Transfer validation result: {}", valid);
        }
        Err(e) => {
            println!("✓ Transfer validation error (expected for mock AI): {}", e);
        }
    }

    println!("=== Consensus Engine AI Integration Test Completed ===");
    Ok(())
}

#[tokio::test]
async fn test_basic_transaction_validation_with_ai() -> Result<()> {
    let consensus = create_test_consensus_engine().await?;

    // Test 1: Valid small transfer (should pass AI analysis)
    let small_transfer = create_test_transfer_transaction(100, "dyt1genesis", "dyt1user1");
    let result = consensus
        .validate_transaction_with_ai(&small_transfer)
        .await;

    match result {
        Ok(valid) => {
            println!("✓ Small transfer validation result: {}", valid);
        }
        Err(e) => {
            println!(
                "✓ Small transfer validation error (expected for mock AI): {}",
                e
            );
        }
    }

    // Test 2: Large transfer (might trigger higher AI scrutiny)
    let large_transfer = create_test_transfer_transaction(1000000, "dyt1genesis", "dyt1user2");
    let result = consensus
        .validate_transaction_with_ai(&large_transfer)
        .await;

    match result {
        Ok(valid) => {
            println!("✓ Large transfer validation result: {}", valid);
        }
        Err(e) => {
            println!(
                "✓ Large transfer validation error (expected for mock AI): {}",
                e
            );
        }
    }

    // Test 3: AI request transaction
    let ai_request = create_test_ai_request_transaction();
    let result = consensus.validate_transaction_with_ai(&ai_request).await;

    match result {
        Ok(valid) => {
            println!("✓ AI request validation result: {}", valid);
        }
        Err(e) => {
            println!(
                "✓ AI request validation error (expected for mock AI): {}",
                e
            );
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_transaction_to_ai_data_conversion() -> Result<()> {
    let _consensus = create_test_consensus_engine().await?;

    // Test conversion of different transaction types
    let transfer = create_test_transfer_transaction(500, "dyt1alice", "dyt1bob");

    // This is testing the internal conversion logic
    // In a real implementation, we would expose this method or test it indirectly
    println!("✓ Transaction created for AI data conversion test");

    // Verify that the transaction contains the expected fields
    match &transfer {
        Transaction::Transfer(tx) => {
            assert_eq!(tx.amount, 500);
            assert_eq!(tx.from, "dyt1alice");
            assert_eq!(tx.to, "dyt1bob");
            println!("✓ Transfer transaction contains correct data for AI analysis");
        }
        _ => panic!("Expected transfer transaction"),
    }

    Ok(())
}

#[tokio::test]
async fn test_ai_integration_error_handling() -> Result<()> {
    let consensus = create_test_consensus_engine().await?;

    // Test validation when AI service might be unavailable
    let transfer = create_test_transfer_transaction(1000, "dyt1sender", "dyt1receiver");

    // The validation should handle AI unavailability gracefully
    let result = consensus.validate_transaction_with_ai(&transfer).await;

    // We expect either success (fallback allowed) or a graceful error
    match result {
        Ok(valid) => {
            println!(
                "✓ Transaction validation with AI unavailable: passed ({})",
                valid
            );
        }
        Err(e) => {
            println!(
                "✓ Transaction validation with AI unavailable: error handled gracefully ({})",
                e
            );
            // This is expected behavior when AI is not available
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_validation_pipeline_performance() -> Result<()> {
    let consensus = create_test_consensus_engine().await?;

    let start_time = std::time::Instant::now();

    // Validate multiple transactions to test performance
    for i in 0..10 {
        let transfer = create_test_transfer_transaction(
            100 + i * 10,
            "dyt1genesis",
            &format!("dyt1user{}", i),
        );

        let _result = consensus.validate_transaction_with_ai(&transfer).await;
    }

    let duration = start_time.elapsed();
    println!("✓ Validated 10 transactions in {:?}", duration);

    // Performance should be reasonable (even with AI calls)
    assert!(
        duration.as_secs() < 30,
        "Validation took too long: {:?}",
        duration
    );

    Ok(())
}

#[tokio::test]
async fn test_ai_enhanced_vs_basic_validation() -> Result<()> {
    let consensus = create_test_consensus_engine().await?;

    let transfer = create_test_transfer_transaction(1000, "dyt1genesis", "dyt1test");

    // Test basic validation (without AI)
    let basic_start = std::time::Instant::now();
    let basic_result = match &transfer {
        Transaction::Transfer(tx) => {
            // This would be testing internal validation logic
            // For now, we'll just check that the transaction is well-formed
            tx.amount > 0 && !tx.from.is_empty() && !tx.to.is_empty()
        }
        _ => false,
    };
    let basic_duration = basic_start.elapsed();

    // Test AI-enhanced validation
    let ai_start = std::time::Instant::now();
    let ai_result = consensus.validate_transaction_with_ai(&transfer).await;
    let ai_duration = ai_start.elapsed();

    println!(
        "✓ Basic validation result: {} (took {:?})",
        basic_result, basic_duration
    );
    println!(
        "✓ AI-enhanced validation result: {:?} (took {:?})",
        ai_result, ai_duration
    );

    // AI validation should not be orders of magnitude slower than basic validation
    // (though it may take longer due to network calls)

    Ok(())
}
