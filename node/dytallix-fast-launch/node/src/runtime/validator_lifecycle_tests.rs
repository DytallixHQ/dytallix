use super::super::reward_runtime::RewardConfig;
use super::*;
use fips204::{
    ml_dsa_65,
    traits::{KeyGen, SerDes, Signer},
};

fn key() -> (String, ml_dsa_65::PrivateKey) {
    let (pk, sk) = ml_dsa_65::KG::try_keygen_with_rng(&mut rand_core::OsRng).unwrap();
    (B64.encode(pk.into_bytes()), sk)
}
fn fixture() -> (LifecycleState, RewardState) {
    let config = LifecycleConfig {
        version: 1,
        profile: PROFILE.into(),
        chain_id: "lifecycle-test".into(),
        approved_operators: BTreeMap::from([
            ("a".into(), "alice".into()),
            ("b".into(), "bob".into()),
            ("c".into(), "carol".into()),
        ]),
        min_self_bond: 10,
        max_active: 3,
        evidence_max_age_blocks: 10,
        evidence_max_age_seconds: 60,
        processing_margin_blocks: 2,
        processing_margin_seconds: 10,
    };
    let rewards = RewardState::new(
        RewardConfig {
            version: 2,
            activation_height: 1,
            decimals: 6,
            profile: "development".into(),
            chain_id: config.chain_id.clone(),
            genesis_digest: "ab".repeat(32),
            max_validators: 3,
            max_positions: 20,
        },
        BTreeMap::from([
            (
                "a".into(),
                ValidatorStatus {
                    active: true,
                    jailed: false,
                },
            ),
            (
                "b".into(),
                ValidatorStatus {
                    active: true,
                    jailed: false,
                },
            ),
        ]),
        BTreeMap::from([
            ("alice".into(), BTreeMap::from([("a".into(), 100)])),
            ("bob".into(), BTreeMap::from([("b".into(), 100)])),
            ("dave".into(), BTreeMap::from([("a".into(), 30)])),
        ]),
        BTreeMap::new(),
    )
    .unwrap();
    let state = LifecycleState::new(
        config,
        BTreeMap::from([
            (
                "a".into(),
                ValidatorIdentity {
                    owner: "alice".into(),
                    pubkey_base64: key().0,
                },
            ),
            (
                "b".into(),
                ValidatorIdentity {
                    owner: "bob".into(),
                    pubkey_base64: key().0,
                },
            ),
        ]),
        &rewards,
    )
    .unwrap();
    (state, rewards)
}
fn step(state: &mut LifecycleState, rewards: &mut RewardState, height: u64) {
    state.advance(height, height * 100, rewards).unwrap();
}
fn register(state: &LifecycleState, height: u64, nonce: u64, amount: u128) -> Operation {
    let (key, sk) = key();
    let expiry = height + 10;
    let bytes = proof_sign_bytes(
        &state.config.chain_id,
        "register",
        "c",
        "carol",
        &key,
        nonce,
        expiry,
        amount,
    )
    .unwrap();
    let proof = B64.encode(
        sk.try_sign_with_rng(&mut rand_core::OsRng, &bytes, b"")
            .unwrap(),
    );
    Operation::Register {
        validator: "c".into(),
        pubkey_base64: key,
        proof_base64: proof,
        expires_at_height: expiry,
        amount,
    }
}
#[test]
fn bond_activates_at_h_plus_two_with_exact_micro_unit_power() {
    let (mut state, mut rewards) = fixture();
    step(&mut state, &mut rewards, 1);
    state
        .schedule(
            1,
            "alice",
            0,
            Operation::Bond {
                validator: "a".into(),
                amount: 1,
            },
        )
        .unwrap();
    assert_eq!(state.pending_bond_total().unwrap(), 1);
    assert_eq!(state.validator_updates(1).unwrap()[0].power, 131);
    assert_eq!(rewards.positions["alice"]["a"], 100);
    step(&mut state, &mut rewards, 2);
    assert_eq!(rewards.positions["alice"]["a"], 100);
    step(&mut state, &mut rewards, 3);
    assert_eq!(rewards.positions["alice"]["a"], 101);
    assert_eq!(state.pending_bond_total().unwrap(), 0);
    state.validate_rewards(&rewards).unwrap();
    assert_eq!(
        LifecycleState::decode(&state.encode().unwrap()).unwrap(),
        state
    );
}
#[test]
fn consecutive_schedules_project_prior_changes() {
    let (mut state, mut rewards) = fixture();
    step(&mut state, &mut rewards, 1);
    state
        .schedule(
            1,
            "alice",
            0,
            Operation::Bond {
                validator: "a".into(),
                amount: 7,
            },
        )
        .unwrap();
    step(&mut state, &mut rewards, 2);
    state
        .schedule(
            2,
            "alice",
            0,
            Operation::Unbond {
                validator: "a".into(),
                amount: 9,
            },
        )
        .unwrap();
    assert_eq!(state.validator_updates(2).unwrap()[0].power, 128);
    step(&mut state, &mut rewards, 3);
    assert_eq!(rewards.positions["alice"]["a"], 107);
    step(&mut state, &mut rewards, 4);
    assert_eq!(rewards.positions["alice"]["a"], 98);
    assert_eq!(state.unbonding_by_owner().unwrap()["alice"], 9);
    let entry = state.unbonding.values().next().unwrap();
    assert_eq!(entry.last_exposure_height, Some(3));
    assert_eq!(entry.last_exposure_time_seconds, Some(400));
}
#[test]
fn below_minimum_self_bond_exits_all_owners() {
    let (mut state, mut rewards) = fixture();
    step(&mut state, &mut rewards, 1);
    state
        .schedule(
            1,
            "alice",
            0,
            Operation::Unbond {
                validator: "a".into(),
                amount: 95,
            },
        )
        .unwrap();
    assert_eq!(state.validator_updates(1).unwrap()[0].power, 0);
    step(&mut state, &mut rewards, 2);
    assert!(rewards.validators.contains_key("a"));
    step(&mut state, &mut rewards, 3);
    assert!(!rewards.validators.contains_key("a"));
    assert_eq!(
        state.unbonding_by_owner().unwrap(),
        BTreeMap::from([("alice".into(), 100), ("dave".into(), 30)])
    );
    assert_eq!(rewards.unbonding["alice"], 100);
    assert_eq!(rewards.unbonding["dave"], 30);
}
#[test]
fn final_validator_exit_rejects_without_state_change() {
    let (mut state, mut rewards) = fixture();
    step(&mut state, &mut rewards, 1);
    state
        .schedule(
            1,
            "alice",
            0,
            Operation::Exit {
                validator: "a".into(),
            },
        )
        .unwrap();
    let prior = state.clone();
    assert!(state
        .schedule(
            1,
            "bob",
            0,
            Operation::Exit {
                validator: "b".into()
            }
        )
        .is_err());
    assert_eq!(prior, state);
}
#[test]
fn registration_requires_key_proof_and_preserves_pending_custody() {
    let (mut state, mut rewards) = fixture();
    step(&mut state, &mut rewards, 1);
    let op = register(&state, 1, 7, 10);
    let prior = state.clone();
    assert!(state.schedule(1, "carol", 8, op.clone()).is_err());
    assert_eq!(state, prior);
    assert!(state.schedule(1, "alice", 7, op.clone()).is_err());
    assert_eq!(state, prior);
    state.schedule(1, "carol", 7, op).unwrap();
    assert_eq!(state.pending_bond_by_owner("carol").unwrap(), 10);
    assert_eq!(state.validator_set(3).unwrap().len(), 3);
    assert_eq!(state.validator_set(1).unwrap().len(), 2);
    step(&mut state, &mut rewards, 2);
    step(&mut state, &mut rewards, 3);
    assert_eq!(rewards.positions["carol"]["c"], 10);
}
#[test]
fn registration_capacity_and_minimum_fail_atomically() {
    let (mut state, mut rewards) = fixture();
    step(&mut state, &mut rewards, 1);
    let op = register(&state, 1, 7, 9);
    let prior = state.clone();
    assert!(state.schedule(1, "carol", 7, op).is_err());
    assert_eq!(state, prior);
    state.config.max_active = 2;
    let op = register(&state, 1, 8, 10);
    let prior = state.clone();
    assert!(state.schedule(1, "carol", 8, op).is_err());
    assert_eq!(state, prior);
}
#[test]
fn rotation_updates_are_atomic_and_retain_history() {
    let (mut state, mut rewards) = fixture();
    step(&mut state, &mut rewards, 1);
    let old = state.effective.validators["a"].pubkey_base64.clone();
    let (new, sk) = key();
    let proof = proof_sign_bytes(
        &state.config.chain_id,
        "rotate",
        "a",
        "alice",
        &new,
        5,
        10,
        0,
    )
    .unwrap();
    state
        .schedule(
            1,
            "alice",
            5,
            Operation::Rotate {
                validator: "a".into(),
                pubkey_base64: new.clone(),
                proof_base64: B64.encode(
                    sk.try_sign_with_rng(&mut rand_core::OsRng, &proof, b"")
                        .unwrap(),
                ),
                expires_at_height: 10,
            },
        )
        .unwrap();
    let updates = state.validator_updates(1).unwrap();
    assert_eq!(updates.len(), 2);
    assert!(updates
        .iter()
        .any(|u| u.pubkey_base64 == old && u.power == 0));
    assert!(updates
        .iter()
        .any(|u| u.pubkey_base64 == new && u.power == 130));
    step(&mut state, &mut rewards, 2);
    state
        .schedule(
            2,
            "alice",
            0,
            Operation::Unbond {
                validator: "a".into(),
                amount: 5,
            },
        )
        .unwrap();
    let updates = state.validator_updates(2).unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].pubkey_base64, new);
    assert_eq!(updates[0].power, 125);
    step(&mut state, &mut rewards, 3);
    step(&mut state, &mut rewards, 4);
    assert_eq!(state.history.at(1).unwrap().validators["a"].pubkey_base64, old);
    assert_eq!(state.effective.validators["a"].pubkey_base64, new);
}
#[test]
fn missing_schedule_update_or_reward_state_rejects() {
    let (mut state, mut rewards) = fixture();
    step(&mut state, &mut rewards, 1);
    state
        .schedule(
            1,
            "alice",
            0,
            Operation::Bond {
                validator: "a".into(),
                amount: 1,
            },
        )
        .unwrap();
    let mut bad = state.clone();
    bad.schedules.clear();
    assert!(bad.validate().is_err());
    let mut bad = state.clone();
    bad.update_history.clear();
    assert!(bad.validate().is_err());
    let mut bad = rewards.clone();
    *bad.positions
        .get_mut("alice")
        .unwrap()
        .get_mut("a")
        .unwrap() += 1;
    assert!(state.validate_rewards(&bad).is_err());
}
#[test]
fn dual_maturity_is_strict_and_cannot_authorize_withdrawal() {
    let (mut state, mut rewards) = fixture();
    step(&mut state, &mut rewards, 1);
    state
        .schedule(
            1,
            "dave",
            0,
            Operation::Unbond {
                validator: "a".into(),
                amount: 7,
            },
        )
        .unwrap();
    step(&mut state, &mut rewards, 2);
    step(&mut state, &mut rewards, 3);
    let entry = state.unbonding.values().next().unwrap();
    assert!(!entry
        .maturity_satisfied(&state.config, 14, 371, true)
        .unwrap());
    assert!(!entry
        .maturity_satisfied(&state.config, 15, 370, true)
        .unwrap());
    assert!(!entry
        .maturity_satisfied(&state.config, 15, 371, false)
        .unwrap());
    assert!(entry
        .maturity_satisfied(&state.config, 15, 371, true)
        .unwrap());
    assert!(state.authorize_withdrawal("dave", &entry.id).is_err());
    let mut missing = entry.clone();
    missing.last_exposure_height = None;
    assert!(!missing
        .maturity_satisfied(&state.config, 100, 1000, true)
        .unwrap());
    let mut changed = state.config.clone();
    changed.evidence_max_age_blocks += 1;
    assert!(entry.maturity_satisfied(&changed, 100, 1000, true).is_err());
    // Governed values (T6) leave an existing unbond valid and maturing.
    let mut governed = state.clone();
    governed.config.min_self_bond = 20;
    governed.config.max_active = 2;
    governed
        .config
        .approved_operators
        .insert("d".into(), "dave".into());
    governed.validate().unwrap();
    assert!(entry
        .maturity_satisfied(&governed.config, 15, 371, true)
        .unwrap());
}
#[test]
fn same_block_pending_principal_cannot_gain_false_exposure() {
    let (mut state, mut rewards) = fixture();
    step(&mut state, &mut rewards, 1);
    state
        .schedule(
            1,
            "carol",
            0,
            Operation::Bond {
                validator: "a".into(),
                amount: 5,
            },
        )
        .unwrap();
    let prior = state.clone();
    assert!(state
        .schedule(
            1,
            "carol",
            0,
            Operation::Unbond {
                validator: "a".into(),
                amount: 5
            }
        )
        .is_err());
    assert_eq!(state, prior);
    assert!(state
        .schedule(
            1,
            "alice",
            0,
            Operation::Exit {
                validator: "a".into()
            }
        )
        .is_err());
    assert_eq!(state, prior);
}

