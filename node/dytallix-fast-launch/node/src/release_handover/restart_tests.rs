use super::super::{is_rejection, PendingPlan, ReleasePlan, Transition, KEY_BYTES};
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestVerifier {
    outcome: u8,
    calls: AtomicUsize,
}
impl TestVerifier {
    fn new(outcome: u8) -> Self {
        Self {
            outcome,
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
        height: u64,
        before: u64,
        after: u64,
        artifact: &[u8],
        signature: &[u8],
    ) -> Result<bool> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        assert_eq!(chain, "restart-test");
        assert_eq!((sequence, height, before, after), (10, 8, 8, 8));
        assert_eq!(key.len(), KEY_BYTES);
        assert_eq!(signature.len(), SIGNATURE_BYTES);
        assert!(artifact.starts_with(b"DYTALLIX/RELEASE-RESTART/v1\0"));
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
        chain_id: "restart-test".into(),
        genesis_sha256: "aa".repeat(32),
        initial_release_sha512: "bb".repeat(64),
        initial_schema: 0,
        authority_epoch: 3,
        authority: emergency::AuthorityPolicy {
            threshold: 2,
            keys: ["a", "b", "c"]
                .iter()
                .enumerate()
                .map(|(i, id)| emergency::AuthorityKey {
                    key_id: (*id).into(),
                    public_key_hex: format!("{:02x}", i + 1).repeat(KEY_BYTES),
                })
                .collect(),
        },
        initial_sequence: 10,
        max_control_bytes: 200_000,
        max_signatures: 3,
    }
}
fn checkpoint() -> Checkpoint {
    Checkpoint {
        height: 7,
        app_hash: "cc".repeat(32),
        emergency_receipt_sha256: None,
    }
}
fn payload(policy: &Policy, state: &State) -> Payload {
    Payload {
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
        halted_block_hash: Some("ee".repeat(32)),
        emergency_receipt_sha256: None,
        pending_admission_receipt_sha256: None,
        evidence_sha256: "ff".repeat(32),
    }
}
fn signed(payload: Payload) -> Authorization {
    Authorization {
        kind: KIND.into(),
        payload,
        signatures: ["a", "b"]
            .iter()
            .map(|id| emergency::ControlSignature {
                key_id: (*id).into(),
                signature_hex: "44".repeat(SIGNATURE_BYTES),
            })
            .collect(),
    }
}
fn raw(authorization: &Authorization) -> Vec<u8> {
    serde_json::to_vec(authorization).unwrap()
}
fn check(policy: &Policy, state: &State, authorization: &Authorization) -> Result<Verified> {
    verify(
        policy,
        state,
        &checkpoint(),
        &raw(authorization),
        &TestVerifier::new(1),
    )
}

#[test]
fn a_restart_switches_the_release_and_nothing_else() {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let authorization = signed(payload(&policy, &state));
    let verifier = TestVerifier::new(1);
    let verified = verify(
        &policy,
        &state,
        &checkpoint(),
        &raw(&authorization),
        &verifier,
    )
    .unwrap();
    assert_eq!(verifier.calls.load(Ordering::Relaxed), 2);
    let next = verified.state();
    assert_eq!(next.active_release_sha512(), "dd".repeat(64));
    assert_eq!(next.active_schema(), state.active_schema());
    assert_eq!(next.next_sequence(), 11);
    assert!(next.pending().is_none());
    let digest = verified.receipt().sha256().unwrap();
    assert_eq!(next.last_receipt_sha256(), Some(digest.as_str()));
    assert_eq!(next.activation_receipt_sha256(), Some(digest.as_str()));
    assert_eq!(next.last_control_height, Some(8));
    assert_eq!(verified.receipt().previous_receipt_sha256, None);
    assert_eq!(
        (verified.receipt().sequence(), verified.halted_height()),
        (10, 8)
    );
    assert_eq!(verified.target_release_sha512(), "dd".repeat(64));
    assert_eq!(
        key(10),
        "consensus:release-handover:v1:restart:00000000000000000010"
    );
}

#[test]
fn a_restart_applies_to_its_block_only() {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let bound = check(&policy, &state, &signed(payload(&policy, &state))).unwrap();
    assert!(bound.applies_to(8, Some(&"ee".repeat(32))));
    assert!(!bound.applies_to(8, Some(&"ef".repeat(32))));
    assert!(!bound.applies_to(8, None));
    assert!(!bound.applies_to(9, Some(&"ee".repeat(32))));
    // An undecided height binds no block.
    let mut open = payload(&policy, &state);
    open.halted_block_hash = None;
    let open = check(&policy, &state, &signed(open)).unwrap();
    assert!(open.applies_to(8, Some(&"ef".repeat(32))) && open.applies_to(8, None));
    assert!(!open.applies_to(7, None));
}

#[test]
fn every_binding_is_checked() {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let cases: Vec<(&str, Box<dyn Fn(&mut Payload)>)> = vec![
        ("schema", Box::new(|p| p.schema = 2)),
        ("chain", Box::new(|p| p.chain_id = "other".into())),
        ("genesis", Box::new(|p| p.genesis_sha256 = "ab".repeat(32))),
        ("policy", Box::new(|p| p.policy_sha256 = "ab".repeat(32))),
        ("epoch", Box::new(|p| p.authority_epoch = 4)),
        ("sequence", Box::new(|p| p.sequence = 11)),
        (
            "source",
            Box::new(|p| p.source_release_sha512 = "ab".repeat(64)),
        ),
        (
            "same target",
            Box::new(|p| p.target_release_sha512 = "bb".repeat(64)),
        ),
        ("schema change", Box::new(|p| p.state_schema = 1)),
        ("parent height", Box::new(|p| p.parent_height = 6)),
        (
            "parent hash",
            Box::new(|p| p.parent_app_hash = "cd".repeat(32)),
        ),
        ("halted height", Box::new(|p| p.halted_height = 9)),
        (
            "block hash",
            Box::new(|p| p.halted_block_hash = Some("EE".repeat(32))),
        ),
        (
            "emergency",
            Box::new(|p| p.emergency_receipt_sha256 = Some("ab".repeat(32))),
        ),
        (
            "pending",
            Box::new(|p| p.pending_admission_receipt_sha256 = Some("ab".repeat(32))),
        ),
        ("evidence", Box::new(|p| p.evidence_sha256 = "short".into())),
    ];
    for (name, change) in cases {
        let mut changed = payload(&policy, &state);
        change(&mut changed);
        let error = check(&policy, &state, &signed(changed)).unwrap_err();
        assert!(is_rejection(&error), "{name}: {error:#}");
    }
}

