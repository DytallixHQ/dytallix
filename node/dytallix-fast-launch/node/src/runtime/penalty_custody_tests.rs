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
        power: sum(view.positions.values().filter_map(|p| p.get("a")).copied()).unwrap() as i64,
        total_power: sum(view.positions.values().flat_map(|p| p.values()).copied()).unwrap() as i64,
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
