//! Pure recovery-component tests. Signers and proofs are trusted authenticated
//! facts supplied by a future verifier. These fixtures do not test signatures,
//! fees, database transactions, RPC enforcement, or actual asset custody.

use dytallix_protocol_types::recovery::*;
use std::collections::BTreeMap;

fn key(id: u8) -> KeyIdentity {
    KeyIdentity {
        algorithm: "fixture-key".to_owned(),
        public_key: vec![id; 16],
    }
}

fn domain() -> RecoveryDomain {
    RecoveryDomain {
        network: 3,
        chain_id: "recovery-component-fixture".to_owned(),
        genesis_digest: [11; 32],
        account_id: [12; 32],
    }
}

fn config() -> RecoveryConfig {
    RecoveryConfig {
        timing_version: 1,
        recovery_delay: 4,
        finalization_window: 3,
        policy_delay: 2,
        policy_window: 3,
        submission_lifetime: 20,
        algorithms: BTreeMap::from([("fixture-key".to_owned(), 16)]),
    }
}

fn policy(first: u8) -> RecoveryPolicy {
    RecoveryPolicy {
        threshold: 2,
        guardians: (first..first + 3)
            .map(|id| Guardian {
                key: key(id),
                control_group: format!("fixture-independent-group-{id}"),
            })
            .collect(),
    }
}

fn fresh() -> RecoveryState {
    RecoveryState::new(domain(), config(), key(1), 1).unwrap()
}

fn facts(action: &Action, signers: &[u8], proofs: &[u8]) -> AuthenticatedFacts {
    AuthenticatedFacts {
        domain: domain(),
        action: action.clone(),
        signers: signers.iter().copied().map(key).collect(),
        proofs: proofs.iter().copied().map(key).collect(),
    }
}

fn active(state: &RecoveryState) -> ActiveAuthorization {
    ActiveAuthorization {
        generation: state.active_generation,
        nonce: state.spending_nonce,
    }
}

fn recovery(state: &RecoveryState) -> RecoveryAuthorization {
    RecoveryAuthorization {
        policy_version: state.policy_version,
        sequence: state.recovery_sequence,
    }
}

fn policy_auth(state: &RecoveryState) -> PolicyAuthorization {
    PolicyAuthorization {
        policy_version: state.policy_version,
        sequence: state.policy_change_sequence,
    }
}

fn action(state: &RecoveryState, kind: ActionKind) -> Action {
    Action {
        submission_expiry: state.last_height + 10,
        kind,
    }
}

fn apply(state: &RecoveryState, action: &Action, signers: &[u8], proofs: &[u8]) -> RecoveryState {
    state
        .transition(state.last_height, action, &facts(action, signers, proofs))
        .unwrap()
}

fn enrolled() -> RecoveryState {
    let state = fresh();
    let enroll = action(
        &state,
        ActionKind::Enroll {
            active: active(&state),
            policy: policy(2),
        },
    );
    apply(&state, &enroll, &[1], &[2, 3, 4])
}

fn start(state: &RecoveryState) -> Action {
    action(
        state,
        ActionKind::Start {
            recovery: recovery(state),
            request_id: [42; 32],
            replacement: key(9),
            timing_version: 1,
        },
    )
}

fn pending() -> RecoveryState {
    let state = enrolled().advance_height(2).unwrap();
    apply(&state, &start(&state), &[2, 3], &[9])
}

fn finalize(state: &RecoveryState) -> Action {
    action(
        state,
        ActionKind::Finalize {
            recovery: recovery(state),
            request_id: [42; 32],
        },
    )
}

fn cancel(state: &RecoveryState) -> Action {
    action(
        state,
        ActionKind::Cancel {
            recovery: recovery(state),
            request_id: [42; 32],
        },
    )
}

fn stage(state: &RecoveryState) -> Action {
    action(
        state,
        ActionKind::StagePolicy {
            active: active(state),
            authorization: policy_auth(state),
            update_id: [43; 32],
            policy: policy(5),
            timing_version: 1,
        },
    )
}

fn staged() -> RecoveryState {
    let state = enrolled();
    apply(&state, &stage(&state), &[1, 2, 3], &[5, 6, 7])
}

fn activate(state: &RecoveryState) -> Action {
    action(
        state,
        ActionKind::ActivatePolicy {
            active: active(state),
            authorization: policy_auth(state),
            update_id: [43; 32],
        },
    )
}

