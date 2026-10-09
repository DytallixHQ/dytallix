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
        "root_authority": {"authority_epoch": 1, "keys": configured(config), "pending": null},
    }))
    .unwrap()
}

/// The configuration's keys, as the node reports them before any kit
/// replacement.
fn configured(config: &ConsensusConfig) -> replacement::Roles<emergency::AuthorityPolicy> {
    let emergency = config.emergency.as_ref().unwrap();
    replacement::Roles {
        upgrade: config.upgrade.as_ref().unwrap().authority().clone(),
        freeze: emergency.freeze_authority.clone(),
        resume: emergency.resume_authority.clone(),
    }
}

/// Assembled with a status at the configuration's epoch.
fn assembled(
    config: &ConsensusConfig,
    request: &Request,
    records: &[SignatureRecord],
) -> Result<Vec<u8>> {
    assemble(config, &status(config, 0, false), request, records)
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
    let bytes = assembled(&config, &request, &records).unwrap();
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
        let error = assembled(&config, request, &records)
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
    let bytes = assembled(
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
    let bytes = assembled(
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
    let bytes = assembled(
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

/// A new public key under its SHA-256 key ID, as a kit's record has it.
fn new_key(seed: u8) -> emergency::AuthorityKey {
    let public = [seed; 64];
    emergency::AuthorityKey {
        key_id: sha256_hex(&public),
        public_key_hex: hex::encode(public),
    }
}
/// The seat of each role's first key leaves for keys `seed` to `seed + 2`.
fn replacement_of(keys: &replacement::Roles<emergency::AuthorityPolicy>, seed: u8) -> Operation {
    Operation::KitReplacement {
        replaced: replacement::Roles {
            upgrade: keys.upgrade.keys[0].key_id.clone(),
            freeze: keys.freeze.keys[0].key_id.clone(),
            resume: keys.resume.keys[0].key_id.clone(),
        },
        keys: replacement::Roles {
            upgrade: new_key(seed),
            freeze: new_key(seed + 1),
            resume: new_key(seed + 2),
        },
    }
}

#[test]
fn a_replacement_request_is_signed_by_current_upgrade_keys_and_the_new_keys() {
    let config = config();
    let policy = config.upgrade.as_ref().unwrap().as_v2().unwrap();
    let keys = configured(&config);
    let operation = replacement_of(&keys, 0xa0);
    let Operation::KitReplacement {
        replaced,
        keys: new,
    } = &operation
    else {
        unreachable!()
    };
    let request = prepare(&config, &status(&config, 100, false), &operation, None).unwrap();
    let payload: replacement::Payload = serde_json::from_value(request.payload.clone()).unwrap();
    let last = 100 + limit(policy.max_validity_blocks, policy.max_anchor_age_blocks);
    assert_eq!((payload.authority_epoch, payload.sequence), (1, 2));
    assert_eq!(
        (payload.not_before_height, payload.not_after_height),
        (101, last)
    );
    assert_eq!((&payload.replaced, &payload.keys), (replaced, new));
    assert_eq!(payload.policy_sha256, policy.sha256().unwrap());
    let artifact = replacement::artifact_bytes(&payload).unwrap();
    assert_eq!(request.artifact_hex, hex::encode(&artifact));
    assert_eq!(request.operation, "kit-replacement");
    assert_eq!(
        (request.envelope.action.as_str(), request.envelope.sequence),
        ("upgrade", 2)
    );
    // The request lists the new keys too, so that they can sign their proofs.
    assert_eq!(request.authority.keys.len(), keys.upgrade.keys.len() + 3);
    assert!(new
        .each()
        .iter()
        .all(|key| request.authority.keys.contains(key)));

    let upgrade = &keys.upgrade.keys;
    let proofs = [&new.upgrade, &new.freeze, &new.resume];
    let records = signed(
        &request,
        &[
            &upgrade[3],
            &new.freeze,
            &upgrade[1],
            &new.resume,
            &new.upgrade,
            &upgrade[2],
        ],
    );
    let bytes = assembled(&config, &request, &records).unwrap();
    let policies = replacement::Policies::of(
        config.emergency.as_ref(),
        config.upgrade.as_ref(),
        config.release_handover.as_ref(),
    )
    .unwrap();
    let control = replacement::decode_control(&policies, &bytes).unwrap();
    let ids: Vec<_> = control
        .signatures
        .iter()
        .map(|s| s.key_id.as_str())
        .collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!((ids.len(), &ids), (3, &sorted));
    assert_eq!(control.payload, payload);
    assert_eq!(B64.decode(&control.proofs.freeze).unwrap(), vec![0; 29_792]);

    let refuse = |records: Vec<SignatureRecord>, why: &str| {
        let error = assembled(&config, &request, &records)
            .unwrap_err()
            .to_string();
        assert!(error.contains(why), "{error}");
    };
    refuse(
        signed(
            &request,
            &[
                &upgrade[1],
                &upgrade[2],
                &upgrade[3],
                &new.upgrade,
                &new.freeze,
            ],
        ),
        "new keys signed",
    );
    refuse(
        signed(
            &request,
            &[
                &upgrade[1],
                &upgrade[2],
                &new.upgrade,
                &new.freeze,
                &new.resume,
            ],
        ),
        "signatures given",
    );
    refuse(
        signed(
            &request,
            &[
                &upgrade[1],
                &upgrade[2],
                &upgrade[3],
                proofs[0],
                proofs[0],
                &new.freeze,
                &new.resume,
            ],
        ),
        "signed twice",
    );
    let freeze_key = &keys.freeze.keys[1];
    refuse(
        signed(
            &request,
            &[
                &upgrade[1],
                &upgrade[2],
                freeze_key,
                &new.upgrade,
                &new.freeze,
                &new.resume,
            ],
        ),
        "not in this control's authority",
    );

    // Preparation refuses what the node would refuse.
    let mut pending = status(&config, 100, false);
    pending.root_authority.as_mut().unwrap().pending = Some(PendingEpoch {
        authority_epoch: 2,
        effect_height: 500,
    });
    assert!(prepare(&config, &pending, &operation, None)
        .unwrap_err()
        .to_string()
        .contains("pending"));
    let frozen = prepare(&config, &status(&config, 100, true), &operation, None).unwrap_err();
    assert!(frozen.to_string().contains("frozen"), "{frozen}");
    let held = keys.upgrade.keys[2].clone();
    type Change = Box<dyn Fn(&mut Operation)>;
    let changes: Vec<(&str, Change)> = vec![
        (
            "is not a current",
            Box::new(|o| {
                if let Operation::KitReplacement { replaced, .. } = o {
                    replaced.upgrade = replaced.freeze.clone()
                }
            }),
        ),
        (
            "SHA-256",
            Box::new(|o| {
                if let Operation::KitReplacement { keys, .. } = o {
                    keys.freeze.key_id = "00".repeat(32)
                }
            }),
        ),
        (
            "already a root key",
            Box::new(move |o| {
                if let Operation::KitReplacement { keys, .. } = o {
                    keys.resume = held.clone()
                }
            }),
        ),
    ];
    for (why, change) in changes {
        let mut operation = operation.clone();
        change(&mut operation);
        let error = prepare(&config, &status(&config, 100, false), &operation, None).unwrap_err();
        assert!(error.to_string().contains(why), "{error}");
    }
}

#[test]
fn after_a_replacement_controls_take_the_keys_of_the_epoch_in_force() {
    let config = config();
    let configured = configured(&config);
    // Epoch 2: each role's first key replaced, as the node reports it.
    let mut keys = configured.clone();
    for (set, seed) in [
        (&mut keys.upgrade, 0xa0),
        (&mut keys.freeze, 0xa1),
        (&mut keys.resume, 0xa2),
    ] {
        set.keys[0] = new_key(seed);
        set.keys.sort_by(|a, b| a.key_id.cmp(&b.key_id));
    }
    let mut after = status(&config, 100, false);
    *after.root_authority.as_mut().unwrap() = RootAuthorityStatus {
        authority_epoch: 2,
        keys: keys.clone(),
        pending: None,
    };
    let freeze = Operation::Freeze {
        incident_sha256: INCIDENT.into(),
    };
    let request = prepare(&config, &after, &freeze, None).unwrap();
    let payload: emergency::Payload = serde_json::from_value(request.payload.clone()).unwrap();
    assert_eq!(payload.v2.unwrap().authority_epoch, 2);
    assert_eq!(request.authority.keys, keys.freeze.keys);
    // The new key signs; the key it replaced no longer counts.
    let new = new_key(0xa1);
    let kept: Vec<_> = keys
        .freeze
        .keys
        .iter()
        .filter(|k| k.key_id != new.key_id)
        .collect();
    assemble(
        &config,
        &after,
        &request,
        &signed(&request, &[&new, kept[0], kept[1]]),
    )
    .unwrap();
    let error = assemble(
        &config,
        &after,
        &request,
        &signed(&request, &[&configured.freeze.keys[0], kept[0], kept[1]]),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("not in this control's authority"), "{error}");
    // A request prepared under epoch 1 is prepared again.
    let old = prepare(&config, &status(&config, 100, false), &freeze, None).unwrap();
    let error = assemble(
        &config,
        &after,
        &old,
        &signed(&old, &[kept[0], kept[1], kept[2]]),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("prepare it again"), "{error}");
    // Upgrades and handovers name the epoch and list its upgrade keys.
    let admit = Operation::UpgradeAdmit {
        authorization_sha256: DIGEST.into(),
    };
    let request = prepare(&config, &after, &admit, None).unwrap();
    let payload: upgrade_v2::Payload = serde_json::from_value(request.payload.clone()).unwrap();
    assert_eq!(
        (payload.authority_epoch, &request.authority.keys),
        (2, &keys.upgrade.keys)
    );
    let handover = Operation::HandoverAdmit {
        target_release_sha512: OTHER_RELEASE.into(),
        transition: TransitionChoice::SchemaPreserving,
        authorization_sha256: DIGEST.into(),
    };
    let request = prepare(&config, &after, &handover, None).unwrap();
    let payload: handover::Payload = serde_json::from_value(request.payload.clone()).unwrap();
    assert_eq!(
        (payload.authority_epoch, &request.authority.keys),
        (2, &keys.upgrade.keys)
    );
    // A schema 2 configuration needs the status's root authority.
    let mut stale = status(&config, 100, false);
    stale.root_authority = None;
    assert!(prepare(&config, &stale, &freeze, None).is_err());
}

#[test]
fn a_window_ends_before_a_pending_replacement_takes_effect() {
    let config = config();
    let freeze = Operation::Freeze {
        incident_sha256: INCIDENT.into(),
    };
    let mut pending = status(&config, 100, false);
    pending.root_authority.as_mut().unwrap().pending = Some(PendingEpoch {
        authority_epoch: 2,
        effect_height: 104,
    });
    let request = prepare(&config, &pending, &freeze, None).unwrap();
    assert_eq!(
        (
            request.envelope.not_before_height,
            request.envelope.not_after_height
        ),
        (101, 103)
    );
    let payload: emergency::Payload = serde_json::from_value(request.payload.clone()).unwrap();
    assert_eq!(payload.v2.unwrap().authority_epoch, 1);
    // An effect height past the window changes nothing.
    pending
        .root_authority
        .as_mut()
        .unwrap()
        .pending
        .as_mut()
        .unwrap()
        .effect_height = 100_000;
    let request = prepare(&config, &pending, &freeze, Some(10)).unwrap();
    assert_eq!(request.envelope.not_after_height, 110);
}
