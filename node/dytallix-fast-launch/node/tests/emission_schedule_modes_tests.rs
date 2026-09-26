// Archived legacy assertions. Current economics use committed reward/issuance settlement.
#[cfg(feature = "legacy-economic-fixtures")]
mod archived_legacy_fixtures {
    use dytallix_fast_node::runtime::emission::{
        EmissionBreakdown, EmissionConfig, EmissionEngine, EmissionPhase, EmissionSchedule,
    };
    use dytallix_fast_node::state::State;
    use dytallix_fast_node::storage::state::Storage;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    #[test]
    fn test_static_emission_schedule() {
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

        let mut engine = EmissionEngine::new_with_config(storage, state, config);

        // Apply emission for several blocks
        for height in 1..=10 {
            engine.apply_until(height);

            if let Some(event) = engine.get_event(height) {
                assert_eq!(
                    event.total_emitted, 1_000_000,
                    "Static emission should be constant at height {height}"
                );

                // Verify distribution
                let pool_sum: u128 = event.pools.values().sum();
                assert_eq!(
                    pool_sum, event.total_emitted,
                    "Distribution sum must equal total emission at height {height}"
                );
            }
        }
    }

    #[test]
    fn test_phased_emission_schedule() {
        let dir = tempdir().unwrap();
        let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
        let state = Arc::new(Mutex::new(State::new(storage.clone())));

        let phases = vec![
            EmissionPhase {
                start_height: 1,
                end_height: Some(5),
                per_block_amount: 2_000_000, // 2 DRT per block for blocks 1-5
            },
            EmissionPhase {
                start_height: 6,
                end_height: Some(10),
                per_block_amount: 1_500_000, // 1.5 DRT per block for blocks 6-10
            },
            EmissionPhase {
                start_height: 11,
                end_height: None,            // Unlimited
                per_block_amount: 1_000_000, // 1 DRT per block for blocks 11+
            },
        ];

        let config = EmissionConfig {
            schedule: EmissionSchedule::Phased { phases },
            initial_supply: 0,
            emission_breakdown: EmissionBreakdown {
                block_rewards: 60,
                staking_rewards: 25,
                ai_module_incentives: 10,
                bridge_operations: 5,
            },
        };

        let mut engine = EmissionEngine::new_with_config(storage, state, config);

        // Test phase 1 (blocks 1-5)
        for height in 1..=5 {
            engine.apply_until(height);

            if let Some(event) = engine.get_event(height) {
                assert_eq!(
                    event.total_emitted, 2_000_000,
                    "Phase 1 emission should be 2 DRT at height {height}"
                );
            }
        }

        // Test phase 2 (blocks 6-10)
        for height in 6..=10 {
            engine.apply_until(height);

            if let Some(event) = engine.get_event(height) {
                assert_eq!(
                    event.total_emitted, 1_500_000,
                    "Phase 2 emission should be 1.5 DRT at height {height}"
                );
            }
        }

        // Test phase 3 (blocks 11+)
        for height in 11..=15 {
            engine.apply_until(height);

            if let Some(event) = engine.get_event(height) {
                assert_eq!(
                    event.total_emitted, 1_000_000,
                    "Phase 3 emission should be 1 DRT at height {height}"
                );
            }
        }
    }

    #[test]
    fn test_phased_emission_no_active_phase() {
        let dir = tempdir().unwrap();
        let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
        let state = Arc::new(Mutex::new(State::new(storage.clone())));

        let phases = vec![EmissionPhase {
            start_height: 10,
            end_height: Some(20),
            per_block_amount: 1_000_000,
        }];

        let config = EmissionConfig {
            schedule: EmissionSchedule::Phased { phases },
            initial_supply: 0,
            emission_breakdown: EmissionBreakdown {
                block_rewards: 60,
                staking_rewards: 25,
                ai_module_incentives: 10,
                bridge_operations: 5,
            },
        };

        let mut engine = EmissionEngine::new_with_config(storage, state, config);

        // Test before first phase (no emission)
        for height in 1..=5 {
            engine.apply_until(height);

            if let Some(event) = engine.get_event(height) {
                assert_eq!(
                    event.total_emitted, 0,
                    "No emission should occur before phase starts at height {height}"
                );
            }
        }

        // Test after phase ends (no emission)
        engine.apply_until(25);
        if let Some(event) = engine.get_event(25) {
            assert_eq!(
                event.total_emitted, 0,
                "No emission should occur after phase ends"
            );
        }
    }

    #[test]
    fn test_percentage_emission_schedule_backward_compatibility() {
        let dir = tempdir().unwrap();
        let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
        let state = Arc::new(Mutex::new(State::new(storage.clone())));

        // Test that the new percentage schedule works the same as the old system
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

        // Bootstrap should still work
        engine.apply_until(1);
        if let Some(event) = engine.get_event(1) {
            assert_eq!(
                event.total_emitted, 1_000_000,
                "Bootstrap emission should be 1 DRT"
            );
        }

        // Subsequent blocks should use percentage calculation
        engine.apply_until(2);
        if let Some(event) = engine.get_event(2) {
            assert!(
                event.total_emitted > 0,
                "Percentage-based emission should be positive"
            );
        }
    }

    #[test]
    fn test_emission_schedule_with_staking_integration() {
        let dir = tempdir().unwrap();
        let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
        let state = Arc::new(Mutex::new(State::new(storage.clone())));

        // Use static emission for predictable testing
        let config = EmissionConfig {
            schedule: EmissionSchedule::Static {
                per_block: 4_000_000,
            }, // 4 DRT per block
            initial_supply: 0,
            emission_breakdown: EmissionBreakdown {
                block_rewards: 60,
                staking_rewards: 25,
                ai_module_incentives: 10,
                bridge_operations: 5,
            },
        };

        let mut emission = EmissionEngine::new_with_config(storage.clone(), state, config);
        let mut staking = dytallix_fast_node::runtime::staking::StakingModule::new(storage);

        // Set up staking
        staking.set_total_stake(1_000_000_000_000); // 1M DGT

        // Apply emission for several blocks
        let mut total_staking_rewards = 0u128;

        for height in 1..=10 {
            emission.apply_until(height);
            let staking_rewards = emission.get_latest_staking_rewards();

            // With 25% going to staking, and 4 DRT total emission, we expect 1 DRT staking rewards
            assert_eq!(
                staking_rewards, 1_000_000,
                "Staking rewards should be 1 DRT (25% of 4 DRT) at height {height}"
            );

            total_staking_rewards += staking_rewards;
            staking.apply_external_emission(staking_rewards);
        }

        // Verify staking module received correct total
        let (_, reward_index, _) = staking.get_stats();
        let expected_reward_index = (total_staking_rewards
            * dytallix_fast_node::runtime::staking::REWARD_SCALE)
            / 1_000_000_000_000;
        assert_eq!(
            reward_index, expected_reward_index,
            "Reward index should match expected value"
        );
    }
}
