//! Real SLH signatures and RocksDB batches with explicit disposable authority.
use super::emergency_tests::RootFixture;
use super::*;
use crate::emergency_freeze as emergency;
use crate::upgrade;
use std::path::Path;
use std::process::Command;

fn sign(
    path: &Path,
    artifact: &[u8],
    action: &str,
    sequence: u64,
    first: u64,
    last: u64,
    key: u8,
) -> serde_json::Value {
    let input = path.join("upgrade-test-artifact.bin");
    let output = path.join("upgrade-test-public.json");
    std::fs::write(&input, artifact).unwrap();
    let result =
        Command::new(std::env::var("DYT_UPGRADE_TEST_SIGNER").expect("explicit disposable signer"))
            .args([
                "--artifact",
                input.to_str().unwrap(),
                "--output",
                output.to_str().unwrap(),
                "--chain",
                CHAIN,
                "--action",
                action,
                "--sequence",
                &sequence.to_string(),
                "--height",
                &first.to_string(),
                "--not-before",
                &first.to_string(),
                "--not-after",
                &last.to_string(),
                "--fixture-key",
                &key.to_string(),
            ])
            .output()
            .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap()
}
fn authority(
    path: &Path,
    first: u8,
    count: u8,
    threshold: usize,
    purpose: &str,
) -> emergency::AuthorityPolicy {
    emergency::AuthorityPolicy {
        threshold,
        keys: (first..first + count)
            .map(|key| {
                let public = sign(path, b"disposable-public-key", "emergency", 1, 1, 1, key);
                emergency::AuthorityKey {
                    key_id: format!("{purpose}-{key}"),
                    public_key_hex: hex::encode(
                        B64.decode(public["PublicKey"].as_str().unwrap()).unwrap(),
                    ),
                }
            })
            .collect(),
    }
}
fn root(f: &mut Fixture, enabled_upgrade: bool) -> RootFixture {
    RootFixture::with_config(f, |f, path| {
        f.config.max_tx_bytes = 262_144;
        let genesis_sha256 = f.config.app_state_sha256.clone();
        let policy = f.config.emergency.as_mut().unwrap();
        policy.schema = 2;
        policy.max_control_bytes = 262_144;
        policy.max_signatures = 3;
        policy.freeze_authority = authority(path, 11, 5, 3, "freeze");
        policy.resume_authority = authority(path, 21, 5, 3, "resume");
        policy.v2 = Some(emergency::PolicyV2 {
            genesis_sha256: genesis_sha256.clone(),
            authority_epoch: 1,
            max_validity_blocks: 4,
            max_anchor_age_blocks: 4,
        });
        if enabled_upgrade {
            f.config.upgrade = Some(upgrade::Policy::V1(upgrade::v1::Policy {
                schema: 1,
                development_only: true,
                chain_id: CHAIN.into(),
                genesis_sha256,
                source_release_sha512: policy.release_sha512.clone(),
                authority_epoch: 1,
                authority: authority(path, 31, 1, 1, "upgrade"),
                initial_sequence: 1,
                max_control_bytes: 262_144,
                max_signatures: 1,
                migration_bounds: upgrade::v1::MigrationBounds {
                    max_receipts: 16,
                    max_receipt_bytes: 4 * 1024 * 1024,
                    max_write_bytes: 64 * 1024,
                },
            }));
        }
    })
}
fn emergency_control(
    root: &RootFixture,
    f: &Fixture,
    app: &ConsensusApplication,
    action: emergency::Action,
    anchor: &Info,
    first: u64,
    last: u64,
) -> Vec<u8> {
    let policy = f.config.emergency.as_ref().unwrap();
    let state = emergency_state(&app.storage, &app.config).unwrap().unwrap();
    let resume = if action == emergency::Action::Resume {
        Some(emergency::ResumeBinding {
            freeze_receipt_sha256: state.freeze_receipt_sha256().unwrap().into(),
            restored_state_sha256: anchor.app_hash.clone(),
            readiness_evidence_sha256: "44".repeat(32),
        })
    } else {
        None
    };
    let payload = emergency::Payload {
        schema: 2,
        chain_id: CHAIN.into(),
        release_sha512: policy.release_sha512.clone(),
        action,
        sequence: state.next_sequence(),
        parent_height: anchor.height,
        parent_app_hash: anchor.app_hash.clone(),
        target_height: first,
        v2: Some(emergency::PayloadV2 {
            genesis_sha256: f.config.app_state_sha256.clone(),
            authority_epoch: 1,
            policy_sha256: policy.sha256().unwrap(),
            not_before_height: first,
            not_after_height: last,
            incident_sha256: "33".repeat(32),
            resume,
        }),
    };
    let artifact = emergency::artifact_bytes(&payload).unwrap();
    let (seed, purpose) = if action == emergency::Action::Freeze {
        (11, "freeze")
    } else {
        (21, "resume")
    };
    let signatures = (seed..seed + 3)
        .map(|key| {
            let signed = sign(
                root._directory.path(),
                &artifact,
                "emergency",
                payload.sequence,
                first,
                last,
                key,
            );
            emergency::ControlSignature {
                key_id: format!("{purpose}-{key}"),
                signature_hex: hex::encode(
                    B64.decode(signed["Request"]["Signature"].as_str().unwrap())
                        .unwrap(),
                ),
            }
        })
        .collect();
    serde_json::to_vec(&emergency::Control {
        kind: emergency::CONTROL_KIND_V2.into(),
        payload,
        signatures,
    })
    .unwrap()
}
fn upgrade_state(app: &ConsensusApplication) -> upgrade::v1::State {
    upgrade::v1::decode_state(
        app.config.upgrade.as_ref().unwrap().as_v1().unwrap(),
        &app.storage.db.get(upgrade::v1::STATE_KEY).unwrap().unwrap(),
    )
    .unwrap()
}
fn plan(f: &Fixture) -> upgrade::v1::MigrationPlan {
    let policy = f.config.upgrade.as_ref().unwrap().as_v1().unwrap();
    upgrade::v1::MigrationPlan {
        target_release_sha512: policy.source_release_sha512.clone(),
        migration_id: upgrade::MIGRATION_ID.into(),
        migration_sha256: upgrade::v1::migration_sha256(),
        source_schema: 0,
        target_schema: 1,
        bounds: policy.migration_bounds.clone(),
        authorization_sha256: "55".repeat(32),
    }
}
fn upgrade_control(
    root: &RootFixture,
    f: &Fixture,
    app: &ConsensusApplication,
    action: upgrade::v1::Action,
) -> Vec<u8> {
    let info = app.info().unwrap();
    let policy = f.config.upgrade.as_ref().unwrap().as_v1().unwrap();
    let payload = upgrade::v1::Payload {
        schema: 1,
        chain_id: CHAIN.into(),
        genesis_sha256: f.config.app_state_sha256.clone(),
        source_release_sha512: policy.source_release_sha512.clone(),
        authority_epoch: 1,
        sequence: upgrade_state(app).next_sequence(),
        parent_height: info.height,
        parent_app_hash: info.app_hash,
        target_height: info.height + 1,
        action,
    };
    let signed = sign(
        root._directory.path(),
        &upgrade::v1::artifact_bytes(&payload).unwrap(),
        "upgrade",
        payload.sequence,
        payload.target_height,
        payload.target_height,
        31,
    );
    serde_json::to_vec(&upgrade::v1::Control {
        kind: upgrade::v1::CONTROL_KIND.into(),
        payload,
        signatures: vec![emergency::ControlSignature {
            key_id: "upgrade-31".into(),
            signature_hex: hex::encode(
                B64.decode(signed["Request"]["Signature"].as_str().unwrap())
                    .unwrap(),
            ),
        }],
    })
    .unwrap()
}
fn activation(root: &RootFixture, f: &Fixture, app: &ConsensusApplication) -> Vec<u8> {
    let state = upgrade_state(app);
    let pending = state.pending().unwrap();
    upgrade_control(
        root,
        f,
        app,
        upgrade::v1::Action::Activate {
            plan: pending.plan.clone(),
            admission_receipt_sha256: pending.admission_receipt_sha256.clone(),
            emergency_receipt_sha256: emergency_state(&app.storage, &app.config)
                .unwrap()
                .unwrap()
                .last_receipt_sha256()
                .map(String::from),
            evidence_sha256: "66".repeat(32),
        },
    )
}
fn controls(app: &ConsensusApplication) -> BTreeMap<Vec<u8>, Vec<u8>> {
    data(app)
        .into_iter()
        .filter(|(key, _)| key.starts_with(emergency::RECEIPT_PREFIX.as_bytes()))
        .collect()
}

