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

#[test]
fn test_emission_staking_integration() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
    let state = Arc::new(Mutex::new(State::new(storage.clone())));

    // Setup emission with deterministic config
    let config = EmissionConfig {
        schedule: EmissionSchedule::Percentage {
            annual_inflation_rate: 500,
        }, // 5%
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

    // Simulate validator registration with 1M DGT stake
    let validator_stake = 1_000_000_000_000u128; // 1M DGT in uDGT
    staking.set_total_stake(validator_stake);

    // Record initial state
    let (initial_stake, initial_reward_index, initial_pending) = staking.get_stats();
    assert_eq!(initial_stake, validator_stake);
    assert_eq!(initial_reward_index, 0);
    assert_eq!(initial_pending, 0);

    // Simulate N blocks of emission and staking reward application
    let num_blocks = 50u64;
    let mut total_staking_rewards = 0u128;

    for block in 1..=num_blocks {
        // Apply emission
        emission.apply_until(block);

        // Get staking rewards for this block
        let staking_rewards = emission.get_latest_staking_rewards();
        total_staking_rewards += staking_rewards;

        // Apply to staking module
        if staking_rewards > 0 {
            staking.apply_external_emission(staking_rewards);
        }
    }

    // Verify final state
    let (final_stake, final_reward_index, final_pending) = staking.get_stats();
    assert_eq!(final_stake, validator_stake);
    assert_eq!(final_pending, 0); // Should be no pending since we have stake

    // Calculate expected reward index
    let expected_reward_index = (total_staking_rewards
        * dytallix_fast_node::runtime::staking::REWARD_SCALE)
        / validator_stake;
    assert_eq!(final_reward_index, expected_reward_index);

    // Verify reward accumulation is meaningful
    assert!(
        final_reward_index > 0,
        "Reward index should increase over {num_blocks} blocks",
    );
    assert!(
        total_staking_rewards > 0,
        "Total staking rewards should be positive"
    );

    // Test precision - calculate potential claimable rewards for a delegator
    let delegator_stake = 100_000_000_000u128; // 100K DGT
    let claimable_rewards =
        (delegator_stake * final_reward_index) / dytallix_fast_node::runtime::staking::REWARD_SCALE;

    // Verify precision is maintained
    let expected_delegator_share = (total_staking_rewards * delegator_stake) / validator_stake;
    let difference = claimable_rewards.abs_diff(expected_delegator_share);

    // Allow for small rounding errors due to integer arithmetic
    assert!(
        difference <= 1,
        "Delegator reward calculation error {difference} should be ≤ 1 unit",
    );
}

