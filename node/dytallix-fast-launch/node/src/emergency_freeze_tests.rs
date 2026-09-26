use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Verifier {
    outcome: u8,
    calls: AtomicUsize,
}
impl Verifier {
    fn valid() -> Self {
        Self {
            outcome: 1,
            calls: AtomicUsize::new(0),
        }
    }
}
impl ControlVerifier for Verifier {
    fn verify(
        &self,
        chain: &str,
        key: &[u8],
        sequence: u64,
        current: u64,
        before: u64,
        after: u64,
        artifact: &[u8],
        signature: &[u8],
    ) -> Result<bool> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        assert_eq!(chain, "development-freeze-test");
        assert_eq!(key.len(), KEY_BYTES);
        assert!(sequence >= 7);
        assert_eq!((before, after), (current, current));
        assert!(artifact.starts_with(b"DYTALLIX/EMERGENCY-TRANSACTION-FREEZE/v1\0"));
        assert_eq!(signature.len(), SIGNATURE_BYTES);
        match self.outcome {
            0 => Ok(false),
            1 => Ok(true),
            _ => anyhow::bail!("helper unavailable"),
        }
    }
}
fn policy() -> Policy {
    let key = |id: &str, byte: &str| AuthorityKey {
        key_id: id.into(),
        public_key_hex: byte.repeat(KEY_BYTES),
    };
    Policy {
        schema: 1,
        development_only: true,
        chain_id: "development-freeze-test".into(),
        release_sha512: "aa".repeat(64),
        initial_sequence: 7,
        freeze_authority: AuthorityPolicy {
            keys: vec![key("freeze-a", "11"), key("freeze-b", "22")],
            threshold: 2,
        },
        resume_authority: AuthorityPolicy {
            keys: vec![key("resume", "33")],
            threshold: 1,
        },
        max_control_bytes: 200_000,
        max_signatures: 2,
        automatic_transition_policy: AutomaticTransitionPolicy::ContinueExisting,
        v2: None,
    }
}
fn context(height: u64) -> BlockContext {
    BlockContext {
        height,
        parent_height: height - 1,
        parent_app_hash: "ab".repeat(32),
        finalized_anchor: None,
        active_release_sha512: None,
    }
}
fn control(policy: &Policy, state: &State, height: u64, action: Action) -> Control {
    let authority = match action {
        Action::Freeze => &policy.freeze_authority,
        Action::Resume => &policy.resume_authority,
    };
    Control {
        kind: CONTROL_KIND.into(),
        payload: Payload {
            schema: 1,
            chain_id: policy.chain_id.clone(),
            release_sha512: policy.release_sha512.clone(),
            action,
            sequence: state.next_sequence(),
            parent_height: height - 1,
            parent_app_hash: context(height).parent_app_hash,
            target_height: height,
            v2: None,
        },
        signatures: authority
            .keys
            .iter()
            .take(authority.threshold)
            .map(|key| ControlSignature {
                key_id: key.key_id.clone(),
                signature_hex: "44".repeat(SIGNATURE_BYTES),
            })
            .collect(),
    }
}
fn apply(policy: &Policy, state: &State, height: u64, action: Action) -> BlockPlan {
    let bytes = serde_json::to_vec(&control(policy, state, height, action)).unwrap();
    plan_block(
        policy,
        state,
        &context(height),
        Some(&bytes),
        &Verifier::valid(),
    )
    .unwrap()
}

#[test]
fn freeze_resume_boundaries_and_durable_upgrade_hold() {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let frozen = apply(&policy, &state, 1, Action::Freeze);
    assert!(
        frozen.state.frozen() && frozen.reject_user_transactions && frozen.block_upgrade_activation
    );
    let resumed = apply(&policy, &frozen.state, 2, Action::Resume);
    assert!(!resumed.state.frozen());
    assert!(resumed.reject_user_transactions && resumed.block_upgrade_activation);
    let later = plan_block(
        &policy,
        &resumed.state,
        &context(3),
        None,
        &Verifier::valid(),
    )
    .unwrap();
    assert!(!later.reject_user_transactions);
    assert!(later.block_upgrade_activation && later.state.blocks_upgrade());
    assert_eq!(later.state.next_sequence(), 9);
}

#[test]
fn no_automatic_expiry_or_restart_resume() {
    let policy = policy();
    let frozen = apply(&policy, &State::new(&policy).unwrap(), 1, Action::Freeze);
    let reopened = decode_state(&policy, &encode_state(&frozen.state).unwrap()).unwrap();
    let later = plan_block(
        &policy,
        &reopened,
        &context(1_000_000),
        None,
        &Verifier::valid(),
    )
    .unwrap();
    assert!(later.state.frozen() && later.reject_user_transactions);
    assert_eq!(later.state, reopened);
}

#[test]
fn ordered_receipts_recover_with_signatures_and_actual_block_bindings() {
    let policy = policy();
    let frozen = apply(&policy, &State::new(&policy).unwrap(), 1, Action::Freeze);
    let resumed = apply(&policy, &frozen.state, 9, Action::Resume);
    let receipts = vec![
        (frozen.receipt.unwrap(), context(1)),
        (resumed.receipt.unwrap(), context(9)),
    ];
    assert_eq!(
        recover(&policy, &receipts, &Verifier::valid()).unwrap(),
        resumed.state
    );
    assert_eq!(recover_recorded(&policy, &receipts).unwrap(), resumed.state);
    assert_eq!(
        receipt_key(7),
        "consensus:emergency:v1:receipt:00000000000000000007"
    );
    let bytes = encode_receipt(&receipts[0].0).unwrap();
    assert_eq!(decode_receipt(&policy, &bytes).unwrap(), receipts[0].0);
}