#[test]
fn omitted_upgrade_configuration_preserves_bytes_and_null_rejects() {
    let f = Fixture::new();
    let bytes = serde_json::to_vec(&f.config).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value.get("upgrade").is_none());
    assert_eq!(
        serde_json::to_vec(&serde_json::from_slice::<ConsensusConfig>(&bytes).unwrap()).unwrap(),
        bytes
    );
    value["upgrade"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<ConsensusConfig>(value).is_err());
}

#[test]
#[ignore = "Requires pinned real SLH helper and disposable fixture signers"]
fn actual_v2_signed_window_uses_stored_anchor_and_resume_checkpoint() {
    let mut f = Fixture::new();
    let root = root(&mut f, false);
    let directory = tempfile::tempdir().unwrap();
    let mut app = root.initialized(&f, directory.path());
    let anchor = app.info().unwrap();
    let freeze = emergency_control(&root, &f, &app, emergency::Action::Freeze, &anchor, 1, 4);
    commit(&mut app, 1, vec![]);
    assert_admitted(app.check_tx(&freeze));
    let before = data(&app);
    let mut forged: emergency::Control = serde_json::from_slice(&freeze).unwrap();
    forged.payload.parent_app_hash = "00".repeat(32);
    let wrong_anchor_artifact = emergency::artifact_bytes(&forged.payload).unwrap();
    for (offset, signature) in forged.signatures.iter_mut().enumerate() {
        let signed = sign(
            root._directory.path(),
            &wrong_anchor_artifact,
            "emergency",
            forged.payload.sequence,
            1,
            4,
            11 + offset as u8,
        );
        signature.signature_hex = hex::encode(
            B64.decode(signed["Request"]["Signature"].as_str().unwrap())
                .unwrap(),
        );
    }
    let rejected = app.check_tx(&serde_json::to_vec(&forged).unwrap());
    assert_ne!(rejected.code, 0);
    assert!(rejected.log.contains("anchor"), "{}", rejected.log);
    assert_eq!(data(&app), before);
    commit(&mut app, 2, vec![freeze]);
    let frozen_anchor = app.info().unwrap();
    let resume = emergency_control(
        &root,
        &f,
        &app,
        emergency::Action::Resume,
        &frozen_anchor,
        3,
        6,
    );
    commit(&mut app, 3, vec![]);
    assert_admitted(app.check_tx(&resume));
    commit(&mut app, 4, vec![resume]);
    let state = emergency_state(&app.storage, &app.config).unwrap().unwrap();
    assert!(!state.frozen());
    assert!(state.blocks_upgrade());
    assert_eq!(state.next_sequence(), 3);
    drop(app);
    let app = root.open(&f, directory.path()).unwrap();
    assert_eq!(app.info().unwrap().height, 4);
    assert!(app.query().unwrap()["emergency_control"]["blocks_upgrade"]
        .as_bool()
        .unwrap());
}

