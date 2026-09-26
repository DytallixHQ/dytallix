//! Comprehensive tests for Oracle Registry and Reputation System
//!
//! This test suite validates the complete oracle management system including
//! registration, reputation tracking, slashing, and performance monitoring.

use anyhow::Result;
use std::time::Duration;
use tokio;

use dytallix_node::consensus::{
    enhanced_ai_integration::{EnhancedAIConfig, EnhancedAIIntegrationManager},
    oracle_registry::{OracleRegistry, OracleRegistryConfig, OracleStatus, RegisterOracleArgs},
};

/// Test data for oracle registry tests
struct OracleTestData {
    oracle_address: String,
    oracle_name: String,
    description: String,
    public_key: Vec<u8>,
    stake_amount: u128,
    oracle_version: String,
    supported_services: Vec<String>,
}

impl OracleTestData {
    fn new(id: u32) -> Self {
        Self {
            oracle_address: format!("dyt1oracle{}", id),
            oracle_name: format!("Test Oracle {}", id),
            description: format!("Test oracle {} for comprehensive testing", id),
            public_key: vec![id as u8; 32], // 32-byte mock public key
            stake_amount: 2000000000 + (id as u128 * 1000000), // Varying stake amounts
            oracle_version: "1.0.0".to_string(),
            supported_services: vec!["risk_scoring".to_string(), "fraud_detection".to_string()],
        }
    }
}

#[tokio::test]
async fn test_oracle_registration_complete_flow() -> Result<()> {
    println!("Testing complete oracle registration flow...");

    let config = OracleRegistryConfig::default();
    let registry = OracleRegistry::new(config)?;
    let test_data = OracleTestData::new(1);

    // Test 1: Successful registration
    let result = registry
        .register_oracle(RegisterOracleArgs {
            oracle_address: test_data.oracle_address.clone(),
            oracle_name: test_data.oracle_name.clone(),
            description: test_data.description.clone(),
            public_key: test_data.public_key.clone(),
            stake_amount: test_data.stake_amount,
            oracle_version: test_data.oracle_version.clone(),
            supported_services: test_data.supported_services.clone(),
            contact_info: Some("test1@example.com".to_string()),
        })
        .await;

    assert!(result.is_ok(), "Oracle registration should succeed");

    // Test 2: Verify oracle is registered with correct data
    let oracle = registry.get_oracle(&test_data.oracle_address).await;
    assert!(oracle.is_some(), "Oracle should be found in registry");

    let oracle = oracle.unwrap();
    assert_eq!(oracle.oracle_name, test_data.oracle_name);
    assert_eq!(oracle.status, OracleStatus::Pending);
    assert_eq!(oracle.stake.total_amount, test_data.stake_amount);
    assert_eq!(oracle.reputation.current_score, 1.0); // Should start with perfect reputation

    // Test 3: Activate oracle
    let activation_result = registry.activate_oracle(&test_data.oracle_address).await;
    assert!(
        activation_result.is_ok(),
        "Oracle activation should succeed"
    );

    let oracle = registry
        .get_oracle(&test_data.oracle_address)
        .await
        .unwrap();
    assert_eq!(oracle.status, OracleStatus::Active);

    // Test 4: Duplicate registration should fail
    let duplicate_result = registry
        .register_oracle(RegisterOracleArgs {
            oracle_address: test_data.oracle_address.clone(),
            oracle_name: "Duplicate Oracle".to_string(),
            description: "Should fail".to_string(),
            public_key: vec![99, 98, 97, 96],
            stake_amount: test_data.stake_amount,
            oracle_version: test_data.oracle_version.clone(),
            supported_services: test_data.supported_services.clone(),
            contact_info: None,
        })
        .await;

    assert!(
        duplicate_result.is_err(),
        "Duplicate registration should fail"
    );

    println!("✓ Complete oracle registration flow test passed");
    Ok(())
}

