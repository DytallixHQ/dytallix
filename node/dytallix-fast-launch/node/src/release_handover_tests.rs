use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
struct TestVerifier {
    outcome: u8,
    calls: AtomicUsize,
}
impl TestVerifier {
    fn valid() -> Self {
        Self {
            outcome: 1,
            calls: AtomicUsize::new(0),
        }
    }
}
impl Verifier for TestVerifier {
    fn verify(
        &self,
        _: &str,
        key: &[u8],
        _: u64,
        height: u64,
        before: u64,
        after: u64,
        artifact: &[u8],
        signature: &[u8],
    ) -> Result<bool> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        assert_eq!((before, after), (height, height));
        assert_eq!(key.len(), KEY_BYTES);
        assert_eq!(signature.len(), SIGNATURE_BYTES);
        assert!(
            artifact.starts_with(b"DYTALLIX/RELEASE-HANDOVER/v1\0")
                || artifact.starts_with(b"DYTALLIX/CHAIN-UPGRADE/v1\0")
        );
        match self.outcome {
            0 => Ok(false),
            1 => Ok(true),
            _ => anyhow::bail!("helper unavailable"),
        }
    }
}
fn policy() -> Policy {
    Policy {
        schema: 1,
        development_only: true,
        chain_id: "handover-test".into(),
        genesis_sha256: "aa".repeat(32),
        initial_release_sha512: "bb".repeat(64),
        initial_schema: 0,
        authority_epoch: 3,
        authority: emergency::AuthorityPolicy {
            threshold: 2,
            keys: vec![
                emergency::AuthorityKey {
                    key_id: "a".into(),
                    public_key_hex: "11".repeat(KEY_BYTES),
                },
                emergency::AuthorityKey {
                    key_id: "b".into(),
                    public_key_hex: "22".repeat(KEY_BYTES),
                },
            ],
        },
        initial_sequence: 10,
        max_control_bytes: 200_000,
        max_signatures: 2,
    }
}
fn context(height: u64) -> BlockContext {
    BlockContext {
        height,
        parent_height: height - 1,
        parent_app_hash: "cc".repeat(32),
        source_schema: 0,
        emergency_frozen: false,
        emergency_control_present: false,
        emergency_upgrade_hold: false,
        emergency_receipt_sha256: None,
    }
}
fn release_plan(migration: bool) -> ReleasePlan {
    ReleasePlan {
        target_release_sha512: "dd".repeat(64),
        authorization_sha256: "ee".repeat(32),
        transition: if migration {
            Transition::ReceiptIndexV1 {
                migration_sha256: upgrade::migration_sha256(),
            }
        } else {
            Transition::SchemaPreserving { schema: 0 }
        },
    }
}
fn control(policy: &Policy, state: &State, context: &BlockContext, action: Action) -> Control {
    Control {
        kind: CONTROL_KIND.into(),
        payload: Payload {
            schema: 1,
            chain_id: policy.chain_id.clone(),
            genesis_sha256: policy.genesis_sha256.clone(),
            policy_sha256: policy.sha256().unwrap(),
            source_release_sha512: state.active_release_sha512.clone(),
            authority_epoch: policy.authority_epoch,
            sequence: state.next_sequence,
            parent_height: context.parent_height,
            parent_app_hash: context.parent_app_hash.clone(),
            target_height: context.height,
            action,
        },
        signatures: vec![
            emergency::ControlSignature {
                key_id: "a".into(),
                signature_hex: "33".repeat(SIGNATURE_BYTES),
            },
            emergency::ControlSignature {
                key_id: "b".into(),
                signature_hex: "44".repeat(SIGNATURE_BYTES),
            },
        ],
    }
}
fn run(
    policy: &Policy,
    state: &State,
    context: &BlockContext,
    control: &Control,
    prepared: Option<&PreparedUpgradeOutcome>,
) -> Result<BlockPlan> {
    plan_block(
        policy,
        state,
        context,
        Some(&serde_json::to_vec(control)?),
        prepared,
        &TestVerifier::valid(),
    )
}
fn admitted(migration: bool) -> (Policy, State) {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let ctx = context(1);
    let c = control(
        &policy,
        &state,
        &ctx,
        Action::Admit {
            plan: release_plan(migration),
        },
    );
    let state = run(&policy, &state, &ctx, &c, None).unwrap().state;
    (policy, state)
}
fn activation(
    policy: &Policy,
    state: &State,
    ctx: &BlockContext,
    prepared: Option<&PreparedUpgradeOutcome>,
) -> Control {
    let pending = state.pending().unwrap();
    control(
        policy,
        state,
        ctx,
        Action::Activate {
            plan: pending.plan.clone(),
            admission_receipt_sha256: pending.admission_receipt_sha256.clone(),
            emergency_receipt_sha256: ctx.emergency_receipt_sha256.clone(),
            evidence_sha256: "ff".repeat(32),
            upgrade_activation_sha256: prepared.map(|p| p.control_sha256.clone()),
        },
    )
}
fn prepared_upgrade() -> PreparedUpgradeOutcome {
    let policy = policy();
    let p = upgrade::Policy {
        schema: 1,
        development_only: true,
        chain_id: policy.chain_id.clone(),
        genesis_sha256: policy.genesis_sha256.clone(),
        source_release_sha512: policy.initial_release_sha512.clone(),
        authority_epoch: policy.authority_epoch,
        authority: policy.authority.clone(),
        initial_sequence: 1,
        max_control_bytes: policy.max_control_bytes,
        max_signatures: policy.max_signatures,
        migration_bounds: upgrade::MigrationBounds {
            max_receipts: 10,
            max_receipt_bytes: 100_000,
            max_write_bytes: 100_000,
        },
    };
    let plan = upgrade::MigrationPlan {
        target_release_sha512: p.source_release_sha512.clone(),
        migration_id: upgrade::MIGRATION_ID.into(),
        migration_sha256: upgrade::migration_sha256(),
        source_schema: 0,
        target_schema: 1,
        bounds: p.migration_bounds.clone(),
        authorization_sha256: "ff".repeat(32),
    };
    let context = |height| upgrade::BlockContext {
        height,
        parent_height: height - 1,
        parent_app_hash: "cc".repeat(32),
        emergency_frozen: false,
        emergency_control_present: false,
        emergency_upgrade_hold: false,
        emergency_receipt_sha256: None,
    };
    let make = |sequence, height, action| upgrade::Control {
        kind: upgrade::CONTROL_KIND.into(),
        payload: upgrade::Payload {
            schema: 1,
            chain_id: p.chain_id.clone(),
            genesis_sha256: p.genesis_sha256.clone(),
            source_release_sha512: p.source_release_sha512.clone(),
            authority_epoch: p.authority_epoch,
            sequence,
            parent_height: height - 1,
            parent_app_hash: "cc".repeat(32),
            target_height: height,
            action,
        },
        signatures: vec![
            emergency::ControlSignature {
                key_id: "a".into(),
                signature_hex: "33".repeat(SIGNATURE_BYTES),
            },
            emergency::ControlSignature {
                key_id: "b".into(),
                signature_hex: "44".repeat(SIGNATURE_BYTES),
            },
        ],
    };
    let first = make(1, 1, upgrade::Action::Admit { plan: plan.clone() });
    let history = upgrade::VerifiedEmergencyHistory::empty();
    let first = upgrade::plan_block(
        &p,
        &upgrade::State::new(&p).unwrap(),
        &context(1),
        Some(&serde_json::to_vec(&first).unwrap()),
        &history,
        &TestVerifier::valid(),
    )
    .unwrap();
    let second = make(
        2,
        2,
        upgrade::Action::Activate {
            plan,
            admission_receipt_sha256: first.receipt.unwrap().sha256().unwrap(),
            emergency_receipt_sha256: None,
            evidence_sha256: "ee".repeat(32),
        },
    );
    let raw = serde_json::to_vec(&second).unwrap();
    let second = upgrade::plan_block(
        &p,
        &first.state,
        &context(2),
        Some(&raw),
        &history,
        &TestVerifier::valid(),
    )
    .unwrap();
    PreparedUpgradeOutcome::from_verified_upgrade(&raw, &second).unwrap()
}
#[test]
fn handover_preserves_initial_identity_and_strict_codec() {
    let p = policy();
    let s = State::new(&p).unwrap();
    assert_eq!(decode_state(&p, &encode_state(&s).unwrap()).unwrap(), s);
    let mut raw = encode_state(&s).unwrap();
    raw.push(b' ');
    assert!(decode_state(&p, &raw).is_err());
    let mut p = p;
    p.development_only = false;
    assert!(p.validate().is_err());
}
#[test]
fn handover_admission_changes_no_active_release_or_schema() {
    let (p, s) = admitted(true);
    assert_eq!(s.active_release_sha512(), p.initial_release_sha512);
    assert_eq!(s.active_schema(), 0);
    assert_eq!(s.next_sequence(), 11);
    assert!(s.pending().is_some());
    assert!(s.activation_receipt_sha256().is_none());
}
#[test]
fn handover_schema_preserving_release_is_explicit_and_replayable() {
    let (p, s) = admitted(false);
    let ctx = context(2);
    let c = activation(&p, &s, &ctx, None);
    let result = run(&p, &s, &ctx, &c, None).unwrap();
    assert_eq!(result.state.active_release_sha512(), "dd".repeat(64));
    assert_eq!(result.state.active_schema(), 0);
    assert_eq!(result.activated_release_sha512, Some("dd".repeat(64)));
    let receipt = result.receipt.unwrap();
    assert!(receipt.upgrade_receipt_sha256.is_none());
    let replay = replay_record(&p, &s, &receipt, &ctx, None, Some(&TestVerifier::valid())).unwrap();
    assert_eq!(replay.state, result.state);
}
#[test]
fn handover_actual_prepared_v1_migration_changes_release_and_schema() {
    let (p, s) = admitted(true);
    let ctx = context(2);
    let prepared = prepared_upgrade();
    let c = activation(&p, &s, &ctx, Some(&prepared));
    let result = run(&p, &s, &ctx, &c, Some(&prepared)).unwrap();
    assert_eq!(result.state.active_schema(), 1);
    assert_eq!(result.state.active_release_sha512(), "dd".repeat(64));
    assert_eq!(
        result
            .receipt
            .as_ref()
            .unwrap()
            .upgrade_receipt_sha256
            .as_deref(),
        Some(prepared.receipt_sha256.as_str())
    );
    assert_eq!(
        replay_record(
            &p,
            &s,
            result.receipt.as_ref().unwrap(),
            &ctx,
            Some(&prepared),
            Some(&TestVerifier::valid())
        )
        .unwrap()
        .state,
        result.state
    );
}
#[test]
fn handover_migration_requires_exact_prepared_outcome() {
    let (p, s) = admitted(true);
    let ctx = context(2);
    let mut prepared = prepared_upgrade();
    let c = activation(&p, &s, &ctx, Some(&prepared));
    assert!(run(&p, &s, &ctx, &c, None).is_err());
    prepared.control_sha256 = "00".repeat(32);
    assert!(run(&p, &s, &ctx, &c, Some(&prepared)).is_err());
    let mut prepared = prepared_upgrade();
    prepared.context.parent_app_hash = "00".repeat(32);
    assert!(run(&p, &s, &ctx, &c, Some(&prepared)).is_err());
}
#[test]
fn handover_cannot_label_migration_as_schema_preserving() {
    let (p, s) = admitted(false);
    let ctx = context(2);
    let prepared = prepared_upgrade();
    let c = activation(&p, &s, &ctx, Some(&prepared));
    assert!(run(&p, &s, &ctx, &c, Some(&prepared)).is_err());
    assert!(plan_block(&p, &s, &ctx, None, Some(&prepared), &TestVerifier::valid()).is_err());
}
#[test]
fn handover_cancel_preserves_history_and_consumes_sequence() {
    let (p, s) = admitted(true);
    let ctx = context(2);
    let pending = s.pending().unwrap();
    let c = control(
        &p,
        &s,
        &ctx,
        Action::Cancel {
            plan_sha256: pending.plan.sha256().unwrap(),
            admission_receipt_sha256: pending.admission_receipt_sha256.clone(),
        },
    );
    let result = run(&p, &s, &ctx, &c, None).unwrap();
    assert!(result.state.pending().is_none());
    assert_eq!(result.state.next_sequence(), 12);
    assert_eq!(
        result.state.active_release_sha512(),
        p.initial_release_sha512
    );
    assert_eq!(
        result.receipt.unwrap().previous_receipt_sha256.as_deref(),
        s.last_receipt_sha256()
    );
}
#[test]
fn handover_freeze_or_same_block_control_prevents_activation() {
    let (p, s) = admitted(false);
    for same_block in [false, true] {
        let mut ctx = context(2);
        ctx.emergency_control_present = same_block;
        ctx.emergency_frozen = !same_block;
        ctx.emergency_upgrade_hold = true;
        ctx.emergency_receipt_sha256 = Some("88".repeat(32));
        let c = activation(&p, &s, &ctx, None);
        assert!(run(&p, &s, &ctx, &c, None).is_err());
    }
}
#[test]
fn handover_fresh_emergency_clearance_does_not_clear_hold() {
    let (p, s) = admitted(false);
    let mut ctx = context(2);
    ctx.emergency_upgrade_hold = true;
    ctx.emergency_receipt_sha256 = Some("88".repeat(32));
    let c = activation(&p, &s, &ctx, None);
    assert!(run(&p, &s, &ctx, &c, None).is_ok());
    let mut changed = ctx.clone();
    changed.emergency_receipt_sha256 = Some("99".repeat(32));
    assert!(run(&p, &s, &changed, &c, None).is_err());
    assert!(ctx.emergency_upgrade_hold);
}
#[test]
fn handover_rejects_changed_identity_height_sequence_and_policy() {
    let (p, s) = admitted(false);
    let ctx = context(2);
    let c = activation(&p, &s, &ctx, None);
    for field in 0..7 {
        let mut c = c.clone();
        match field {
            0 => c.payload.source_release_sha512 = "00".repeat(64),
            1 => c.payload.policy_sha256 = "00".repeat(32),
            2 => c.payload.genesis_sha256 = "00".repeat(32),
            3 => c.payload.authority_epoch += 1,
            4 => c.payload.sequence += 1,
            5 => c.payload.parent_app_hash = "00".repeat(32),
            _ => c.payload.target_height += 1,
        }
        assert!(run(&p, &s, &ctx, &c, None).is_err());
    }
}
#[test]
fn handover_signature_refusal_and_infrastructure_failure_differ() {
    let (p, s) = admitted(false);
    let ctx = context(2);
    let c = activation(&p, &s, &ctx, None);
    let raw = serde_json::to_vec(&c).unwrap();
    let refused = TestVerifier {
        outcome: 0,
        calls: AtomicUsize::new(0),
    };
    let error = plan_block(&p, &s, &ctx, Some(&raw), None, &refused).unwrap_err();
    assert!(is_rejection(&error));
    let failed = TestVerifier {
        outcome: 2,
        calls: AtomicUsize::new(0),
    };
    let error = plan_block(&p, &s, &ctx, Some(&raw), None, &failed).unwrap_err();
    assert!(!is_rejection(&error));
}
#[test]
fn handover_duplicate_or_insufficient_signers_fail_before_verifier() {
    let (p, s) = admitted(false);
    let ctx = context(2);
    let c = activation(&p, &s, &ctx, None);
    for duplicate in [false, true] {
        let mut c = c.clone();
        if duplicate {
            c.signatures[1] = c.signatures[0].clone();
        } else {
            c.signatures.pop();
        }
        let verifier = TestVerifier::valid();
        assert!(plan_block(
            &p,
            &s,
            &ctx,
            Some(&serde_json::to_vec(&c).unwrap()),
            None,
            &verifier
        )
        .is_err());
        assert_eq!(verifier.calls.load(Ordering::Relaxed), 0);
    }
}
#[test]
fn handover_forward_recovery_requires_new_signed_release_control() {
    let (p, s) = admitted(false);
    let ctx = context(2);
    let old = activation(&p, &s, &ctx, None);
    let next = run(&p, &s, &ctx, &old, None).unwrap().state;
    assert!(run(&p, &next, &context(3), &old, None).is_err());
    let mut back = release_plan(false);
    back.target_release_sha512 = p.initial_release_sha512.clone();
    let c = control(&p, &next, &context(3), Action::Admit { plan: back });
    let admitted = run(&p, &next, &context(3), &c, None).unwrap().state;
    let c = activation(&p, &admitted, &context(4), None);
    let final_state = run(&p, &admitted, &context(4), &c, None).unwrap().state;
    assert_eq!(
        final_state.active_release_sha512(),
        p.initial_release_sha512
    );
    assert_eq!(final_state.next_sequence(), 14);
}
#[test]
fn handover_corrupt_receipt_or_actual_context_fails_replay() {
    let (p, s) = admitted(false);
    let ctx = context(2);
    let c = activation(&p, &s, &ctx, None);
    let result = run(&p, &s, &ctx, &c, None).unwrap();
    let mut receipt = result.receipt.unwrap();
    receipt.previous_receipt_sha256 = None;
    assert!(replay_record(&p, &s, &receipt, &ctx, None, Some(&TestVerifier::valid())).is_err());
}
#[test]
fn handover_target_source_and_schema_must_match_transition() {
    let p = policy();
    let s = State::new(&p).unwrap();
    let ctx = context(1);
    let mut plan = release_plan(false);
    plan.target_release_sha512 = p.initial_release_sha512.clone();
    assert!(run(
        &p,
        &s,
        &ctx,
        &control(&p, &s, &ctx, Action::Admit { plan }),
        None
    )
    .is_err());
    let mut ctx = ctx;
    ctx.source_schema = 1;
    assert!(plan_block(&p, &s, &ctx, None, None, &TestVerifier::valid()).is_err());
}