#[test]
fn test_zero_stake_then_delegation() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
    let state = Arc::new(Mutex::new(State::new(storage.clone())));

    let config = EmissionConfig {
        schedule: EmissionSchedule::Percentage {
            annual_inflation_rate: 500,
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

    // Simulate several blocks with no stake - rewards should accumulate
    let blocks_without_stake = 10u64;
    let mut accumulated_rewards = 0u128;

    for block in 1..=blocks_without_stake {
        emission.apply_until(block);
        let staking_rewards = emission.get_latest_staking_rewards();
        accumulated_rewards += staking_rewards;

        staking.apply_external_emission(staking_rewards);
    }

    // Verify rewards are pending
    let (stake, reward_index, pending) = staking.get_stats();
    assert_eq!(stake, 0);
    assert_eq!(reward_index, 0);
    assert_eq!(pending, accumulated_rewards);
    assert!(accumulated_rewards > 0, "Should have accumulated rewards");

    // Now add stake - should apply all pending rewards
    let validator_stake = 2_000_000_000_000u128; // 2M DGT
    staking.set_total_stake(validator_stake);

    let (new_stake, new_reward_index, new_pending) = staking.get_stats();
    assert_eq!(new_stake, validator_stake);
    assert_eq!(new_pending, 0);

    let expected_reward_index = (accumulated_rewards
        * dytallix_fast_node::runtime::staking::REWARD_SCALE)
        / validator_stake;
    assert_eq!(new_reward_index, expected_reward_index);

    // Continue with more blocks - should apply normally
    for block in (blocks_without_stake + 1)..=(blocks_without_stake + 5) {
        emission.apply_until(block);
        let staking_rewards = emission.get_latest_staking_rewards();
        staking.apply_external_emission(staking_rewards);
    }

    let (final_stake, final_reward_index, final_pending) = staking.get_stats();
    assert_eq!(final_stake, validator_stake);
    assert_eq!(final_pending, 0);
    assert!(
        final_reward_index > new_reward_index,
        "Reward index should continue to increase"
    );
}

#[test]
fn test_multiple_stake_changes() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
    let state = Arc::new(Mutex::new(State::new(storage.clone())));

    let config = EmissionConfig {
        schedule: EmissionSchedule::Percentage {
            annual_inflation_rate: 500,
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

    // Start with stake
    staking.set_total_stake(1_000_000_000_000); // 1M DGT

    // Apply some blocks
    for block in 1..=10 {
        emission.apply_until(block);
        let staking_rewards = emission.get_latest_staking_rewards();
        staking.apply_external_emission(staking_rewards);
    }

    let (_, reward_index_1, _) = staking.get_stats();
    assert!(reward_index_1 > 0);

    // Increase stake (simulate new delegation)
    staking.set_total_stake(2_000_000_000_000); // 2M DGT

    // Apply more blocks - reward index should increase slower due to higher stake
    for block in 11..=20 {
        emission.apply_until(block);
        let staking_rewards = emission.get_latest_staking_rewards();
        staking.apply_external_emission(staking_rewards);
    }

    let (final_stake, final_reward_index, final_pending) = staking.get_stats();
    assert_eq!(final_stake, 2_000_000_000_000);
    assert_eq!(final_pending, 0);
    assert!(
        final_reward_index > reward_index_1,
        "Reward index should continue increasing"
    );

    // Rate of increase should be slower with higher stake
    let increase_per_block_1 = reward_index_1 / 10;
    let increase_per_block_2 = (final_reward_index - reward_index_1) / 10;
    assert!(
        increase_per_block_2 < increase_per_block_1,
        "Reward rate should decrease with higher stake"
    );
}

#[test]
fn test_emission_event_consistency() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
    let state = Arc::new(Mutex::new(State::new(storage.clone())));

    let config = EmissionConfig {
        schedule: EmissionSchedule::Percentage {
            annual_inflation_rate: 500,
        },
        initial_supply: 0,
        emission_breakdown: EmissionBreakdown {
            block_rewards: 60,
            staking_rewards: 25,
            ai_module_incentives: 10,
            bridge_operations: 5,
        },
    };

    let mut emission = EmissionEngine::new_with_config(storage, state, config);

    // Apply several blocks
    for block in 1..=20 {
        emission.apply_until(block);
    }

    // Verify each event is consistent
    for height in 1..=20 {
        if let Some(event) = emission.get_event(height) {
            // Verify distribution percentages
            let total = event.total_emitted;

            let block_rewards = event.pools["block_rewards"];
            let staking_rewards = event.pools["staking_rewards"];
            let ai_incentives = event.pools["ai_module_incentives"];
            let bridge_ops = event.pools["bridge_operations"];

            // First, assert the pool allocations sum exactly to total emitted
            let sum_pools = block_rewards + staking_rewards + ai_incentives + bridge_ops;
            assert_eq!(
                sum_pools, total,
                "Pool allocations must sum to total at height {height}"
            );

            // When total emission is very small (< 100), integer rounding makes per-block
            // percentage checks meaningless (1% granularity). Only enforce percentage
            // bands when total >= 100 so 1% steps are representable.
            if total >= 100 {
                // Check rough percentages (allowing for integer division rounding)
                let block_rewards_pct = (block_rewards * 100) / total;
                let staking_rewards_pct = (staking_rewards * 100) / total;
                let ai_incentives_pct = (ai_incentives * 100) / total;
                let bridge_ops_pct = (bridge_ops * 100) / total;

                // Allow for ±1% deviation due to rounding and remainder allocation
                assert!(
                    (59..=61).contains(&block_rewards_pct),
                    "Block rewards should be ~60% at height {height}",
                );
                assert!(
                    (24..=26).contains(&staking_rewards_pct),
                    "Staking rewards should be ~25% at height {height}",
                );
                assert!(
                    (9..=11).contains(&ai_incentives_pct),
                    "AI incentives should be ~10% at height {height}",
                );
                assert!(
                    (4..=6).contains(&bridge_ops_pct),
                    "Bridge ops should be ~5% at height {height}",
                );
            }

            // Verify circulating supply increases monotonically
            if height > 1 {
                if let Some(prev_event) = emission.get_event(height - 1) {
                    assert!(
                        event.circulating_supply > prev_event.circulating_supply,
                        "Circulating supply should increase at height {height}",
                    );
                    assert_eq!(
                        event.circulating_supply,
                        prev_event.circulating_supply + event.total_emitted,
                        "Circulating supply increment should equal emission at height {height}",
                    );
                }
            }
        }
    }
}

#[test]
fn test_staking_rewards_precision() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
    let state = Arc::new(Mutex::new(State::new(storage.clone())));

    let config = EmissionConfig {
        schedule: EmissionSchedule::Percentage {
            annual_inflation_rate: 500,
        },
        initial_supply: 100_000_000_000_000, // 100M DRT initial supply
        emission_breakdown: EmissionBreakdown {
            block_rewards: 60,
            staking_rewards: 25,
            ai_module_incentives: 10,
            bridge_operations: 5,
        },
    };

    let mut emission = EmissionEngine::new_with_config(storage.clone(), state, config);
    let mut staking = StakingModule::new(storage);

    // Set realistic stake
    let total_stake = 50_000_000_000_000u128; // 50M DGT
    staking.set_total_stake(total_stake);

    // Apply many blocks to test precision and accumulate total staking rewards
    let mut total_staking_rewards: u128 = 0;
    for block in 1..=100 {
        emission.apply_until(block);
        let staking_rewards = emission.get_latest_staking_rewards();
        total_staking_rewards = total_staking_rewards.saturating_add(staking_rewards);
        staking.apply_external_emission(staking_rewards);
    }

    let (_, final_reward_index, _) = staking.get_stats();

    // Test with various delegator stakes
    let test_stakes = vec![
        1_000_000_000_000u128,  // 1M DGT
        10_000_000_000_000u128, // 10M DGT
        100_000_000_000u128,    // 100K DGT
        1_000_000_000u128,      // 1K DGT
    ];

    for delegator_stake in test_stakes {
        let claimable = (delegator_stake * final_reward_index)
            / dytallix_fast_node::runtime::staking::REWARD_SCALE;

        // Verify claimable amount is reasonable
        assert!(
            claimable > 0,
            "Delegator with {delegator_stake} stake should earn some rewards",
        );

        // Integer-precise expectation: delegator share of total staking rewards
        let expected_claimable = (total_staking_rewards * delegator_stake) / total_stake;
        let diff = claimable.abs_diff(expected_claimable);
        // Allow at most 1 unit rounding difference across 100 blocks
        assert!(
            diff <= 1,
            "Delegator reward rounding error {diff} should be ≤ 1 for stake {delegator_stake}",
        );
    }
}
