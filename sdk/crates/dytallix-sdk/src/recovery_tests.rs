use super::*;
use dytallix_protocol_types::ordinary_client::{
    AccountDomain, CommittedContext, PendingPolicyView, PendingRecoveryView, RecoveryTimingView,
};

struct Keys {
    active: DytallixKeypair,
    guardians: Vec<DytallixKeypair>,
    replacement: DytallixKeypair,
    sponsor: DytallixKeypair,
}
impl Keys {
    fn new() -> Self {
        let mut guardians: Vec<DytallixKeypair> =
            (0..3).map(|_| DytallixKeypair::generate()).collect();
        guardians.sort_by(|a, b| identity(a).unwrap().cmp(&identity(b).unwrap()));
        Self {
            active: DytallixKeypair::generate(),
            guardians,
            replacement: DytallixKeypair::generate(),
            sponsor: DytallixKeypair::generate(),
        }
    }
    fn policy(&self) -> RecoveryPolicy {
        RecoveryPolicy {
            threshold: 2,
            guardians: self
                .guardians
                .iter()
                .enumerate()
                .map(|(i, key)| Guardian {
                    key: identity(key).unwrap(),
                    control_group: format!("custodian-{i}"),
                })
                .collect(),
        }
    }
}

fn fee() -> RecoveryFeeView {
    RecoveryFeeView {
        profile_version: 1,
        profile_digest: [9; 32],
        denomination: "udrt".into(),
        gas_price: 2,
        minimum_gas: 10,
        max_transaction_gas: 1_000_000,
        max_fee_cap: 2_000_000,
    }
}
fn view(id: [u8; 32], key: &DytallixKeypair) -> RecoveryAccountView {
    RecoveryAccountView {
        version: 1,
        context: CommittedContext {
            chain_id: "recovery-test".into(),
            genesis_digest: [1; 32],
            height: 20,
            app_hash: [2; 32],
        },
        domain: AccountDomain {
            network: 3,
            chain_id: "recovery-test".into(),
            genesis_digest: [1; 32],
            account_id: id,
        },
        address: "address".into(),
        status: RecoveryStatus::Normal,
        active_key: identity(key).unwrap(),
        active_generation: 4,
        spending_nonce: 7,
        sponsor_nonce: 3,
        policy: None,
        policy_version: 0,
        recovery_sequence: 0,
        policy_change_sequence: 0,
        pending_recovery: None,
        pending_policy: None,
        timing: RecoveryTimingView {
            timing_version: 1,
            recovery_delay: 2,
            finalization_window: 5,
            policy_delay: 2,
            policy_window: 5,
            submission_lifetime: 30,
            algorithms: vec!["mldsa65".into()],
        },
        fee: fee(),
    }
}
fn enrolled(keys: &Keys) -> RecoveryAccountView {
    let mut v = view([11; 32], &keys.active);
    v.policy = Some(keys.policy());
    v.policy_version = 1;
    v
}