#[test]
fn handover_mempool_admission_defers_only_actual_pair() {
    let (p, s) = admitted(true);
    let ctx = context(2);
    let prepared = prepared_upgrade();
    let c = activation(&p, &s, &ctx, Some(&prepared));
    let raw = serde_json::to_vec(&c).unwrap();
    let verifier = TestVerifier::valid();
    check_admission(&p, &s, &ctx, &raw, &verifier).unwrap();
    assert_eq!(verifier.calls.load(Ordering::Relaxed), 2);
    assert!(run(&p, &s, &ctx, &c, None).is_err());
    let mut wrong = prepared.clone();
    wrong.control_sha256 = "00".repeat(32);
    assert!(run(&p, &s, &ctx, &c, Some(&wrong)).is_err());
    assert!(run(&p, &s, &ctx, &c, Some(&prepared)).is_ok());
    assert_eq!(s.active_release_sha512(), p.initial_release_sha512);
    assert_eq!(s.next_sequence(), 11);
}

#[test]
fn handover_mempool_admission_requires_signature_verification() {
    let (p, s) = admitted(true);
    let ctx = context(2);
    let c = activation(&p, &s, &ctx, Some(&prepared_upgrade()));
    let raw = serde_json::to_vec(&c).unwrap();
    let refused = TestVerifier {
        outcome: 0,
        calls: AtomicUsize::new(0),
    };
    let error = check_admission(&p, &s, &ctx, &raw, &refused).unwrap_err();
    assert!(is_rejection(&error));
    assert_eq!(refused.calls.load(Ordering::Relaxed), 1);
    let failed = TestVerifier {
        outcome: 2,
        calls: AtomicUsize::new(0),
    };
    let error = check_admission(&p, &s, &ctx, &raw, &failed).unwrap_err();
    assert!(!is_rejection(&error));
    let mut insufficient = c;
    insufficient.signatures.pop();
    let verifier = TestVerifier::valid();
    assert!(check_admission(
        &p,
        &s,
        &ctx,
        &serde_json::to_vec(&insufficient).unwrap(),
        &verifier
    )
    .is_err());
    assert_eq!(verifier.calls.load(Ordering::Relaxed), 0);
}

