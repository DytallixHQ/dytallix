use super::*;
use crate::runtime::reward_runtime::{RewardConfig, ValidatorStatus, VestingLock};
use crate::runtime::validator_lifecycle::{LifecycleConfig, Operation, ValidatorIdentity};
use base64::{engine::general_purpose::STANDARD as B64, Engine};

fn fixture() -> (PenaltyState, LifecycleState, RewardState) {
    let identities: BTreeMap<String, ValidatorIdentity> = BTreeMap::from([
        (
            "a".into(),
            ValidatorIdentity {
                owner: "alice".into(),
                pubkey_base64: B64.encode(vec![1; 1952]),
            },
        ),
        (
            "b".into(),
            ValidatorIdentity {
                owner: "bob".into(),
                pubkey_base64: B64.encode(vec![2; 1952]),
            },
        ),
    ]);
    // Synthetic keys model custody only. These tests make no signature verification claim.
    let rewards = RewardState::new(
        RewardConfig {
            version: 2,
            activation_height: 1,
            decimals: 6,
            profile: "development".into(),
            chain_id: "penalty-unit".into(),
            genesis_digest: "ab".repeat(32),
            max_validators: 2,
            max_positions: 20,
        },
        identities
            .keys()
            .map(|id| {
                (
                    id.clone(),
                    ValidatorStatus {
                        active: true,
                        jailed: false,
                    },
                )
            })
            .collect(),
        BTreeMap::from([
            ("alice".into(), BTreeMap::from([("a".into(), 100)])),
            ("bob".into(), BTreeMap::from([("b".into(), 100)])),
        ]),
        BTreeMap::new(),
    )
    .unwrap();
    let lifecycle = LifecycleState::new(
        LifecycleConfig {
            version: 1,
            profile: super::super::validator_lifecycle::PROFILE.into(),
            chain_id: "penalty-unit".into(),
            approved_operators: BTreeMap::from([
                ("a".into(), "alice".into()),
                ("b".into(), "bob".into()),
            ]),
            min_self_bond: 10,
            max_active: 2,
            evidence_max_age_blocks: 3,
            evidence_max_age_seconds: 3,
            processing_margin_blocks: 1,
            processing_margin_seconds: 1,
        },
        identities,
        &rewards,
    )
    .unwrap();
    let state = PenaltyState::initialize(
        PenaltyConfig {
            version: 1,
            profile: PROFILE.into(),
            chain_id: "penalty-unit".into(),
            penalty_numerator: 1,
            penalty_denominator: 20,
            production_activation: false,
        },
        &lifecycle,
        &rewards,
    )
    .unwrap();
    (state, lifecycle, rewards)
}
fn step(
    state: &mut PenaltyState,
    lifecycle: &mut LifecycleState,
    rewards: &mut RewardState,
    height: u64,
) {
    let before = lifecycle.clone();
    lifecycle
        .advance(height, (height - 1) * 10, rewards)
        .unwrap();
    state.sync_lifecycle(&before, lifecycle).unwrap();
    state
        .begin_block(height, ((height - 1) * 10, 0), lifecycle)
        .unwrap();
}
fn operation(state: &mut PenaltyState, lifecycle: &mut LifecycleState, operation: Operation) {
    let before = lifecycle.clone();
    lifecycle
        .schedule(lifecycle.last_height, "alice", 0, operation)
        .unwrap();
    state.sync_lifecycle(&before, lifecycle).unwrap();
}
fn fact(lifecycle: &LifecycleState, height: u64) -> EvidenceFact {
    let view = historical_view(lifecycle, height).unwrap();
    EvidenceFact {
        kind: "duplicate_vote".into(),
        validator_address: consensus_address(&view.validators["a"].pubkey_base64).unwrap(),
        height,
        time_seconds: height * 10,
        time_nanos: 0,
        power: view.powers["a"] as i64,
        total_power: sum(view.powers.values().copied()).unwrap() as i64,
    }
}
fn assess(
    state: &mut PenaltyState,
    lifecycle: &mut LifecycleState,
    evidence: &EvidenceFact,
) -> Assessment {
    let result = state
        .assess(
            evidence,
            (evidence.time_seconds, evidence.time_nanos),
            state.last_height - 1,
            state.parent_time,
            lifecycle,
        )
        .unwrap();
    if result.requires_exit {
        operation(
            state,
            lifecycle,
            Operation::Exit {
                validator: result.validator.clone(),
            },
        );
    }
    result
}