#[test]
#[ignore = "Requires pinned real SLH helper and disposable fixture signers"]
fn actual_index_migration_is_atomic_replayable_and_used_by_queries() {
    let mut f = Fixture::new();
    let root = root(&mut f, true);
    let directory = tempfile::tempdir().unwrap();
    let mut app = root.initialized(&f, directory.path());
    let freeze = emergency_control(
        &root,
        &f,
        &app,
        emergency::Action::Freeze,
        &app.info().unwrap(),
        1,
        2,
    );
    commit(&mut app, 1, vec![freeze]);
    let freeze_digest = emergency_state(&app.storage, &app.config)
        .unwrap()
        .unwrap()
        .last_receipt_sha256()
        .unwrap()
        .to_owned();
    assert_eq!(
        app.query_emergency_receipt(&freeze_digest).unwrap()["status"],
        "index_unavailable"
    );
    let resume = emergency_control(
        &root,
        &f,
        &app,
        emergency::Action::Resume,
        &app.info().unwrap(),
        2,
        3,
    );
    commit(&mut app, 2, vec![resume]);
    let admit = upgrade_control(&root, &f, &app, upgrade::v1::Action::Admit { plan: plan(&f) });
    assert_admitted(app.check_tx(&admit));
    commit(&mut app, 3, vec![admit]);
    let activate = activation(&root, &f, &app);
    assert_admitted(app.check_tx(&activate));
    let before = data(&app);
    let originals = controls(&app);
    let input = block(4, vec![activate]);
    let finalized = app.finalize_block(input.clone()).unwrap();
    assert_eq!(data(&app), before);
    assert!(app
        .commit_with(|_, _| anyhow::bail!("controlled pre-write refusal"))
        .is_err());
    assert_eq!(data(&app), before);
    assert!(app
        .commit_with(|storage, batch| {
            storage.db.write(batch)?;
            anyhow::bail!("lost acknowledgement after actual batch")
        })
        .is_err());
    drop(app);
    let mut app = root.open(&f, directory.path()).unwrap();
    assert_eq!(upgrade_state(&app).active_schema(), 1);
    assert_eq!(upgrade_state(&app).next_sequence(), 3);
    assert_eq!(controls(&app), originals);
    assert_eq!(app.finalize_block(input).unwrap(), finalized);
    app.commit().unwrap();
    assert_eq!(upgrade_state(&app).next_sequence(), 3);
    let found = app.query_emergency_receipt(&freeze_digest).unwrap();
    assert_eq!(found["status"], "committed_receipt_reported");
    assert_eq!(found["sequence"], 1);
    assert_eq!(
        app.query_emergency_receipt(&"ab".repeat(32)).unwrap()["status"],
        "receipt_absent"
    );
    assert!(app.query_emergency_receipt(&"AB".repeat(32)).is_err());
    let freeze = emergency_control(
        &root,
        &f,
        &app,
        emergency::Action::Freeze,
        &app.info().unwrap(),
        5,
        6,
    );
    commit(&mut app, 5, vec![freeze]);
    let digest = emergency_state(&app.storage, &app.config)
        .unwrap()
        .unwrap()
        .last_receipt_sha256()
        .unwrap()
        .to_owned();
    assert_eq!(
        app.query_emergency_receipt(&digest).unwrap()["status"],
        "committed_receipt_reported"
    );
    drop(app);
    let app = root.open(&f, directory.path()).unwrap();
    assert_eq!(
        app.query_emergency_receipt(&freeze_digest).unwrap()["sequence"],
        1
    );
    assert_eq!(app.query_emergency_receipt(&digest).unwrap()["sequence"], 3);
    let key = upgrade::index_key(&freeze_digest).unwrap();
    let value = app.storage.db.get(&key).unwrap().unwrap();
    app.storage.db.delete(&key).unwrap();
    assert!(app.info().is_err());
    app.storage.db.put(&key, &value).unwrap();
    app.info().unwrap();
    app.storage.db.put(&key, 99_u64.to_be_bytes()).unwrap();
    assert!(app.info().is_err());
    app.storage.db.put(&key, &value).unwrap();
    let extra = upgrade::index_key(&"ab".repeat(32)).unwrap();
    app.storage.db.put(&extra, 1_u64.to_be_bytes()).unwrap();
    assert!(app.info().is_err());
    app.storage.db.delete(extra).unwrap();
    app.info().unwrap();
}