#[tokio::test]
async fn test_stake_requirements() -> Result<()> {
    println!("Testing stake requirements...");

    let config = OracleRegistryConfig {
        min_stake_amount: 5000000000, // 50 DYTX minimum
        ..Default::default()
    };
    let registry = OracleRegistry::new(config)?;
    let test_data = OracleTestData::new(2);

    // Test 1: Registration with insufficient stake should fail
    let insufficient_result = registry
        .register_oracle(RegisterOracleArgs {
oracle_address: test_data.oracle_address.clone(),
oracle_name: test_data.oracle_name.clone(),
description: test_data.description.clone(),
public_key: test_data.public_key.clone(),
stake_amount: 1000000000,
oracle_version: // Only 10 DYTX (below minimum)
            test_data.oracle_version.clone(),
supported_services: test_data.supported_services.clone(),
contact_info: None,
})
        .await;

    assert!(
        insufficient_result.is_err(),
        "Registration with insufficient stake should fail"
    );

    // Test 2: Registration with sufficient stake should succeed
    let sufficient_result = registry
        .register_oracle(RegisterOracleArgs {
oracle_address: test_data.oracle_address.clone(),
oracle_name: test_data.oracle_name.clone(),
description: test_data.description.clone(),
public_key: test_data.public_key.clone(),
stake_amount: 6000000000,
oracle_version: // 60 DYTX (above minimum)
            test_data.oracle_version.clone(),
supported_services: test_data.supported_services.clone(),
contact_info: None,
})
        .await;

    assert!(
        sufficient_result.is_ok(),
        "Registration with sufficient stake should succeed"
    );

    println!("✓ Stake requirements test passed");
    Ok(())
}

#[tokio::test]
async fn test_reputation_scoring_system() -> Result<()> {
    println!("Testing reputation scoring system...");

    let config = OracleRegistryConfig::default();
    let registry = OracleRegistry::new(config)?;
    let test_data = OracleTestData::new(3);

    // Register and activate oracle
    registry
        .register_oracle(RegisterOracleArgs {
            oracle_address: test_data.oracle_address.clone(),
            oracle_name: test_data.oracle_name.clone(),
            description: test_data.description.clone(),
            public_key: test_data.public_key.clone(),
            stake_amount: test_data.stake_amount,
            oracle_version: test_data.oracle_version.clone(),
            supported_services: test_data.supported_services.clone(),
            contact_info: None,
        })
        .await?;

    registry.activate_oracle(&test_data.oracle_address).await?;

    // Test 1: Accurate response with good timing should maintain high reputation
    registry
        .update_reputation(
            &test_data.oracle_address,
            1000, // 1 second response time
            true, // accurate
            true, // valid signature
        )
        .await?;

    let oracle = registry
        .get_oracle(&test_data.oracle_address)
        .await
        .unwrap();
    assert!(
        oracle.reputation.current_score > 0.9,
        "Good response should maintain high reputation"
    );
    assert_eq!(oracle.reputation.accurate_responses, 1);

    // Test 2: Inaccurate response should lower reputation
    registry
        .update_reputation(
            &test_data.oracle_address,
            2000,  // 2 second response time
            false, // inaccurate
            true,  // valid signature
        )
        .await?;

    let oracle = registry
        .get_oracle(&test_data.oracle_address)
        .await
        .unwrap();
    assert!(
        oracle.reputation.current_score < 0.9,
        "Inaccurate response should lower reputation"
    );
    assert_eq!(oracle.reputation.inaccurate_responses, 1);

    // Test 3: Invalid signature should significantly impact reputation
    registry
        .update_reputation(
            &test_data.oracle_address,
            1500,  // 1.5 second response time
            true,  // accurate (but signature invalid)
            false, // invalid signature
        )
        .await?;

    let oracle = registry
        .get_oracle(&test_data.oracle_address)
        .await
        .unwrap();
    assert!(
        oracle.reputation.current_score < 0.8,
        "Invalid signature should significantly impact reputation"
    );
    assert_eq!(oracle.reputation.invalid_signature_responses, 1);

    // Test 4: Multiple good responses should improve reputation
    for _ in 0..10 {
        registry
            .update_reputation(
                &test_data.oracle_address,
                800,  // Fast response time
                true, // accurate
                true, // valid signature
            )
            .await?;
    }

    let oracle = registry
        .get_oracle(&test_data.oracle_address)
        .await
        .unwrap();
    assert!(
        oracle.reputation.current_score > 0.8,
        "Multiple good responses should improve reputation"
    );
    assert_eq!(oracle.reputation.total_responses, 13); // 3 + 10

    println!("✓ Reputation scoring system test passed");
    Ok(())
}