fn assert_rejected(state: &RecoveryState, height: u64, action: &Action, auth: &AuthenticatedFacts) {
    let before = serde_json::to_vec(state).unwrap();
    assert!(state.transition(height, action, auth).is_err());
    assert_eq!(serde_json::to_vec(state).unwrap(), before);
    state.validate().unwrap();
}

#[test]
fn explicit_timing_and_key_configuration_are_required() {
    let valid = config();
    let mut invalid = Vec::new();
    let mut value = valid.clone();
    value.recovery_delay = 0;
    invalid.push(value);
    let mut value = valid.clone();
    value.finalization_window = 0;
    invalid.push(value);
    let mut value = valid.clone();
    value.policy_delay = 0;
    invalid.push(value);
    let mut value = valid.clone();
    value.policy_window = 0;
    invalid.push(value);
    let mut value = valid.clone();
    value.submission_lifetime = 0;
    invalid.push(value);
    let mut value = valid.clone();
    value.algorithms.clear();
    invalid.push(value);
    for invalid in invalid {
        assert!(RecoveryState::new(domain(), invalid, key(1), 1).is_err());
    }
    let mut wrong_length = key(1);
    wrong_length.public_key.pop();
    assert!(RecoveryState::new(domain(), valid.clone(), wrong_length, 1).is_err());
    let mut unknown_algorithm = key(1);
    unknown_algorithm.algorithm = "unregistered".to_owned();
    assert!(RecoveryState::new(domain(), valid, unknown_algorithm, 1).is_err());
}

#[test]
fn initial_state_reload_is_validated_and_height_never_rewinds() {
    let original = fresh();
    original.validate().unwrap();
    let bytes = serde_json::to_vec(&original).unwrap();
    let reloaded: RecoveryState = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_vec(&reloaded).unwrap(), bytes);
    let advanced = reloaded.advance_height(3).unwrap();
    assert!(advanced.advance_height(2).is_err());
    assert_eq!(
        serde_json::to_vec(&advanced.advance_height(3).unwrap()).unwrap(),
        serde_json::to_vec(&advanced).unwrap()
    );
}

#[test]
fn enrollment_requires_the_approved_threshold_distinct_keys_and_groups() {
    let state = fresh();
    let mut invalid_policies = Vec::new();
    for threshold in [0, 1, 3, 4] {
        let mut p = policy(2);
        p.threshold = threshold;
        invalid_policies.push(p);
    }
    let mut p = policy(2);
    p.guardians.pop();
    invalid_policies.push(p);
    let mut p = policy(2);
    p.guardians[1].key = p.guardians[0].key.clone();
    invalid_policies.push(p);
    let mut p = policy(2);
    p.guardians[1].control_group = p.guardians[0].control_group.clone();
    invalid_policies.push(p);
    let mut p = policy(2);
    p.guardians[0].key = key(1);
    invalid_policies.push(p);
    for p in invalid_policies {
        let proofs = p.guardians.iter().map(|g| g.key.clone()).collect();
        let a = action(
            &state,
            ActionKind::Enroll {
                active: active(&state),
                policy: p,
            },
        );
        let mut auth = facts(&a, &[1], &[]);
        auth.proofs = proofs;
        assert_rejected(&state, 1, &a, &auth);
    }
    let a = action(
        &state,
        ActionKind::Enroll {
            active: active(&state),
            policy: policy(2),
        },
    );
    assert_rejected(&state, 1, &a, &facts(&a, &[], &[2, 3, 4]));
    assert_rejected(&state, 1, &a, &facts(&a, &[1], &[2, 3]));
    let accepted = apply(&state, &a, &[1], &[2, 3, 4]);
    assert_eq!(accepted.policy, Some(policy(2)));
    assert_eq!(accepted.status, RecoveryStatus::Normal);
}