#[test]
fn penalty_profile_rejects_production_and_vesting_inputs() {
    let (state, lifecycle, mut rewards) = fixture();
    let mut config = state.config.clone();
    config.production_activation = true;
    assert!(PenaltyState::initialize(config, &lifecycle, &rewards).is_err());
    rewards.locks.insert(
        "alice".into(),
        VestingLock {
            total_amount: 100,
            start_time: 0,
            cliff_duration: 0,
            vesting_duration: 100,
            permits_staking: true,
        },
    );
    assert!(PenaltyState::initialize(state.config.clone(), &lifecycle, &rewards).is_err());
    let mut config = state.config;
    config.penalty_denominator = 0;
    assert!(config.validate().is_err());
}

#[test]
fn wide_penalty_fraction_preserves_floor_without_intermediate_overflow() {
    assert_eq!(
        floor_ratio(u128::MAX, u64::MAX, u64::MAX).unwrap(),
        u128::MAX
    );
    assert_eq!(floor_ratio(39, 1, 20).unwrap(), 1);
    assert_eq!(floor_ratio(40, 1, 20).unwrap(), 2);
    assert!(floor_ratio(100, 1, 0).is_err());
}

#[test]
fn later_topup_is_excluded_and_deduction_waits_for_complete_exit() {
    let (mut state, mut lifecycle, mut rewards) = fixture();
    step(&mut state, &mut lifecycle, &mut rewards, 1);
    operation(
        &mut state,
        &mut lifecycle,
        Operation::Bond {
            validator: "a".into(),
            amount: 100,
        },
    );
    state.finalize_evidence_batch(&lifecycle).unwrap();
    step(&mut state, &mut lifecycle, &mut rewards, 2);
    let evidence = fact(&lifecycle, 1);
    assess(&mut state, &mut lifecycle, &evidence);
    state.finalize_evidence_batch(&lifecycle).unwrap();
    assert_eq!(state.deducted_total().unwrap(), 0);
    step(&mut state, &mut lifecycle, &mut rewards, 3);
    assert_eq!(lifecycle.effective.positions["alice"]["a"], 200);
    assert_eq!(state.deducted_total().unwrap(), 0);
    state.finalize_evidence_batch(&lifecycle).unwrap();
    step(&mut state, &mut lifecycle, &mut rewards, 4);
    assert!(!lifecycle.effective.validators.contains_key("a"));
    assert_eq!(rewards.unbonding["alice"], 200);
    assert_eq!(
        state.net_unbonding_by_owner(&lifecycle).unwrap()["alice"],
        195
    );
    assert_eq!(state.penalty_reserve().unwrap(), 5);
    assert!(state
        .tranches
        .values()
        .filter(|t| t.start_height == 3)
        .all(|t| t.deducted == 0));
    assert!(state.ensure_validator_allowed("a").is_err());
}

#[test]
fn partial_exit_keeps_exposure_and_uses_explicit_oldest_tranche_allocation() {
    let (mut state, mut lifecycle, mut rewards) = fixture();
    step(&mut state, &mut lifecycle, &mut rewards, 1);
    operation(
        &mut state,
        &mut lifecycle,
        Operation::Bond {
            validator: "a".into(),
            amount: 100,
        },
    );
    operation(
        &mut state,
        &mut lifecycle,
        Operation::Unbond {
            validator: "a".into(),
            amount: 40,
        },
    );
    let partial = lifecycle.schedules[&3].removals[0].id.clone();
    state.finalize_evidence_batch(&lifecycle).unwrap();
    step(&mut state, &mut lifecycle, &mut rewards, 2);
    let evidence = fact(&lifecycle, 1);
    assess(&mut state, &mut lifecycle, &evidence);
    state.finalize_evidence_batch(&lifecycle).unwrap();
    for height in 3..=4 {
        step(&mut state, &mut lifecycle, &mut rewards, height);
        state.finalize_evidence_batch(&lifecycle).unwrap();
    }
    // The old source ID stays on the unsplit remainder. The exited child has a new ID.
    assert_eq!(
        sum(state
            .tranches
            .values()
            .filter(|t| t.unbond_id.as_ref() == Some(&partial))
            .map(|t| t.deducted))
        .unwrap(),
        0
    );
    assert_eq!(state.deducted_total().unwrap(), 5);
    assert_eq!(state.net_unbonding_total(&lifecycle).unwrap(), 195);
    assert_eq!(lifecycle.unbonding[&partial].amount, 40);
}