#[tokio::test]
async fn test_oracle_slashing_system() -> Result<()> {
    println!("Testing oracle slashing system...");

    let config = OracleRegistryConfig {
        slashing_percentage: 0.2,  // 20% slashing
        slashing_grace_period: 10, // 10 seconds for testing
        ..Default::default()
    };
    let registry = OracleRegistry::new(config)?;
    let test_data = OracleTestData::new(4);

    // Register and activate oracle
    registry
        .register_oracle(RegisterOracleArgs {
            oracle_address: test_data.oracle_address.clone(),
            oracle_name: test_data.oracle_name.clone(),
            description: test_data.description.clone(),
            public_key: test_data.public_key.clone(),
            stake_amount: test_data.stake_amount,
            oracle_version: test_data.oracle_version.clone(),
            supported_services: test_data.supported_services.clone(),
            contact_info: None,
        })
        .await?;

    registry.activate_oracle(&test_data.oracle_address).await?;

    // Test 1: Immediate slashing
    let immediate_slash_result = registry
        .slash_oracle(
            &test_data.oracle_address,
            "Malicious behavior detected".to_string(),
            true, // immediate
        )
        .await;

    assert!(
        immediate_slash_result.is_ok(),
        "Immediate slashing should succeed"
    );

    let oracle = registry
        .get_oracle(&test_data.oracle_address)
        .await
        .unwrap();
    assert_eq!(oracle.status, OracleStatus::Slashed);
    assert_eq!(oracle.stake.locked_amount, test_data.stake_amount / 5);

    println!("✓ Oracle slashing system test passed");
    Ok(())
}

#[tokio::test]
async fn test_grace_period_slashing() -> Result<()> {
    println!("Testing grace period slashing...");

    let config = OracleRegistryConfig {
        slashing_percentage: 0.15, // 15% slashing
        slashing_grace_period: 2,  // 2 seconds for testing
        ..Default::default()
    };
    let registry = OracleRegistry::new(config)?;
    let test_data = OracleTestData::new(5);

    // Register and activate oracle
    registry
        .register_oracle(RegisterOracleArgs {
            oracle_address: test_data.oracle_address.clone(),
            oracle_name: test_data.oracle_name.clone(),
            description: test_data.description.clone(),
            public_key: test_data.public_key.clone(),
            stake_amount: test_data.stake_amount,
            oracle_version: test_data.oracle_version.clone(),
            supported_services: test_data.supported_services.clone(),
            contact_info: None,
        })
        .await?;

    registry.activate_oracle(&test_data.oracle_address).await?;

    // Test 1: Grace period slashing
    let grace_slash_result = registry
        .slash_oracle(
            &test_data.oracle_address,
            "Accuracy issues detected".to_string(),
            false, // grace period
        )
        .await;

    assert!(
        grace_slash_result.is_ok(),
        "Grace period slashing should succeed"
    );

    let oracle = registry
        .get_oracle(&test_data.oracle_address)
        .await
        .unwrap();
    assert_eq!(oracle.status, OracleStatus::Suspended);
    assert!(oracle.stake.pending_slash > 0);
    assert!(oracle.stake.slash_grace_end.is_some());

    // Test 2: Process pending slashing before grace period ends
    let process_result = registry.process_pending_slashing().await;
    assert!(
        process_result.is_ok(),
        "Processing pending slashing should succeed"
    );

    // Oracle should still be suspended (grace period not ended)
    let oracle = registry
        .get_oracle(&test_data.oracle_address)
        .await
        .unwrap();
    assert_eq!(oracle.status, OracleStatus::Suspended);

    // Test 3: Wait for grace period and process again
    tokio::time::sleep(Duration::from_secs(3)).await;
    let process_result = registry.process_pending_slashing().await;
    assert!(
        process_result.is_ok(),
        "Processing after grace period should succeed"
    );

    // Oracle should now be slashed
    let oracle = registry
        .get_oracle(&test_data.oracle_address)
        .await
        .unwrap();
    assert_eq!(oracle.status, OracleStatus::Slashed);
    assert_eq!(oracle.stake.pending_slash, 0);
    assert!(oracle.stake.locked_amount > 0);

    println!("✓ Grace period slashing test passed");
    Ok(())
}

