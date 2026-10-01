//! Handover schema 2 (production activation v1, A3): the upgrade custodians'
//! three-of-five authority, anchored windows and the activation notice.
use super::*;
use std::sync::Mutex;

/// Accepts every signature and records the window it was asked about.
#[derive(Default)]
struct WindowVerifier {
    windows: Mutex<Vec<(u64, u64, u64)>>,
}
impl Verifier for WindowVerifier {
    fn verify(
        &self,
        _: &str,
        key: &[u8],
        _: u64,
        current: u64,
        before: u64,
        after: u64,
        artifact: &[u8],
        signature: &[u8],
    ) -> Result<bool> {
        assert_eq!(key.len(), KEY_BYTES);
        assert_eq!(signature.len(), SIGNATURE_BYTES);
        assert!(
            artifact.starts_with(b"DYTALLIX/RELEASE-HANDOVER/v2\0")
                || artifact.starts_with(b"DYTALLIX/CHAIN-UPGRADE/v2\0")
                || artifact.starts_with(b"DYTALLIX/RELEASE-RESTART/v1\0")
        );
        self.windows.lock().unwrap().push((current, before, after));
        Ok(true)
    }
}

fn authority() -> emergency::AuthorityPolicy {
    let mut keys: Vec<_> = (1..=5u8)
        .map(|i| {
            let public = [0xb0 + i; KEY_BYTES];
            emergency::AuthorityKey {
                key_id: hash(&public),
                public_key_hex: hex::encode(public),
            }
        })
        .collect();
    keys.sort_by(|a, b| a.key_id.cmp(&b.key_id));
    emergency::AuthorityPolicy { keys, threshold: 3 }
}
fn policy() -> Policy {
    Policy {
        schema: 2,
        development_only: true,
        chain_id: "handover-v2-test".into(),
        genesis_sha256: "aa".repeat(32),
        initial_release_sha512: "bb".repeat(64),
        initial_schema: 0,
        authority_epoch: 3,
        authority: authority(),
        initial_sequence: 10,
        max_control_bytes: 400_000,
        max_signatures: 5,
        v2: Some(PolicyV2 {
            min_notice_blocks: 10,
            max_validity_blocks: 4,
            max_anchor_age_blocks: 6,
        }),
    }
}
/// A block whose controls name the anchor two blocks back.
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
        finalized_anchor: Some(emergency::FinalizedAnchor {
            height: height - 2,
            app_hash: "ab".repeat(32),
        }),
    }
}
fn plan(migration_sha256: Option<String>) -> ReleasePlan {
    ReleasePlan {
        target_release_sha512: "dd".repeat(64),
        authorization_sha256: "ee".repeat(32),
        transition: match migration_sha256 {
            Some(migration_sha256) => Transition::ReceiptIndexV1 { migration_sha256 },
            None => Transition::SchemaPreserving { schema: 0 },
        },
    }
}
/// Signed by three keys over the window from one block after the anchor.
fn control(policy: &Policy, state: &State, context: &BlockContext, action: Action) -> Control {
    let anchor = context.finalized_anchor.as_ref().unwrap();
    Control {
        kind: CONTROL_KIND_V2.into(),
        payload: Payload {
            schema: 2,
            chain_id: policy.chain_id.clone(),
            genesis_sha256: policy.genesis_sha256.clone(),
            policy_sha256: policy.sha256().unwrap(),
            source_release_sha512: state.active_release_sha512.clone(),
            authority_epoch: policy.authority_epoch,
            sequence: state.next_sequence,
            parent_height: anchor.height,
            parent_app_hash: anchor.app_hash.clone(),
            target_height: anchor.height + 1,
            action,
            v2: Some(PayloadV2 {
                not_before_height: anchor.height + 1,
                not_after_height: anchor.height + 4,
            }),
        },
        signatures: policy.authority.keys[..3]
            .iter()
            .map(|key| emergency::ControlSignature {
                key_id: key.key_id.clone(),
                signature_hex: "33".repeat(SIGNATURE_BYTES),
            })
            .collect(),
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
        &WindowVerifier::default(),
    )
}
fn refusal(policy: &Policy, state: &State, context: &BlockContext, control: &Control) -> String {
    format!(
        "{:#}",
        run(policy, state, context, control, None).unwrap_err()
    )
}
fn admitted(plan: ReleasePlan) -> (Policy, State) {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let context = context(5);
    let control = control(&policy, &state, &context, Action::Admit { plan });
    let state = run(&policy, &state, &context, &control, None)
        .unwrap()
        .state;
    (policy, state)
}
fn activate(
    state: &State,
    context: &BlockContext,
    prepared: Option<&PreparedUpgradeOutcome>,
) -> Action {
    let pending = state.pending().unwrap();
    Action::Activate {
        plan: pending.plan.clone(),
        admission_receipt_sha256: pending.admission_receipt_sha256.clone(),
        emergency_receipt_sha256: context.emergency_receipt_sha256.clone(),
        evidence_sha256: "ff".repeat(32),
        upgrade_activation_sha256: prepared.map(|p| p.control_sha256.clone()),
    }
}