#[test]
fn handover_mempool_admission_preserves_context_and_plan_checks() {
    let (p, s) = admitted(true);
    let ctx = context(2);
    let base = activation(&p, &s, &ctx, Some(&prepared_upgrade()));
    for field in 0..9 {
        let mut c = base.clone();
        match field {
            0 => c.payload.parent_app_hash = "00".repeat(32),
            1 => c.payload.target_height += 1,
            2 => c.payload.sequence += 1,
            3 => c.payload.policy_sha256 = "00".repeat(32),
            4 => c.payload.source_release_sha512 = "00".repeat(64),
            5 => c.payload.genesis_sha256 = "00".repeat(32),
            6 => c.payload.authority_epoch += 1,
            7 => {
                if let Action::Activate {
                    admission_receipt_sha256,
                    ..
                } = &mut c.payload.action
                {
                    *admission_receipt_sha256 = "00".repeat(32);
                }
            }
            _ => {
                if let Action::Activate { plan, .. } = &mut c.payload.action {
                    plan.target_release_sha512 = "00".repeat(64);
                }
            }
        }
        let verifier = TestVerifier::valid();
        assert!(
            check_admission(&p, &s, &ctx, &serde_json::to_vec(&c).unwrap(), &verifier).is_err(),
            "field {field}"
        );
        assert_eq!(verifier.calls.load(Ordering::Relaxed), 0);
    }
    let initial = State::new(&p).unwrap();
    assert!(check_admission(
        &p,
        &initial,
        &ctx,
        &serde_json::to_vec(&base).unwrap(),
        &TestVerifier::valid()
    )
    .is_err());
    let mut raw = serde_json::to_vec(&base).unwrap();
    raw.push(b' ');
    assert!(check_admission(&p, &s, &ctx, &raw, &TestVerifier::valid()).is_err());
}