#[test]
fn repeated_flattened_facts_are_idempotent_and_later_fault_is_capped() {
    let (mut state, mut lifecycle, mut rewards) = fixture();
    step(&mut state, &mut lifecycle, &mut rewards, 1);
    state.finalize_evidence_batch(&lifecycle).unwrap();
    step(&mut state, &mut lifecycle, &mut rewards, 2);
    let evidence = fact(&lifecycle, 1);
    let first = assess(&mut state, &mut lifecycle, &evidence);
    let before = state.clone();
    let repeated = assess(&mut state, &mut lifecycle, &evidence);
    assert_eq!(state, before);
    assert!(!repeated.first_fault);
    assert_eq!(first.incident_id, repeated.incident_id);
    state.finalize_evidence_batch(&lifecycle).unwrap();
    step(&mut state, &mut lifecycle, &mut rewards, 3);
    let second = fact(&lifecycle, 2);
    let next = assess(&mut state, &mut lifecycle, &second);
    assert!(!next.first_fault);
    assert_eq!(state.incidents.len(), 2);
    state.finalize_evidence_batch(&lifecycle).unwrap();
    step(&mut state, &mut lifecycle, &mut rewards, 4);
    assert_eq!(state.deducted_total().unwrap(), 5);
}

#[test]
fn evidence_failure_leaves_custody_unchanged_and_checks_nanos_and_power() {
    let (mut state, mut lifecycle, mut rewards) = fixture();
    step(&mut state, &mut lifecycle, &mut rewards, 1);
    state.finalize_evidence_batch(&lifecycle).unwrap();
    step(&mut state, &mut lifecycle, &mut rewards, 2);
    let evidence = fact(&lifecycle, 1);
    let before = state.clone();
    assert!(state
        .assess(&evidence, (10, 1), 1, (10, 0), &lifecycle)
        .is_err());
    assert_eq!(state, before);
    let mut wrong = evidence.clone();
    wrong.power += 1;
    assert!(state
        .assess(&wrong, (10, 0), 1, (10, 0), &lifecycle)
        .is_err());
    assert_eq!(state, before);
    let mut wrong = evidence;
    wrong.kind = "unknown".into();
    assert!(state
        .assess(&wrong, (10, 0), 1, (10, 0), &lifecycle)
        .is_err());
    assert_eq!(state, before);
}

#[test]
fn withdrawal_uses_strict_parent_age_owner_and_single_receipt() {
    let (mut state, mut lifecycle, mut rewards) = fixture();
    step(&mut state, &mut lifecycle, &mut rewards, 1);
    operation(
        &mut state,
        &mut lifecycle,
        Operation::Exit {
            validator: "a".into(),
        },
    );
    let entry = lifecycle.schedules[&3].removals[0].id.clone();
    state.finalize_evidence_batch(&lifecycle).unwrap();
    for height in 2..=7 {
        step(&mut state, &mut lifecycle, &mut rewards, height);
        state.finalize_evidence_batch(&lifecycle).unwrap();
    }
    // Last exposure is 2. Parent height 6 equals 2+3+1 and is not mature.
    let before = state.clone();
    assert!(state.withdraw("alice", &entry, 6, 60, &lifecycle).is_err());
    assert_eq!(state, before);
    step(&mut state, &mut lifecycle, &mut rewards, 8);
    assert!(state.withdraw("alice", &entry, 7, 70, &lifecycle).is_err());
    state.finalize_evidence_batch(&lifecycle).unwrap();
    assert!(state.withdraw("bob", &entry, 7, 70, &lifecycle).is_err());
    assert_eq!(
        state.withdraw("alice", &entry, 7, 70, &lifecycle).unwrap(),
        100
    );
    assert_eq!(lifecycle.unbonding[&entry].amount, 100);
    assert_eq!(state.net_unbonding_total(&lifecycle).unwrap(), 0);
    let after = state.clone();
    assert!(state.withdraw("alice", &entry, 7, 70, &lifecycle).is_err());
    assert_eq!(state, after);
    assert_eq!(
        PenaltyState::decode(&state.encode().unwrap()).unwrap(),
        state
    );
}