#[test]
fn pending_reward_owner_capacity_is_reserved_before_acceptance() {
    let (mut state, mut rewards) = fixture();
    state.max_positions = 4;
    rewards.config.max_positions = 4;
    step(&mut state, &mut rewards, 1);
    state
        .schedule(
            1,
            "erin",
            0,
            Operation::Bond {
                validator: "a".into(),
                amount: 1,
            },
        )
        .unwrap();
    let before = state.clone();
    assert!(state
        .schedule(
            1,
            "frank",
            0,
            Operation::Bond {
                validator: "b".into(),
                amount: 1
            }
        )
        .is_err());
    assert_eq!(state, before);
    step(&mut state, &mut rewards, 2);
    step(&mut state, &mut rewards, 3);
    state.validate_rewards(&rewards).unwrap();
}

#[test]
fn power_overflow_and_missing_unbond_records_fail_closed() {
    let (mut state, mut rewards) = fixture();
    step(&mut state, &mut rewards, 1);
    let before = state.clone();
    assert!(state
        .schedule(
            1,
            "alice",
            0,
            Operation::Bond {
                validator: "a".into(),
                amount: MAX_TOTAL_POWER
            }
        )
        .is_err());
    assert_eq!(state, before);
    state
        .schedule(
            1,
            "dave",
            0,
            Operation::Unbond {
                validator: "a".into(),
                amount: 1,
            },
        )
        .unwrap();
    step(&mut state, &mut rewards, 2);
    step(&mut state, &mut rewards, 3);
    // A removed entry is allowed (released unbonds are pruned), but not
    // without the matching custody change.
    let mut bad = state.clone();
    bad.unbonding.clear();
    bad.validate().unwrap();
    assert!(bad.validate_rewards(&rewards).is_err());
    let mut bad = state.clone();
    bad.next_unbond_id = 0;
    assert!(bad.validate().is_err());
}

