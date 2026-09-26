use super::*;
use crate::runtime::{penalty_custody::PenaltyConfig, reward_runtime::RewardConfig};
use dytallix_protocol_types::recovery::{
    KeyIdentity, RecoveryConfig, RecoveryDomain, RecoveryState,
};

const MAX: u32 = 1_000_000;
// Independent layout oracle: no serde, bincode or LogicalRecord::new calls.
fn string(out: &mut Vec<u8>, s: &str) {
    out.extend((s.len() as u64).to_le_bytes());
    out.extend(s.as_bytes());
}
fn blob(out: &mut Vec<u8>, v: &[u8]) {
    out.extend((v.len() as u32).to_be_bytes());
    out.extend(v);
}
fn record(key: &str, fields: Vec<(u16, u8, Vec<u8>)>) -> Vec<u8> {
    let mut b = b"DYTALLIX/ORDINARY-LOGICAL\0".to_vec();
    b.extend(1u16.to_be_bytes());
    b.extend((key.len() as u16).to_be_bytes());
    b.extend(key.as_bytes());
    b.extend((fields.len() as u16).to_be_bytes());
    for (id, tag, v) in fields {
        b.extend(id.to_be_bytes());
        b.push(tag);
        b.extend(v);
    }
    b
}
fn payload_record(key: &str, payload: Vec<u8>) -> Vec<u8> {
    let mut b = Vec::new();
    blob(&mut b, &payload);
    record(key, vec![(1, 3, b)])
}
fn empty_maps(out: &mut Vec<u8>, n: usize) {
    for _ in 0..n {
        out.extend(0u64.to_le_bytes());
    }
}

#[test]
fn native_account_vector_always_includes_both_fixed_width_denominations() {
    let account = AccountState {
        balances: BTreeMap::from([("udrt".into(), u128::MAX), ("udgt".into(), 1)]),
        nonce: u64::MAX,
    };
    let got = native_account("a", &account, MAX).unwrap();
    let mut address = Vec::new();
    blob(&mut address, b"a");
    assert_eq!(
        got.canonical_bytes(),
        record(
            "acct:account:a",
            vec![
                (1, 4, address),
                (2, 2, 1u128.to_be_bytes().to_vec()),
                (3, 2, u128::MAX.to_be_bytes().to_vec()),
                (4, 1, u64::MAX.to_be_bytes().to_vec()),
            ]
        )
    );
    let mut reserved = account.clone();
    reserved.balances.remove("udrt");
    let reserved_record = native_account("a", &reserved, MAX).unwrap();
    assert_eq!(got.byte_len(), reserved_record.byte_len());
    reserved.balances.insert("udrt".into(), 0);
    assert_eq!(
        reserved_record,
        native_account("a", &reserved, MAX).unwrap()
    );
    reserved.balances.insert("udrt".into(), 7);
    assert_eq!(
        got.byte_len(),
        native_account("a", &reserved, MAX).unwrap().byte_len()
    );
    reserved.balances.clear();
    reserved.nonce = 0;
    assert_eq!(
        got.byte_len(),
        native_account("a", &reserved, MAX).unwrap().byte_len()
    );
    assert!(native_account("a", &account, got.byte_len() as u32 - 1).is_err());
    assert_eq!(
        got,
        native_account("a", &account, got.byte_len() as u32).unwrap()
    );
}
#[test]
fn native_account_rejects_unknown_denominations_even_with_zero_balance() {
    for amount in [0, 1, u128::MAX] {
        let account = AccountState {
            balances: BTreeMap::from([("other".into(), amount)]),
            nonce: 0,
        };
        assert!(matches!(
            native_account("a", &account, MAX),
            Err(MeterError::Internal(
                "unsupported logical native denomination"
            ))
        ));
    }
}