#[test]
fn recovery_rejects_missing_reordered_and_changed_history() {
    let policy = policy();
    let frozen = apply(&policy, &State::new(&policy).unwrap(), 1, Action::Freeze);
    let resumed = apply(&policy, &frozen.state, 9, Action::Resume);
    let first = (frozen.receipt.unwrap(), context(1));
    let second = (resumed.receipt.unwrap(), context(9));
    assert!(recover_recorded(&policy, std::slice::from_ref(&second)).is_err());
    assert!(recover_recorded(&policy, &[second.clone(), first.clone()]).is_err());
    let mut changed = second.clone();
    changed.0.previous_receipt_sha256 = Some("00".repeat(32));
    assert!(recover_recorded(&policy, &[first.clone(), changed]).is_err());
    let mut changed = first.clone();
    changed.1.parent_app_hash = "00".repeat(32);
    assert!(recover_recorded(&policy, &[changed]).is_err());
}

#[test]
fn policy_and_state_schema_are_strict() {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let bytes = encode_state(&state).unwrap();
    let mut changed = policy.clone();
    changed.resume_authority.threshold = 2;
    assert!(decode_state(&changed, &bytes).is_err());
    let mut value = serde_json::to_value(&state).unwrap();
    value["schema"] = 2.into();
    assert!(decode_state(&policy, &serde_json::to_vec(&value).unwrap()).is_err());
    value["schema"] = 1.into();
    value["extra"] = true.into();
    assert!(serde_json::from_value::<State>(value).is_err());
    assert!(decode_state(&policy, &[b' '; 1025]).is_err());
}

#[test]
fn production_activation_and_invalid_authority_configuration_are_rejected() {
    let original = policy();
    let mut changed = original.clone();
    changed.development_only = false;
    assert!(State::new(&changed).is_err());
    let mut changed = original.clone();
    changed.freeze_authority.keys[1].public_key_hex =
        changed.freeze_authority.keys[0].public_key_hex.clone();
    assert!(changed.validate().is_err());
    let mut changed = original.clone();
    changed.max_signatures = 1;
    assert!(changed.validate().is_err());
    let mut changed = original;
    changed.freeze_authority.keys.swap(0, 1);
    assert!(changed.validate().is_err());
}

#[test]
fn policy_matches_existing_root_sequence_and_chain_rules() {
    let original = policy();
    let mut changed = original.clone();
    changed.initial_sequence = 0;
    assert!(changed.validate().is_err());
    for chain in [
        "".to_string(),
        "bad chain".to_string(),
        "x".repeat(129),
        "chain/other".to_string(),
    ] {
        let mut changed = original.clone();
        changed.chain_id = chain;
        assert!(changed.validate().is_err());
    }
}

#[test]
fn canonical_control_and_declared_resource_limits() {
    let policy = policy();
    let control = control(&policy, &State::new(&policy).unwrap(), 1, Action::Freeze);
    let bytes = serde_json::to_vec(&control).unwrap();
    assert_eq!(decode_control(&policy, &bytes).unwrap(), control);
    assert!(decode_control(&policy, &serde_json::to_vec_pretty(&control).unwrap()).is_err());
    let mut bounded = policy.clone();
    bounded.max_control_bytes = bytes.len() - 1;
    assert!(decode_control(&bounded, &bytes).is_err());
    let duplicate = String::from_utf8(bytes).unwrap().replacen(
        "{",
        "{\"kind\":\"dytallix-emergency-control-v1\",",
        1,
    );
    assert!(decode_control(&policy, duplicate.as_bytes()).is_err());
}

#[test]
fn admission_checks_all_bindings_before_verifier() {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let original = control(&policy, &state, 1, Action::Freeze);
    let verifier = Verifier::valid();
    let mut cases = Vec::new();
    let mut changed = original.clone();
    changed.payload.sequence += 1;
    cases.push(changed);
    let mut changed = original.clone();
    changed.payload.chain_id.push('x');
    cases.push(changed);
    let mut changed = original.clone();
    changed.payload.release_sha512 = "ff".repeat(64);
    cases.push(changed);
    let mut changed = original.clone();
    changed.payload.parent_app_hash = "00".repeat(32);
    cases.push(changed);
    let mut changed = original.clone();
    changed.payload.target_height += 1;
    cases.push(changed);
    let mut changed = original.clone();
    changed.payload.parent_height += 1;
    cases.push(changed);
    let mut changed = original;
    changed.payload.action = Action::Resume;
    cases.push(changed);
    for invalid in cases {
        let bytes = serde_json::to_vec(&invalid).unwrap();
        let error = plan_block(&policy, &state, &context(1), Some(&bytes), &verifier).unwrap_err();
        assert!(is_rejection(&error), "{error}");
    }
    assert_eq!(verifier.calls.load(Ordering::Relaxed), 0);
    assert!(!state.frozen());
}