#[test]
fn schema_two_policy_is_the_three_of_five_custodians_with_bounds() {
    assert!(policy().validate().is_ok());
    let cases: [(&str, fn(&mut Policy)); 7] = [
        ("schema 1 with bounds", |p| p.schema = 1),
        ("schema 2 without bounds", |p| p.v2 = None),
        ("two of five", |p| p.authority.threshold = 2),
        ("four keys", |p| {
            p.authority.keys.pop();
        }),
        ("named key", |p| p.authority.keys[0].key_id = "a".into()),
        ("no notice", |p| {
            p.v2.as_mut().unwrap().min_notice_blocks = 0
        }),
        ("no anchor age", |p| {
            p.v2.as_mut().unwrap().max_anchor_age_blocks = 0
        }),
    ];
    for (name, change) in cases {
        let mut p = policy();
        change(&mut p);
        assert!(p.validate().is_err(), "accepted {name}");
    }
    // A schema 1 policy encodes exactly as before: no `v2` member.
    let mut v1 = policy();
    v1.schema = 1;
    v1.v2 = None;
    assert!(!String::from_utf8(serde_json::to_vec(&v1).unwrap())
        .unwrap()
        .contains("\"v2\""));
    let mut value = serde_json::to_value(policy()).unwrap();
    value["v2"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<Policy>(value).is_err());
}

#[test]
fn admission_signs_a_window_over_a_finalized_anchor() {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let context = context(5);
    let control = control(
        &policy,
        &state,
        &context,
        Action::Admit { plan: plan(None) },
    );
    let verifier = WindowVerifier::default();
    let result = plan_block(
        &policy,
        &state,
        &context,
        Some(&serde_json::to_vec(&control).unwrap()),
        None,
        &verifier,
    )
    .unwrap();
    assert_eq!(*verifier.windows.lock().unwrap(), vec![(5, 4, 7); 3]);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.schema, 2);
    assert_eq!(result.state.pending().unwrap().admitted_height, 5);
    // The receipt's context records the anchor; replay checks it.
    let mut other = context.clone();
    other.finalized_anchor.as_mut().unwrap().app_hash = "00".repeat(32);
    assert!(replay_record(&policy, &state, &receipt, &other, None, None).is_err());
    assert_eq!(
        replay_record(&policy, &state, &receipt, &context, None, None)
            .unwrap()
            .state,
        result.state
    );
}

#[test]
fn window_anchor_and_kind_are_enforced() {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let context = context(5);
    let base = control(
        &policy,
        &state,
        &context,
        Action::Admit { plan: plan(None) },
    );
    let mut late = context.clone();
    late.height = 8;
    late.parent_height = 7;
    assert!(refusal(&policy, &state, &late, &base).contains("outside its validity window"));
    let cases: [(&str, fn(&mut Control), &str); 6] = [
        (
            "wide window",
            |c| c.payload.v2.as_mut().unwrap().not_after_height += 1,
            "window bound",
        ),
        (
            "target off window",
            |c| c.payload.target_height += 1,
            "window bound",
        ),
        (
            "other anchor",
            |c| c.payload.parent_app_hash = "00".repeat(32),
            "anchor binding",
        ),
        ("no window", |c| c.payload.v2 = None, "identity/policy"),
        ("schema 1", |c| c.payload.schema = 1, "identity/policy"),
        (
            "two signers",
            |c| {
                c.signatures.pop();
            },
            "threshold",
        ),
    ];
    for (name, change, expected) in cases {
        let mut control = base.clone();
        change(&mut control);
        let error = refusal(&policy, &state, &context, &control);
        assert!(error.contains(expected), "{name}: {error}");
    }
    let mut missing = context.clone();
    missing.finalized_anchor = None;
    assert!(refusal(&policy, &state, &missing, &base).contains("anchor required"));
    let mut v1_kind = base.clone();
    v1_kind.kind = CONTROL_KIND.into();
    assert!(decode_control(&policy, &serde_json::to_vec(&v1_kind).unwrap()).is_err());
}

