// Archived legacy assertions. Current economics use committed reward/issuance settlement.
#[cfg(feature = "legacy-economic-fixtures")]
mod archived_legacy_fixtures {
    use dytallix_fast_node::runtime::emission::{
        EmissionBreakdown, EmissionConfig, EmissionEngine, EmissionSchedule,
    };
    use dytallix_fast_node::state::State;
    use dytallix_fast_node::storage::state::Storage;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    const TOLERANCE: f64 = 1e-9;

    #[test]
    fn test_distribution_sum_matches_total() {
        let dir = tempdir().unwrap();
        let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
        let state = Arc::new(Mutex::new(State::new(storage.clone())));

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

        let mut engine = EmissionEngine::new_with_config(storage, state, config);

        // Apply emission for several blocks
        for _ in 0..10 {
            let height = engine.last_accounted_height() + 1;
            engine.apply_until(height);

            if let Some(event) = engine.get_event(height) {
                let pool_sum: u128 = event.pools.values().sum();
                assert_eq!(
                    pool_sum, event.total_emitted,
                    "Distribution sum must equal total emission at height {height}"
                );
            }
        }
    }

    #[test]
    fn test_remainder_allocation_stable() {
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

        let mut engine = EmissionEngine::new_with_config(storage, state, config);

        // Test with odd total emission amounts that might cause remainder
        engine.apply_until(1);

        if let Some(event) = engine.get_event(1) {
            let total = event.total_emitted;
            let expected_bridge =
                total - (total * 60 / 100) - (total * 25 / 100) - (total * 10 / 100);
            let actual_bridge = event.pools["bridge_operations"];

            assert_eq!(
                actual_bridge, expected_bridge,
                "Bridge operations should get remainder"
            );

            // Verify no emission is lost
            let pool_sum: u128 = event.pools.values().sum();
            assert_eq!(
                pool_sum, total,
                "No emission should be lost due to rounding"
            );
        }
    }

    #[test]
    fn test_zero_stake_carry_and_first_application() {
        let dir = tempdir().unwrap();
        let storage = Arc::new(Storage::open(dir.path().join("test.db")).unwrap());

        let mut staking = dytallix_fast_node::runtime::staking::StakingModule::new(storage);

        // Apply emission when no stake - should accumulate
        staking.apply_external_emission(1000);
        staking.apply_external_emission(500);

        let (total_stake, reward_index, pending) = staking.get_stats();
        assert_eq!(total_stake, 0);
        assert_eq!(reward_index, 0);
        assert_eq!(pending, 1500);

        // Set stake - should apply all pending
        staking.set_total_stake(1_000_000); // 1M uDGT

        let (total_stake, reward_index, pending) = staking.get_stats();
        assert_eq!(total_stake, 1_000_000);
        assert_eq!(pending, 0);

        let expected_reward_index =
            (1500 * dytallix_fast_node::runtime::staking::REWARD_SCALE) / 1_000_000;
        assert_eq!(reward_index, expected_reward_index);

        // Apply new emission with stake
        staking.apply_external_emission(2000);

        let (_, new_reward_index, _) = staking.get_stats();
        let additional_reward =
            (2000 * dytallix_fast_node::runtime::staking::REWARD_SCALE) / 1_000_000;
        assert_eq!(new_reward_index, expected_reward_index + additional_reward);
    }

    #[test]
    fn test_reward_index_precision() {
        let dir = tempdir().unwrap();
        let storage = Arc::new(Storage::open(dir.path().join("test.db")).unwrap());

        let mut staking = dytallix_fast_node::runtime::staking::StakingModule::new(storage);

        // Test with realistic values
        staking.set_total_stake(1_000_000_000_000); // 1M DGT in uDGT
        staking.apply_external_emission(1_000_000); // 1 DRT in uDRT

        let (_, reward_index, _) = staking.get_stats();

        // Calculate expected reward index
        let expected =
            (1_000_000 * dytallix_fast_node::runtime::staking::REWARD_SCALE) / 1_000_000_000_000;
        assert_eq!(reward_index, expected);

        // Verify precision is maintained (should be exactly 1e6)
        assert_eq!(expected, 1_000_000);

        // Test that precision error is within tolerance
        let floating_calculation = (1_000_000.0 * 1e12) / 1_000_000_000_000.0;
        let integer_result = reward_index as f64;
        let error = (floating_calculation - integer_result).abs();

        assert!(
            error < TOLERANCE,
            "Precision error {error} exceeds tolerance {TOLERANCE}"
        );
    }