#[test]
fn signature_set_requires_threshold_distinct_keys_and_correct_action_authority() {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let original = control(&policy, &state, 1, Action::Freeze);
    let mut cases = Vec::new();
    let mut changed = original.clone();
    changed.signatures.pop();
    cases.push(changed);
    let mut changed = original.clone();
    changed.signatures[1] = changed.signatures[0].clone();
    cases.push(changed);
    let mut changed = original.clone();
    changed.signatures[0].key_id = "resume".into();
    cases.push(changed);
    let mut changed = original.clone();
    changed.signatures[0].signature_hex.pop();
    cases.push(changed);
    let mut changed = original;
    changed.signatures.swap(0, 1);
    cases.push(changed);
    let verifier = Verifier::valid();
    for invalid in cases {
        let bytes = serde_json::to_vec(&invalid).unwrap();
        assert!(is_rejection(
            &plan_block(&policy, &state, &context(1), Some(&bytes), &verifier).unwrap_err()
        ));
    }
    assert_eq!(verifier.calls.load(Ordering::Relaxed), 0);
}

#[test]
fn cryptographic_refusal_and_infrastructure_failure_are_distinct() {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let bytes = serde_json::to_vec(&control(&policy, &state, 1, Action::Freeze)).unwrap();
    let invalid = Verifier {
        outcome: 0,
        calls: AtomicUsize::new(0),
    };
    let unavailable = Verifier {
        outcome: 2,
        calls: AtomicUsize::new(0),
    };
    let error = plan_block(&policy, &state, &context(1), Some(&bytes), &invalid).unwrap_err();
    assert!(is_rejection(&error));
    let error = plan_block(&policy, &state, &context(1), Some(&bytes), &unavailable).unwrap_err();
    assert!(!is_rejection(&error));
    assert_eq!(error.to_string(), "helper unavailable");
    assert_eq!(state, State::new(&policy).unwrap());
}

#[test]
fn repeated_controls_and_sequence_overflow_cannot_consume_state() {
    let policy = policy();
    let frozen = apply(&policy, &State::new(&policy).unwrap(), 1, Action::Freeze);
    let repeated = serde_json::to_vec(&control(&policy, &frozen.state, 2, Action::Freeze)).unwrap();
    assert!(is_rejection(
        &plan_block(
            &policy,
            &frozen.state,
            &context(2),
            Some(&repeated),
            &Verifier::valid()
        )
        .unwrap_err()
    ));
    assert!(plan_block(
        &policy,
        &frozen.state,
        &context(1),
        None,
        &Verifier::valid()
    )
    .is_err());
    let mut limit = policy.clone();
    limit.initial_sequence = u64::MAX - 1;
    let state = State::new(&limit).unwrap();
    let bytes = serde_json::to_vec(&control(&limit, &state, 1, Action::Freeze)).unwrap();
    assert!(is_rejection(
        &plan_block(
            &limit,
            &state,
            &context(1),
            Some(&bytes),
            &Verifier::valid()
        )
        .unwrap_err()
    ));
}

#[test]
fn structural_recovery_does_not_claim_signature_acceptance() {
    let policy = policy();
    let frozen = apply(&policy, &State::new(&policy).unwrap(), 1, Action::Freeze);
    let records = [(frozen.receipt.unwrap(), context(1))];
    assert!(recover_recorded(&policy, &records).is_ok());
    let invalid = Verifier {
        outcome: 0,
        calls: AtomicUsize::new(0),
    };
    assert!(is_rejection(
        &recover(&policy, &records, &invalid).unwrap_err()
    ));
}

