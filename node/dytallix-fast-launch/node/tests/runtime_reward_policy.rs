use dytallix_fast_node::runtime::reward_runtime::{
    RewardConfig, RewardState, ValidatorStatus, VestingLock,
};
use std::collections::BTreeMap;

fn state() -> RewardState {
    RewardState::new(
        RewardConfig {
            version: 2,
            activation_height: 1,
            decimals: 6,
            profile: "development".into(),
            chain_id: "reward-test".into(),
            genesis_digest: "a".repeat(64),
            max_validators: 4,
            max_positions: 8,
        },
        BTreeMap::from([
            (
                "v1".into(),
                ValidatorStatus {
                    active: true,
                    jailed: false,
                },
            ),
            (
                "v2".into(),
                ValidatorStatus {
                    active: false,
                    jailed: false,
                },
            ),
        ]),
        BTreeMap::new(),
        BTreeMap::new(),
    )
    .unwrap()
}

#[test]
fn interval_exact_original_entitlements_and_claims_survive_decode() {
    let mut state = state();
    state.bond("alice", "v1", 600_000_000_000).unwrap();
    state.stage_interval(1, "genesis", 250_000).unwrap();
    assert_eq!(state.unpaid["alice"], 250_000);
    state.bond("bob", "v1", 400_000_000_000).unwrap();
    assert!(!state.stage_interval(1, "genesis", 250_000).unwrap());
    assert!(!state.unpaid.contains_key("bob"));
    state.stage_interval(2, "block1", 250_000).unwrap();
    let mut recovered = RewardState::decode(&state.encode().unwrap()).unwrap();
    assert_eq!(recovered.claim("alice").unwrap(), 400_000);
    assert_eq!(recovered.claim("bob").unwrap(), 100_000);
    assert_eq!(recovered.claim("alice").unwrap(), 0);
    assert_eq!(recovered.total_claimed, 500_000);
    assert_eq!(recovered.rounding_reserve, 0);
    recovered.validate_internal().unwrap();
}

#[test]
fn inactive_and_rounding_reserves_never_reach_later_entrants() {
    let mut state = state();
    state.stage_interval(1, "genesis", 17).unwrap();
    assert_eq!(state.inactive_reserve, 17);
    state.bond("alice", "v1", 2).unwrap();
    state.bond("bob", "v1", 1).unwrap();
    state.stage_interval(2, "b1", 2).unwrap();
    assert_eq!(state.claim("alice").unwrap(), 1);
    assert_eq!(state.claim("bob").unwrap(), 0);
    assert_eq!(state.rounding_reserve, 1);
    state.begin_unbond("alice", "v1", 2).unwrap();
    state.begin_unbond("bob", "v1", 1).unwrap();
    state.bond("carol", "v1", 1).unwrap();
    state.stage_interval(3, "b2", 5).unwrap();
    assert_eq!(state.claim("carol").unwrap(), 5);
    assert_eq!(state.inactive_reserve, 17);
    assert_eq!(state.rounding_reserve, 1);
    assert_eq!(state.total_unbonding().unwrap(), 3);
}

#[test]
fn eligibility_counts_owner_positions_once_and_excludes_unbonding_and_validator_status() {
    let mut state = state();
    state.bond("alice", "v1", 10).unwrap();
    state.bond("alice", "v2", 30).unwrap();
    state.bond("v1", "v1", 20).unwrap();
    assert_eq!(
        state.eligible_snapshot().unwrap(),
        BTreeMap::from([("alice".into(), 10), ("v1".into(), 20)])
    );
    state.begin_unbond("alice", "v1", 6).unwrap();
    assert_eq!(state.eligible_snapshot().unwrap()["alice"], 4);
    state.validators.get_mut("v1").unwrap().jailed = true;
    assert!(state.eligible_snapshot().unwrap().is_empty());
    state.validators.get_mut("v2").unwrap().active = true;
    assert_eq!(
        state.eligible_snapshot().unwrap(),
        BTreeMap::from([("alice".into(), 30)])
    );
}

#[test]
fn rejected_interval_and_overflow_leave_state_unchanged() {
    let mut state = state();
    state.bond("alice", "v1", u128::MAX).unwrap();
    state.stage_interval(1, "genesis", u128::MAX).unwrap();
    let before = state.clone();
    assert!(state.stage_interval(1, "other-parent", u128::MAX).is_err());
    assert!(state.stage_interval(1, "genesis", u128::MAX - 1).is_err());
    assert!(state.stage_interval(3, "b2", 0).is_err());
    assert!(state.stage_interval(2, "b1", 1).is_err());
    assert!(state.bond("alice", "v1", 1).is_err());
    assert!(state.begin_unbond("alice", "v2", 1).is_err());
    assert_eq!(state, before);
}