#[test]
fn every_distinct_guardian_pair_can_start_but_one_or_duplicates_cannot() {
    let state = enrolled();
    let a = start(&state);
    for signers in [
        vec![],
        vec![1],
        vec![2],
        vec![3],
        vec![4],
        vec![2, 2],
        vec![2, 8],
    ] {
        assert_rejected(&state, 1, &a, &facts(&a, &signers, &[9]));
    }
    for signers in [[2, 3], [2, 4], [3, 4]] {
        let started = apply(&state, &a, &signers, &[9]);
        assert_eq!(started.status, RecoveryStatus::PendingRecovery);
        assert_eq!(started.spending_nonce, state.spending_nonce);
        assert_eq!(started.recovery_sequence, state.recovery_sequence + 1);
    }
    assert_rejected(&state, 1, &a, &facts(&a, &[2, 3], &[]));
}

#[test]
fn facts_bind_domain_and_complete_action_and_submission_window() {
    let state = enrolled();
    let a = start(&state);
    for field in 0..4 {
        let mut auth = facts(&a, &[2, 3], &[9]);
        match field {
            0 => auth.domain.network = 2,
            1 => auth.domain.chain_id.push_str("-other"),
            2 => auth.domain.genesis_digest[0] ^= 1,
            _ => auth.domain.account_id[0] ^= 1,
        }
        assert_rejected(&state, 1, &a, &auth);
    }
    let mut altered = a.clone();
    altered.submission_expiry += 1;
    assert_rejected(&state, 1, &altered, &facts(&a, &[2, 3], &[9]));
    let mut expired = a.clone();
    expired.submission_expiry = 0;
    assert_rejected(&state, 1, &expired, &facts(&expired, &[2, 3], &[9]));
    let mut too_far = a.clone();
    too_far.submission_expiry = 22;
    assert_rejected(&state, 1, &too_far, &facts(&too_far, &[2, 3], &[9]));
    for kind in [
        ActionKind::Start {
            recovery: RecoveryAuthorization {
                policy_version: state.policy_version + 1,
                sequence: state.recovery_sequence,
            },
            request_id: [42; 32],
            replacement: key(9),
            timing_version: 1,
        },
        ActionKind::Start {
            recovery: RecoveryAuthorization {
                policy_version: state.policy_version,
                sequence: state.recovery_sequence + 1,
            },
            request_id: [42; 32],
            replacement: key(9),
            timing_version: 1,
        },
        ActionKind::Start {
            recovery: recovery(&state),
            request_id: [42; 32],
            replacement: key(9),
            timing_version: 2,
        },
    ] {
        let a = action(&state, kind);
        assert_rejected(&state, 1, &a, &facts(&a, &[2, 3], &[9]));
    }
}

#[test]
fn active_nonce_and_generation_churn_cannot_veto_collected_guardian_approval() {
    let state = enrolled();
    let collected = start(&state);
    let old_spend = action(
        &state,
        ActionKind::Spend {
            active: active(&state),
        },
    );
    let spent = apply(&state, &old_spend, &[1], &[]);
    let rotate = action(
        &spent,
        ActionKind::Rotate {
            active: active(&spent),
            replacement: key(8),
        },
    );
    let rotated = apply(&spent, &rotate, &[1], &[8]);
    assert!(rotated.spending_nonce > state.spending_nonce);
    assert!(rotated.active_generation > state.active_generation);
    assert_eq!(rotated.recovery_sequence, state.recovery_sequence);
    assert_eq!(rotated.policy_version, state.policy_version);
    let started = apply(&rotated, &collected, &[2, 3], &[9]);
    assert_eq!(started.active_key, key(8));
    assert_eq!(started.spending_nonce, rotated.spending_nonce);
    assert_rejected(&started, 1, &old_spend, &facts(&old_spend, &[1], &[]));
    let fresh_spend = action(
        &started,
        ActionKind::Spend {
            active: active(&started),
        },
    );
    assert_rejected(&started, 1, &fresh_spend, &facts(&fresh_spend, &[8], &[]));
    let fresh_rotation = action(
        &started,
        ActionKind::Rotate {
            active: active(&started),
            replacement: key(10),
        },
    );
    assert_rejected(
        &started,
        1,
        &fresh_rotation,
        &facts(&fresh_rotation, &[8], &[10]),
    );
}