// These keys, height limits and signatures are disposable unit-test inputs.
// This verifier tests deterministic admission, not cryptographic validity.
struct V2Verifier {
    calls: AtomicUsize,
    outcome: u8,
}
impl V2Verifier {
    fn valid() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            outcome: 1,
        }
    }
}
impl ControlVerifier for V2Verifier {
    fn verify(
        &self,
        chain: &str,
        key: &[u8],
        sequence: u64,
        current: u64,
        before: u64,
        after: u64,
        artifact: &[u8],
        signature: &[u8],
    ) -> Result<bool> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        assert_eq!(chain, "development-freeze-test");
        assert_eq!(key.len(), KEY_BYTES);
        assert!(sequence >= 7 && before <= current && current <= after);
        let prefix = b"DYTALLIX/EMERGENCY-TRANSACTION-FREEZE/v2\0";
        assert!(artifact.starts_with(prefix));
        let payload: Payload = serde_json::from_slice(&artifact[prefix.len()..]).unwrap();
        let v2 = payload.v2.unwrap();
        assert_eq!((before, after), (v2.not_before_height, v2.not_after_height));
        assert_eq!(signature.len(), SIGNATURE_BYTES);
        match self.outcome {
            0 => Ok(false),
            1 => Ok(true),
            _ => anyhow::bail!("v2 helper unavailable"),
        }
    }
}
fn v2_policy() -> Policy {
    let mut p = policy();
    p.schema = 2;
    p.max_signatures = 5;
    p.max_control_bytes = 400_000;
    p.v2 = Some(PolicyV2 {
        genesis_sha256: "55".repeat(32),
        authority_epoch: 1,
        max_validity_blocks: 4,
        max_anchor_age_blocks: 8,
    });
    let authority = |purpose: &str, start: u8| AuthorityPolicy {
        keys: (0..5)
            .map(|i| AuthorityKey {
                key_id: format!("{purpose}-{i}"),
                public_key_hex: hex::encode(vec![start + i; KEY_BYTES]),
            })
            .collect(),
        threshold: 3,
    };
    p.freeze_authority = authority("freeze", 11);
    p.resume_authority = authority("resume", 21);
    p
}
fn v2_context(height: u64, anchor_height: u64) -> BlockContext {
    BlockContext {
        active_release_sha512: None,
        height,
        parent_height: height - 1,
        parent_app_hash: "ab".repeat(32),
        finalized_anchor: Some(FinalizedAnchor {
            height: anchor_height,
            app_hash: if anchor_height == height - 1 {
                "ab"
            } else {
                "cd"
            }
            .repeat(32),
        }),
    }
}
fn v2_control(p: &Policy, s: &State, ctx: &BlockContext, action: Action) -> Control {
    let mut c = control(p, s, ctx.height, action);
    let anchor = ctx.finalized_anchor.as_ref().unwrap();
    c.kind = CONTROL_KIND_V2.into();
    c.payload.schema = 2;
    c.payload.parent_height = anchor.height;
    c.payload.parent_app_hash = anchor.app_hash.clone();
    c.payload.target_height = anchor.height + 1;
    c.payload.v2 = Some(PayloadV2 {
        genesis_sha256: p.v2.as_ref().unwrap().genesis_sha256.clone(),
        authority_epoch: p.v2.as_ref().unwrap().authority_epoch,
        policy_sha256: p.sha256().unwrap(),
        not_before_height: anchor.height + 1,
        not_after_height: anchor.height + 4,
        incident_sha256: s.incident_sha256().unwrap_or(&"66".repeat(32)).to_owned(),
        resume: if action == Action::Resume {
            Some(ResumeBinding {
                freeze_receipt_sha256: s.freeze_receipt_sha256().unwrap().to_owned(),
                restored_state_sha256: anchor.app_hash.clone(),
                readiness_evidence_sha256: "77".repeat(32),
            })
        } else {
            None
        },
    });
    c
}
fn v2_plan(
    p: &Policy,
    s: &State,
    ctx: &BlockContext,
    c: &Control,
    v: &V2Verifier,
) -> Result<BlockPlan> {
    plan_block(p, s, ctx, Some(&serde_json::to_vec(c)?), v)
}

#[test]
fn v2_freeze_resume_windows_checkpoint_and_persistent_hold() {
    let p = v2_policy();
    let s = State::new(&p).unwrap();
    let fc = v2_context(3, 0);
    let control = v2_control(&p, &s, &fc, Action::Freeze);
    let verifier = V2Verifier::valid();
    let frozen = v2_plan(&p, &s, &fc, &control, &verifier).unwrap();
    assert_eq!(verifier.calls.load(Ordering::Relaxed), 3);
    assert!(frozen.reject_user_transactions && frozen.state.frozen());
    assert_eq!(
        frozen.state.freeze_receipt_sha256(),
        frozen.state.last_receipt_sha256()
    );
    // The signed restored checkpoint is height 3. Inclusion at 6 has a different parent.
    let rc = v2_context(6, 3);
    let resume = v2_control(&p, &frozen.state, &rc, Action::Resume);
    let resumed = v2_plan(&p, &frozen.state, &rc, &resume, &verifier).unwrap();
    assert!(resumed.reject_user_transactions && resumed.block_upgrade_activation);
    assert!(!resumed.state.frozen() && resumed.state.blocks_upgrade());
    let next = plan_block(&p, &resumed.state, &context(7), None, &verifier).unwrap();
    assert!(!next.reject_user_transactions && next.block_upgrade_activation);
    assert_eq!(
        next.state.freeze_receipt_sha256(),
        frozen.state.freeze_receipt_sha256()
    );
    let records = [
        (frozen.receipt.unwrap(), fc),
        (resumed.receipt.unwrap(), rc),
    ];
    assert_eq!(recover(&p, &records, &verifier).unwrap(), resumed.state);
    assert_eq!(recover_recorded(&p, &records).unwrap(), resumed.state);
    assert_eq!(
        decode_state(&p, &encode_state(&resumed.state).unwrap()).unwrap(),
        resumed.state
    );
    assert_eq!(
        decode_receipt(&p, &encode_receipt(&records[1].0).unwrap()).unwrap(),
        records[1].0
    );
}

#[test]
fn v2_bounds_quorum_and_purpose_configuration_are_explicit() {
    let original = v2_policy();
    let mut cases = Vec::new();
    let mut p = original.clone();
    p.v2.as_mut().unwrap().max_validity_blocks = 0;
    cases.push(p);
    let mut p = original.clone();
    p.v2.as_mut().unwrap().max_anchor_age_blocks = 0;
    cases.push(p);
    let mut p = original.clone();
    p.v2.as_mut().unwrap().authority_epoch = 0;
    cases.push(p);
    let mut p = original.clone();
    p.freeze_authority.threshold = 2;
    cases.push(p);
    let mut p = original.clone();
    p.resume_authority.keys.pop();
    cases.push(p);
    let mut p = original.clone();
    p.resume_authority.keys[0].public_key_hex = p.freeze_authority.keys[0].public_key_hex.clone();
    cases.push(p);
    let mut p = original.clone();
    p.resume_authority.keys[0].key_id = p.freeze_authority.keys[0].key_id.clone();
    cases.push(p);
    let mut p = original.clone();
    p.development_only = false;
    cases.push(p);
    let mut p = original.clone();
    p.schema = 1;
    cases.push(p);
    let mut p = original;
    p.v2 = None;
    cases.push(p);
    for p in cases {
        assert!(p.validate().is_err());
    }
}

