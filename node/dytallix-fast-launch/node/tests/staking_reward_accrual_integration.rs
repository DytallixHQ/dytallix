#![cfg(feature = "legacy-economic-fixtures")]
// Archived legacy economics. Original assertions intentionally remain unchanged.
use dytallix_fast_node::runtime::emission::{
    EmissionBreakdown, EmissionConfig, EmissionEngine, EmissionSchedule,
};
use dytallix_fast_node::runtime::staking::StakingModule;
use dytallix_fast_node::state::State;
use dytallix_fast_node::storage::state::Storage;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

/// Test the complete staking reward accrual and claim workflow
#[test]
fn test_staking_reward_accrual_and_claim() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
    let state = Arc::new(Mutex::new(State::new(storage.clone())));

    // Setup emission with deterministic config
    let config = EmissionConfig {
        schedule: EmissionSchedule::Static {
            per_block: 1_000_000,
        }, // 1 DRT per block
        initial_supply: 0,
        emission_breakdown: EmissionBreakdown {
            block_rewards: 60,
            staking_rewards: 25,
            ai_module_incentives: 10,
            bridge_operations: 5,
        },
    };

    let mut emission = EmissionEngine::new_with_config(storage.clone(), state, config);
    let mut staking = StakingModule::new(storage);

    // Test 1: No delegators initially
    assert_eq!(staking.get_accrued_rewards("delegator1"), 0);
    assert_eq!(staking.get_accrued_rewards("delegator2"), 0);

    // Test 2: Add first delegator with 600k DGT (60% of total stake)
    staking
        .delegate("delegator1", "test-validator", 600_000_000_000)
        .unwrap(); // 600k DGT in uDGT
    let (total_stake, reward_index, _) = staking.get_stats();
    assert_eq!(total_stake, 600_000_000_000);
    assert_eq!(reward_index, 0); // No emissions yet

    // Test 3: Apply emission - should give 250k uDRT staking rewards (25% of 1M uDRT)
    emission.apply_until(1);
    let staking_rewards = emission.get_latest_staking_rewards();
    assert_eq!(staking_rewards, 250_000); // 25% of 1M

    staking.apply_external_emission(staking_rewards);
    let (_, new_reward_index, _) = staking.get_stats();
    assert!(new_reward_index > 0);

    // delegator1 should get all 250k uDRT rewards (100% of stake)
    let accrued1 = staking.get_accrued_rewards("delegator1");
    assert_eq!(accrued1, 250_000);

    // Test 4: Add second delegator with 400k DGT (bringing total to 1M DGT)
    staking
        .delegate("delegator2", "test-validator", 400_000_000_000)
        .unwrap(); // 400k DGT in uDGT
    let (total_stake, _, _) = staking.get_stats();
    assert_eq!(total_stake, 1_000_000_000_000); // Now 1M DGT total

    // delegator2 should have 0 accrued (just joined)
    let accrued2 = staking.get_accrued_rewards("delegator2");
    assert_eq!(accrued2, 0);

    // Test 5: Apply another emission block
    emission.apply_until(2);
    let staking_rewards2 = emission.get_latest_staking_rewards();
    assert_eq!(staking_rewards2, 250_000); // Another 250k uDRT

    staking.apply_external_emission(staking_rewards2);

    // Now delegator1 (60% stake) should have 250k + 150k = 400k uDRT
    // delegator2 (40% stake) should have 100k uDRT
    let accrued1_after = staking.get_accrued_rewards("delegator1");
    let accrued2_after = staking.get_accrued_rewards("delegator2");

    // delegator1: 250k (from first block) + 60% of 250k (150k) = 400k
    assert_eq!(accrued1_after, 400_000);
    // delegator2: 40% of 250k = 100k
    assert_eq!(accrued2_after, 100_000);

    // Test 6: Claim rewards for delegator1
    let claimed1 = staking.claim_rewards("delegator1");
    assert_eq!(claimed1, 400_000);

    // After claiming, delegator1 should have 0 accrued
    let accrued1_post_claim = staking.get_accrued_rewards("delegator1");
    assert_eq!(accrued1_post_claim, 0);

    // delegator2 should still have their accrued amount
    let accrued2_unchanged = staking.get_accrued_rewards("delegator2");
    assert_eq!(accrued2_unchanged, 100_000);

    // Test 7: Apply another emission and check new accrual
    emission.apply_until(3);
    let staking_rewards3 = emission.get_latest_staking_rewards();
    staking.apply_external_emission(staking_rewards3);

    // delegator1: 60% of 250k = 150k (new)
    // delegator2: 100k (old) + 40% of 250k (100k) = 200k
    let final_accrued1 = staking.get_accrued_rewards("delegator1");
    let final_accrued2 = staking.get_accrued_rewards("delegator2");

    assert_eq!(final_accrued1, 150_000);
    assert_eq!(final_accrued2, 200_000);

    // Test 8: Claim both and verify
    let final_claimed1 = staking.claim_rewards("delegator1");
    let final_claimed2 = staking.claim_rewards("delegator2");

    assert_eq!(final_claimed1, 150_000);
    assert_eq!(final_claimed2, 200_000);

    // Both should have 0 accrued after final claim
    assert_eq!(staking.get_accrued_rewards("delegator1"), 0);
    assert_eq!(staking.get_accrued_rewards("delegator2"), 0);
}