#[test]
fn start_locks_once_and_finalization_uses_exact_replacement_authority() {
    let state = enrolled().advance_height(2).unwrap();
    let a = start(&state);
    let started = apply(&state, &a, &[2, 3], &[9]);
    let pending = started.pending_recovery.as_ref().unwrap();
    assert_eq!(
        (
            pending.start_height,
            pending.activation_height,
            pending.expiry_height
        ),
        (2, 6, 9)
    );
    assert_rejected(&started, 2, &a, &facts(&a, &[2, 3], &[9]));
    let early = started.advance_height(5).unwrap();
    let f = finalize(&early);
    assert_rejected(&early, 5, &f, &facts(&f, &[9], &[]));
    for height in [6, 8] {
        let mature = started.advance_height(height).unwrap();
        let f = finalize(&mature);
        for signer in [1, 2, 8] {
            assert_rejected(&mature, height, &f, &facts(&f, &[signer], &[]));
        }
        assert_rejected(&mature, height, &f, &facts(&f, &[], &[]));
        let finalized = apply(&mature, &f, &[9], &[]);
        assert_eq!(finalized.status, RecoveryStatus::Normal);
        assert_eq!(finalized.active_key, key(9));
        assert_eq!(finalized.spending_nonce, state.spending_nonce);
        assert_eq!(finalized.policy, state.policy);
        assert_eq!(finalized.domain, state.domain);
        assert!(finalized.pending_recovery.is_none());
        assert_rejected(&finalized, height, &f, &facts(&f, &[9], &[]));
    }
}

#[test]
fn expiry_commits_before_failed_actions_and_never_restores_active_control() {
    let state = pending();
    let expired = state.advance_height(9).unwrap();
    assert_eq!(expired.status, RecoveryStatus::RecoveryLocked);
    assert_eq!(expired.active_key, key(1));
    assert!(expired.pending_recovery.is_none());
    assert_eq!(expired.recovery_sequence, state.recovery_sequence + 1);
    assert_eq!(expired.active_generation, state.active_generation + 1);
    let f = finalize(&expired);
    assert_rejected(&expired, 9, &f, &facts(&f, &[9], &[]));
    let spend = action(
        &expired,
        ActionKind::Spend {
            active: active(&expired),
        },
    );
    assert_rejected(&expired, 9, &spend, &facts(&spend, &[1], &[]));
    assert_eq!(expired.advance_height(9).unwrap(), expired);
    let retry = start(&expired);
    let restarted = apply(&expired, &retry, &[2, 4], &[9]);
    assert_eq!(restarted.pending_recovery.unwrap().activation_height, 13);
    let json = serde_json::to_vec(&expired).unwrap();
    assert_eq!(
        serde_json::from_slice::<RecoveryState>(&json).unwrap(),
        expired
    );
}

#[test]
fn cancellation_locks_and_resume_requires_quorum_bound_to_current_key() {
    let state = pending();
    let c = cancel(&state);
    for signers in [vec![1], vec![2], vec![8]] {
        assert_rejected(&state, 2, &c, &facts(&c, &signers, &[]));
    }
    let locked = apply(&state, &c, &[2, 3], &[]);
    assert_eq!(locked.status, RecoveryStatus::RecoveryLocked);
    assert_eq!(locked.policy, state.policy);
    let resume = action(
        &locked,
        ActionKind::Resume {
            recovery: recovery(&locked),
            active_key: key(1),
        },
    );
    assert_rejected(&locked, 2, &resume, &facts(&resume, &[1], &[]));
    let wrong = action(
        &locked,
        ActionKind::Resume {
            recovery: recovery(&locked),
            active_key: key(9),
        },
    );
    assert_rejected(&locked, 2, &wrong, &facts(&wrong, &[2, 3], &[]));
    let resumed = apply(&locked, &resume, &[3, 4], &[]);
    assert_eq!(resumed.status, RecoveryStatus::Normal);
    assert_eq!(resumed.active_key, key(1));
    assert_eq!(resumed.spending_nonce, state.spending_nonce);
    assert_rejected(&resumed, 2, &resume, &facts(&resume, &[3, 4], &[]));
}

#[test]
fn cancellation_and_finalization_obey_first_valid_action_order() {
    let state = pending().advance_height(6).unwrap();
    let c = cancel(&state);
    let f = finalize(&state);
    let cancelled = apply(&state, &c, &[2, 3], &[]);
    assert_rejected(&cancelled, 6, &f, &facts(&f, &[9], &[]));
    let finalized = apply(&state, &f, &[9], &[]);
    assert_rejected(&finalized, 6, &c, &facts(&c, &[2, 3], &[]));
}