#[test]
fn v2_bindings_refuse_changes_before_signature_work() {
    let p = v2_policy();
    let s = State::new(&p).unwrap();
    let ctx = v2_context(3, 0);
    let original = v2_control(&p, &s, &ctx, Action::Freeze);
    let mut cases = Vec::new();
    let mut c = original.clone();
    c.payload.v2.as_mut().unwrap().genesis_sha256 = "00".repeat(32);
    cases.push(c);
    let mut c = original.clone();
    c.payload.v2.as_mut().unwrap().policy_sha256 = "00".repeat(32);
    cases.push(c);
    let mut c = original.clone();
    c.payload.v2.as_mut().unwrap().authority_epoch += 1;
    cases.push(c);
    let mut c = original.clone();
    c.payload.release_sha512 = "00".repeat(64);
    cases.push(c);
    let mut c = original.clone();
    c.payload.parent_app_hash = "00".repeat(32);
    cases.push(c);
    let mut c = original.clone();
    c.payload.parent_height = 1;
    cases.push(c);
    let mut c = original.clone();
    c.payload.target_height = 2;
    cases.push(c);
    let mut c = original.clone();
    c.payload.sequence += 1;
    cases.push(c);
    let mut c = original.clone();
    c.payload.v2.as_mut().unwrap().incident_sha256 = "AA".repeat(32);
    cases.push(c);
    let mut c = original.clone();
    c.payload.v2.as_mut().unwrap().not_after_height = 5;
    cases.push(c);
    let mut c = original.clone();
    c.payload.v2.as_mut().unwrap().not_after_height = 0;
    cases.push(c);
    let mut c = original.clone();
    c.payload.v2 = None;
    cases.push(c);
    let mut c = original.clone();
    c.kind = CONTROL_KIND.into();
    cases.push(c);
    let mut c = original;
    c.signatures.pop();
    cases.push(c);
    let verifier = V2Verifier::valid();
    for c in cases {
        assert!(is_rejection(
            &v2_plan(&p, &s, &ctx, &c, &verifier).unwrap_err()
        ));
    }
    assert_eq!(verifier.calls.load(Ordering::Relaxed), 0);
}

#[test]
fn v2_validity_endpoints_anchor_age_and_unsigned_context() {
    let mut p = v2_policy();
    let ctx = v2_context(3, 0);
    let s = State::new(&p).unwrap();
    let c = v2_control(&p, &s, &ctx, Action::Freeze);
    let verifier = V2Verifier::valid();
    for height in [1, 4] {
        let mut actual = v2_context(height, 0);
        actual.finalized_anchor.as_mut().unwrap().app_hash = c.payload.parent_app_hash.clone();
        // At height 1 the genesis commitment is also the actual parent commitment.
        if height == 1 {
            actual.parent_app_hash = c.payload.parent_app_hash.clone();
        }
        assert!(v2_plan(&p, &s, &actual, &c, &verifier).is_ok());
    }
    assert!(is_rejection(
        &v2_plan(&p, &s, &v2_context(5, 0), &c, &verifier).unwrap_err()
    ));
    let mut missing = ctx.clone();
    missing.finalized_anchor = None;
    assert!(is_rejection(
        &v2_plan(&p, &s, &missing, &c, &verifier).unwrap_err()
    ));
    let mut wrong = ctx.clone();
    wrong.finalized_anchor.as_mut().unwrap().app_hash = "ef".repeat(32);
    assert!(is_rejection(
        &v2_plan(&p, &s, &wrong, &c, &verifier).unwrap_err()
    ));
    p.v2.as_mut().unwrap().max_anchor_age_blocks = 2;
    let s = State::new(&p).unwrap();
    let c = v2_control(&p, &s, &ctx, Action::Freeze);
    assert!(is_rejection(
        &v2_plan(&p, &s, &ctx, &c, &verifier).unwrap_err()
    ));
}

