use super::super::governance_ballot::BondSnapshot;
use super::super::reward_runtime::RewardConfig;
use super::*;
use crate::recovery_fees::{RecoveryAccount, RecoveryBook};
use dytallix_protocol_types::{
    recovery::{KeyIdentity, RecoveryConfig, RecoveryDomain, RecoveryState},
    recovery_sponsor::FeeProfile,
};
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
fn governance_owner_snapshot_uses_effective_bonds_at_finalized_height() {
    let (mut state, mut rewards) = fixture();
    let owners = BTreeSet::from(["alice".to_string(), "dave".to_string()]);
    assert!(state
        .bonded_owner_weights_at_finalized_height(0, &owners)
        .is_err());
    step(&mut state, &mut rewards, 1);
    let first = state
        .bonded_owner_weights_at_finalized_height(1, &owners)
        .unwrap();
    let parent = BondSnapshot::from_finalized_lifecycle(&state, 1, [9; 32], &owners, 2).unwrap();
    assert_eq!(parent.finalized_height, 1);
    assert_eq!(parent.source_app_hash, [9; 32]);
    assert_eq!(parent.owner_weights, first);
    assert_eq!(parent.total_weight, 130);
    assert!(BondSnapshot::from_finalized_lifecycle(&state, 2, [9; 32], &owners, 2).is_err());
    assert!(BondSnapshot::from_finalized_lifecycle(&state, 1, [0; 32], &owners, 2).is_err());
    assert!(BondSnapshot::from_finalized_lifecycle(&state, 1, [9; 32], &owners, 1).is_err());
    assert_eq!(
        first,
        BTreeMap::from([("alice".into(), 100), ("dave".into(), 30)])
    );
    assert!(state
        .bonded_owner_weights_at_finalized_height(2, &owners)
        .is_err());
    assert!(state
        .bonded_owner_weights_at_finalized_height(1, &BTreeSet::from(["absent".into()]))
        .is_err());
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
    let pending = BondSnapshot::from_finalized_lifecycle(&state, 2, [8; 32], &owners, 2).unwrap();
    assert_eq!(pending.owner_weights, first);
    assert_eq!(
        state
            .bonded_owner_weights_at_finalized_height(2, &owners)
            .unwrap(),
        first
    );
    step(&mut state, &mut rewards, 3);
    assert_eq!(
        state
            .bonded_owner_weights_at_finalized_height(3, &owners)
            .unwrap(),
        BTreeMap::from([("alice".into(), 107), ("dave".into(), 30)])
    );
    assert_eq!(first["alice"], 100);
}