/// Admit `operation` at `height` only when the largest of the pending state
/// and every projected activation fits the capacity. Returns that peak.
fn assert_capacity_is_the_projected_peak(
    before: &LifecycleState,
    rewards: &RewardState,
    height: u64,
    nonce: u64,
    operation: Operation,
) -> u64 {
    let mut pending = before.clone();
    pending
        .schedule(height, "alice", nonce, operation.clone())
        .unwrap();
    let mut projected = pending.clone();
    let mut future_rewards = rewards.clone();
    let mut peak = bincode::serialized_size(&pending).unwrap();
    for next in height + 1..=height + 2 {
        step(&mut projected, &mut future_rewards, next);
        peak = peak.max(bincode::serialized_size(&projected).unwrap());
    }
    let mut rejected = before.clone();
    let original = rejected.encode().unwrap();
    assert!(rejected
        .schedule_with_capacity(height, "alice", nonce, operation.clone(), peak - 1)
        .is_err());
    assert_eq!(
        rejected.encode().unwrap(),
        original,
        "Rejected schedule changed principal or registry state"
    );
    let mut accepted = before.clone();
    accepted
        .schedule_with_capacity(height, "alice", nonce, operation, peak)
        .unwrap();
    assert_eq!(accepted, pending);
    peak
}
#[test]
fn activation_footprint_is_reserved_before_unbond_acceptance() {
    let (mut before, mut rewards) = fixture();
    step(&mut before, &mut rewards, 1);
    let operation = Operation::Unbond {
        validator: "a".into(),
        amount: 1,
    };
    assert_capacity_is_the_projected_peak(&before, &rewards, 1, 0, operation);
}

