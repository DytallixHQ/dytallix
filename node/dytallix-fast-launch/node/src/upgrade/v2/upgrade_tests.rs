use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const CHAIN: &str = "development-upgrade-v2-test";
const RELEASE: &str = "bb";

/// Records each call's window; returns a fixed outcome.
struct TestVerifier {
    outcome: u8,
    calls: AtomicUsize,
    windows: Mutex<Vec<(u64, u64, u64)>>,
}
impl TestVerifier {
    fn with(outcome: u8) -> Self {
        Self {
            outcome,
            calls: AtomicUsize::new(0),
            windows: Mutex::new(Vec::new()),
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
        self.windows.lock().unwrap().push((current, before, after));
        assert_eq!(chain, CHAIN);
        assert_eq!(key.len(), KEY_BYTES);
        assert!(sequence > 0);
        assert!(artifact.starts_with(b"DYTALLIX/CHAIN-UPGRADE/v2\0"));
        assert_eq!(signature.len(), SIGNATURE_BYTES);
        match self.outcome {
            0 => Ok(false),
            1 => Ok(true),
            _ => anyhow::bail!("helper unavailable"),
        }
    }
}

fn keys() -> Vec<emergency::AuthorityKey> {
    let mut keys: Vec<_> = (1..=5u8)
        .map(|i| {
            let public = [0xa0 + i; KEY_BYTES];
            emergency::AuthorityKey {
                key_id: hash(&public),
                public_key_hex: hex::encode(public),
            }
        })
        .collect();
    keys.sort_by(|a, b| a.key_id.cmp(&b.key_id));
    keys
}
fn policy() -> Policy {
    Policy {
        schema: 2,
        chain_id: CHAIN.into(),
        genesis_sha256: "aa".repeat(32),
        initial_release_sha512: RELEASE.repeat(64),
        authority_epoch: 7,
        authority: emergency::AuthorityPolicy {
            keys: keys(),
            threshold: 3,
        },
        initial_sequence: 3,
        max_control_bytes: 300_000,
        max_signatures: 5,
        migration_bounds: MigrationBounds {
            max_receipts: 10,
            max_receipt_bytes: 1_000_000,
            max_write_bytes: 10_000,
        },
        min_notice_blocks: 10,
        max_validity_blocks: 4,
        max_anchor_age_blocks: 6,
    }
}
/// A block at `height` whose control names the anchor two blocks back.
fn context(height: u64) -> BlockContext {
    BlockContext {
        height,
        parent_height: height - 1,
        parent_app_hash: "cc".repeat(32),
        active_release_sha512: RELEASE.repeat(64),
        finalized_anchor: Some(emergency::FinalizedAnchor {
            height: height - 2,
            app_hash: "ab".repeat(32),
        }),
        emergency_frozen: false,
        emergency_control_present: false,
        emergency_upgrade_hold: false,
        emergency_receipt_sha256: None,
    }
}
fn migration(policy: &Policy) -> MigrationPlan {
    MigrationPlan {
        target_release_sha512: policy.initial_release_sha512.clone(),
        migration_id: MIGRATION_ID.into(),
        migration_sha256: migration_sha256(),
        source_schema: 0,
        target_schema: 1,
        bounds: policy.migration_bounds.clone(),
        authorization_sha256: "dd".repeat(32),
    }
}
/// A control signed by the first three keys over the window from one block
/// after the anchor to three blocks later.
fn control(policy: &Policy, state: &State, context: &BlockContext, action: Action) -> Control {
    let anchor = context.finalized_anchor.as_ref().unwrap();
    Control {
        kind: CONTROL_KIND.into(),
        payload: Payload {
            schema: 2,
            chain_id: policy.chain_id.clone(),
            genesis_sha256: policy.genesis_sha256.clone(),
            policy_sha256: policy.sha256().unwrap(),
            source_release_sha512: context.active_release_sha512.clone(),
            authority_epoch: policy.authority_epoch,
            sequence: state.next_sequence(),
            anchor_height: anchor.height,
            anchor_app_hash: anchor.app_hash.clone(),
            not_before_height: anchor.height + 1,
            not_after_height: anchor.height + 4,
            action,
        },
        signatures: policy.authority.keys[..3]
            .iter()
            .map(|k| emergency::ControlSignature {
                key_id: k.key_id.clone(),
                signature_hex: "44".repeat(SIGNATURE_BYTES),
            })
            .collect(),
    }
}
fn run(
    policy: &Policy,
    state: &State,
    context: &BlockContext,
    control: &Control,
    verifier: &TestVerifier,
) -> Result<BlockPlan> {
    plan_block(
        policy,
        state,
        context,
        Some(&serde_json::to_vec(control).unwrap()),
        &VerifiedEmergencyHistory::empty(),
        verifier,
    )
}
fn apply(policy: &Policy, state: &State, context: &BlockContext, action: Action) -> BlockPlan {
    run(
        policy,
        state,
        context,
        &control(policy, state, context, action),
        &TestVerifier::with(1),
    )
    .unwrap()
}
fn admitted(policy: &Policy) -> BlockPlan {
    apply(
        policy,
        &State::new(policy).unwrap(),
        &context(5),
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
fn refusal(policy: &Policy, state: &State, context: &BlockContext, control: &Control) -> String {
    format!(
        "{:#}",
        run(policy, state, context, control, &TestVerifier::with(1)).unwrap_err()
    )
}

#[test]
fn policy_is_three_of_five_with_key_digests_and_explicit_bounds() {
    assert!(policy().validate().is_ok());
    let cases: [(&str, fn(&mut Policy)); 9] = [
        ("schema 1", |p| p.schema = 1),
        ("two of five", |p| p.authority.threshold = 2),
        ("four keys", |p| {
            p.authority.keys.pop();
        }),
        ("unsorted", |p| p.authority.keys.swap(0, 1)),
        ("named key", |p| {
            p.authority.keys[0].key_id = "upgrade-a".into()
        }),
        ("signature bound", |p| p.max_signatures = 2),
        ("no notice", |p| p.min_notice_blocks = 0),
        ("no window", |p| p.max_validity_blocks = 0),
        ("no anchor age", |p| p.max_anchor_age_blocks = 0),
    ];
    for (name, change) in cases {
        let mut p = policy();
        change(&mut p);
        assert!(p.validate().is_err(), "accepted {name}");
    }
    // Strict codec: no development flag, no unknown field.
    let mut value = serde_json::to_value(policy()).unwrap();
    value["development_only"] = true.into();
    assert!(serde_json::from_value::<Policy>(value).is_err());
}

#[test]
fn admission_binds_the_window_and_the_verifier_sees_it() {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let context = context(5);
    let verifier = TestVerifier::with(1);
    let control = control(
        &policy,
        &state,
        &context,
        Action::Admit {
            plan: migration(&policy),
        },
    );
    let plan = run(&policy, &state, &context, &control, &verifier).unwrap();
    assert_eq!(verifier.calls.load(Ordering::Relaxed), 3);
    assert!(verifier
        .windows
        .lock()
        .unwrap()
        .iter()
        .all(|window| *window == (5, 4, 7)));
    assert_eq!(plan.state.pending().unwrap().admitted_height, 5);
    assert_eq!(plan.state.active_schema(), 0);
    assert!(plan.migration.is_none());
    assert_eq!(plan.receipt.unwrap().schema, 2);
    // The same control commits anywhere in its window, never outside it.
    for height in [4, 6, 7] {
        let mut later = super::tests::context(height);
        later.finalized_anchor = context.finalized_anchor.clone();
        if later.parent_height == 3 {
            later.parent_app_hash = "ab".repeat(32);
        }
        assert!(
            run(&policy, &state, &later, &control, &verifier).is_ok(),
            "{height}"
        );
    }
    let mut late = super::tests::context(8);
    late.finalized_anchor = context.finalized_anchor.clone();
    assert!(refusal(&policy, &state, &late, &control).contains("outside its validity window"));
}

#[test]
fn window_and_anchor_bounds_are_enforced() {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let context = context(5);
    let base = control(
        &policy,
        &state,
        &context,
        Action::Admit {
            plan: migration(&policy),
        },
    );
    let cases: [(&str, fn(&mut Payload), &str); 5] = [
        (
            "wide window",
            |p| p.not_after_height = p.not_before_height + 4,
            "window bound",
        ),
        (
            "inverted window",
            |p| p.not_after_height = p.not_before_height - 1,
            "window bound",
        ),
        (
            "other anchor hash",
            |p| p.anchor_app_hash = "00".repeat(32),
            "anchor binding",
        ),
        (
            "other anchor height",
            |p| p.anchor_height -= 1,
            "anchor binding",
        ),
        (
            "anchor inside window",
            |p| {
                p.not_before_height = p.anchor_height;
                p.not_after_height = p.anchor_height + 3;
            },
            "anchor age",
        ),
    ];
    for (name, change, expected) in cases {
        let mut control = base.clone();
        change(&mut control.payload);
        let error = refusal(&policy, &state, &context, &control);
        assert!(error.contains(expected), "{name}: {error}");
    }
    // An anchor older than its age bound.
    let mut old = context.clone();
    old.finalized_anchor.as_mut().unwrap().height = 0;
    let mut stale = control(
        &policy,
        &state,
        &old,
        Action::Admit {
            plan: migration(&policy),
        },
    );
    stale.payload.not_before_height = 4;
    stale.payload.not_after_height = 7;
    let mut at = old.clone();
    at.height = 7;
    at.parent_height = 6;
    assert!(refusal(&policy, &state, &at, &stale).contains("anchor age"));
    // The adapter must supply the anchor it read from committed history.
    let mut missing = context.clone();
    missing.finalized_anchor = None;
    assert!(refusal(&policy, &state, &missing, &base).contains("anchor required"));
}

#[test]
fn identity_release_and_signers_are_bound() {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let context = context(5);
    let base = control(
        &policy,
        &state,
        &context,
        Action::Admit {
            plan: migration(&policy),
        },
    );
    let cases: [(&str, fn(&mut Control)); 7] = [
        ("schema", |c| c.payload.schema = 1),
        ("chain", |c| c.payload.chain_id = "other".into()),
        ("policy", |c| c.payload.policy_sha256 = "00".repeat(32)),
        ("release", |c| {
            c.payload.source_release_sha512 = "ee".repeat(64)
        }),
        ("epoch", |c| c.payload.authority_epoch = 8),
        ("sequence", |c| c.payload.sequence = 4),
        ("two signatures", |c| {
            c.signatures.pop();
        }),
    ];
    for (name, change) in cases {
        let mut control = base.clone();
        change(&mut control);
        assert!(
            run(&policy, &state, &context, &control, &TestVerifier::with(1)).is_err(),
            "accepted {name}"
        );
    }
    // The migration runs in the active release, which handovers change.
    let mut moved = context.clone();
    moved.active_release_sha512 = "ee".repeat(64);
    let mut other = base.clone();
    other.payload.source_release_sha512 = moved.active_release_sha512.clone();
    assert!(refusal(&policy, &state, &moved, &other).contains("active release"));
    // A refused signature is a rejection; a failed helper is not.
    let error = run(&policy, &state, &context, &base, &TestVerifier::with(0)).unwrap_err();
    assert!(is_rejection(&error));
    let error = run(&policy, &state, &context, &base, &TestVerifier::with(2)).unwrap_err();
    assert!(!is_rejection(&error));
}

#[test]
fn activation_waits_for_the_notice_and_migrates() {
    let policy = policy();
    let admission = admitted(&policy);
    let state = admission.state;
    // Admitted at 5 with notice 10: the earliest activation is 15.
    let early = context(14);
    let control_early = control(&policy, &state, &early, activate(&state, &early));
    assert!(refusal(&policy, &state, &early, &control_early).contains("precedes its notice"));
    let on_time = context(15);
    let plan = apply(&policy, &state, &on_time, activate(&state, &on_time));
    assert_eq!(plan.state.active_schema(), 1);
    assert!(plan.state.pending().is_none());
    let migration = plan.migration.as_ref().unwrap();
    let marker: IndexState =
        serde_json::from_slice(&migration.writes[INDEX_STATE_KEY.as_bytes()]).unwrap();
    assert_eq!(marker.activation_height, 15);
    assert_eq!(
        plan.state.activation_receipt_sha256(),
        Some(plan.receipt.as_ref().unwrap().sha256().unwrap().as_str())
    );
    // Replay of the committed receipt gives the same plan.
    let receipt = plan.receipt.clone().unwrap();
    let replayed = replay_record(
        &policy,
        &state,
        &receipt,
        &on_time,
        &VerifiedEmergencyHistory::empty(),
        Some(&TestVerifier::with(1)),
    )
    .unwrap();
    assert_eq!(replayed.state, plan.state);
    let mut other = on_time.clone();
    other.parent_app_hash = "ff".repeat(32);
    assert!(replay_record(
        &policy,
        &state,
        &receipt,
        &other,
        &VerifiedEmergencyHistory::empty(),
        None
    )
    .is_err());
}

#[test]
fn cancellation_needs_no_notice_and_frees_the_plan() {
    let policy = policy();
    let state = admitted(&policy).state;
    let context = context(6);
    let pending = state.pending().unwrap();
    let plan = apply(
        &policy,
        &state,
        &context,
        Action::Cancel {
            plan_sha256: pending.plan.sha256().unwrap(),
            admission_receipt_sha256: pending.admission_receipt_sha256.clone(),
        },
    );
    assert!(plan.state.pending().is_none());
    assert_eq!(plan.state.next_sequence(), 5);
    assert_eq!(plan.state.active_schema(), 0);
}

#[test]
fn emergency_blocks_and_codec_is_strict() {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let mut frozen = context(5);
    frozen.emergency_control_present = true;
    let control = control(
        &policy,
        &state,
        &frozen,
        Action::Admit {
            plan: migration(&policy),
        },
    );
    assert!(refusal(&policy, &state, &frozen, &control).contains("Emergency control"));
    let raw = serde_json::to_vec(&control).unwrap();
    assert!(decode_control(&policy, &raw).is_ok());
    assert!(decode_control(&policy, &[raw.as_slice(), b" "].concat()).is_err());
    let mut v1_kind = control.clone();
    v1_kind.kind = "dytallix-upgrade-control-v1".into();
    assert!(decode_control(&policy, &serde_json::to_vec(&v1_kind).unwrap()).is_err());
    let encoded = encode_state(&state).unwrap();
    assert_eq!(decode_state(&policy, &encoded).unwrap(), state);
}