#[test]
fn governance_electorate_uses_registered_stable_account_ids() {
    let (mut state, mut rewards) = fixture();
    step(&mut state, &mut rewards, 1);
    let account_id = [1; 32];
    let owner = hex::encode(account_id);
    state
        .config
        .approved_operators
        .insert("a".into(), owner.clone());
    state.effective.validators.get_mut("a").unwrap().owner = owner.clone();
    let positions = state.effective.positions.remove("alice").unwrap();
    state.effective.positions.insert(owner.clone(), positions);
    state.reserved_owners.remove("alice");
    state.reserved_owners.insert(owner.clone());
    state.history.insert(1, state.effective.clone());
    state.validate().unwrap();

    let profile = FeeProfile {
        version: 1,
        activation_height: 1,
        denomination: "udrt".into(),
        gas_price: 1,
        minimum_gas: 10,
        max_transaction_gas: 100_000,
        max_block_gas: 200_000,
        max_block_recovery_bytes: 262_144,
        max_block_recovery_signatures: 16,
        max_fee_cap: 100_000,
        max_pending_accounts: 2,
        max_due_expiry_events_per_height: 2,
        mandatory_expiry_gas_budget: 20,
        expiry_event_gas_cost: 10,
        action_costs: [10; 9],
        wire_byte_cost: 1,
        read_byte_cost: 1,
        write_byte_cost: 1,
        signature_costs: BTreeMap::from([("mldsa65".into(), 10)]),
    };
    let account = RecoveryAccount {
        address: "registered-owner".into(),
        recovery: RecoveryState::new(
            RecoveryDomain {
                network: 3,
                chain_id: state.config.chain_id.clone(),
                genesis_digest: [3; 32],
                account_id,
            },
            RecoveryConfig {
                timing_version: 1,
                recovery_delay: 2,
                finalization_window: 3,
                policy_delay: 2,
                policy_window: 3,
                submission_lifetime: 100,
                algorithms: BTreeMap::from([("mldsa65".into(), 1952)]),
            },
            KeyIdentity {
                algorithm: "mldsa65".into(),
                public_key: vec![1; 1952],
            },
            0,
        )
        .unwrap(),
        sponsor_nonce: 0,
    };
    let mut book = RecoveryBook::new(profile, vec![account]).unwrap();
    for account in book.accounts.values_mut() {
        account.recovery = account.recovery.advance_height(1).unwrap();
    }
    book.last_height = 1;
    book.validate().unwrap();

    let snapshot =
        BondSnapshot::from_finalized_registered_accounts(&state, &book, 1, [9; 32], 1).unwrap();
    assert_eq!(snapshot.owner_weights, BTreeMap::from([(owner, 100)]));
    assert_eq!(snapshot.total_weight, 100);
    assert!(
        BondSnapshot::from_finalized_registered_accounts(&state, &book, 2, [9; 32], 1).is_err()
    );
    assert!(
        BondSnapshot::from_finalized_registered_accounts(&state, &book, 1, [9; 32], 0).is_err()
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
    assert_eq!(state.history[&1].validators["a"].pubkey_base64, old);
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
    let mut bad = state.clone();
    bad.unbonding.clear();
    assert!(bad.validate().is_err());
    let mut bad = state.clone();
    bad.next_unbond_id = 0;
    assert!(bad.validate().is_err());
}

#[test]
fn activation_footprint_is_reserved_before_unbond_acceptance() {
    let (mut before, mut rewards) = fixture();
    step(&mut before, &mut rewards, 1);
    let operation = Operation::Unbond {
        validator: "a".into(),
        amount: 1,
    };
    let mut pending = before.clone();
    pending.schedule(1, "alice", 0, operation.clone()).unwrap();
    let pending_bytes = bincode::serialized_size(&pending).unwrap();
    let mut activated = pending.clone();
    let mut future_rewards = rewards.clone();
    step(&mut activated, &mut future_rewards, 2);
    step(&mut activated, &mut future_rewards, 3);
    let activated_bytes = bincode::serialized_size(&activated).unwrap();
    assert!(
        activated_bytes > pending_bytes,
        "Final exposure fields and unbond map key must increase this fixture's size"
    );

    let original = before.encode().unwrap();
    let error = before
        .schedule_with_capacity(1, "alice", 0, operation.clone(), pending_bytes)
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("Scheduled activation exceeds lifecycle state bound"));
    assert_eq!(
        before.encode().unwrap(),
        original,
        "Rejected schedule changed principal or registry state"
    );
    before
        .schedule_with_capacity(1, "alice", 0, operation, activated_bytes)
        .unwrap();
    assert_eq!(before, pending);
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
    let mut pending = before.clone();
    pending.schedule(2, "alice", 1, operation.clone()).unwrap();
    let mut projected = pending.clone();
    let mut future_rewards = rewards.clone();
    let pending_bytes = bincode::serialized_size(&projected).unwrap();
    step(&mut projected, &mut future_rewards, 3);
    let first_bytes = bincode::serialized_size(&projected).unwrap();
    step(&mut projected, &mut future_rewards, 4);
    let second_bytes = bincode::serialized_size(&projected).unwrap();
    let first_only_capacity = pending_bytes.max(first_bytes);
    assert!(second_bytes > first_only_capacity);

    let original = before.encode().unwrap();
    assert!(before
        .schedule_with_capacity(2, "alice", 1, operation.clone(), first_only_capacity)
        .is_err());
    assert_eq!(before.encode().unwrap(), original);
    before
        .schedule_with_capacity(2, "alice", 1, operation, second_bytes)
        .unwrap();
    assert_eq!(before, pending);
}