#[test]
fn admission_reserves_bytes_for_future_penalty_receipt_growth() {
    let (mut state, mut lifecycle, mut rewards) = fixture();
    step(&mut state, &mut lifecycle, &mut rewards, 1);
    state.finalize_evidence_batch(&lifecycle).unwrap();
    step(&mut state, &mut lifecycle, &mut rewards, 2);
    let evidence = fact(&lifecycle, 1);
    assess(&mut state, &mut lifecycle, &evidence);
    let current = bincode::serialized_size(&state).unwrap();
    assert!(state.validate_capacity(current + 7).is_err());
    assert!(state.validate_capacity(current + 8).is_ok());
}

#[test]
fn pruning_a_penalized_release_keeps_the_reserve_and_custody_balance() {
    let (mut state, mut lifecycle, mut rewards) = fixture();
    step(&mut state, &mut lifecycle, &mut rewards, 1);
    state.finalize_evidence_batch(&lifecycle).unwrap();
    step(&mut state, &mut lifecycle, &mut rewards, 2);
    let evidence = fact(&lifecycle, 1);
    assess(&mut state, &mut lifecycle, &evidence);
    state.finalize_evidence_batch(&lifecycle).unwrap();
    let mut height = 3;
    let mut withdrawn = None;
    while withdrawn.is_none() && height < 40 {
        step(&mut state, &mut lifecycle, &mut rewards, height);
        state.finalize_evidence_batch(&lifecycle).unwrap();
        if let Some(entry) = lifecycle.unbonding.values().find(|e| e.owner == "alice") {
            let id = entry.id.clone();
            let (parent, seconds) = (state.last_height - 1, state.parent_time.0);
            if state.withdraw("alice", &id, parent, seconds, &lifecycle).is_ok() {
                withdrawn = Some(id);
            }
        }
        height += 1;
    }
    let id = withdrawn.expect("alice's exit matures");
    let reserve = state.penalty_reserve().unwrap();
    let released = state.released_total().unwrap();
    assert!(reserve > 0 && released > 0);

    let ids = state.prune_released().unwrap();
    assert_eq!(ids, vec![id]);
    lifecycle.remove_released(&ids, &mut rewards).unwrap();
    state.validate(&lifecycle).unwrap();
    assert!(state.tranches.values().all(|t| t.unbond_id.is_none()));
    assert_eq!((state.pruned_deducted, state.pruned_released), (reserve, released));
    assert_eq!(
        (state.penalty_reserve().unwrap(), state.released_total().unwrap()),
        (reserve, released)
    );
    // Gross custody equals net plus the deductions and releases it still holds.
    assert_eq!(
        state.net_unbonding_total(&lifecycle).unwrap()
            + state.live_deducted().unwrap()
            + state.live_released().unwrap(),
        rewards.total_unbonding().unwrap()
    );
    // The settled incident still names the removed tranche and stays valid.
    assert!(state.incidents.values().any(|i| !i.allocations.is_empty()));
    assert_eq!(
        PenaltyState::decode(&state.encode().unwrap()).unwrap(),
        state
    );
}

/// A block start as `block_lifecycle` runs it: lifecycle advance, penalty
/// sync and block start, then removal of released unbonds and of records
/// past the evidence horizon.
fn block_start(
    state: &mut PenaltyState,
    lifecycle: &mut LifecycleState,
    rewards: &mut RewardState,
    height: u64,
) {
    step(state, lifecycle, rewards, height);
    let released = state.prune_released().unwrap();
    lifecycle.remove_released(&released, rewards).unwrap();
    let parent_time = (height - 1) * 10;
    state.prune_incidents(lifecycle, height - 1, parent_time).unwrap();
    lifecycle.prune_history(height - 1, parent_time).unwrap();
    state.consolidate(lifecycle.history.base_height).unwrap();
    state.validate(lifecycle).unwrap();
}