#[test]
#[ignore = "Requires pinned real SLH helper and disposable fixture signers"]
fn actual_same_block_freeze_wins_and_resume_does_not_clear_upgrade() {
    let mut f = Fixture::new();
    let root = root(&mut f, true);
    for freeze_first in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut app = root.initialized(&f, directory.path());
        let admit = upgrade_control(&root, &f, &app, upgrade::v1::Action::Admit { plan: plan(&f) });
        commit(&mut app, 1, vec![admit]);
        let activate = activation(&root, &f, &app);
        let freeze = emergency_control(
            &root,
            &f,
            &app,
            emergency::Action::Freeze,
            &app.info().unwrap(),
            2,
            3,
        );
        let txs = if freeze_first {
            vec![freeze, activate.clone(), activate]
        } else {
            vec![activate.clone(), activate, freeze]
        };
        let before = upgrade_state(&app);
        commit(&mut app, 2, txs);
        assert_eq!(upgrade_state(&app), before);
        assert_eq!(upgrade_state(&app).active_schema(), 0);
        let resume = emergency_control(
            &root,
            &f,
            &app,
            emergency::Action::Resume,
            &app.info().unwrap(),
            3,
            4,
        );
        commit(&mut app, 3, vec![resume]);
        assert_eq!(upgrade_state(&app), before);
        let pending = before.pending().unwrap();
        let stale_clearance = upgrade_control(
            &root,
            &f,
            &app,
            upgrade::v1::Action::Activate {
                plan: pending.plan.clone(),
                admission_receipt_sha256: pending.admission_receipt_sha256.clone(),
                emergency_receipt_sha256: None,
                evidence_sha256: "66".repeat(32),
            },
        );
        assert_ne!(app.check_tx(&stale_clearance).code, 0);
        let activate = activation(&root, &f, &app);
        commit(&mut app, 4, vec![activate]);
        assert_eq!(upgrade_state(&app).active_schema(), 1);
        assert!(emergency_state(&app.storage, &app.config)
            .unwrap()
            .unwrap()
            .blocks_upgrade());
    }
}