#[test]
fn handover_mempool_admission_requires_canonical_paired_hash() {
    let (p, s) = admitted(true);
    let ctx = context(2);
    let base = activation(&p, &s, &ctx, Some(&prepared_upgrade()));
    for invalid in [
        None,
        Some("00".repeat(31)),
        Some("AA".repeat(32)),
        Some("zz".repeat(32)),
    ] {
        let mut c = base.clone();
        if let Action::Activate {
            upgrade_activation_sha256,
            ..
        } = &mut c.payload.action
        {
            *upgrade_activation_sha256 = invalid;
        }
        let verifier = TestVerifier::valid();
        assert!(
            check_admission(&p, &s, &ctx, &serde_json::to_vec(&c).unwrap(), &verifier).is_err()
        );
        assert_eq!(verifier.calls.load(Ordering::Relaxed), 0);
    }
}

#[test]
fn handover_mempool_admission_keeps_emergency_and_schema_checks() {
    let (p, s) = admitted(false);
    let mut ctx = context(2);
    ctx.emergency_upgrade_hold = true;
    ctx.emergency_receipt_sha256 = Some("88".repeat(32));
    let c = activation(&p, &s, &ctx, None);
    let raw = serde_json::to_vec(&c).unwrap();
    check_admission(&p, &s, &ctx, &raw, &TestVerifier::valid()).unwrap();
    for field in 0..4 {
        let mut changed = ctx.clone();
        match field {
            0 => changed.emergency_frozen = true,
            1 => changed.emergency_control_present = true,
            2 => changed.emergency_receipt_sha256 = Some("99".repeat(32)),
            _ => changed.source_schema = 1,
        }
        let verifier = TestVerifier::valid();
        assert!(check_admission(&p, &s, &changed, &raw, &verifier).is_err());
        assert_eq!(verifier.calls.load(Ordering::Relaxed), 0);
    }
    let mut c = c;
    if let Action::Activate {
        upgrade_activation_sha256,
        ..
    } = &mut c.payload.action
    {
        *upgrade_activation_sha256 = Some("00".repeat(32));
    }
    assert!(check_admission(
        &p,
        &s,
        &ctx,
        &serde_json::to_vec(&c).unwrap(),
        &TestVerifier::valid()
    )
    .is_err());
}