#[test]
fn both_consecutive_activation_footprints_are_reserved() {
    let (mut before, mut rewards) = fixture();
    step(&mut before, &mut rewards, 1);
    before
        .schedule(
            1,
            "alice",
            0,
            Operation::Unbond {
                validator: "a".into(),
                amount: 1,
            },
        )
        .unwrap();
    step(&mut before, &mut rewards, 2);
    let operation = Operation::Unbond {
        validator: "a".into(),
        amount: 1,
    };
    assert_capacity_is_the_projected_peak(&before, &rewards, 2, 1, operation);
}

#[test]
fn released_unbond_is_removed_and_frees_an_empty_staker_slot() {
    let (mut state, mut rewards) = fixture();
    step(&mut state, &mut rewards, 1);
    state
        .schedule(
            1,
            "dave",
            0,
            Operation::Unbond {
                validator: "a".into(),
                amount: 30,
            },
        )
        .unwrap();
    step(&mut state, &mut rewards, 2);
    step(&mut state, &mut rewards, 3);
    let id = state.unbonding.keys().next().unwrap().clone();
    assert!(state.reserved_owners.contains("dave"));
    assert_eq!(rewards.unbonding.get("dave"), Some(&30));
    // Removing the released entry drops dave's gross custody and frees the
    // slot dave no longer needs; alice keeps hers.
    state.remove_released(&[id.clone()], &mut rewards).unwrap();
    assert!(state.unbonding.is_empty() && !rewards.unbonding.contains_key("dave"));
    assert!(!state.reserved_owners.contains("dave") && state.reserved_owners.contains("alice"));
    // The ID sequence keeps counting, and a removed entry cannot go twice.
    assert_eq!(state.next_unbond_id, 1);
    assert!(state
        .clone()
        .remove_released(&[id], &mut rewards.clone())
        .is_err());
}
#[test]
fn history_past_the_evidence_horizon_is_pruned_and_the_current_set_stays() {
    let (mut state, mut rewards) = fixture();
    step(&mut state, &mut rewards, 1);
    state
        .schedule(
            1,
            "dave",
            0,
            Operation::Bond {
                validator: "a".into(),
                amount: 5,
            },
        )
        .unwrap();
    for height in 2..=20 {
        step(&mut state, &mut rewards, height);
    }
    assert_eq!(state.history.base_height, 1);
    let old = state.historical_set(2).unwrap();
    // Evidence limits 10 blocks and 60 s plus margins 2 and 10: the genesis
    // set, last in effect at height 2 (time 200), is still needed at parent
    // height 14 and not at 15.
    let mut kept = state.clone();
    kept.prune_history(14, 1_400).unwrap();
    assert_eq!(kept.historical_set(2).unwrap(), old);
    state.prune_history(15, 1_500).unwrap();
    assert_eq!(state.history.base_height, 3);
    assert!(state.historical_set(2).is_err());
    assert_eq!(
        state.historical_set(3).unwrap(),
        HistoricalSet::of(&state.effective).unwrap()
    );
    state.validate().unwrap();
}