#[test]
fn policy_updates_need_both_authorities_and_cannot_replace_a_staged_update() {
    let state = enrolled();
    let a = stage(&state);
    for signers in [vec![1], vec![2, 3], vec![1, 2]] {
        assert_rejected(&state, 1, &a, &facts(&a, &signers, &[5, 6, 7]));
    }
    assert_rejected(&state, 1, &a, &facts(&a, &[1, 2, 3], &[5, 6]));
    let staged = apply(&state, &a, &[1, 2, 3], &[5, 6, 7]);
    assert_eq!(staged.policy, state.policy);
    assert_eq!(staged.policy_version, state.policy_version);
    assert_eq!(staged.recovery_sequence, state.recovery_sequence);
    assert_eq!(
        staged.policy_change_sequence,
        state.policy_change_sequence + 1
    );
    assert_rejected(
        &staged,
        1,
        &stage(&staged),
        &facts(&stage(&staged), &[1, 2, 3], &[5, 6, 7]),
    );
    let early = staged.advance_height(2).unwrap();
    let activation = activate(&early);
    assert_rejected(&early, 2, &activation, &facts(&activation, &[1, 2, 3], &[]));
    let mature = staged.advance_height(3).unwrap();
    let activation = activate(&mature);
    assert_rejected(&mature, 3, &activation, &facts(&activation, &[1], &[]));
    let changed = apply(&mature, &activation, &[1, 2, 3], &[]);
    assert_eq!(changed.policy, Some(policy(5)));
    assert_eq!(changed.policy_version, state.policy_version + 1);
    assert_eq!(
        changed.policy_change_sequence,
        state.policy_change_sequence + 2
    );
}

#[test]
fn policy_activation_and_old_policy_recovery_follow_canonical_order() {
    let state = staged().advance_height(3).unwrap();
    let recovery = start(&state);
    let activation = activate(&state);
    let changed = apply(&state, &activation, &[1, 2, 3], &[]);
    assert_rejected(&changed, 3, &recovery, &facts(&recovery, &[2, 3], &[9]));
    let recovering = apply(&state, &recovery, &[2, 3], &[9]);
    assert!(recovering.pending_policy.is_none());
    assert_eq!(
        recovering.policy_change_sequence,
        state.policy_change_sequence + 1
    );
    assert_rejected(
        &recovering,
        3,
        &activation,
        &facts(&activation, &[1, 2, 3], &[]),
    );
}

#[test]
fn staged_policy_cancel_and_expiry_preserve_old_recovery_policy() {
    let state = staged();
    let c = action(
        &state,
        ActionKind::CancelPolicy {
            authorization: policy_auth(&state),
            update_id: [43; 32],
        },
    );
    assert_rejected(&state, 1, &c, &facts(&c, &[1], &[]));
    let cancelled = apply(&state, &c, &[2, 3], &[]);
    let expired = state.advance_height(6).unwrap();
    for result in [cancelled, expired] {
        assert!(result.pending_policy.is_none());
        assert_eq!(result.policy, state.policy);
        assert_eq!(result.policy_version, state.policy_version);
        assert_eq!(result.recovery_sequence, state.recovery_sequence);
        assert_eq!(
            result.policy_change_sequence,
            state.policy_change_sequence + 1
        );
        assert_eq!(result.status, RecoveryStatus::Normal);
    }
}

#[test]
fn pending_state_reload_preserves_boundaries_and_rejects_corruption() {
    let state = pending();
    let bytes = serde_json::to_vec(&state).unwrap();
    let restored: RecoveryState = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(restored, state);
    assert_eq!(
        restored.advance_height(9).unwrap(),
        state.advance_height(9).unwrap()
    );
    for field in ["pending_recovery", "status", "recovery_sequence"] {
        let mut value = serde_json::to_value(&state).unwrap();
        value[field] = match field {
            "pending_recovery" => serde_json::Value::Null,
            "status" => serde_json::json!("Normal"),
            _ => serde_json::json!(0),
        };
        assert!(
            serde_json::from_value::<RecoveryState>(value).is_err(),
            "{field}"
        );
    }
    let mut value = serde_json::to_value(&state).unwrap();
    value["unrecognized_authority"] = serde_json::json!(true);
    assert!(serde_json::from_value::<RecoveryState>(value).is_err());
}