#[test]
fn v2_resume_requires_incident_receipt_restored_checkpoint_and_evidence() {
    let p = v2_policy();
    let s = State::new(&p).unwrap();
    let fc = v2_context(3, 0);
    let verifier = V2Verifier::valid();
    let frozen = v2_plan(
        &p,
        &s,
        &fc,
        &v2_control(&p, &s, &fc, Action::Freeze),
        &verifier,
    )
    .unwrap();
    let rc = v2_context(6, 3);
    let original = v2_control(&p, &frozen.state, &rc, Action::Resume);
    let mut cases = Vec::new();
    let mut c = original.clone();
    c.payload.v2.as_mut().unwrap().incident_sha256 = "88".repeat(32);
    cases.push(c);
    let mut c = original.clone();
    c.payload.v2.as_mut().unwrap().resume = None;
    cases.push(c);
    let mut c = original.clone();
    c.payload
        .v2
        .as_mut()
        .unwrap()
        .resume
        .as_mut()
        .unwrap()
        .freeze_receipt_sha256 = "88".repeat(32);
    cases.push(c);
    let mut c = original.clone();
    c.payload
        .v2
        .as_mut()
        .unwrap()
        .resume
        .as_mut()
        .unwrap()
        .restored_state_sha256 = rc.parent_app_hash.clone();
    cases.push(c);
    let mut c = original;
    c.payload
        .v2
        .as_mut()
        .unwrap()
        .resume
        .as_mut()
        .unwrap()
        .readiness_evidence_sha256
        .clear();
    cases.push(c);
    let refused = V2Verifier::valid();
    for c in cases {
        assert!(is_rejection(
            &v2_plan(&p, &frozen.state, &rc, &c, &refused).unwrap_err()
        ));
    }
    let old = v2_context(4, 2);
    let c = v2_control(&p, &frozen.state, &old, Action::Resume);
    assert!(is_rejection(
        &v2_plan(&p, &frozen.state, &old, &c, &refused).unwrap_err()
    ));
    assert_eq!(refused.calls.load(Ordering::Relaxed), 0);
}

#[test]
fn v2_freeze_cannot_smuggle_resume_evidence_and_cannot_expire() {
    let p = v2_policy();
    let s = State::new(&p).unwrap();
    let ctx = v2_context(3, 0);
    let verifier = V2Verifier::valid();
    let mut c = v2_control(&p, &s, &ctx, Action::Freeze);
    c.payload.v2.as_mut().unwrap().resume = Some(ResumeBinding {
        freeze_receipt_sha256: "11".repeat(32),
        restored_state_sha256: "22".repeat(32),
        readiness_evidence_sha256: "33".repeat(32),
    });
    assert!(is_rejection(
        &v2_plan(&p, &s, &ctx, &c, &verifier).unwrap_err()
    ));
    c.payload.v2.as_mut().unwrap().resume = None;
    let frozen = v2_plan(&p, &s, &ctx, &c, &verifier).unwrap();
    let expired = plan_block(&p, &frozen.state, &context(1_000_000), None, &verifier).unwrap();
    assert_eq!(expired.state, frozen.state);
    assert!(expired.reject_user_transactions && expired.block_upgrade_activation);
}

#[test]
fn v2_replay_rejects_changed_anchor_incident_and_policy_without_migration() {
    let p = v2_policy();
    let s = State::new(&p).unwrap();
    let ctx = v2_context(3, 0);
    let verifier = V2Verifier::valid();
    let c = v2_control(&p, &s, &ctx, Action::Freeze);
    let frozen = v2_plan(&p, &s, &ctx, &c, &verifier).unwrap();
    let record = (frozen.receipt.unwrap(), ctx);
    let mut changed = record.clone();
    changed.1.finalized_anchor.as_mut().unwrap().app_hash = "88".repeat(32);
    assert!(recover_recorded(&p, &[changed]).is_err());
    let mut changed = record;
    changed
        .0
        .control
        .payload
        .v2
        .as_mut()
        .unwrap()
        .incident_sha256 = "88".repeat(32);
    // Canonical replay changes the receipt hash and state; persisted state comparison rejects it.
    let reconstructed = recover_recorded(&p, &[changed]).unwrap();
    assert_ne!(reconstructed, frozen.state);
    let mut next_policy = p.clone();
    next_policy.v2.as_mut().unwrap().authority_epoch += 1;
    assert!(decode_state(&next_policy, &encode_state(&frozen.state).unwrap()).is_err());
    assert!(decode_state(&p, &encode_state(&State::new(&policy()).unwrap()).unwrap()).is_err());
}

#[test]
fn v2_cryptographic_refusal_and_helper_failure_preserve_state() {
    let p = v2_policy();
    let s = State::new(&p).unwrap();
    let ctx = v2_context(3, 0);
    let c = v2_control(&p, &s, &ctx, Action::Freeze);
    let refused = V2Verifier {
        calls: AtomicUsize::new(0),
        outcome: 0,
    };
    let missing = V2Verifier {
        calls: AtomicUsize::new(0),
        outcome: 2,
    };
    assert!(is_rejection(
        &v2_plan(&p, &s, &ctx, &c, &refused).unwrap_err()
    ));
    assert!(!is_rejection(
        &v2_plan(&p, &s, &ctx, &c, &missing).unwrap_err()
    ));
    assert_eq!(s, State::new(&p).unwrap());
}