#[tokio::test]
async fn test_oracle_whitelist_blacklist() -> Result<()> {
    println!("Testing oracle whitelist/blacklist system...");

    let config = OracleRegistryConfig::default();
    let registry = OracleRegistry::new(config)?;
    let test_data1 = OracleTestData::new(6);
    let test_data2 = OracleTestData::new(7);

    // Test 1: Blacklist oracle before registration
    registry
        .blacklist_oracle(
            test_data1.oracle_address.clone(),
            "Known malicious actor".to_string(),
        )
        .await?;

    // Attempt to register blacklisted oracle should fail
    let blacklisted_result = registry
        .register_oracle(RegisterOracleArgs {
            oracle_address: test_data1.oracle_address.clone(),
            oracle_name: test_data1.oracle_name.clone(),
            description: test_data1.description.clone(),
            public_key: test_data1.public_key.clone(),
            stake_amount: test_data1.stake_amount,
            oracle_version: test_data1.oracle_version.clone(),
            supported_services: test_data1.supported_services.clone(),
            contact_info: None,
        })
        .await;

    assert!(
        blacklisted_result.is_err(),
        "Blacklisted oracle registration should fail"
    );

    // Test 2: Whitelist oracle and register successfully
    registry
        .whitelist_oracle(test_data2.oracle_address.clone())
        .await?;

    let whitelisted_result = registry
        .register_oracle(RegisterOracleArgs {
            oracle_address: test_data2.oracle_address.clone(),
            oracle_name: test_data2.oracle_name.clone(),
            description: test_data2.description.clone(),
            public_key: test_data2.public_key.clone(),
            stake_amount: test_data2.stake_amount,
            oracle_version: test_data2.oracle_version.clone(),
            supported_services: test_data2.supported_services.clone(),
            contact_info: None,
        })
        .await;

    assert!(
        whitelisted_result.is_ok(),
        "Whitelisted oracle registration should succeed"
    );

    println!("✓ Oracle whitelist/blacklist test passed");
    Ok(())
}

#[tokio::test]
async fn test_oracle_performance_monitoring() -> Result<()> {
    println!("Testing oracle performance monitoring...");

    let config = OracleRegistryConfig::default();
    let registry = OracleRegistry::new(config)?;
    let test_data = OracleTestData::new(8);

    // Register and activate oracle
    registry
        .register_oracle(RegisterOracleArgs {
            oracle_address: test_data.oracle_address.clone(),
            oracle_name: test_data.oracle_name.clone(),
            description: test_data.description.clone(),
            public_key: test_data.public_key.clone(),
            stake_amount: test_data.stake_amount,
            oracle_version: test_data.oracle_version.clone(),
            supported_services: test_data.supported_services.clone(),
            contact_info: None,
        })
        .await?;

    registry.activate_oracle(&test_data.oracle_address).await?;

    // Simulate multiple responses with varying performance
    let response_data = vec![
        (1000, true, true),  // Good response
        (2000, true, true),  // Slower but good
        (500, false, true),  // Fast but inaccurate
        (3000, true, false), // Slow with invalid signature
        (800, true, true),   // Good response
    ];

    for (response_time, is_accurate, signature_valid) in response_data {
        registry
            .update_reputation(
                &test_data.oracle_address,
                response_time,
                is_accurate,
                signature_valid,
            )
            .await?;
    }

    // Verify performance metrics
    let oracle = registry
        .get_oracle(&test_data.oracle_address)
        .await
        .unwrap();
    assert_eq!(oracle.reputation.total_responses, 5);
    // Invalid signatures do not count as accurate responses.
    assert_eq!(oracle.reputation.accurate_responses, 3);
    assert_eq!(oracle.reputation.inaccurate_responses, 1);
    assert_eq!(oracle.reputation.invalid_signature_responses, 1);
    assert_eq!(
        oracle.reputation.total_responses,
        oracle.reputation.accurate_responses
            + oracle.reputation.inaccurate_responses
            + oracle.reputation.invalid_signature_responses
    );

    // Average response time should be calculated correctly
    let expected_avg = (1000.0 + 2000.0 + 500.0 + 3000.0 + 800.0) / 5.0;
    assert!((oracle.reputation.avg_response_time_ms - expected_avg).abs() < 1.0);

    println!("✓ Oracle performance monitoring test passed");
    Ok(())
}

