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
        assert_eq!(chain, "development-upgrade-test");
        assert_eq!(key.len(), KEY_BYTES);
        assert!(sequence > 0);
        assert_eq!((before, after), (current, current));
        assert!(artifact.starts_with(b"DYTALLIX/CHAIN-UPGRADE/v1\0"));
        assert_eq!(signature.len(), SIGNATURE_BYTES);
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
        chain_id: "development-upgrade-test".into(),
        genesis_sha256: "aa".repeat(32),
        source_release_sha512: "bb".repeat(64),
        authority_epoch: 7,
        authority: emergency::AuthorityPolicy {
            keys: vec![
                emergency::AuthorityKey {
                    key_id: "upgrade-a".into(),
                    public_key_hex: "11".repeat(KEY_BYTES),
                },
                emergency::AuthorityKey {
                    key_id: "upgrade-b".into(),
                    public_key_hex: "22".repeat(KEY_BYTES),
                },
            ],
            threshold: 2,
        },
        initial_sequence: 3,
        max_control_bytes: 200_000,
        max_signatures: 2,
        migration_bounds: MigrationBounds {
            max_receipts: 10,
            max_receipt_bytes: 1_000_000,
            max_write_bytes: 10_000,
        },
    }
}
fn context(height: u64) -> BlockContext {
    BlockContext {
        height,
        parent_height: height - 1,
        parent_app_hash: "cc".repeat(32),
        emergency_frozen: false,
        emergency_control_present: false,
        emergency_upgrade_hold: false,
        emergency_receipt_sha256: None,
    }
}
fn migration(policy: &Policy) -> MigrationPlan {
    MigrationPlan {
        target_release_sha512: policy.source_release_sha512.clone(),
        migration_id: MIGRATION_ID.into(),
        migration_sha256: migration_sha256(),
        source_schema: 0,
        target_schema: 1,
        bounds: policy.migration_bounds.clone(),
        authorization_sha256: "dd".repeat(32),
    }
}
fn control(policy: &Policy, state: &State, context: &BlockContext, action: Action) -> Control {
    Control {
        kind: CONTROL_KIND.into(),
        payload: Payload {
            schema: 1,
            chain_id: policy.chain_id.clone(),
            genesis_sha256: policy.genesis_sha256.clone(),
            source_release_sha512: policy.source_release_sha512.clone(),
            authority_epoch: policy.authority_epoch,
            sequence: state.next_sequence(),
            parent_height: context.parent_height,
            parent_app_hash: context.parent_app_hash.clone(),
            target_height: context.height,
            action,
        },
        signatures: policy
            .authority
            .keys
            .iter()
            .map(|k| emergency::ControlSignature {
                key_id: k.key_id.clone(),
                signature_hex: "44".repeat(SIGNATURE_BYTES),
            })
            .collect(),
    }
}
fn apply(
    policy: &Policy,
    state: &State,
    context: &BlockContext,
    history: &VerifiedEmergencyHistory,
    action: Action,
) -> BlockPlan {
    let bytes = serde_json::to_vec(&control(policy, state, context, action)).unwrap();
    plan_block(
        policy,
        state,
        context,
        Some(&bytes),
        history,
        &TestVerifier::valid(),
    )
    .unwrap()
}
fn admitted(policy: &Policy) -> BlockPlan {
    apply(
        policy,
        &State::new(policy).unwrap(),
        &context(1),
        &VerifiedEmergencyHistory::empty(),
        Action::Admit {
            plan: migration(policy),
        },
    )
}
fn activate(state: &State, context: &BlockContext) -> Action {
    let pending = state.pending().unwrap();
    Action::Activate {
        plan: pending.plan.clone(),
        admission_receipt_sha256: pending.admission_receipt_sha256.clone(),
        emergency_receipt_sha256: context.emergency_receipt_sha256.clone(),
        evidence_sha256: "ee".repeat(32),
    }
}
fn reject(
    policy: &Policy,
    state: &State,
    context: &BlockContext,
    history: &VerifiedEmergencyHistory,
    control: &Control,
) -> String {
    plan_block(
        policy,
        state,
        context,
        Some(&serde_json::to_vec(control).unwrap()),
        history,
        &TestVerifier::valid(),
    )
    .unwrap_err()
    .to_string()
}
struct EmergencyVerifier;
impl emergency::ControlVerifier for EmergencyVerifier {
    fn verify(
        &self,
        _: &str,
        _: &[u8],
        _: u64,
        _: u64,
        _: u64,
        _: u64,
        _: &[u8],
        _: &[u8],
    ) -> Result<bool> {
        Ok(true)
    }
}
fn emergency_history() -> VerifiedEmergencyHistory {
    // Deserialize the retained v1 qualification contract. New optional v2 fields
    // must not change historical v1 receipt encoding.
    let p: emergency::Policy = serde_json::from_value(serde_json::json!({
        "schema":1,"development_only":true,"chain_id":"development-upgrade-test","release_sha512":"bb".repeat(64),
        "initial_sequence":1,"freeze_authority":{"keys":[{"key_id":"freeze","public_key_hex":"33".repeat(64)}],"threshold":1},
        "resume_authority":{"keys":[{"key_id":"resume","public_key_hex":"55".repeat(64)}],"threshold":1},
        "max_control_bytes":100000,"max_signatures":1,"automatic_transition_policy":"continue_existing"
    })).unwrap();
    let mut state = emergency::State::new(&p).unwrap();
    let mut records = Vec::new();
    for (height, action, key) in [(2, "freeze", "freeze"), (3, "resume", "resume")] {
        let context: emergency::BlockContext = serde_json::from_value(serde_json::json!({
            "height":height,"parent_height":height-1,"parent_app_hash":"cc".repeat(32)
        }))
        .unwrap();
        let ctrl: emergency::Control = serde_json::from_value(serde_json::json!({
            "kind":emergency::CONTROL_KIND,"payload":{"schema":1,"chain_id":p.chain_id,"release_sha512":p.release_sha512,
                "action":action,"sequence":state.next_sequence(),"parent_height":height-1,"parent_app_hash":"cc".repeat(32),"target_height":height},
            "signatures":[{"key_id":key,"signature_hex":"66".repeat(SIGNATURE_BYTES)}]
        })).unwrap();
        let planned = emergency::plan_block(
            &p,
            &state,
            &context,
            Some(&serde_json::to_vec(&ctrl).unwrap()),
            &EmergencyVerifier,
        )
        .unwrap();
        records.push((planned.receipt.unwrap(), context));
        state = planned.state;
    }
    VerifiedEmergencyHistory::from_records(&p, &records, &state).unwrap()
}
fn after_history(history: &VerifiedEmergencyHistory) -> BlockContext {
    let mut ctx = context(4);
    ctx.emergency_upgrade_hold = history.upgrade_hold;
    ctx.emergency_frozen = history.frozen;
    ctx.emergency_receipt_sha256 = history.last_receipt_sha256.clone();
    ctx
}

