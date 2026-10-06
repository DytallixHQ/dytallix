use super::*;

// The development genesis rehearsal: emergency, upgrade and handover
// policies at schema 2, the same shapes a production configuration has.
const CONFIG: &[u8] = include_bytes!(
    "../../../tools/mainnet-preparation/fixtures/genesis-rehearsal/application-config.json"
);
const APP_HASH: &str = "aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11";
const INCIDENT: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const DIGEST: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const RECEIPT: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const OTHER_RELEASE: &str = "44444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444";

fn config() -> ConsensusConfig {
    let config: ConsensusConfig = serde_json::from_slice(CONFIG).unwrap();
    config.validate().unwrap();
    config
}

fn status(config: &ConsensusConfig, height: u64, frozen: bool) -> Status {
    let release = config.emergency.as_ref().unwrap().release_sha512.clone();
    serde_json::from_value(serde_json::json!({
        "height": height, "app_hash": APP_HASH, "engine_hash": "ignored",
        "emergency_control": {"frozen": frozen, "next_sequence": 4,
            "last_receipt_sha256": if frozen { Some(RECEIPT) } else { None }},
        "release_handover": {"active_release_sha512": release, "active_schema": 0,
            "next_sequence": 2, "pending": null},
        "upgrade": {"active_schema": 0, "active_release_sha512": release,
            "next_sequence": 3, "pending": null},
    }))
    .unwrap()
}

fn signed(request: &Request, keys: &[&emergency::AuthorityKey]) -> Vec<SignatureRecord> {
    keys.iter()
        .map(|key| SignatureRecord {
            schema: SIGNATURE_SCHEMA.into(),
            key_id: key.key_id.clone(),
            artifact_sha512: request.envelope.artifact_sha512.clone(),
            sequence: request.envelope.sequence,
            signature_hex: "00".repeat(29_792),
        })
        .collect()
}

fn limit(validity: u64, age: u64) -> u64 {
    validity.min(age)
}

#[test]
fn a_freeze_request_is_the_nodes_payload_and_envelope() {
    let config = config();
    let policy = config.emergency.as_ref().unwrap();
    let v2 = policy.v2.as_ref().unwrap();
    let freeze = Operation::Freeze {
        incident_sha256: INCIDENT.into(),
    };
    let request = prepare(&config, &status(&config, 100, false), &freeze, None).unwrap();
    let payload: emergency::Payload = serde_json::from_value(request.payload.clone()).unwrap();
    let last = 100 + limit(v2.max_validity_blocks, v2.max_anchor_age_blocks);
    assert_eq!(
        (
            payload.sequence,
            payload.parent_height,
            payload.target_height
        ),
        (4, 100, 101)
    );
    assert_eq!(payload.parent_app_hash, APP_HASH);
    let window = payload.v2.as_ref().unwrap();
    assert_eq!(
        (window.not_before_height, window.not_after_height),
        (101, last)
    );
    assert_eq!(window.policy_sha256, policy.sha256().unwrap());
    assert_eq!(window.incident_sha256, INCIDENT);
    assert!(window.resume.is_none());
    let artifact = emergency::artifact_bytes(&payload).unwrap();
    assert_eq!(request.artifact_hex, hex::encode(&artifact));
    assert_eq!(
        request.envelope,
        Envelope {
            chain_id: policy.chain_id.clone(),
            action: "emergency".into(),
            sequence: 4,
            not_before_height: 101,
            not_after_height: last,
            artifact_sha512: hex::encode(Sha512::digest(&artifact)),
        }
    );
    assert_eq!(request.authority.purpose, "freeze");
    assert_eq!(request.authority.keys, policy.freeze_authority.keys);
    // A narrower window, and the limits.
    let narrow = prepare(&config, &status(&config, 100, false), &freeze, Some(10)).unwrap();
    assert_eq!(narrow.envelope.not_after_height, 110);
    for blocks in [
        0,
        limit(v2.max_validity_blocks, v2.max_anchor_age_blocks) + 1,
    ] {
        assert!(prepare(&config, &status(&config, 100, false), &freeze, Some(blocks)).is_err());
    }
    assert!(prepare(&config, &status(&config, 100, true), &freeze, None).is_err());
}