#[tokio::test]
async fn test_enhanced_ai_integration() -> Result<()> {
    println!("Testing enhanced AI integration with oracle registry...");

    let config = EnhancedAIConfig::default();
    let manager = EnhancedAIIntegrationManager::new(config).await?;
    let test_data = OracleTestData::new(9);

    // Test 1: Register oracle through enhanced manager
    let registration_result = manager
        .register_oracle(RegisterOracleArgs {
            oracle_address: test_data.oracle_address.clone(),
            oracle_name: test_data.oracle_name.clone(),
            description: test_data.description.clone(),
            public_key: test_data.public_key.clone(),
            stake_amount: test_data.stake_amount,
            oracle_version: test_data.oracle_version.clone(),
            supported_services: test_data.supported_services.clone(),
            contact_info: Some("enhanced_test@example.com".to_string()),
        })
        .await;

    assert!(
        registration_result.is_ok(),
        "Enhanced oracle registration should succeed"
    );

    // Test 2: Activate oracle
    let activation_result = manager.activate_oracle(&test_data.oracle_address).await;
    assert!(
        activation_result.is_ok(),
        "Oracle activation should succeed"
    );

    // Test 3: Validate oracle authorization
    let auth_result = manager
        .validate_oracle_authorization(&test_data.oracle_address)
        .await;
    assert!(
        auth_result.is_authorized,
        "Active oracle should be authorized"
    );
    assert!(
        auth_result.reputation_score > 0.9,
        "New oracle should have high reputation"
    );

    // Test 4: Test oracle leaderboard
    let leaderboard = manager.get_oracle_leaderboard().await;
    assert!(
        !leaderboard.is_empty(),
        "Leaderboard should contain the registered oracle"
    );

    let (addr, reputation, status) = &leaderboard[0];
    assert_eq!(addr, &test_data.oracle_address);
    assert_eq!(*status, OracleStatus::Active);
    assert!(*reputation > 0.9);

    // Test 5: Get statistics
    let stats = manager.get_oracle_statistics().await?;
    assert!(stats["registry"]["total_registered"].as_u64().unwrap() >= 1);
    assert!(stats["registry"]["active_count"].as_u64().unwrap() >= 1);

    println!("✓ Enhanced AI integration test passed");
    Ok(())
}

#[tokio::test]
async fn test_daily_maintenance() -> Result<()> {
    println!("Testing daily maintenance functionality...");

    let config = OracleRegistryConfig {
        reputation_decay_factor: 0.95, // 5% decay for testing
        ..Default::default()
    };
    let registry = OracleRegistry::new(config)?;
    let test_data = OracleTestData::new(10);

    // Register and activate oracle
    registry
        .register_oracle(RegisterOracleArgs {
            oracle_address: test_data.oracle_address.clone(),
            oracle_name: test_data.oracle_name.clone(),
            description: test_data.description.clone(),
            public_key: test_data.public_key.clone(),
            stake_amount: test_data.stake_amount,
            oracle_version: test_data.oracle_version.clone(),
            supported_services: test_data.supported_services.clone(),
            contact_info: None,
        })
        .await?;

    registry.activate_oracle(&test_data.oracle_address).await?;

    // Get initial reputation
    let oracle_before = registry
        .get_oracle(&test_data.oracle_address)
        .await
        .unwrap();
    let initial_reputation = oracle_before.reputation.current_score;

    // Run daily maintenance
    let maintenance_result = registry.daily_maintenance().await;
    assert!(
        maintenance_result.is_ok(),
        "Daily maintenance should succeed"
    );

    // Check reputation decay
    let oracle_after = registry
        .get_oracle(&test_data.oracle_address)
        .await
        .unwrap();
    let final_reputation = oracle_after.reputation.current_score;

    assert!(
        final_reputation < initial_reputation,
        "Reputation should decay after maintenance"
    );
    assert!(
        (final_reputation - initial_reputation * 0.95).abs() < 0.01,
        "Decay should match configured factor"
    );

    println!("✓ Daily maintenance test passed");
    Ok(())
}

#[tokio::test]
async fn test_registry_capacity_limits() -> Result<()> {
    println!("Testing registry capacity limits...");

    let config = OracleRegistryConfig {
        max_oracle_count: 3, // Limit to 3 oracles for testing
        ..Default::default()
    };
    let registry = OracleRegistry::new(config)?;

    // Register maximum number of oracles
    for i in 1..=3 {
        let test_data = OracleTestData::new(10 + i);
        let result = registry
            .register_oracle(RegisterOracleArgs {
                oracle_address: test_data.oracle_address,
                oracle_name: test_data.oracle_name,
                description: test_data.description,
                public_key: test_data.public_key,
                stake_amount: test_data.stake_amount,
                oracle_version: test_data.oracle_version,
                supported_services: test_data.supported_services,
                contact_info: None,
            })
            .await;
        assert!(result.is_ok(), "Registration {} should succeed", i);
    }

    // Attempt to register one more should fail
    let test_data = OracleTestData::new(99);
    let overflow_result = registry
        .register_oracle(RegisterOracleArgs {
            oracle_address: test_data.oracle_address,
            oracle_name: test_data.oracle_name,
            description: test_data.description,
            public_key: test_data.public_key,
            stake_amount: test_data.stake_amount,
            oracle_version: test_data.oracle_version,
            supported_services: test_data.supported_services,
            contact_info: None,
        })
        .await;

    assert!(
        overflow_result.is_err(),
        "Registration beyond capacity should fail"
    );

    println!("✓ Registry capacity limits test passed");
    Ok(())
}
