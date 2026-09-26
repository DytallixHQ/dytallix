//! Simple performance optimizer test

use super::performance_optimizer::*;
use crate::consensus::ai_integration::{AIVerificationResult, RiskProcessingDecision};
use crate::types::{PQCTransactionSignature, Transaction, TransferTransaction};
use chrono::Utc;

#[tokio::test]
async fn test_performance_optimizer_basic_functionality() {
    let config = PerformanceConfig::default();
    let optimizer = PerformanceOptimizer::new(config);

    // Test cache operations
    let tx_hash = "test_hash".to_string();

    // Should return None for uncached result
    let result = optimizer.get_cached_result(&tx_hash).await;
    assert!(result.is_none());

    // Cache a result
    let ai_result = AIVerificationResult::Verified {
        oracle_id: "test".to_string(),
        response_id: "test".to_string(),
        risk_score: Some(0.5),
        confidence: Some(0.9),
        processing_decision: RiskProcessingDecision::AutoApprove,
        fraud_probability: Some(0.3),
    };

    optimizer.cache_result(&tx_hash, &ai_result).await.unwrap();

    // Should now return cached result
    let cached = optimizer.get_cached_result(&tx_hash).await;
    assert!(cached.is_some());

    println!("✓ Cache operations working correctly");
}

#[tokio::test]
async fn test_fallback_modes() {
    let config = PerformanceConfig::default();
    let optimizer = PerformanceOptimizer::new(config);

    let tx = create_test_transaction(5000u128);
    let tx_hash = "test_tx".to_string();

    // Test different fallback modes
    for mode in [
        FallbackMode::BasicOnly,
        FallbackMode::PatternBased,
        FallbackMode::HistoricalBased,
        FallbackMode::Conservative,
    ] {
        optimizer.activate_fallback(mode).await.unwrap();

        let result = optimizer.fallback_validation(&tx_hash, &tx).await;
        assert!(result.is_ok());

        optimizer.deactivate_fallback().await.unwrap();
    }

    println!("✓ Fallback modes working correctly");
}

#[tokio::test]
async fn test_performance_metrics() {
    let config = PerformanceConfig::default();
    let optimizer = PerformanceOptimizer::new(config);

    // Record some metrics
    optimizer.record_request_metrics(100, true).await;
    optimizer.record_request_metrics(200, false).await;
    optimizer.record_timeout().await;

    let metrics = optimizer.get_metrics().await;
    assert_eq!(metrics.total_requests, 3);
    assert_eq!(metrics.error_count, 1);
    assert_eq!(metrics.timeout_count, 1);
    assert_eq!(metrics.average_response_time_ms, 150.0);

    println!("✓ Performance metrics working correctly");
}

fn create_test_transaction(amount: u128) -> Transaction {
    Transaction::Transfer(TransferTransaction {
        hash: "test_tx".to_string(),
        from: "sender".to_string(),
        to: "recipient".to_string(),
        amount,
        fee: 10,
        nonce: 1,
        timestamp: Utc::now().timestamp() as u64,
        signature: PQCTransactionSignature {
            signature: dytallix_pqc::Signature {
                data: vec![],
                algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium5,
            },
            public_key: vec![],
        },
        ai_risk_score: None,
    })
}