#[path = "cross_binary_compat_tests.rs"]
mod cross_binary_compat_tests;

#[path = "release_handover_process_tests.rs"]
mod release_handover_process_tests;

/// Upgrades have their own custodian group (P01, 30 September 2026): a key
/// that holds a freeze or resume role cannot also sign upgrades. Synthetic
/// keys: this checks configuration only, with no signature.
#[test]
fn upgrade_keys_cannot_hold_an_emergency_role() {
    let mut f = Fixture::new();
    let key = |n: u8, purpose: &str| emergency::AuthorityKey {
        key_id: format!("{purpose}-{n}"),
        public_key_hex: hex::encode([n; 64]),
    };
    let one = |key: emergency::AuthorityKey| emergency::AuthorityPolicy {
        keys: vec![key],
        threshold: 1,
    };
    let release = hex::encode(sha2::Sha512::digest(b"synthetic release"));
    f.config.emergency = Some(emergency::Policy {
        schema: 1,
        development_only: true,
        chain_id: CHAIN.into(),
        release_sha512: release.clone(),
        initial_sequence: 1,
        freeze_authority: one(key(11, "freeze")),
        resume_authority: one(key(12, "resume")),
        max_control_bytes: 65_536,
        max_signatures: 1,
        automatic_transition_policy: emergency::AutomaticTransitionPolicy::ContinueExisting,
        v2: None,
    });
    let policy = |key: emergency::AuthorityKey| upgrade::Policy::V1(upgrade::v1::Policy {
        schema: 1,
        development_only: true,
        chain_id: CHAIN.into(),
        genesis_sha256: f.config.app_state_sha256.clone(),
        source_release_sha512: release.clone(),
        authority_epoch: 1,
        authority: one(key),
        initial_sequence: 1,
        max_control_bytes: 65_536,
        max_signatures: 1,
        migration_bounds: upgrade::v1::MigrationBounds {
            max_receipts: 16,
            max_receipt_bytes: 4 * 1024 * 1024,
            max_write_bytes: 64 * 1024,
        },
    });
    let mut config = f.config.clone();
    config.upgrade = Some(policy(key(31, "upgrade")));
    config.validate().unwrap();
    for shared in [11, 12] {
        config.upgrade = Some(policy(key(shared, "upgrade")));
        let error = config.validate().unwrap_err().to_string();
        assert!(error.contains("share a key"), "{error}");
    }
}