/// Test edge case: rewards accumulate properly when no stake exists initially
#[test]
fn test_zero_stake_pending_emission() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
    let state = Arc::new(Mutex::new(State::new(storage.clone())));

    let config = EmissionConfig {
        schedule: EmissionSchedule::Static {
            per_block: 1_000_000,
        }, // 1 DRT per block
        initial_supply: 0,
        emission_breakdown: EmissionBreakdown {
            block_rewards: 60,
            staking_rewards: 25,
            ai_module_incentives: 10,
            bridge_operations: 5,
        },
    };

    let mut emission = EmissionEngine::new_with_config(storage.clone(), state, config);
    let mut staking = StakingModule::new(storage);

    // Apply emissions while no stake exists - should accumulate in pending
    emission.apply_until(3);
    for _block in 1..=3 {
        let staking_rewards = emission.get_latest_staking_rewards();
        staking.apply_external_emission(staking_rewards);
    }

    let (total_stake, reward_index, pending) = staking.get_stats();
    assert_eq!(total_stake, 0);
    assert_eq!(reward_index, 0);
    assert_eq!(pending, 750_000); // 3 blocks * 250k = 750k pending

    // Delegation updates stake. An emission update distributes pending rewards.
    staking
        .delegate("delegator1", "test-validator", 500_000_000_000)
        .unwrap(); // 500k DGT

    staking.apply_external_emission(0);
    let (new_total_stake, new_reward_index, new_pending) = staking.get_stats();
    assert_eq!(new_total_stake, 500_000_000_000);
    assert!(new_reward_index > 0); // Should have applied pending rewards
    assert_eq!(new_pending, 0); // Pending should be cleared

    // Delegator should have accrued the full pending amount
    let accrued = staking.get_accrued_rewards("delegator1");
    assert_eq!(accrued, 750_000); // All the pending emission
}

/// Test that claiming with zero accrued returns 0
#[test]
fn test_claim_zero_rewards() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
    let mut staking = StakingModule::new(storage);

    // Add delegator but no emissions
    staking
        .delegate("delegator1", "test-validator", 100_000_000_000)
        .unwrap();

    // Claim should return 0
    let claimed = staking.claim_rewards("delegator1");
    assert_eq!(claimed, 0);

    // Still 0 after claim
    let accrued = staking.get_accrued_rewards("delegator1");
    assert_eq!(accrued, 0);
}

/// Test that double claiming returns 0 on second attempt
#[test]
fn test_double_claim_returns_zero() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
    let state = Arc::new(Mutex::new(State::new(storage.clone())));

    let config = EmissionConfig {
        schedule: EmissionSchedule::Static {
            per_block: 1_000_000,
        },
        initial_supply: 0,
        emission_breakdown: EmissionBreakdown {
            block_rewards: 60,
            staking_rewards: 25,
            ai_module_incentives: 10,
            bridge_operations: 5,
        },
    };

    let mut emission = EmissionEngine::new_with_config(storage.clone(), state, config);
    let mut staking = StakingModule::new(storage);

    // Setup delegator and apply emission
    staking
        .delegate("delegator1", "test-validator", 1_000_000_000_000)
        .unwrap();
    emission.apply_until(1);
    let staking_rewards = emission.get_latest_staking_rewards();
    staking.apply_external_emission(staking_rewards);

    // First claim should work
    let first_claim = staking.claim_rewards("delegator1");
    assert_eq!(first_claim, 250_000);

    // Immediate second claim should return 0
    let second_claim = staking.claim_rewards("delegator1");
    assert_eq!(second_claim, 0);

    // Accrued should be 0
    let accrued = staking.get_accrued_rewards("delegator1");
    assert_eq!(accrued, 0);
}