#[test]
fn every_action_names_its_signers() {
    let keys = Keys::new();
    let guardians: Vec<KeyIdentity> = keys
        .guardians
        .iter()
        .map(|k| identity(k).unwrap())
        .collect();
    let active = identity(&keys.active).unwrap();
    let replacement = identity(&keys.replacement).unwrap();

    let enroll = build(
        &view([11; 32], &keys.active),
        &RecoveryRequest::Enroll {
            policy: keys.policy(),
        },
        40,
    )
    .unwrap();
    assert_eq!(enroll.requirements.operation_keys, vec![active.clone()]);
    assert_eq!(enroll.requirements.possession_keys, guardians);
    assert!(matches!(
        enroll.operation.action.kind,
        ActionKind::Enroll {
            active: ActiveAuthorization {
                generation: 4,
                nonce: 7
            },
            ..
        }
    ));

    let base = enrolled(&keys);
    let rotate = build(
        &base,
        &RecoveryRequest::Rotate {
            replacement: replacement.clone(),
        },
        40,
    )
    .unwrap();
    assert_eq!(
        rotate.requirements.possession_keys,
        vec![replacement.clone()]
    );

    let start = build(
        &base,
        &RecoveryRequest::Start {
            replacement: replacement.clone(),
            request_id: Some([5; 32]),
        },
        40,
    )
    .unwrap();
    assert_eq!(
        (
            start.requirements.guardian_threshold,
            start.requirements.guardians.clone()
        ),
        (2, guardians.clone())
    );
    assert!(start.requirements.operation_keys.is_empty());
    assert!(
        matches!(start.operation.action.kind, ActionKind::Start { request_id, timing_version: 1, .. } if request_id == [5; 32])
    );
    let random = build(
        &base,
        &RecoveryRequest::Start {
            replacement: replacement.clone(),
            request_id: None,
        },
        40,
    )
    .unwrap();
    assert_ne!(
        random.operation, start.operation,
        "a request ID is chosen when none is given"
    );

    let mut pending = base.clone();
    pending.status = RecoveryStatus::PendingRecovery;
    pending.pending_recovery = Some(PendingRecoveryView {
        request_id: [5; 32],
        replacement: replacement.clone(),
        activation_height: 21,
        expiry_height: 26,
    });
    let finalize = build(&pending, &RecoveryRequest::Finalize {}, 40).unwrap();
    assert_eq!(
        finalize.requirements.operation_keys,
        vec![replacement.clone()]
    );
    assert!(finalize.requirements.guardians.is_empty());
    let cancel = build(&pending, &RecoveryRequest::Cancel {}, 40).unwrap();
    assert!(
        matches!(cancel.operation.action.kind, ActionKind::Cancel { request_id, .. } if request_id == [5; 32])
    );

    let mut locked = base.clone();
    locked.status = RecoveryStatus::RecoveryLocked;
    let resume = build(&locked, &RecoveryRequest::Resume {}, 40).unwrap();
    assert!(
        matches!(&resume.operation.action.kind, ActionKind::Resume { active_key, .. } if *active_key == active)
    );

    let stage = build(
        &base,
        &RecoveryRequest::StagePolicy {
            policy: keys.policy(),
            update_id: Some([6; 32]),
        },
        40,
    )
    .unwrap();
    assert_eq!(stage.requirements.operation_keys, vec![active.clone()]);
    assert_eq!(stage.requirements.possession_keys, guardians);
    let mut staged = base.clone();
    staged.pending_policy = Some(PendingPolicyView {
        update_id: [6; 32],
        policy: keys.policy(),
        activation_height: 21,
        expiry_height: 26,
    });
    let activate = build(&staged, &RecoveryRequest::ActivatePolicy {}, 40).unwrap();
    assert_eq!(
        (
            activate.requirements.operation_keys.clone(),
            activate.requirements.guardian_threshold
        ),
        (vec![active.clone()], 2)
    );
    let cancel_policy = build(&staged, &RecoveryRequest::CancelPolicy {}, 40).unwrap();
    assert!(cancel_policy.requirements.operation_keys.is_empty());
    for prepared in [
        enroll,
        rotate,
        start,
        finalize,
        cancel,
        resume,
        stage,
        activate,
        cancel_policy,
    ] {
        let bytes = unsigned_bytes(&prepared.operation).unwrap();
        assert_eq!(decode_unsigned(&bytes).unwrap(), prepared.operation);
        assert_eq!(prepared.operation.domain.account_id, [11; 32]);
    }
}