#[test]
fn activation_waits_for_the_notice() {
    let (policy, state) = admitted(plan(None));
    // Admitted at 5 with notice 10: the earliest activation is 15.
    let early = context(14);
    let control_early = control(&policy, &state, &early, activate(&state, &early, None));
    assert!(refusal(&policy, &state, &early, &control_early).contains("precedes its notice"));
    let on_time = context(15);
    let activation = control(&policy, &state, &on_time, activate(&state, &on_time, None));
    let result = run(&policy, &state, &on_time, &activation, None).unwrap();
    assert_eq!(result.state.active_release_sha512(), "dd".repeat(64));
    assert_eq!(result.activated_release_sha512, Some("dd".repeat(64)));
    // Cancellation needs no notice.
    let (policy, state) = admitted(plan(None));
    let pending = state.pending().unwrap();
    let later = context(6);
    let cancel = control(
        &policy,
        &state,
        &later,
        Action::Cancel {
            plan_sha256: pending.plan.sha256().unwrap(),
            admission_receipt_sha256: pending.admission_receipt_sha256.clone(),
        },
    );
    assert!(run(&policy, &state, &later, &cancel, None)
        .unwrap()
        .state
        .pending()
        .is_none());
}

/// A v2 upgrade admitted at 5 and activated at 15 in the same block as the
/// handover, under the same authority.
fn prepared_v2_upgrade(handover: &Policy, at: &BlockContext) -> PreparedUpgradeOutcome {
    let p = upgrade::v2::Policy {
        schema: 2,
        chain_id: handover.chain_id.clone(),
        genesis_sha256: handover.genesis_sha256.clone(),
        initial_release_sha512: handover.initial_release_sha512.clone(),
        authority_epoch: handover.authority_epoch,
        authority: handover.authority.clone(),
        initial_sequence: 1,
        max_control_bytes: handover.max_control_bytes,
        max_signatures: 5,
        migration_bounds: upgrade::v2::MigrationBounds {
            max_receipts: 10,
            max_receipt_bytes: 100_000,
            max_write_bytes: 100_000,
        },
        min_notice_blocks: 10,
        max_validity_blocks: 4,
        max_anchor_age_blocks: 6,
    };
    let plan = upgrade::v2::MigrationPlan {
        target_release_sha512: p.initial_release_sha512.clone(),
        migration_id: upgrade::MIGRATION_ID.into(),
        migration_sha256: upgrade::v2::migration_sha256(),
        source_schema: 0,
        target_schema: 1,
        bounds: p.migration_bounds.clone(),
        authorization_sha256: "ff".repeat(32),
    };
    let block = |c: &BlockContext| upgrade::v2::BlockContext {
        height: c.height,
        parent_height: c.parent_height,
        parent_app_hash: c.parent_app_hash.clone(),
        active_release_sha512: p.initial_release_sha512.clone(),
        finalized_anchor: c.finalized_anchor.clone(),
        emergency_frozen: false,
        emergency_control_present: false,
        emergency_upgrade_hold: false,
        emergency_receipt_sha256: None,
    };
    let make = |sequence, c: &BlockContext, action| {
        let anchor = c.finalized_anchor.as_ref().unwrap();
        upgrade::v2::Control {
            kind: upgrade::v2::CONTROL_KIND.into(),
            payload: upgrade::v2::Payload {
                schema: 2,
                chain_id: p.chain_id.clone(),
                genesis_sha256: p.genesis_sha256.clone(),
                policy_sha256: p.sha256().unwrap(),
                source_release_sha512: p.initial_release_sha512.clone(),
                authority_epoch: p.authority_epoch,
                sequence,
                anchor_height: anchor.height,
                anchor_app_hash: anchor.app_hash.clone(),
                not_before_height: anchor.height + 1,
                not_after_height: anchor.height + 4,
                action,
            },
            signatures: p.authority.keys[..3]
                .iter()
                .map(|key| emergency::ControlSignature {
                    key_id: key.key_id.clone(),
                    signature_hex: "44".repeat(SIGNATURE_BYTES),
                })
                .collect(),
        }
    };
    let history = upgrade::v2::VerifiedEmergencyHistory::empty();
    let verifier = WindowVerifier::default();
    let admit_at = context(5);
    let first = make(
        1,
        &admit_at,
        upgrade::v2::Action::Admit { plan: plan.clone() },
    );
    let first = upgrade::v2::plan_block(
        &p,
        &upgrade::v2::State::new(&p).unwrap(),
        &block(&admit_at),
        Some(&serde_json::to_vec(&first).unwrap()),
        &history,
        &verifier,
    )
    .unwrap();
    let second = make(
        2,
        at,
        upgrade::v2::Action::Activate {
            plan,
            admission_receipt_sha256: first.receipt.unwrap().sha256().unwrap(),
            emergency_receipt_sha256: None,
            evidence_sha256: "ee".repeat(32),
        },
    );
    let raw = serde_json::to_vec(&second).unwrap();
    let second = upgrade::v2::plan_block(
        &p,
        &first.state,
        &block(at),
        Some(&raw),
        &history,
        &verifier,
    )
    .unwrap();
    PreparedUpgradeOutcome::from_verified_upgrade(&raw, &second.into()).unwrap()
}