#[test]
fn signatures_need_the_threshold_of_distinct_sorted_known_keys() {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let with = |ids: &[&str]| {
        let mut authorization = signed(payload(&policy, &state));
        authorization.signatures = ids
            .iter()
            .map(|id| emergency::ControlSignature {
                key_id: (*id).into(),
                signature_hex: "44".repeat(SIGNATURE_BYTES),
            })
            .collect();
        authorization
    };
    for ids in [&["a"][..], &["b", "a"], &["a", "a"], &["a", "z"]] {
        assert!(check(&policy, &state, &with(ids)).is_err(), "{ids:?}");
    }
    assert!(check(&policy, &state, &with(&["a", "c"])).is_ok());
    let mut wrong_kind = signed(payload(&policy, &state));
    wrong_kind.kind = super::super::CONTROL_KIND.into();
    assert!(check(&policy, &state, &wrong_kind).is_err());
}

#[test]
fn a_rejected_signature_differs_from_a_helper_failure() {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let bytes = raw(&signed(payload(&policy, &state)));
    let rejected = verify(
        &policy,
        &state,
        &checkpoint(),
        &bytes,
        &TestVerifier::new(0),
    )
    .unwrap_err();
    assert!(is_rejection(&rejected));
    let unavailable = verify(
        &policy,
        &state,
        &checkpoint(),
        &bytes,
        &TestVerifier::new(2),
    )
    .unwrap_err();
    assert!(!is_rejection(&unavailable), "{unavailable:#}");
}

#[test]
fn noncanonical_or_oversized_bytes_are_refused() {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let mut bytes = raw(&signed(payload(&policy, &state)));
    bytes.push(b' ');
    assert!(verify(
        &policy,
        &state,
        &checkpoint(),
        &bytes,
        &TestVerifier::new(1)
    )
    .is_err());
    let mut small = policy.clone();
    small.max_control_bytes = 100;
    let bytes = raw(&signed(payload(&small, &State::new(&small).unwrap())));
    assert!(decode_authorization(&small, &bytes).is_err());
}

#[test]
fn a_restart_supersedes_the_pending_admission_it_names() {
    let policy = policy();
    let mut state = State::new(&policy).unwrap();
    state.pending = Some(PendingPlan {
        plan: ReleasePlan {
            target_release_sha512: "12".repeat(64),
            transition: Transition::SchemaPreserving { schema: 0 },
            authorization_sha256: "34".repeat(32),
        },
        admission_receipt_sha256: "56".repeat(32),
        admitted_height: 5,
    });
    state.next_sequence = 10;
    state.last_receipt_sha256 = Some("56".repeat(32));
    state.last_control_height = Some(5);
    let mut policy = policy;
    policy.initial_sequence = 9;
    state.policy_sha256 = policy.sha256().unwrap();
    state.validate(&policy).unwrap();
    // It must name the pending admission.
    assert!(check(&policy, &state, &signed(payload(&policy, &state))).is_err());
    let mut named = payload(&policy, &state);
    named.pending_admission_receipt_sha256 = Some("56".repeat(32));
    let verified = check(&policy, &state, &signed(named)).unwrap();
    assert!(verified.state().pending().is_none());
    assert_eq!(
        verified.receipt().previous_receipt_sha256.as_deref(),
        Some("56".repeat(32).as_str())
    );
    // A height already holding a control cannot be restarted.
    state.last_control_height = Some(8);
    let mut consumed = payload(&policy, &state);
    consumed.pending_admission_receipt_sha256 = Some("56".repeat(32));
    assert!(check(&policy, &state, &signed(consumed)).is_err());
}

#[test]
fn replay_reproduces_the_committed_receipt() {
    let policy = policy();
    let state = State::new(&policy).unwrap();
    let verified = check(&policy, &state, &signed(payload(&policy, &state))).unwrap();
    let bytes = encode_receipt(verified.receipt()).unwrap();
    let receipt = decode_receipt(&policy, &bytes).unwrap();
    let replayed = replay(&policy, &state, &checkpoint(), &receipt, None).unwrap();
    assert_eq!(replayed.state(), verified.state());
    let verifier = TestVerifier::new(1);
    replay(&policy, &state, &checkpoint(), &receipt, Some(&verifier)).unwrap();
    assert_eq!(verifier.calls.load(Ordering::Relaxed), 2);
    let mut altered = receipt.clone();
    altered.previous_receipt_sha256 = Some("ab".repeat(32));
    assert!(replay(&policy, &state, &checkpoint(), &altered, None).is_err());
    let mut elsewhere = checkpoint();
    elsewhere.app_hash = "cd".repeat(32);
    assert!(replay(&policy, &state, &elsewhere, &receipt, None).is_err());
    let mut other_policy = policy.clone();
    other_policy.authority_epoch = 4;
    assert!(decode_receipt(&other_policy, &bytes).is_err());
}
