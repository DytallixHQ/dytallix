//! Integration tests for AI integration with replay protection and response caching
//!
//! This module tests the basic functionality of the AI integration system
//! with replay protection and response caching enabled.

use anyhow::Result;

use crate::consensus::{
    ai_integration::{AIIntegrationConfig, AIIntegrationManager, RiskThresholds},
    replay_protection::ReplayProtectionConfig,
    signature_verification::VerificationConfig,
    AIServiceConfig,
};

/// Create a test AI integration manager
async fn create_test_ai_integration() -> Result<AIIntegrationManager> {
    let config = AIIntegrationConfig {
        verification_config: VerificationConfig::default(),
        ai_service_config: AIServiceConfig {
            base_url: "http://localhost:8000".to_string(),
            timeout_seconds: 30,
            max_retries: 3,
            api_key: "test_key".to_string(),
            ..Default::default()
        },
        replay_protection_config: ReplayProtectionConfig {
            max_response_age_seconds: 300,    // 5 minutes
            response_cache_ttl_seconds: 3600, // 1 hour
            max_response_cache_size: 10000,
            cache_cleanup_interval_seconds: 300, // 5 minutes
            ..Default::default()
        },
        require_ai_verification: true,
        fail_on_ai_unavailable: false,
        ai_timeout_ms: 5000,
        enable_response_caching: true,
        response_cache_ttl: 300,
        enable_risk_based_processing: true,
        log_risk_decisions: true,
        risk_thresholds: RiskThresholds::default(),
    };

    AIIntegrationManager::new(config).await
}

#[tokio::test]
async fn test_replay_protection_integration() -> Result<()> {
    let ai_manager = create_test_ai_integration().await?;

    // Test that the system correctly initializes with replay protection
    let health = ai_manager.health_check().await?;
    assert!(health.get("config").is_some());
    assert!(health.get("cache_stats").is_some());

    // Test cache statistics
    let cache_stats = ai_manager.get_cache_stats().await;
    assert!(cache_stats.get("response_cache_size").is_some());
    assert!(cache_stats.get("replay_protection").is_some());

    // Test replay protection statistics
    let replay_stats = ai_manager.get_replay_protection_stats().await;
    assert!(replay_stats.is_object());

    println!("✓ AI Integration with Replay Protection initialized successfully");
    println!("✓ Cache statistics available");
    println!("✓ Replay protection statistics available");

    Ok(())
}

#[tokio::test]
async fn test_ai_integration_cleanup() -> Result<()> {
    let ai_manager = create_test_ai_integration().await?;

    // Test that cleanup works without errors
    ai_manager.cleanup().await;

    // Check that system is still functional after cleanup
    let health = ai_manager.health_check().await?;
    assert!(health.get("config").is_some());

    println!("✓ Cleanup executed successfully");
    println!("✓ System remains functional after cleanup");

    Ok(())
}

#[tokio::test]
async fn test_cache_invalidation_functionality() -> Result<()> {
    let ai_manager = create_test_ai_integration().await?;

    // Test oracle cache invalidation
    ai_manager.invalidate_oracle_cache("test-oracle").await;

    // Check that cache stats are accessible after invalidation
    let cache_stats = ai_manager.get_cache_stats().await;
    assert!(cache_stats.get("response_cache_size").is_some());

    println!("✓ Oracle cache invalidation executed");
    println!("✓ Cache statistics remain accessible");

    Ok(())
}

#[tokio::test]
async fn test_ai_integration_configuration() -> Result<()> {
    let ai_manager = create_test_ai_integration().await?;

    // Test that AI verification requirement is properly configured
    assert!(ai_manager.is_ai_verification_required());

    // Test getting statistics
    let stats = ai_manager.get_statistics().await;
    assert_eq!(stats.total_requests, 0);
    assert_eq!(stats.successful_verifications, 0);
    assert_eq!(stats.failed_verifications, 0);

    println!("✓ AI verification requirement properly configured");
    println!("✓ Statistics properly initialized");

    Ok(())
}

#[tokio::test]
async fn test_oracle_management() -> Result<()> {
    let ai_manager = create_test_ai_integration().await?;

    // Test listing oracles (should be empty initially)
    let oracles = ai_manager.list_oracles().await;
    assert!(oracles.is_empty());

    // Test getting non-existent oracle
    let oracle = ai_manager.get_oracle("non-existent").await;
    assert!(oracle.is_none());

    println!("✓ Oracle listing works correctly");
    println!("✓ Non-existent oracle handling works");

    Ok(())
}

#[tokio::test]
async fn test_verification_statistics() -> Result<()> {
    let ai_manager = create_test_ai_integration().await?;

    // Test getting verification statistics
    let verification_stats = ai_manager.get_verification_statistics().await;
    assert!(!verification_stats.is_empty());

    println!("✓ Verification statistics available");

    Ok(())
}