#[test]
fn production_policy_is_disabled() {
    let mut p = policy();
    p.development_only = false;
    assert!(State::new(&p).is_err());
}
#[test]
fn admission_changes_no_active_schema_or_index() {
    let p = policy();
    let out = admitted(&p);
    assert_eq!(out.state.active_schema(), 0);
    assert_eq!(out.state.next_sequence(), p.initial_sequence + 1);
    assert!(out.migration.is_none());
    assert!(out.state.pending().is_some());
}
#[test]
fn activation_backfills_real_receipts_and_preserves_history() {
    let p = policy();
    let initial = admitted(&p);
    let history = emergency_history();
    let ctx = after_history(&history);
    let before = serde_json::to_vec(&history.receipts).unwrap();
    let out = apply(
        &p,
        &initial.state,
        &ctx,
        &history,
        activate(&initial.state, &ctx),
    );
    let m = out.migration.unwrap();
    assert_eq!(m.receipt_count, 2);
    assert_eq!(m.writes.len(), 3);
    for r in &history.receipts {
        assert_eq!(
            m.writes
                .get(index_key(&r.sha256().unwrap()).unwrap().as_bytes())
                .unwrap(),
            &r.sequence().to_be_bytes()
        );
    }
    let marker: IndexState =
        serde_json::from_slice(m.writes.get(INDEX_STATE_KEY.as_bytes()).unwrap()).unwrap();
    assert_eq!(
        marker.activation_receipt_sha256,
        out.receipt.unwrap().sha256().unwrap()
    );
    assert_eq!(before, serde_json::to_vec(&history.receipts).unwrap());
    assert!(history.upgrade_hold);
    assert_eq!(out.state.active_schema(), 1);
    assert_eq!(out.state.active_release_sha512(), p.source_release_sha512);
    assert!(out.state.pending().is_none());
}
#[test]
fn activation_requires_separate_admission_and_sequence() {
    let p = policy();
    let state = State::new(&p).unwrap();
    let ctx = context(1);
    let action = Action::Activate {
        plan: migration(&p),
        admission_receipt_sha256: "11".repeat(32),
        emergency_receipt_sha256: None,
        evidence_sha256: "22".repeat(32),
    };
    assert!(reject(
        &p,
        &state,
        &ctx,
        &VerifiedEmergencyHistory::empty(),
        &control(&p, &state, &ctx, action)
    )
    .contains("No admitted"));
}
#[test]
fn stale_parent_and_missed_height_require_fresh_signed_control() {
    let p = policy();
    let admitted = admitted(&p);
    let ctx = context(2);
    let ctrl = control(&p, &admitted.state, &ctx, activate(&admitted.state, &ctx));
    assert!(reject(
        &p,
        &admitted.state,
        &context(3),
        &VerifiedEmergencyHistory::empty(),
        &ctrl
    )
    .contains("finalized parent/target"));
    let mut changed = ctx.clone();
    changed.parent_app_hash = "77".repeat(32);
    assert!(reject(
        &p,
        &admitted.state,
        &changed,
        &VerifiedEmergencyHistory::empty(),
        &ctrl
    )
    .contains("finalized parent/target"));
}
#[test]
fn freeze_and_same_block_emergency_prevent_activation() {
    let p = policy();
    let a = admitted(&p);
    let mut history = emergency_history();
    let mut ctx = after_history(&history);
    history.frozen = true;
    ctx.emergency_frozen = true;
    let ctrl = control(&p, &a.state, &ctx, activate(&a.state, &ctx));
    assert!(reject(&p, &a.state, &ctx, &history, &ctrl).contains("Emergency control blocks"));
    history.frozen = false;
    ctx.emergency_frozen = false;
    ctx.emergency_control_present = true;
    assert!(reject(&p, &a.state, &ctx, &history, &ctrl).contains("Emergency control blocks"));
}
#[test]
fn global_hold_requires_exact_fresh_candidate_clearance() {
    let p = policy();
    let a = admitted(&p);
    let history = emergency_history();
    let ctx = after_history(&history);
    let mut ctrl = control(&p, &a.state, &ctx, activate(&a.state, &ctx));
    if let Action::Activate {
        emergency_receipt_sha256,
        ..
    } = &mut ctrl.payload.action
    {
        *emergency_receipt_sha256 = None;
    }
    assert!(reject(&p, &a.state, &ctx, &history, &ctrl).contains("emergency clearance differs"));
}
#[test]
fn cancellation_preserves_sequence_and_receipt_chain() {
    let p = policy();
    let a = admitted(&p);
    let pending = a.state.pending().unwrap();
    let out = apply(
        &p,
        &a.state,
        &context(2),
        &VerifiedEmergencyHistory::empty(),
        Action::Cancel {
            plan_sha256: pending.plan.sha256().unwrap(),
            admission_receipt_sha256: pending.admission_receipt_sha256.clone(),
        },
    );
    assert!(out.state.pending().is_none());
    assert!(out.migration.is_none());
    assert_eq!(out.state.active_schema(), 0);
    assert_eq!(out.state.next_sequence(), p.initial_sequence + 2);
    assert_eq!(
        out.receipt.unwrap().previous_receipt_sha256,
        a.state.last_receipt_sha256
    );
}
#[test]
fn no_control_never_activates_pending_plan() {
    let p = policy();
    let a = admitted(&p);
    let out = plan_block(
        &p,
        &a.state,
        &context(99),
        None,
        &VerifiedEmergencyHistory::empty(),
        &TestVerifier::valid(),
    )
    .unwrap();
    assert_eq!(out.state, a.state);
    assert!(out.receipt.is_none());
    assert!(out.migration.is_none());
}
#[test]
fn target_candidate_and_compiled_migration_are_exact() {
    let p = policy();
    let s = State::new(&p).unwrap();
    let ctx = context(1);
    for field in 0..3 {
        let mut plan = migration(&p);
        match field {
            0 => plan.target_release_sha512 = "77".repeat(64),
            1 => plan.migration_id = "external-command".into(),
            _ => plan.migration_sha256 = "77".repeat(32),
        };
        assert!(!reject(
            &p,
            &s,
            &ctx,
            &VerifiedEmergencyHistory::empty(),
            &control(&p, &s, &ctx, Action::Admit { plan })
        )
        .is_empty());
    }
}
#[test]
fn signatures_require_distinct_keys_and_full_threshold() {
    let p = policy();
    let s = State::new(&p).unwrap();
    let ctx = context(1);
    let mut ctrl = control(
        &p,
        &s,
        &ctx,
        Action::Admit {
            plan: migration(&p),
        },
    );
    ctrl.signatures.pop();
    assert!(reject(&p, &s, &ctx, &VerifiedEmergencyHistory::empty(), &ctrl).contains("threshold"));
    ctrl.signatures.push(ctrl.signatures[0].clone());
    assert!(
        reject(&p, &s, &ctx, &VerifiedEmergencyHistory::empty(), &ctrl).contains("strictly sorted")
    );
}
#[test]
fn invalid_signature_is_refusal_and_helper_failure_propagates() {
    let p = policy();
    let s = State::new(&p).unwrap();
    let ctx = context(1);
    let ctrl = control(
        &p,
        &s,
        &ctx,
        Action::Admit {
            plan: migration(&p),
        },
    );
    let bytes = serde_json::to_vec(&ctrl).unwrap();
    for outcome in [0, 2] {
        let verifier = TestVerifier {
            outcome,
            calls: AtomicUsize::new(0),
        };
        let err = plan_block(
            &p,
            &s,
            &ctx,
            Some(&bytes),
            &VerifiedEmergencyHistory::empty(),
            &verifier,
        )
        .unwrap_err();
        assert_eq!(is_rejection(&err), outcome == 0);
        assert_eq!(verifier.calls.load(Ordering::Relaxed), 1);
    }
}
#[test]
fn policy_identity_and_epoch_cannot_be_changed_by_payload() {
    let p = policy();
    let s = State::new(&p).unwrap();
    let ctx = context(1);
    for field in 0..4 {
        let mut ctrl = control(
            &p,
            &s,
            &ctx,
            Action::Admit {
                plan: migration(&p),
            },
        );
        match field {
            0 => ctrl.payload.genesis_sha256 = "77".repeat(32),
            1 => ctrl.payload.chain_id = "other".into(),
            2 => ctrl.payload.authority_epoch += 1,
            _ => ctrl.payload.source_release_sha512 = "77".repeat(64),
        };
        assert!(
            reject(&p, &s, &ctx, &VerifiedEmergencyHistory::empty(), &ctrl)
                .contains("identity/policy")
        );
    }
}
#[test]
fn byte_count_and_write_bounds_apply_before_any_commit() {
    let p = policy();
    let history = emergency_history();
    let ctx = after_history(&history);
    for bound in 0..3 {
        let mut plan = migration(&p);
        match bound {
            0 => plan.bounds.max_receipts = 1,
            1 => plan.bounds.max_receipt_bytes = 1,
            _ => plan.bounds.max_write_bytes = 1,
        };
        let a = apply(
            &p,
            &State::new(&p).unwrap(),
            &context(1),
            &VerifiedEmergencyHistory::empty(),
            Action::Admit { plan },
        );
        let before = a.state.clone();
        let ctrl = control(&p, &a.state, &ctx, activate(&a.state, &ctx));
        assert!(reject(&p, &a.state, &ctx, &history, &ctrl).contains("limit"));
        assert_eq!(a.state, before);
    }
}
#[test]
fn replay_reconstructs_receipts_and_checks_actual_context() {
    let p = policy();
    let s = State::new(&p).unwrap();
    let a = admitted(&p);
    let r = a.receipt.unwrap();
    let out = replay_record(
        &p,
        &s,
        &r,
        &context(1),
        &VerifiedEmergencyHistory::empty(),
        Some(&TestVerifier::valid()),
    )
    .unwrap();
    assert_eq!(out.state, a.state);
    assert!(replay_record(
        &p,
        &s,
        &r,
        &context(2),
        &VerifiedEmergencyHistory::empty(),
        None
    )
    .is_err());
    let mut altered = r.clone();
    altered.previous_receipt_sha256 = Some("88".repeat(32));
    assert!(replay_record(
        &p,
        &s,
        &altered,
        &context(1),
        &VerifiedEmergencyHistory::empty(),
        None
    )
    .is_err());
}
#[test]
fn activation_replay_recomputes_mapping_and_source_digests() {
    let p = policy();
    let a = admitted(&p);
    let history = emergency_history();
    let ctx = after_history(&history);
    let out = apply(&p, &a.state, &ctx, &history, activate(&a.state, &ctx));
    let mut receipt = out.receipt.clone().unwrap();
    let replay = replay_record(&p, &a.state, &receipt, &ctx, &history, None).unwrap();
    assert_eq!(replay.state, out.state);
    assert_eq!(
        replay.migration.unwrap().writes,
        out.migration.unwrap().writes
    );
    receipt.migration_index_digest = Some("99".repeat(32));
    assert!(replay_record(&p, &a.state, &receipt, &ctx, &history, None).is_err());
}
#[test]
fn activated_schema_cannot_run_migration_twice() {
    let p = policy();
    let a = admitted(&p);
    let ctx = context(2);
    let out = apply(
        &p,
        &a.state,
        &ctx,
        &VerifiedEmergencyHistory::empty(),
        activate(&a.state, &ctx),
    );
    let ctrl = control(
        &p,
        &out.state,
        &context(3),
        Action::Admit {
            plan: migration(&p),
        },
    );
    assert!(reject(
        &p,
        &out.state,
        &context(3),
        &VerifiedEmergencyHistory::empty(),
        &ctrl
    )
    .contains("activated upgrade"));
}
#[test]
fn canonical_encoding_rejects_whitespace_and_unknown_fields() {
    let p = policy();
    let s = State::new(&p).unwrap();
    let ctrl = control(
        &p,
        &s,
        &context(1),
        Action::Admit {
            plan: migration(&p),
        },
    );
    assert!(decode_control(&p, &serde_json::to_vec_pretty(&ctrl).unwrap()).is_err());
    let mut value = serde_json::to_value(ctrl).unwrap();
    value["authority_override"] = serde_json::json!(true);
    assert!(decode_control(&p, &serde_json::to_vec(&value).unwrap()).is_err());
    assert!(index_key(&"AB".repeat(32)).is_err());
}
#[test]
fn state_cannot_claim_activation_without_receipt() {
    let p = policy();
    let mut s = State::new(&p).unwrap();
    s.active_schema = 1;
    assert!(s.validate(&p).is_err());
    let mut s = admitted(&p).state;
    s.next_sequence = p.initial_sequence;
    assert!(s.validate(&p).is_err());
}
#[test]
fn sequence_exhaustion_does_not_strand_admitted_plan() {
    let mut p = policy();
    p.initial_sequence = u64::MAX - 1;
    let s = State::new(&p).unwrap();
    let ctx = context(1);
    let ctrl = control(
        &p,
        &s,
        &ctx,
        Action::Admit {
            plan: migration(&p),
        },
    );
    assert!(reject(&p, &s, &ctx, &VerifiedEmergencyHistory::empty(), &ctrl).contains("preserve"));
}