#[test]
fn a_migrating_handover_pairs_with_a_v2_upgrade_only() {
    // Schema 2 names the v2 implementation; the v1 digest is refused.
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let admission = context(5);
    let v1 = control(
        &policy,
        &state,
        &admission,
        Action::Admit {
            plan: plan(Some(upgrade::v1::migration_sha256())),
        },
    );
    assert!(refusal(&policy, &state, &admission, &v1).contains("migration version"));
    let (policy, state) = admitted(plan(Some(upgrade::v2::migration_sha256())));
    let at = context(15);
    let prepared = prepared_v2_upgrade(&policy, &at);
    let activation = control(&policy, &state, &at, activate(&state, &at, Some(&prepared)));
    let result = run(&policy, &state, &at, &activation, Some(&prepared)).unwrap();
    assert_eq!(result.state.active_schema(), 1);
    assert_eq!(result.state.active_release_sha512(), "dd".repeat(64));
    // An unpaired activation is refused.
    assert!(run(&policy, &state, &at, &activation, None).is_err());
}

#[test]
fn a_restart_keeps_its_exact_height_under_schema_two() {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let payload = restart::Payload {
        schema: 1,
        chain_id: policy.chain_id.clone(),
        genesis_sha256: policy.genesis_sha256.clone(),
        policy_sha256: policy.sha256().unwrap(),
        authority_epoch: policy.authority_epoch,
        sequence: state.next_sequence,
        source_release_sha512: state.active_release_sha512.clone(),
        target_release_sha512: "dd".repeat(64),
        state_schema: state.active_schema,
        parent_height: 7,
        parent_app_hash: "cc".repeat(32),
        halted_height: 8,
        halted_block_hash: None,
        emergency_receipt_sha256: None,
        pending_admission_receipt_sha256: None,
        evidence_sha256: "ff".repeat(32),
    };
    let authorization = restart::Authorization {
        kind: restart::KIND.into(),
        payload,
        signatures: policy.authority.keys[..3]
            .iter()
            .map(|key| emergency::ControlSignature {
                key_id: key.key_id.clone(),
                signature_hex: "55".repeat(SIGNATURE_BYTES),
            })
            .collect(),
    };
    let verifier = WindowVerifier::default();
    let verified = restart::verify(
        &policy,
        &state,
        &restart::Checkpoint {
            height: 7,
            app_hash: "cc".repeat(32),
            emergency_receipt_sha256: None,
        },
        &serde_json::to_vec(&authorization).unwrap(),
        &verifier,
    )
    .unwrap();
    assert_eq!(verified.target_release_sha512(), "dd".repeat(64));
    assert_eq!(*verifier.windows.lock().unwrap(), vec![(8, 8, 8); 3]);
}