    #[test]
    fn test_event_persistence_and_idempotency() {
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

        let mut engine =
            EmissionEngine::new_with_config(storage.clone(), state.clone(), config.clone());

        // Apply emission to height 3
        engine.apply_until(3);

        // Verify events exist
        assert!(engine.get_event(1).is_some());
        assert!(engine.get_event(2).is_some());
        assert!(engine.get_event(3).is_some());
        assert!(engine.get_event(4).is_none());

        // Get snapshot of state
        let snapshot1 = engine.snapshot();
        let event3_first = engine.get_event(3).unwrap();

        // Create new engine with same storage (simulates restart)
        let mut engine2 = EmissionEngine::new_with_config(storage, state, config);

        // Should not reapply - idempotent
        engine2.apply_until(3);

        let snapshot2 = engine2.snapshot();
        let event3_second = engine2.get_event(3).unwrap();

        // Verify idempotency
        assert_eq!(snapshot1.height, snapshot2.height);
        assert_eq!(snapshot1.pools, snapshot2.pools);
        assert_eq!(event3_first.total_emitted, event3_second.total_emitted);
        assert_eq!(event3_first.pools, event3_second.pools);
    }

    #[test]
    fn test_circulating_supply_tracking() {
        let dir = tempdir().unwrap();
        let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
        let state = Arc::new(Mutex::new(State::new(storage.clone())));

        let config = EmissionConfig {
            schedule: EmissionSchedule::Percentage {
                annual_inflation_rate: 500,
            },
            initial_supply: 1_000_000_000_000, // 1M DRT initial supply
            emission_breakdown: EmissionBreakdown {
                block_rewards: 60,
                staking_rewards: 25,
                ai_module_incentives: 10,
                bridge_operations: 5,
            },
        };

        let mut engine = EmissionEngine::new_with_config(storage, state, config);

        // Apply several blocks
        engine.apply_until(5);

        // Verify circulating supply increases
        let mut last_supply = 0;
        for height in 1..=5 {
            if let Some(event) = engine.get_event(height) {
                assert!(
                    event.circulating_supply > last_supply,
                    "Circulating supply should increase"
                );
                assert_eq!(event.circulating_supply, last_supply + event.total_emitted);
                last_supply = event.circulating_supply;
            }
        }
    }

    #[test]
    fn test_bootstrap_emission_calculation() {
        let dir = tempdir().unwrap();
        let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
        let state = Arc::new(Mutex::new(State::new(storage.clone())));

        let config = EmissionConfig {
            schedule: EmissionSchedule::Percentage {
                annual_inflation_rate: 500,
            },
            initial_supply: 0, // Bootstrap case
            emission_breakdown: EmissionBreakdown {
                block_rewards: 60,
                staking_rewards: 25,
                ai_module_incentives: 10,
                bridge_operations: 5,
            },
        };

        let mut engine = EmissionEngine::new_with_config(storage, state, config);

        // First block should use bootstrap emission
        engine.apply_until(1);

        if let Some(event) = engine.get_event(1) {
            assert_eq!(
                event.total_emitted, 1_000_000,
                "Bootstrap emission should be 1 DRT"
            );
            assert!(
                event.circulating_supply > 0,
                "Circulating supply should increase"
            );
        }

        // Second block should use calculated emission based on new supply
        engine.apply_until(2);

        if let Some(event) = engine.get_event(2) {
            // Should be calculated based on first block's supply
            assert!(
                event.total_emitted > 0,
                "Calculated emission should be positive"
            );
            // With small supply, calculated emission might be very small due to integer division
        }
    }
}