/// The production control tools end to end (E05; P01, 6 October 2026): five
/// throwaway key kits from `dytallix-root-sign kit`, a freeze prepared from
/// the node's status view (`control_request::prepare`), signed by three kits
/// with `dytallix-root-sign sign-control`, assembled and admitted; then a
/// resume signed by three other kits' resume keys.
#[test]
#[ignore = "Requires pinned real SLH helper and disposable fixture signers"]
fn control_tools_freeze_and_resume_end_to_end() {
    use crate::control_request::{self, Operation, Request, SignatureRecord, Status};
    let signer = std::env::var("DYT_ROOT_SIGNER").expect("dytallix-root-sign");
    let kits = tempfile::tempdir().unwrap();
    let public = kits.path().join("public");
    std::fs::create_dir(&public).unwrap();
    let drive = |n: u8| kits.path().join(format!("kit-{n}"));
    let run = |args: &[&str]| {
        let result = Command::new(&signer).args(args).output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    };
    for n in 1..=5u8 {
        std::fs::create_dir(drive(n)).unwrap();
        run(&[
            "kit",
            "-number",
            &n.to_string(),
            "-private-out",
            drive(n).to_str().unwrap(),
            "-public-out",
            public.to_str().unwrap(),
        ]);
    }
    let authority = |purpose: &str| {
        let mut keys: Vec<emergency::AuthorityKey> = (1..=5u8)
            .map(|n| {
                let raw = std::fs::read(public.join(format!("kit-{n}-{purpose}.json"))).unwrap();
                serde_json::from_slice(&raw).unwrap()
            })
            .collect();
        keys.sort_by(|a, b| a.key_id.cmp(&b.key_id));
        emergency::AuthorityPolicy { keys, threshold: 3 }
    };
    let mut f = Fixture::new();
    let root = RootFixture::with_config(&mut f, |f, _| {
        f.config.max_tx_bytes = 262_144;
        let genesis_sha256 = f.config.app_state_sha256.clone();
        let policy = f.config.emergency.as_mut().unwrap();
        policy.schema = 2;
        policy.max_control_bytes = 262_144;
        policy.max_signatures = 3;
        policy.freeze_authority = authority("freeze");
        policy.resume_authority = authority("resume");
        policy.v2 = Some(emergency::PolicyV2 {
            genesis_sha256,
            authority_epoch: 1,
            max_validity_blocks: 4,
            max_anchor_age_blocks: 4,
        });
    });
    let directory = tempfile::tempdir().unwrap();
    let mut app = root.initialized(&f, directory.path());
    // Prepare from the status view, sign offline with three kits, assemble.
    let control =
        |app: &ConsensusApplication, operation: Operation, purpose: &str, kits_used: [u8; 3]| {
            let status: Status = serde_json::from_value(app.query().unwrap()).unwrap();
            let request: Request =
                control_request::prepare(&f.config, &status, &operation, None).unwrap();
            let request_path = kits.path().join(format!("{purpose}-request.json"));
            std::fs::write(&request_path, serde_json::to_vec_pretty(&request).unwrap()).unwrap();
            let signatures: Vec<SignatureRecord> = kits_used
                .iter()
                .map(|n| {
                    let out = kits.path().join(format!("{purpose}-signature-{n}.json"));
                    run(&[
                        "sign-control",
                        "-request",
                        request_path.to_str().unwrap(),
                        "-private-key",
                        drive(*n)
                            .join(format!("kit-{n}-{purpose}.key"))
                            .to_str()
                            .unwrap(),
                        "-public-key",
                        public
                            .join(format!("kit-{n}-{purpose}.json"))
                            .to_str()
                            .unwrap(),
                        "-operation",
                        purpose,
                        "-sequence",
                        &request.envelope.sequence.to_string(),
                        "-out",
                        out.to_str().unwrap(),
                    ]);
                    serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap()
                })
                .collect();
            control_request::assemble(&f.config, &request, &signatures).unwrap()
        };
    let freeze = control(
        &app,
        Operation::Freeze {
            incident_sha256: "33".repeat(32),
        },
        "freeze",
        [1, 3, 5],
    );
    commit(&mut app, 1, vec![]);
    assert_admitted(app.check_tx(&freeze));
    commit(&mut app, 2, vec![freeze]);
    assert!(app.query().unwrap()["emergency_control"]["frozen"]
        .as_bool()
        .unwrap());
    let resume = control(
        &app,
        Operation::Resume {
            incident_sha256: "33".repeat(32),
            readiness_evidence_sha256: "44".repeat(32),
        },
        "resume",
        [2, 3, 4],
    );
    commit(&mut app, 3, vec![]);
    assert_admitted(app.check_tx(&resume));
    commit(&mut app, 4, vec![resume]);
    let state = emergency_state(&app.storage, &app.config).unwrap().unwrap();
    assert!(!state.frozen());
    assert_eq!(state.next_sequence(), 3);
}