#[test]
fn start_reserves_terminal_counters_and_expiry_cannot_overflow() {
    for missing in [0, 1] {
        for generation_counter in [true, false] {
            let mut state = enrolled();
            if generation_counter {
                state.active_generation = u64::MAX - missing;
            } else {
                state.recovery_sequence = u64::MAX - missing;
            }
            state.validate().unwrap();
            let a = start(&state);
            assert_rejected(&state, 1, &a, &facts(&a, &[2, 3], &[9]));
        }
    }
    let mut state = enrolled();
    state.active_generation = u64::MAX - 2;
    state.recovery_sequence = u64::MAX - 2;
    let started = apply(&state, &start(&state), &[2, 3], &[9]);
    assert_eq!(started.active_generation, u64::MAX - 1);
    let expired = started.advance_height(8).unwrap();
    assert_eq!(expired.active_generation, u64::MAX);
    assert_eq!(expired.recovery_sequence, u64::MAX);
    assert_eq!(expired.status, RecoveryStatus::RecoveryLocked);
    assert_eq!(
        expired.advance_height(9).unwrap().active_generation,
        u64::MAX
    );
    let retry = start(&expired);
    assert_rejected(&expired, 8, &retry, &facts(&retry, &[2, 3], &[9]));
}

#[test]
fn active_rotation_cannot_spend_the_last_recovery_generation_capacity() {
    let mut state = enrolled();
    state.active_generation = u64::MAX - 3;
    let rotate = action(
        &state,
        ActionKind::Rotate {
            active: active(&state),
            replacement: key(8),
        },
    );
    let rotated = apply(&state, &rotate, &[1], &[8]);
    assert_eq!(rotated.active_generation, u64::MAX - 2);
    let again = action(
        &rotated,
        ActionKind::Rotate {
            active: active(&rotated),
            replacement: key(10),
        },
    );
    assert_rejected(&rotated, 1, &again, &facts(&again, &[8], &[10]));
    let started = apply(&rotated, &start(&rotated), &[2, 3], &[9]);
    let expired = started.advance_height(8).unwrap();
    assert_eq!(expired.active_generation, u64::MAX);
}

#[test]
fn staged_policy_reserves_terminal_and_version_capacity() {
    for remaining in [0, 1] {
        let mut state = enrolled();
        state.policy_change_sequence = u64::MAX - remaining;
        let a = stage(&state);
        assert_rejected(&state, 1, &a, &facts(&a, &[1, 2, 3], &[5, 6, 7]));
    }
    let mut state = enrolled();
    state.policy_version = u64::MAX;
    let a = stage(&state);
    assert_rejected(&state, 1, &a, &facts(&a, &[1, 2, 3], &[5, 6, 7]));
    state.policy_version = u64::MAX - 1;
    state.policy_change_sequence = u64::MAX - 2;
    let staged = apply(&state, &stage(&state), &[1, 2, 3], &[5, 6, 7]);
    let expired = staged.advance_height(6).unwrap();
    assert_eq!(expired.policy_change_sequence, u64::MAX);
    assert_eq!(expired.policy_version, u64::MAX - 1);
    assert!(expired.pending_policy.is_none());
    assert_eq!(
        expired.advance_height(7).unwrap().policy_change_sequence,
        u64::MAX
    );
    let interrupted = apply(&staged, &start(&staged), &[2, 3], &[9]);
    assert_eq!(interrupted.policy_change_sequence, u64::MAX);
    assert!(interrupted.pending_policy.is_none());
    let mature = staged.advance_height(3).unwrap();
    let changed = apply(&mature, &activate(&mature), &[1, 2, 3], &[]);
    assert_eq!(changed.policy_version, u64::MAX);
    assert_eq!(changed.policy_change_sequence, u64::MAX);
}

#[test]
fn only_processed_heights_accept_actions_and_height_overflow_is_atomic() {
    let state = enrolled();
    let a = start(&state);
    assert_rejected(&state, 2, &a, &facts(&a, &[2, 3], &[9]));
    let far = state.advance_height(u64::MAX - 5).unwrap();
    let a = Action {
        submission_expiry: u64::MAX,
        kind: ActionKind::Start {
            recovery: recovery(&far),
            request_id: [42; 32],
            replacement: key(9),
            timing_version: 1,
        },
    };
    assert_rejected(&far, far.last_height, &a, &facts(&a, &[2, 3], &[9]));
}