#[test]
fn resource_limits_bound_positions_and_unpaid_churn() {
    let mut state = state();
    state.config.max_positions = 1;
    state.bond("alice", "v1", 1).unwrap();
    state.stage_interval(1, "genesis", 1).unwrap();
    let before = state.clone();
    assert!(state.bond("bob", "v1", 1).is_err());
    assert_eq!(state, before);
    state.begin_unbond("alice", "v1", 1).unwrap();
    let before = state.clone();
    assert!(state.bond("bob", "v1", 1).is_err());
    assert_eq!(state, before);
    // Reject admission before the next interval rather than failing allocation.
    state.stage_interval(2, "b1", 1).unwrap();
    assert_eq!(state.inactive_reserve, 1);
    assert_eq!(state.claim("alice").unwrap(), 1);
}

#[test]
fn explicit_vesting_preserves_locked_principal_through_nonliquid_custody() {
    let mut state = state();
    state.locks.insert(
        "alice".into(),
        VestingLock {
            total_amount: 100,
            start_time: 10,
            cliff_duration: 10,
            vesting_duration: 30,
            permits_staking: true,
        },
    );
    assert_eq!(state.locked_amount("alice", 9).unwrap(), 100);
    assert_eq!(state.locked_amount("alice", 20).unwrap(), 100);
    assert_eq!(state.locked_amount("alice", 30).unwrap(), 50);
    assert_eq!(state.locked_amount("alice", 40).unwrap(), 0);
    assert_eq!(state.liquid_spendable("alice", 60, 30, 10, 20).unwrap(), 0);
    assert_eq!(state.liquid_spendable("alice", 60, 30, 10, 30).unwrap(), 50);
    assert_eq!(state.liquid_spendable("alice", 60, 0, 40, 30).unwrap(), 50);
    assert!(state.liquid_spendable("alice", 59, 30, 10, 20).is_err());
    let lock = VestingLock {
        total_amount: u128::MAX,
        start_time: 0,
        cliff_duration: 0,
        vesting_duration: 2,
        permits_staking: false,
    };
    assert_eq!(lock.locked_amount(1).unwrap(), u128::MAX - u128::MAX / 2);
}

#[test]
fn activation_and_decode_reject_invalid_or_noncanonical_state() {
    let mut state = state();
    state.config.profile = "mainnet".into();
    assert!(state.validate_internal().is_err());
    state.config.profile = "development".into();
    state.config.decimals = 18;
    assert!(state.encode().is_err());
    state.config.decimals = 6;
    let bytes = state.encode().unwrap();
    assert_eq!(RewardState::decode(&bytes).unwrap(), state);
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(RewardState::decode(&trailing).is_err());
    assert!(RewardState::decode(&bytes[..bytes.len() - 1]).is_err());
    state.rounding_reserve = 1;
    assert!(RewardState::decode(&bincode::serialize(&state).unwrap()).is_err());
}

#[test]
fn disallowed_locked_stake_cannot_become_backing_after_an_unlocked_bond() {
    let mut state = state();
    state.locks.insert(
        "alice".into(),
        VestingLock {
            total_amount: 100,
            start_time: 0,
            cliff_duration: 0,
            vesting_duration: 100,
            permits_staking: false,
        },
    );
    // At time 20, 80 units remain locked and 20 liquid units can bond.
    assert_eq!(state.liquid_spendable("alice", 100, 0, 0, 20).unwrap(), 20);
    state.bond("alice", "v1", 20).unwrap();
    let liquid_after_funded_bond = 80;
    assert_eq!(
        state
            .liquid_spendable(
                "alice",
                liquid_after_funded_bond,
                state.owner_bonded("alice").unwrap(),
                0,
                20
            )
            .unwrap(),
        0
    );
    // Bonding the unlocked amount must not make another 20 units transferable.
    assert!(state.liquid_spendable("alice", 60, 20, 0, 20).is_err());
    state.begin_unbond("alice", "v1", 20).unwrap();
    assert_eq!(state.liquid_spendable("alice", 80, 0, 20, 20).unwrap(), 0);
    assert!(state.liquid_spendable("alice", 60, 0, 20, 20).is_err());
    // Only later vesting releases more of the liquid principal.
    assert_eq!(state.liquid_spendable("alice", 80, 0, 20, 40).unwrap(), 20);
}