#[test]
fn a_freeze_assembles_into_a_control_the_node_decodes() {
    let config = config();
    let policy = config.emergency.as_ref().unwrap();
    let freeze = Operation::Freeze {
        incident_sha256: INCIDENT.into(),
    };
    let request = prepare(&config, &status(&config, 100, false), &freeze, None).unwrap();
    let keys = &policy.freeze_authority.keys;
    // Given out of order; assembled sorted by key ID.
    let records = signed(&request, &[&keys[4], &keys[0], &keys[2]]);
    let bytes = assemble(&config, &request, &records).unwrap();
    let control = emergency::decode_control(policy, &bytes).unwrap();
    let ids: Vec<_> = control
        .signatures
        .iter()
        .map(|s| s.key_id.clone())
        .collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
    assert_eq!(serde_json::to_vec(&control).unwrap(), bytes);
    assert_eq!(
        emergency::artifact_bytes(&control.payload).unwrap(),
        hex::decode(&request.artifact_hex).unwrap()
    );

    let refuse = |records: Vec<SignatureRecord>, request: &Request, why: &str| {
        let error = assemble(&config, request, &records)
            .unwrap_err()
            .to_string();
        assert!(error.contains(why), "{error}");
    };
    refuse(
        signed(&request, &[&keys[0], &keys[1]]),
        &request,
        "signatures given",
    );
    refuse(
        signed(&request, &[&keys[0], &keys[1], &keys[1]]),
        &request,
        "signed twice",
    );
    let resume_key = &policy.resume_authority.keys[0];
    refuse(
        signed(&request, &[&keys[0], &keys[1], resume_key]),
        &request,
        "not in this control's authority",
    );
    let mut other = signed(&request, &[&keys[0], &keys[1], &keys[2]]);
    other[1].sequence += 1;
    refuse(other, &request, "for another control");
    let mut other = signed(&request, &[&keys[0], &keys[1], &keys[2]]);
    other[0].artifact_sha512 = "00".repeat(64);
    refuse(other, &request, "for another control");
    let mut other = signed(&request, &[&keys[0], &keys[1], &keys[2]]);
    other[2].signature_hex = "zz".repeat(29_792);
    refuse(other, &request, "lowercase hex");
    // A payload changed after signing no longer matches the artifact.
    let mut tampered = request.clone();
    tampered.payload["sequence"] = serde_json::json!(5);
    refuse(
        signed(&request, &[&keys[0], &keys[1], &keys[2]]),
        &tampered,
        "not the signed artifact",
    );
}

#[test]
fn a_resume_binds_the_freeze_receipt_and_the_restored_anchor() {
    let config = config();
    let policy = config.emergency.as_ref().unwrap();
    let resume = Operation::Resume {
        incident_sha256: INCIDENT.into(),
        readiness_evidence_sha256: DIGEST.into(),
    };
    assert!(prepare(&config, &status(&config, 200, false), &resume, None).is_err());
    let request = prepare(&config, &status(&config, 200, true), &resume, None).unwrap();
    let payload: emergency::Payload = serde_json::from_value(request.payload.clone()).unwrap();
    assert_eq!(payload.action, emergency::Action::Resume);
    let binding = payload.v2.unwrap().resume.unwrap();
    assert_eq!(binding.freeze_receipt_sha256, RECEIPT);
    assert_eq!(binding.restored_state_sha256, APP_HASH);
    assert_eq!(binding.readiness_evidence_sha256, DIGEST);
    assert_eq!(request.authority.keys, policy.resume_authority.keys);
    let keys = &policy.resume_authority.keys;
    let bytes = assemble(
        &config,
        &request,
        &signed(&request, &[&keys[1], &keys[2], &keys[3]]),
    )
    .unwrap();
    emergency::decode_control(policy, &bytes).unwrap();
}