#[test]
fn absent_v2_preserves_v1_bytes_and_explicit_null_is_rejected() {
    let p = policy();
    let s = State::new(&p).unwrap();
    let c = control(&p, &s, 1, Action::Freeze);
    let bytes = serde_json::to_vec(&p).unwrap();
    assert!(!String::from_utf8(bytes.clone()).unwrap().contains("v2"));
    assert_eq!(
        serde_json::to_vec(&serde_json::from_slice::<Policy>(&bytes).unwrap()).unwrap(),
        bytes
    );
    let mut artifact = b"DYTALLIX/EMERGENCY-TRANSACTION-FREEZE/v1\0".to_vec();
    artifact.extend_from_slice(&serde_json::to_vec(&c.payload).unwrap());
    assert_eq!(artifact_bytes(&c.payload).unwrap(), artifact);
    // Fixed hashes use the original v1 field order and original fixture values.
    assert_eq!(
        p.sha256().unwrap(),
        "3abbaef180dbfc0350f8cd5717c90257e9bfdf1e51d8eb134868a214cf41b252"
    );
    assert_eq!(
        digest(&serde_json::to_vec(&c.payload).unwrap()),
        "31149ce8b8e48f3f709186191d643a9a9977114b9c54bc1f027eb0f5e8a6b433"
    );
    assert_eq!(
        digest(&encode_state(&s).unwrap()),
        "ea7b9e6e138ffba082df7132f292cd80c86fa24388f21d5a7b652d70ad3c984f"
    );
    assert_eq!(
        digest(&serde_json::to_vec(&c).unwrap()),
        "31e0d35cd3fc5febf6d8143829fa5d1f9d8eae76435c9de42493cadf5f3731b9"
    );
    assert_eq!(
        digest(&artifact),
        "e1ff6210d5d61362698329cb9d2432a66bdca00108276a54cfe920eccdbb3ecb"
    );
    assert_eq!(
        apply(&p, &s, 1, Action::Freeze)
            .receipt
            .unwrap()
            .sha256()
            .unwrap(),
        "329391c700124e090291c08e904ef86729bffe36dccf000b1fdd450e8c74a972"
    );
    let mut value = serde_json::to_value(&p).unwrap();
    value["v2"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<Policy>(value).is_err());
    let mut value = serde_json::to_value(&c.payload).unwrap();
    value["v2"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<Payload>(value).is_err());
    let mut value = serde_json::to_value(context(1)).unwrap();
    value["finalized_anchor"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<BlockContext>(value).is_err());
    let mut value = serde_json::to_value(&s).unwrap();
    value["incident_sha256"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<State>(value).is_err());
}

#[test]
fn v2_new_freeze_replaces_resume_binding_without_clearing_upgrade_hold() {
    let p = v2_policy();
    let mut state = State::new(&p).unwrap();
    let verifier = V2Verifier::valid();
    let mut first_receipt = String::new();
    for (height, anchor, action) in [
        (3, 0, Action::Freeze),
        (6, 3, Action::Resume),
        (9, 6, Action::Freeze),
    ] {
        let ctx = v2_context(height, anchor);
        let c = v2_control(&p, &state, &ctx, action);
        state = v2_plan(&p, &state, &ctx, &c, &verifier).unwrap().state;
        assert!(state.blocks_upgrade());
        if height == 3 {
            first_receipt = state.freeze_receipt_sha256().unwrap().to_owned();
        }
    }
    assert_ne!(state.freeze_receipt_sha256(), Some(first_receipt.as_str()));
    let ctx = v2_context(12, 9);
    let mut resume = v2_control(&p, &state, &ctx, Action::Resume);
    resume
        .payload
        .v2
        .as_mut()
        .unwrap()
        .resume
        .as_mut()
        .unwrap()
        .freeze_receipt_sha256 = first_receipt;
    assert!(is_rejection(
        &v2_plan(&p, &state, &ctx, &resume, &verifier).unwrap_err()
    ));
    assert!(state.frozen());
}

#[test]
fn historical_release_v1_uses_trusted_context_without_mutating_policy() {
    let p = policy();
    let before = serde_json::to_vec(&p).unwrap();
    let s = State::new(&p).unwrap();
    let mut ctx = context(4);
    ctx.active_release_sha512 = Some("bb".repeat(64));
    let mut c = control(&p, &s, 4, Action::Freeze);
    let verifier = Verifier::valid();
    assert!(plan_block(
        &p,
        &s,
        &ctx,
        Some(&serde_json::to_vec(&c).unwrap()),
        &verifier
    )
    .is_err());
    assert_eq!(verifier.calls.load(Ordering::Relaxed), 0);
    let old_artifact = artifact_bytes(&c.payload).unwrap();
    c.payload.release_sha512 = ctx.active_release_sha512.clone().unwrap();
    assert_ne!(artifact_bytes(&c.payload).unwrap(), old_artifact);
    let next = plan_block(
        &p,
        &s,
        &ctx,
        Some(&serde_json::to_vec(&c).unwrap()),
        &verifier,
    )
    .unwrap();
    assert!(next.state.frozen());
    assert_eq!(serde_json::to_vec(&p).unwrap(), before);
    let receipt = next.receipt.unwrap();
    assert_eq!(receipt.policy_sha256, p.sha256().unwrap());
    assert_eq!(
        receipt.context.active_release_sha512,
        ctx.active_release_sha512
    );
    assert_eq!(
        recover(&p, &[(receipt.clone(), ctx.clone())], &Verifier::valid()).unwrap(),
        next.state
    );
    assert_eq!(recover_recorded(&p, &[(receipt, ctx)]).unwrap(), next.state);
}