#[test]
fn preconditions_come_from_the_view() {
    let keys = Keys::new();
    let base = enrolled(&keys);
    let replacement = identity(&keys.replacement).unwrap();
    assert!(build(
        &base,
        &RecoveryRequest::Enroll {
            policy: keys.policy()
        },
        40
    )
    .is_err());
    assert!(build(
        &view([11; 32], &keys.active),
        &RecoveryRequest::Cancel {},
        40
    )
    .is_err());
    assert!(build(&base, &RecoveryRequest::Finalize {}, 40).is_err());
    assert!(build(&base, &RecoveryRequest::Resume {}, 40).is_err());
    assert!(build(&base, &RecoveryRequest::ActivatePolicy {}, 40).is_err());
    for expiry in [20, 51] {
        assert!(build(
            &base,
            &RecoveryRequest::Rotate {
                replacement: replacement.clone()
            },
            expiry
        )
        .is_err());
    }
    let mut early = base.clone();
    early.pending_recovery = Some(PendingRecoveryView {
        request_id: [5; 32],
        replacement: replacement.clone(),
        activation_height: 30,
        expiry_height: 35,
    });
    assert!(
        build(&early, &RecoveryRequest::Finalize {}, 40).is_err(),
        "before its delay"
    );
    assert!(build(
        &early,
        &RecoveryRequest::Start {
            replacement,
            request_id: None
        },
        40
    )
    .is_err());
    let mut locked = base;
    locked.status = RecoveryStatus::RecoveryLocked;
    assert!(build(
        &locked,
        &RecoveryRequest::StagePolicy {
            policy: keys.policy(),
            update_id: None
        },
        40
    )
    .is_err());
}

fn started(keys: &Keys) -> Prepared {
    build(
        &enrolled(keys),
        &RecoveryRequest::Start {
            replacement: identity(&keys.replacement).unwrap(),
            request_id: Some([5; 32]),
        },
        40,
    )
    .unwrap()
}

#[test]
fn assembly_checks_every_signature_and_the_quorum() {
    let keys = Keys::new();
    let prepared = started(&keys);
    let op = &prepared.operation;
    let g0 = sign(op, SignatureRole::Operation, &keys.guardians[2]).unwrap();
    let g1 = sign(op, SignatureRole::Operation, &keys.guardians[0]).unwrap();
    let proof = sign(op, SignatureRole::Possession, &keys.replacement).unwrap();
    let signed = assemble(
        op,
        &prepared.requirements,
        vec![proof.clone(), g0.clone(), g1.clone(), g0.clone()],
    )
    .unwrap();
    assert_eq!(signed.signatures.len(), 3, "a duplicate is dropped");
    assert_eq!(signed.signatures[2].role, SignatureRole::Possession);
    assert!(signed.signatures[0].key < signed.signatures[1].key);
    let wire = recovery_wire::encode(&signed).unwrap();
    assert_eq!(recovery_wire::decode(&wire).unwrap(), signed);

    let refused = |signatures: Vec<RecoverySignature>| {
        assemble(op, &prepared.requirements, signatures).is_err()
    };
    assert!(
        refused(vec![g0.clone(), proof.clone()]),
        "one guardian is below the quorum"
    );
    assert!(
        refused(vec![g0.clone(), g1.clone()]),
        "the possession proof is missing"
    );
    let active = sign(op, SignatureRole::Operation, &keys.active).unwrap();
    assert!(
        refused(vec![g0.clone(), g1.clone(), proof.clone(), active]),
        "Start takes guardians only"
    );
    let other = build(
        &enrolled(&keys),
        &RecoveryRequest::Start {
            replacement: identity(&keys.replacement).unwrap(),
            request_id: Some([8; 32]),
        },
        40,
    )
    .unwrap();
    let elsewhere = sign(
        &other.operation,
        SignatureRole::Operation,
        &keys.guardians[1],
    )
    .unwrap();
    assert!(
        refused(vec![g0.clone(), elsewhere, proof.clone()]),
        "a signature over another operation"
    );
    let mut forged = g1.clone();
    forged.signature[0] ^= 1;
    assert!(refused(vec![g0, forged, proof]));
}