fn recovery_fixture() -> RecoveryAccount {
    let recovery = RecoveryState::new(
        RecoveryDomain {
            network: 3,
            chain_id: "c".into(),
            genesis_digest: [1; 32],
            account_id: [2; 32],
        },
        RecoveryConfig {
            timing_version: 1,
            recovery_delay: 2,
            finalization_window: 3,
            policy_delay: 4,
            policy_window: 5,
            submission_lifetime: 6,
            algorithms: BTreeMap::from([("x".into(), 1)]),
        },
        KeyIdentity {
            algorithm: "x".into(),
            public_key: vec![7],
        },
        2,
    )
    .unwrap();
    RecoveryAccount {
        address: "a".into(),
        recovery,
        sponsor_nonce: 19,
    }
}
#[test]
fn recovery_account_vector_includes_domain_counters_and_sponsor_nonce() {
    let account = recovery_fixture();
    let got = recovery_account(&[2; 32], &account, MAX).unwrap();
    let mut b = Vec::new();
    string(&mut b, "a");
    b.push(3);
    string(&mut b, "c");
    b.extend([1; 32]);
    b.extend([2; 32]);
    for n in 1..=6u64 {
        b.extend(n.to_le_bytes());
    }
    b.extend(1u64.to_le_bytes());
    string(&mut b, "x");
    b.extend(1u64.to_le_bytes());
    string(&mut b, "x");
    b.extend(1u64.to_le_bytes());
    b.push(7);
    b.extend(0u64.to_le_bytes());
    b.extend(0u64.to_le_bytes());
    b.push(0);
    for _ in 0..3 {
        b.extend(0u64.to_le_bytes());
    }
    b.extend(0u32.to_le_bytes());
    b.push(0);
    b.push(0);
    b.extend(2u64.to_le_bytes());
    b.extend(19u64.to_le_bytes());
    assert_eq!(
        got.canonical_bytes(),
        payload_record(&format!("recovery:account:{}", hex::encode([2; 32])), b)
    );
    let mut changed = account.clone();
    changed.sponsor_nonce = u64::MAX;
    changed.recovery.spending_nonce = u64::MAX;
    let other = recovery_account(&[2; 32], &changed, MAX).unwrap();
    assert_eq!(got.byte_len(), other.byte_len());
    assert_ne!(got, other);
    assert!(recovery_account(&[3; 32], &account, MAX).is_err());
    assert!(recovery_account(&[2; 32], &account, got.byte_len() as u32 - 1).is_err());
}

#[test]
fn grant_vectors_distinguish_absence_and_bind_generation_at_fixed_width() {
    let id = [2; 32];
    let key = format!("ordinary:grant:{}", hex::encode(id));
    assert_eq!(
        grant(&id, None, MAX).unwrap().canonical_bytes(),
        record(&key, vec![(1, 6, vec![0])])
    );
    let g = DiscretionaryGrant {
        version: 1,
        owner: id,
        beneficiary: [3; 32],
        owner_generation: 7,
        period_blocks: 9,
        last_active_height: 11,
    };
    let expected = record(
        &key,
        vec![
            (1, 6, vec![1]),
            (2, 5, id.to_vec()),
            (3, 5, vec![3; 32]),
            (4, 1, 7u64.to_be_bytes().to_vec()),
            (5, 1, 9u64.to_be_bytes().to_vec()),
            (6, 1, 11u64.to_be_bytes().to_vec()),
        ],
    );
    let got = grant(&id, Some(&g), MAX).unwrap();
    assert_eq!(got.canonical_bytes(), expected);
    let mut changed = g.clone();
    changed.owner_generation = u64::MAX;
    assert_eq!(
        got.byte_len(),
        grant(&id, Some(&changed), MAX).unwrap().byte_len()
    );
    assert!(grant(&[4; 32], Some(&g), MAX).is_err());
    changed.version = 2;
    assert!(grant(&id, Some(&changed), MAX).is_err());
    assert!(grant(&id, Some(&g), got.byte_len() as u32 - 1).is_err());
    assert!(grant(&id, None, 0).is_err());
}