#[test]
fn upgrade_admission_activation_and_cancellation() {
    let config = config();
    let policy = upgrade_policy(&config).unwrap();
    let admit = Operation::UpgradeAdmit {
        authorization_sha256: DIGEST.into(),
    };
    let request = prepare(&config, &status(&config, 50, false), &admit, None).unwrap();
    let payload: upgrade_v2::Payload = serde_json::from_value(request.payload.clone()).unwrap();
    let upgrade_v2::Action::Admit { plan } = payload.action.clone() else {
        panic!("not an admission")
    };
    assert_eq!(plan.migration_sha256, upgrade_v2::migration_sha256());
    assert_eq!(plan.bounds, policy.migration_bounds);
    assert_eq!((payload.anchor_height, payload.not_before_height), (50, 51));
    assert_eq!(request.envelope.action, "upgrade");
    let keys = &policy.authority.keys;
    let bytes = assemble(
        &config,
        &request,
        &signed(&request, &[&keys[0], &keys[1], &keys[2]]),
    )
    .unwrap();
    upgrade_v2::decode_control(policy, &bytes).unwrap();

    // Admitted at height 60: activation waits for the notice.
    let mut pending = status(&config, 70, false);
    pending.upgrade.as_mut().unwrap().pending = Some(upgrade_v2::PendingPlan {
        plan: plan.clone(),
        admission_receipt_sha256: RECEIPT.into(),
        admitted_height: 60,
    });
    let activate = Operation::UpgradeActivate {
        evidence_sha256: DIGEST.into(),
    };
    let error = prepare(&config, &pending, &activate, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("notice period ends"), "{error}");
    pending.height = 60 + policy.min_notice_blocks;
    let request = prepare(&config, &pending, &activate, None).unwrap();
    let payload: upgrade_v2::Payload = serde_json::from_value(request.payload.clone()).unwrap();
    assert_eq!(
        payload.action,
        upgrade_v2::Action::Activate {
            plan: plan.clone(),
            admission_receipt_sha256: RECEIPT.into(),
            emergency_receipt_sha256: None,
            evidence_sha256: DIGEST.into(),
        }
    );
    let request = prepare(&config, &pending, &Operation::UpgradeCancel, None).unwrap();
    let payload: upgrade_v2::Payload = serde_json::from_value(request.payload.clone()).unwrap();
    assert_eq!(
        payload.action,
        upgrade_v2::Action::Cancel {
            plan_sha256: plan.sha256().unwrap(),
            admission_receipt_sha256: RECEIPT.into(),
        }
    );
    assert!(prepare(&config, &pending, &admit, None).is_err());
}

#[test]
fn handover_admission_and_paired_activation() {
    let config = config();
    let policy = config.release_handover.as_ref().unwrap();
    let admit = Operation::HandoverAdmit {
        target_release_sha512: OTHER_RELEASE.into(),
        transition: TransitionChoice::ReceiptIndexV1,
        authorization_sha256: DIGEST.into(),
    };
    let request = prepare(&config, &status(&config, 80, false), &admit, None).unwrap();
    let payload: handover::Payload = serde_json::from_value(request.payload.clone()).unwrap();
    let handover::Action::Admit { plan } = payload.action.clone() else {
        panic!("not an admission")
    };
    assert_eq!(
        plan.transition,
        handover::Transition::ReceiptIndexV1 {
            migration_sha256: upgrade_v2::migration_sha256()
        }
    );
    assert_eq!(payload.target_height, 81);
    let keys = &policy.authority.keys;
    let bytes = assemble(
        &config,
        &request,
        &signed(&request, &[&keys[2], &keys[3], &keys[4]]),
    )
    .unwrap();
    handover::decode_control(policy, &bytes).unwrap();

    let mut pending = status(&config, 0, false);
    pending.height = 90 + policy.v2.as_ref().unwrap().min_notice_blocks;
    pending.release_handover.as_mut().unwrap().pending = Some(handover::PendingPlan {
        plan: plan.clone(),
        admission_receipt_sha256: RECEIPT.into(),
        admitted_height: 90,
    });
    // The receipt index transition binds the signed upgrade activation.
    let unpaired = Operation::HandoverActivate {
        evidence_sha256: DIGEST.into(),
        upgrade_activation_sha256: None,
    };
    assert!(prepare(&config, &pending, &unpaired, None).is_err());
    let paired = Operation::HandoverActivate {
        evidence_sha256: DIGEST.into(),
        upgrade_activation_sha256: Some(INCIDENT.into()),
    };
    let request = prepare(&config, &pending, &paired, None).unwrap();
    let payload: handover::Payload = serde_json::from_value(request.payload.clone()).unwrap();
    let handover::Action::Activate {
        upgrade_activation_sha256,
        ..
    } = payload.action
    else {
        panic!("not an activation")
    };
    assert_eq!(upgrade_activation_sha256.as_deref(), Some(INCIDENT));
    // A schema-preserving admission keeps the active schema.
    let preserve = Operation::HandoverAdmit {
        target_release_sha512: OTHER_RELEASE.into(),
        transition: TransitionChoice::SchemaPreserving,
        authorization_sha256: DIGEST.into(),
    };
    let request = prepare(&config, &status(&config, 80, false), &preserve, None).unwrap();
    let payload: handover::Payload = serde_json::from_value(request.payload).unwrap();
    let handover::Action::Admit { plan } = payload.action else {
        panic!("not an admission")
    };
    assert_eq!(
        plan.transition,
        handover::Transition::SchemaPreserving { schema: 0 }
    );
}