#[test]
fn a_separate_sponsor_pays_within_the_fee_bounds() {
    let keys = Keys::new();
    let prepared = started(&keys);
    let op = &prepared.operation;
    let signed = assemble(
        op,
        &prepared.requirements,
        vec![
            sign(op, SignatureRole::Operation, &keys.guardians[0]).unwrap(),
            sign(op, SignatureRole::Operation, &keys.guardians[1]).unwrap(),
            sign(op, SignatureRole::Possession, &keys.replacement).unwrap(),
        ],
    )
    .unwrap();
    let target = enrolled(&keys);
    let payer = view([12; 32], &keys.sponsor);
    let bounds = SponsorBounds {
        gas_limit: 100_000,
        maximum_charge: 200_000,
        expiry_height: 40,
    };
    let sponsored = sponsor(&signed, &target, &payer, &keys.sponsor, bounds).unwrap();
    assert_eq!(sponsored.sponsor.sponsor_account_id, [12; 32]);
    assert_eq!(
        (
            sponsored.sponsor.sponsor_nonce,
            sponsored.sponsor.sponsor_generation
        ),
        (3, 4)
    );
    assert_eq!(sponsored.sponsor.fee_profile_digest, [9; 32]);
    assert_eq!(sponsored.sponsor.operation_id, operation_id(op).unwrap());
    let envelope = sponsor_wire::encode(&sponsored).unwrap();
    assert_eq!(sponsor_wire::decode(&envelope).unwrap(), sponsored);
    let bytes = sponsor_wire::sponsor_signing_bytes(&sponsored.sponsor).unwrap();
    assert!(dytallix_core::signature::verify_mldsa65(
        keys.sponsor.public_key(),
        &bytes,
        &sponsored.signature
    )
    .unwrap());
    let tx: serde_json::Value = serde_json::from_slice(&transaction(&sponsored).unwrap()).unwrap();
    assert_eq!(tx["kind"], "recovery");
    assert_eq!(
        authorization_id(&sponsored).unwrap(),
        authorization_id(&sponsored).unwrap()
    );

    let refused =
        |target: &RecoveryAccountView,
         payer: &RecoveryAccountView,
         key: &DytallixKeypair,
         bounds: SponsorBounds| { sponsor(&signed, target, payer, key, bounds).is_err() };
    assert!(
        refused(
            &target,
            &view([11; 32], &keys.sponsor),
            &keys.sponsor,
            bounds
        ),
        "the target pays for itself"
    );
    assert!(
        refused(&target, &payer, &keys.active, bounds),
        "not the sponsor's key"
    );
    for bad in [
        SponsorBounds {
            gas_limit: 9,
            ..bounds
        },
        SponsorBounds {
            gas_limit: 1_000_001,
            ..bounds
        },
        SponsorBounds {
            maximum_charge: 199_999,
            ..bounds
        },
        SponsorBounds {
            maximum_charge: 2_000_001,
            ..bounds
        },
        SponsorBounds {
            expiry_height: 20,
            ..bounds
        },
    ] {
        assert!(refused(&target, &payer, &keys.sponsor, bad), "{bad:?}");
    }
    let mut locked = payer.clone();
    locked.status = RecoveryStatus::RecoveryLocked;
    assert!(refused(&target, &locked, &keys.sponsor, bounds));
}

#[test]
fn requests_are_json_with_optional_ids() {
    let keys = Keys::new();
    let replacement = identity(&keys.replacement).unwrap();
    let request = RecoveryRequest::Start {
        replacement: replacement.clone(),
        request_id: Some([0xab; 32]),
    };
    let text = serde_json::to_string(&request).unwrap();
    assert!(text.contains("\"action\":\"start\"") && text.contains(&"ab".repeat(32)));
    assert_eq!(
        serde_json::from_str::<RecoveryRequest>(&text).unwrap(),
        request
    );
    let bare: RecoveryRequest =
        serde_json::from_value(serde_json::json!({"action": "finalize"})).unwrap();
    assert_eq!(bare, RecoveryRequest::Finalize {});
    assert!(serde_json::from_value::<RecoveryRequest>(
        serde_json::json!({"action": "finalize", "extra": 1})
    )
    .is_err());
    assert!(serde_json::from_value::<RecoveryRequest>(
        serde_json::json!({"action": "start", "replacement": replacement, "request_id": "AB"})
    )
    .is_err());
}