// These are structural encoding fixtures, not valid production module profiles.
fn reward_fixture() -> RewardState {
    RewardState {
        config: RewardConfig {
            version: 2,
            activation_height: 1,
            decimals: 6,
            profile: "p".into(),
            chain_id: "c".into(),
            genesis_digest: "g".into(),
            max_validators: 3,
            max_positions: 4,
        },
        validators: BTreeMap::new(),
        positions: BTreeMap::new(),
        unbonding: BTreeMap::new(),
        locks: BTreeMap::new(),
        unpaid: BTreeMap::new(),
        rounding_reserve: 1,
        inactive_reserve: 2,
        total_budget: 3,
        total_claimed: 4,
        last_height: 7,
        last_interval_digest: None,
        last_interval_input_digest: None,
    }
}
#[test]
fn reward_module_vector_freezes_config_map_and_reserve_layout() {
    let state = reward_fixture();
    let got = reward(&state, MAX).unwrap();
    let mut b = 2u32.to_le_bytes().to_vec();
    b.extend(1u64.to_le_bytes());
    b.push(6);
    for s in ["p", "c", "g"] {
        string(&mut b, s);
    }
    b.extend(3u64.to_le_bytes());
    b.extend(4u64.to_le_bytes());
    empty_maps(&mut b, 5);
    for n in 1..=4u128 {
        b.extend(n.to_le_bytes());
    }
    b.extend(7u64.to_le_bytes());
    b.push(0);
    b.push(0);
    assert_eq!(got.canonical_bytes(), payload_record("rewards:v2:state", b));
    let mut changed = state.clone();
    changed.total_claimed = u128::MAX;
    changed.last_height = u64::MAX;
    assert_eq!(got.byte_len(), reward(&changed, MAX).unwrap().byte_len());
    assert!(reward(&state, got.byte_len() as u32 - 1).is_err());
}
fn lifecycle_fixture() -> LifecycleState {
    LifecycleState {
        config: LifecycleConfig {
            version: 1,
            profile: "p".into(),
            chain_id: "c".into(),
            approved_operators: BTreeMap::new(),
            min_self_bond: 1,
            max_active: 3,
            evidence_max_age_blocks: 4,
            evidence_max_age_seconds: 5,
            processing_margin_blocks: 6,
            processing_margin_seconds: 7,
        },
        effective: ValidatorView {
            validators: BTreeMap::new(),
            positions: BTreeMap::new(),
        },
        schedules: BTreeMap::new(),
        unbonding: BTreeMap::new(),
        history: BTreeMap::new(),
        update_history: BTreeMap::new(),
        last_height: 8,
        max_positions: 9,
        next_unbond_id: 10,
        reserved_owners: BTreeSet::new(),
    }
}
#[test]
fn lifecycle_vector_uses_fixed_self_bond_and_usize_width() {
    let state = lifecycle_fixture();
    let got = lifecycle(&state, MAX).unwrap();
    let mut b = 1u32.to_le_bytes().to_vec();
    string(&mut b, "p");
    string(&mut b, "c");
    empty_maps(&mut b, 1);
    b.extend(1u128.to_le_bytes());
    for n in 3..=7u64 {
        b.extend(n.to_le_bytes());
    }
    empty_maps(&mut b, 6);
    for n in 8..=10u64 {
        b.extend(n.to_le_bytes());
    }
    empty_maps(&mut b, 1);
    assert_eq!(
        got.canonical_bytes(),
        payload_record("lifecycle:v1:state", b)
    );
    let mut changed = state.clone();
    changed.config.min_self_bond = u128::MAX;
    changed.max_positions = usize::MAX;
    assert_eq!(got.byte_len(), lifecycle(&changed, MAX).unwrap().byte_len());
    assert!(lifecycle(&state, got.byte_len() as u32 - 1).is_err());
}
#[test]
fn lifecycle_evidence_snapshots_also_use_fixed_self_bond_width() {
    let mut state = lifecycle_fixture();
    let unbond = UnbondEntry {
        id: "u".into(),
        owner: "a".into(),
        validator: "v".into(),
        amount: 1,
        request_height: 1,
        effective_height: 3,
        last_exposure_height: Some(2),
        last_exposure_time_seconds: Some(10),
        evidence_config: state.config.clone(),
    };
    state.unbonding.insert("u".into(), unbond.clone());
    state.schedules.insert(
        3,
        ScheduledChange {
            request_height: 1,
            view: state.effective.clone(),
            additions: vec![],
            removals: vec![unbond],
        },
    );
    let before = lifecycle(&state, MAX).unwrap();
    state
        .unbonding
        .get_mut("u")
        .unwrap()
        .evidence_config
        .min_self_bond = u128::MAX;
    state.schedules.get_mut(&3).unwrap().removals[0]
        .evidence_config
        .min_self_bond = u128::MAX;
    let after = lifecycle(&state, MAX).unwrap();
    assert_ne!(before, after);
    assert_eq!(before.byte_len(), after.byte_len());
    // Exact fixed-width substitutions at both snapshots; no decimal string growth.
    let differences = before
        .canonical_bytes()
        .iter()
        .zip(after.canonical_bytes())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(differences, 32);
}
fn penalty_fixture() -> PenaltyState {
    PenaltyState {
        config: PenaltyConfig {
            version: 1,
            profile: "p".into(),
            chain_id: "c".into(),
            penalty_numerator: 2,
            penalty_denominator: 3,
            production_activation: false,
        },
        tranches: BTreeMap::new(),
        incidents: BTreeMap::new(),
        first_faults: BTreeMap::new(),
        releases: BTreeMap::new(),
        next_tranche_id: 5,
        last_height: 7,
        parent_time: (11, -3),
        evidence_processed_height: 7,
    }
}
#[test]
fn penalty_vector_freezes_signed_time_and_counter_widths() {
    let state = penalty_fixture();
    let got = penalty(&state, MAX).unwrap();
    let mut b = 1u32.to_le_bytes().to_vec();
    string(&mut b, "p");
    string(&mut b, "c");
    b.extend(2u64.to_le_bytes());
    b.extend(3u64.to_le_bytes());
    b.push(0);
    empty_maps(&mut b, 4);
    for n in [5u64, 7, 11] {
        b.extend(n.to_le_bytes());
    }
    b.extend((-3i32).to_le_bytes());
    b.extend(7u64.to_le_bytes());
    assert_eq!(got.canonical_bytes(), payload_record("penalty:v1:state", b));
    assert!(penalty(&state, got.byte_len() as u32 - 1).is_err());
}
#[test]
fn staking_pool_vector_keeps_sixteen_byte_amount_and_exact_bound() {
    for amount in [0, 1, u128::MAX] {
        let got = staking_pool(amount, MAX).unwrap();
        assert_eq!(
            got.canonical_bytes(),
            record(
                "emission:pool:staking_rewards",
                vec![(1, 2, amount.to_be_bytes().to_vec())]
            )
        );
        assert_eq!(got, staking_pool(amount, got.byte_len() as u32).unwrap());
        assert!(staking_pool(amount, got.byte_len() as u32 - 1).is_err());
    }
}

#[test]
fn governance_state_vector_uses_canonical_bounded_payload() {
    let state = GovernanceState::new(1, "c".into(), [3; 32], 7).unwrap();
    let got = governance_state(&state, MAX).unwrap();
    let mut payload = 1u16.to_le_bytes().to_vec();
    string(&mut payload, "c");
    payload.extend([3; 32]);
    payload.extend(7u64.to_le_bytes());
    payload.extend(1u64.to_le_bytes());
    payload.extend(0u64.to_le_bytes());
    assert_eq!(got.key(), b"governance:v1:state");
    assert_eq!(
        got.canonical_bytes(),
        payload_record("governance:v1:state", payload)
    );
    assert!(governance_state(&state, got.byte_len() as u32 - 1).is_err());
}