#[test]
fn validator_payouts_follow_voting_power_and_skip_jailed_validators() {
    let (mut state, mut rewards) = fixture();
    step(&mut state, &mut rewards, 1);
    // a: alice 100 + dave 30; b: bob 100. The operator owner is paid.
    assert_eq!(
        state.payout_weights(&rewards).unwrap(),
        BTreeMap::from([("alice".into(), 130), ("bob".into(), 100)])
    );
    rewards.stage_validator_payouts(1_001, &state.payout_weights(&rewards).unwrap()).unwrap();
    assert_eq!(rewards.validator_payouts.unpaid["alice"], 565);
    assert_eq!(rewards.validator_payouts.unpaid["bob"], 435);
    assert_eq!(rewards.validator_payouts.reserve, 1);
    rewards.validators.get_mut("b").unwrap().jailed = true;
    assert_eq!(
        state.payout_weights(&rewards).unwrap(),
        BTreeMap::from([("alice".into(), 130)])
    );
    // No eligible validator: the budget stays in the reserve.
    rewards.stage_validator_payouts(7, &BTreeMap::new()).unwrap();
    assert_eq!(rewards.validator_payouts.reserve, 8);
    assert_eq!(rewards.claim("alice").unwrap(), (0, 565));
    assert_eq!(rewards.validator_payouts.total_claimed, 565);
    assert!(!rewards.validator_payouts.unpaid.contains_key("alice"));
    rewards.validate_internal().unwrap();
    rewards.validators.get_mut("b").unwrap().jailed = false;
    state.validate_rewards(&rewards).unwrap();
    // A payout owed to an owner keeps that owner's reward slot.
    assert!(rewards.owners().any(|owner| owner == "bob"));
    let mut broken = rewards.clone();
    broken.validator_payouts.reserve += 1;
    assert!(broken.validate_internal().is_err());
}