/// `rounds` blocks, each bonding two, unbonding one and withdrawing every
/// matured unbond, so every activation changes power. Retained records must
/// stay bounded by the evidence horizon, and custody must be conserved.
fn churn_stays_bounded(rounds: u64) {
    let (mut state, mut lifecycle, mut rewards) = fixture();
    block_start(&mut state, &mut lifecycle, &mut rewards, 1);
    let (mut withdrawn, mut released, mut peak) = (0u64, 0u128, 0usize);
    for height in 2..=rounds + 12 {
        block_start(&mut state, &mut lifecycle, &mut rewards, height);
        state.finalize_evidence_batch(&lifecycle).unwrap();
        let ids: Vec<String> = lifecycle.unbonding.keys().cloned().collect();
        for id in ids {
            let parent_time = state.parent_time.0;
            if let Ok(amount) = state.withdraw("alice", &id, height - 1, parent_time, &lifecycle) {
                withdrawn += 1;
                released += amount;
            }
        }
        if height <= rounds + 1 {
            // Bond two and unbond one: every activation changes power.
            operation(
                &mut state,
                &mut lifecycle,
                Operation::Bond {
                    validator: "a".into(),
                    amount: 2,
                },
            );
            operation(
                &mut state,
                &mut lifecycle,
                Operation::Unbond {
                    validator: "a".into(),
                    amount: 1,
                },
            );
        }
        peak = peak.max(
            lifecycle.history.changes.len()
                + lifecycle.unbonding.len()
                + state.tranches.len()
                + state.releases.len(),
        );
    }
    assert!(lifecycle.next_unbond_id >= rounds && state.next_tranche_id >= rounds);
    assert_eq!(withdrawn, rounds);
    assert!(peak < 64, "retained records grew to {peak}");
    // Principal is conserved: alice keeps her original stake plus one per
    // round, and every unbonded unit was released.
    assert_eq!(lifecycle.effective.positions["alice"]["a"], 100 + u128::from(rounds));
    assert_eq!(state.released_total().unwrap(), released);
    assert_eq!(released, u128::from(rounds));
    assert_eq!(
        state.net_unbonding_total(&lifecycle).unwrap()
            + state.live_deducted().unwrap()
            + state.live_released().unwrap(),
        rewards.total_unbonding().unwrap()
    );
    assert_eq!(
        PenaltyState::decode(&state.encode().unwrap()).unwrap(),
        state
    );
    assert_eq!(
        LifecycleState::decode(&lifecycle.encode().unwrap()).unwrap(),
        lifecycle
    );
}

#[test]
fn churn_of_hundreds_of_bonds_unbonds_and_withdrawals_stays_bounded() {
    churn_stays_bounded(300);
}
/// Past every former lifetime cap of 10,000. About 20 minutes in a debug
/// build: `cargo test --release -- --ignored ten_thousand`.
#[test]
#[ignore = "long: run in release with --ignored"]
fn ten_thousand_bonds_unbonds_withdrawals_and_set_changes_stay_bounded() {
    churn_stays_bounded(10_001);
}

#[test]
fn expired_later_faults_are_pruned_and_first_fault_stays_while_its_deduction_is_held() {
    let (mut state, mut lifecycle, mut rewards) = fixture();
    block_start(&mut state, &mut lifecycle, &mut rewards, 1);
    state.finalize_evidence_batch(&lifecycle).unwrap();
    block_start(&mut state, &mut lifecycle, &mut rewards, 2);
    let first = fact(&lifecycle, 1);
    assert!(assess(&mut state, &mut lifecycle, &first).first_fault);
    let later = EvidenceFact {
        height: 2,
        time_seconds: 20,
        ..fact(&lifecycle, 2)
    };
    state.finalize_evidence_batch(&lifecycle).unwrap();
    block_start(&mut state, &mut lifecycle, &mut rewards, 3);
    assert!(!assess(&mut state, &mut lifecycle, &later).first_fault);
    state.finalize_evidence_batch(&lifecycle).unwrap();
    assert_eq!(state.incidents.len(), 2);
    let mut height = 4;
    while state.incidents.len() > 1 && height < 40 {
        block_start(&mut state, &mut lifecycle, &mut rewards, height);
        state.finalize_evidence_batch(&lifecycle).unwrap();
        height += 1;
    }
    // The later fault expired and went; the first fault's deduction still
    // sits in alice's held tranche, so its incident stays.
    assert_eq!(state.incidents.len(), 1);
    assert!(state.incidents.values().all(|i| i.first_fault));
    // After alice withdraws, the tranche and then the incident go; the
    // first-fault marker stays, so the validator remains barred.
    let id = lifecycle.unbonding.keys().next().unwrap().clone();
    while state
        .withdraw("alice", &id, state.last_height - 1, state.parent_time.0, &lifecycle)
        .is_err()
    {
        block_start(&mut state, &mut lifecycle, &mut rewards, height);
        state.finalize_evidence_batch(&lifecycle).unwrap();
        height += 1;
        assert!(height < 80);
    }
    block_start(&mut state, &mut lifecycle, &mut rewards, height);
    assert!(state.incidents.is_empty());
    // The offence heights' history is gone too; only the marker survives.
    assert!(lifecycle.history.base_height > 1);
    assert!(state.ensure_validator_allowed("a").is_err());
    assert!(state.penalty_reserve().unwrap() > 0);
}