#[test]
fn historical_release_v2_replays_old_and_new_receipts_at_their_original_release() {
    let p = v2_policy();
    let policy_bytes = serde_json::to_vec(&p).unwrap();
    let policy_hash = p.sha256().unwrap();
    let mut s = State::new(&p).unwrap();
    let mut receipts = Vec::new();
    for (height, action, release) in [
        (1, Action::Freeze, None),
        (2, Action::Resume, None),
        (4, Action::Freeze, Some("bb".repeat(64))),
        (5, Action::Resume, Some("bb".repeat(64))),
    ] {
        let mut ctx = v2_context(height, height - 1);
        ctx.active_release_sha512 = release.clone();
        let mut c = v2_control(&p, &s, &ctx, action);
        let verifier = V2Verifier::valid();
        if let Some(selected) = release {
            assert!(v2_plan(&p, &s, &ctx, &c, &verifier).is_err());
            assert_eq!(verifier.calls.load(Ordering::Relaxed), 0);
            c.payload.release_sha512 = selected;
        }
        let result = v2_plan(&p, &s, &ctx, &c, &verifier).unwrap();
        let receipt = result.receipt.unwrap();
        assert_eq!(receipt.policy_sha256, policy_hash);
        assert_eq!(
            receipt.control.payload.v2.as_ref().unwrap().policy_sha256,
            policy_hash
        );
        receipts.push((receipt, ctx));
        s = result.state;
    }
    assert_eq!(recover(&p, &receipts, &V2Verifier::valid()).unwrap(), s);
    assert_eq!(recover_recorded(&p, &receipts).unwrap(), s);
    assert_eq!(serde_json::to_vec(&p).unwrap(), policy_bytes);
    let mut wrong_current_release = receipts.clone();
    wrong_current_release[0].1.active_release_sha512 = Some("bb".repeat(64));
    assert!(recover(&p, &wrong_current_release, &V2Verifier::valid()).is_err());
    let mut missing_handover = receipts.clone();
    missing_handover[2].0.context.active_release_sha512 = None;
    missing_handover[2].1.active_release_sha512 = None;
    assert!(recover_recorded(&p, &missing_handover).is_err());
    let mut changed_handover = receipts;
    changed_handover[2].0.context.active_release_sha512 = Some("cc".repeat(64));
    changed_handover[2].1.active_release_sha512 = Some("cc".repeat(64));
    assert!(recover(&p, &changed_handover, &V2Verifier::valid()).is_err());
}

#[test]
fn historical_release_payload_cannot_select_release_without_committed_context() {
    let p = v2_policy();
    let s = State::new(&p).unwrap();
    let ctx = v2_context(1, 0);
    let mut c = v2_control(&p, &s, &ctx, Action::Freeze);
    c.payload.release_sha512 = "bb".repeat(64);
    let verifier = V2Verifier::valid();
    assert!(v2_plan(&p, &s, &ctx, &c, &verifier).is_err());
    assert_eq!(verifier.calls.load(Ordering::Relaxed), 0);
}

#[test]
fn historical_release_context_requires_canonical_digest_and_omits_genesis() {
    let p = v2_policy();
    let s = State::new(&p).unwrap();
    for selected in ["BB".repeat(64), "b".repeat(127), p.release_sha512.clone()] {
        let mut ctx = v2_context(1, 0);
        ctx.active_release_sha512 = Some(selected.clone());
        let mut c = v2_control(&p, &s, &ctx, Action::Freeze);
        c.payload.release_sha512 = selected;
        let verifier = V2Verifier::valid();
        assert!(v2_plan(&p, &s, &ctx, &c, &verifier).is_err());
        assert_eq!(verifier.calls.load(Ordering::Relaxed), 0);
    }
    let mut value = serde_json::to_value(v2_context(1, 0)).unwrap();
    value["active_release_sha512"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<BlockContext>(value).is_err());
}

#[test]
fn historical_release_absence_preserves_exact_v2_receipt_bytes() {
    #[derive(Serialize)]
    struct PriorContext<'a> {
        height: u64,
        parent_height: u64,
        parent_app_hash: &'a str,
        finalized_anchor: &'a FinalizedAnchor,
    }
    #[derive(Serialize)]
    struct PriorReceipt<'a> {
        schema: u16,
        policy_sha256: &'a str,
        control: &'a Control,
        context: PriorContext<'a>,
        previous_receipt_sha256: &'a Option<String>,
    }
    let p = v2_policy();
    let s = State::new(&p).unwrap();
    let ctx = v2_context(1, 0);
    let c = v2_control(&p, &s, &ctx, Action::Freeze);
    let receipt = v2_plan(&p, &s, &ctx, &c, &V2Verifier::valid())
        .unwrap()
        .receipt
        .unwrap();
    let prior = PriorReceipt {
        schema: receipt.schema,
        policy_sha256: &receipt.policy_sha256,
        control: &receipt.control,
        context: PriorContext {
            height: ctx.height,
            parent_height: ctx.parent_height,
            parent_app_hash: &ctx.parent_app_hash,
            finalized_anchor: ctx.finalized_anchor.as_ref().unwrap(),
        },
        previous_receipt_sha256: &receipt.previous_receipt_sha256,
    };
    assert_eq!(
        serde_json::to_vec(&receipt).unwrap(),
        serde_json::to_vec(&prior).unwrap()
    );
    let encoded = serde_json::to_vec(&receipt).unwrap();
    assert!(!String::from_utf8(encoded.clone())
        .unwrap()
        .contains("active_release_sha512"));
    assert_eq!(
        serde_json::from_slice::<Receipt>(&encoded).unwrap(),
        receipt
    );
}
