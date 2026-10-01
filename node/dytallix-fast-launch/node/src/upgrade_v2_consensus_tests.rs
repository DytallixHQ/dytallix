//! Upgrade schema 2 and handover schema 2 through the consensus adapter, with
//! real SLH signatures and RocksDB batches (production activation v1, A3).
use super::emergency_tests::RootFixture;
use super::*;
use crate::{emergency_freeze as emergency, release_handover as handover, upgrade};
use std::path::Path;
use std::process::Command;

/// Blocks from admission to the earliest activation, in these fixtures.
const NOTICE: u64 = 3;

/// One disposable SLH signature over `artifact` for heights `first..=last`.
fn sign(
    path: &Path,
    artifact: &[u8],
    sequence: u64,
    first: u64,
    last: u64,
    key: u8,
) -> serde_json::Value {
    let input = path.join("upgrade-v2-artifact.bin");
    let output = path.join("upgrade-v2-public.json");
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
                "upgrade",
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

/// The five upgrade custodians (disposable keys 41 to 45), keyed by their
/// SHA-256 key IDs, with each ID's signing key.
struct Custodians {
    authority: emergency::AuthorityPolicy,
    keys: BTreeMap<String, u8>,
}
fn custodians(path: &Path) -> Custodians {
    let mut keys = BTreeMap::new();
    let mut authority = emergency::AuthorityPolicy {
        keys: Vec::new(),
        threshold: 3,
    };
    for key in 41..=45u8 {
        let public = B64
            .decode(
                sign(path, b"disposable-public-key", 1, 1, 1, key)["PublicKey"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
        let key_id = hex::encode(Sha256::digest(&public));
        keys.insert(key_id.clone(), key);
        authority.keys.push(emergency::AuthorityKey {
            key_id,
            public_key_hex: hex::encode(public),
        });
    }
    authority.keys.sort_by(|a, b| a.key_id.cmp(&b.key_id));
    Custodians { authority, keys }
}
/// Three of the five custodians sign a window.
fn signatures(
    root: &RootFixture,
    custodians: &Custodians,
    artifact: &[u8],
    sequence: u64,
    first: u64,
    last: u64,
) -> Vec<emergency::ControlSignature> {
    custodians.authority.keys[..3]
        .iter()
        .map(|key| {
            let signed = sign(
                root._directory.path(),
                artifact,
                sequence,
                first,
                last,
                custodians.keys[&key.key_id],
            );
            emergency::ControlSignature {
                key_id: key.key_id.clone(),
                signature_hex: hex::encode(
                    B64.decode(signed["Request"]["Signature"].as_str().unwrap())
                        .unwrap(),
                ),
            }
        })
        .collect()
}

fn upgrade_policy(f: &Fixture, custodians: &Custodians) -> upgrade::v2::Policy {
    upgrade::v2::Policy {
        schema: 2,
        chain_id: CHAIN.into(),
        genesis_sha256: f.config.app_state_sha256.clone(),
        initial_release_sha512: f.config.emergency.as_ref().unwrap().release_sha512.clone(),
        authority_epoch: 1,
        authority: custodians.authority.clone(),
        initial_sequence: 1,
        max_control_bytes: 262_144,
        max_signatures: 5,
        migration_bounds: upgrade::v2::MigrationBounds {
            max_receipts: 16,
            max_receipt_bytes: 4 * 1024 * 1024,
            max_write_bytes: 64 * 1024,
        },
        min_notice_blocks: NOTICE,
        max_validity_blocks: 4,
        max_anchor_age_blocks: 4,
    }
}
fn root(f: &mut Fixture) -> (RootFixture, Custodians) {
    let mut chosen = None;
    let root = RootFixture::with_config(f, |f, path| {
        f.config.max_tx_bytes = 262_144;
        let custodians = custodians(path);
        f.config.upgrade = Some(upgrade::Policy::V2(upgrade_policy(f, &custodians)));
        chosen = Some(custodians);
    });
    (root, chosen.unwrap())
}

fn upgrade_v2(app: &ConsensusApplication) -> upgrade::v2::State {
    upgrade_state(&app.storage, &app.config)
        .unwrap()
        .unwrap()
        .as_v2()
        .unwrap()
        .clone()
}
/// A control signed over the window starting one block after the anchor.
fn upgrade_control(
    root: &RootFixture,
    custodians: &Custodians,
    app: &ConsensusApplication,
    anchor: &Info,
    action: upgrade::v2::Action,
) -> Vec<u8> {
    let policy = app.config.upgrade.as_ref().unwrap().as_v2().unwrap();
    let payload = upgrade::v2::Payload {
        schema: 2,
        chain_id: CHAIN.into(),
        genesis_sha256: app.config.app_state_sha256.clone(),
        policy_sha256: policy.sha256().unwrap(),
        source_release_sha512: policy.initial_release_sha512.clone(),
        authority_epoch: 1,
        sequence: upgrade_v2(app).next_sequence(),
        anchor_height: anchor.height,
        anchor_app_hash: anchor.app_hash.clone(),
        not_before_height: anchor.height + 1,
        not_after_height: anchor.height + 3,
        action,
    };
    let artifact = upgrade::v2::artifact_bytes(&payload).unwrap();
    let signatures = signatures(
        root,
        custodians,
        &artifact,
        payload.sequence,
        payload.not_before_height,
        payload.not_after_height,
    );
    serde_json::to_vec(&upgrade::v2::Control {
        kind: upgrade::v2::CONTROL_KIND.into(),
        payload,
        signatures,
    })
    .unwrap()
}
fn plan(app: &ConsensusApplication) -> upgrade::v2::MigrationPlan {
    let policy = app.config.upgrade.as_ref().unwrap().as_v2().unwrap();
    upgrade::v2::MigrationPlan {
        target_release_sha512: policy.initial_release_sha512.clone(),
        migration_id: upgrade::MIGRATION_ID.into(),
        migration_sha256: upgrade::v2::migration_sha256(),
        source_schema: 0,
        target_schema: 1,
        bounds: policy.migration_bounds.clone(),
        authorization_sha256: "55".repeat(32),
    }
}

#[test]
fn schema_two_configuration_couples_the_authorities() {
    let f = Fixture::new();
    let mut config = f.config.clone();
    // Fixed keys suffice: configuration checks never verify a signature.
    let mut keys: Vec<_> = (1..=5u8)
        .map(|i| {
            let public = [0xc0 + i; 64];
            emergency::AuthorityKey {
                key_id: hex::encode(Sha256::digest(public)),
                public_key_hex: hex::encode(public),
            }
        })
        .collect();
    keys.sort_by(|a, b| a.key_id.cmp(&b.key_id));
    let custodians = Custodians {
        authority: emergency::AuthorityPolicy { keys, threshold: 3 },
        keys: BTreeMap::new(),
    };
    let one = |id: &str, byte: u8| emergency::AuthorityPolicy {
        keys: vec![emergency::AuthorityKey {
            key_id: id.into(),
            public_key_hex: hex::encode([byte; 64]),
        }],
        threshold: 1,
    };
    config.emergency = Some(emergency::Policy {
        schema: 1,
        development_only: true,
        chain_id: CHAIN.into(),
        release_sha512: "aa".repeat(64),
        initial_sequence: 1,
        freeze_authority: one("freeze", 0x11),
        resume_authority: one("resume", 0x12),
        max_control_bytes: 65_536,
        max_signatures: 1,
        automatic_transition_policy: emergency::AutomaticTransitionPolicy::ContinueExisting,
        v2: None,
    });
    config.max_tx_bytes = 262_144;
    let mut fixture = Fixture::new();
    fixture.config = config.clone();
    let upgrade = upgrade_policy(&fixture, &custodians);
    config.upgrade = Some(upgrade::Policy::V2(upgrade.clone()));
    config.validate().unwrap();
    // The handover uses the same authority and epoch, and the same schema.
    let handover = handover::Policy {
        schema: 2,
        development_only: true,
        chain_id: CHAIN.into(),
        genesis_sha256: config.app_state_sha256.clone(),
        initial_release_sha512: upgrade.initial_release_sha512.clone(),
        initial_schema: 0,
        authority_epoch: 1,
        authority: upgrade.authority.clone(),
        initial_sequence: 1,
        max_control_bytes: 262_144,
        max_signatures: 5,
        v2: Some(handover::PolicyV2 {
            min_notice_blocks: NOTICE,
            max_validity_blocks: 4,
            max_anchor_age_blocks: 4,
        }),
    };
    config.release_handover = Some(handover.clone());
    config.validate().unwrap();
    let mut other = handover.clone();
    other.authority_epoch = 2;
    config.release_handover = Some(other);
    let error = config.validate().unwrap_err().to_string();
    assert!(error.contains("upgrade authority"), "{error}");
    let mut v1 = handover.clone();
    v1.schema = 1;
    v1.v2 = None;
    config.release_handover = Some(v1);
    let error = config.validate().unwrap_err().to_string();
    assert!(error.contains("schemas differ"), "{error}");
    // No upgrade custodian may hold an emergency role.
    let mut shared = upgrade.clone();
    shared.authority.keys[0].public_key_hex = hex::encode([0x11; 64]);
    shared.authority.keys[0].key_id = hex::encode(Sha256::digest([0x11; 64]));
    shared
        .authority
        .keys
        .sort_by(|a, b| a.key_id.cmp(&b.key_id));
    config.release_handover = None;
    config.upgrade = Some(upgrade::Policy::V2(shared));
    let error = config.validate().unwrap_err().to_string();
    assert!(error.contains("share a key"), "{error}");
}

#[test]
#[ignore = "Requires pinned real SLH helper and disposable fixture signers"]
fn schema_two_upgrade_waits_for_notice_binds_its_anchor_and_replays() {
    let mut f = Fixture::new();
    let (root, custodians) = root(&mut f);
    let directory = tempfile::tempdir().unwrap();
    let mut app = root.initialized(&f, directory.path());
    commit(&mut app, 1, vec![]);
    let anchor = app.info().unwrap();
    let admit = upgrade_control(
        &root,
        &custodians,
        &app,
        &anchor,
        upgrade::v2::Action::Admit { plan: plan(&app) },
    );
    // A control naming another anchor hash is refused before any write.
    let mut forged: upgrade::v2::Control = serde_json::from_slice(&admit).unwrap();
    forged.payload.anchor_app_hash = "00".repeat(32);
    let artifact = upgrade::v2::artifact_bytes(&forged.payload).unwrap();
    forged.signatures = signatures(&root, &custodians, &artifact, 1, 2, 4);
    let before = data(&app);
    let refused = app.check_tx(&serde_json::to_vec(&forged).unwrap());
    assert_ne!(refused.code, 0);
    assert!(refused.log.contains("anchor"), "{}", refused.log);
    assert_eq!(data(&app), before);
    assert_admitted(app.check_tx(&admit));
    commit(&mut app, 2, vec![admit]);
    let state = upgrade_v2(&app);
    let pending = state.pending().unwrap().clone();
    assert_eq!(pending.admitted_height, 2);
    let status = app.query().unwrap();
    assert_eq!(status["upgrade"]["schema"], 2);
    assert_eq!(status["upgrade"]["pending"]["admitted_height"], 2);
    let activation = |app: &ConsensusApplication| {
        let anchor = app.info().unwrap();
        upgrade_control(
            &root,
            &custodians,
            app,
            &anchor,
            upgrade::v2::Action::Activate {
                plan: pending.plan.clone(),
                admission_receipt_sha256: pending.admission_receipt_sha256.clone(),
                emergency_receipt_sha256: None,
                evidence_sha256: "66".repeat(32),
            },
        )
    };
    // Admitted at 2 with notice 3: block 3 is too early.
    let early = activation(&app);
    let refused = app.check_tx(&early);
    assert_ne!(refused.code, 0);
    assert!(refused.log.contains("notice"), "{}", refused.log);
    commit(&mut app, 3, vec![]);
    commit(&mut app, 4, vec![]);
    let activate = activation(&app);
    assert_admitted(app.check_tx(&activate));
    commit(&mut app, 5, vec![activate]);
    let state = upgrade_v2(&app);
    assert_eq!(state.active_schema(), 1);
    assert!(state.pending().is_none());
    assert_eq!(
        app.query_emergency_receipt(&"77".repeat(32)).unwrap()["status"],
        "receipt_absent"
    );
    // The anchors both controls named are kept with the history.
    let anchors = control_anchors(&app.storage, &app.config).unwrap();
    assert!(anchors.contains(&1) && anchors.contains(&4), "{anchors:?}");
    // Restart replays every signature through the real helper.
    drop(app);
    let app = root.open(&f, directory.path()).unwrap();
    assert_eq!(app.info().unwrap().height, 5);
    assert_eq!(upgrade_v2(&app).active_schema(), 1);
}

/// Upgrade and handover schema 2 under one custodian group, with a release
/// manifest for this test executable.
fn handover_root(f: &mut Fixture) -> (RootFixture, Custodians) {
    use sha2::Sha512;
    let executable = std::fs::read(std::env::current_exe().unwrap()).unwrap();
    let manifest = crate::runtime_candidate::ManifestV1 {
        schema: 1,
        chain_id: CHAIN.into(),
        app_genesis_sha256: f.config.app_state_sha256.clone(),
        target: crate::runtime_candidate::Target {
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
        },
        consensus_stdio: crate::runtime_candidate::Executable {
            bytes: executable.len() as u64,
            sha256: hex::encode(Sha256::digest(&executable)),
            sha512: hex::encode(Sha512::digest(&executable)),
        },
        migration_registry_sha256: upgrade::registry_sha256(),
    };
    let mut chosen = None;
    let root = RootFixture::with_release_manifest(
        f,
        &serde_json::to_vec(&manifest).unwrap(),
        |f, path| {
            f.config.max_tx_bytes = 262_144;
            f.config.max_txs = 3;
            let custodians = custodians(path);
            let upgrade = upgrade_policy(f, &custodians);
            f.config.release_handover = Some(handover::Policy {
                schema: 2,
                development_only: true,
                chain_id: CHAIN.into(),
                genesis_sha256: f.config.app_state_sha256.clone(),
                initial_release_sha512: upgrade.initial_release_sha512.clone(),
                initial_schema: 0,
                authority_epoch: 1,
                authority: custodians.authority.clone(),
                initial_sequence: 1,
                max_control_bytes: 262_144,
                max_signatures: 5,
                v2: Some(handover::PolicyV2 {
                    min_notice_blocks: NOTICE,
                    max_validity_blocks: 4,
                    max_anchor_age_blocks: 4,
                }),
            });
            f.config.upgrade = Some(upgrade::Policy::V2(upgrade));
            chosen = Some(custodians);
        },
    );
    (root, chosen.unwrap())
}
fn open_candidate(
    root: &RootFixture,
    f: &Fixture,
    path: &Path,
) -> anyhow::Result<ConsensusApplication> {
    ConsensusApplication::open_with_development_candidate(
        path,
        f.config.clone(),
        f.genesis.clone(),
        &root.source,
        root.root.clone(),
        root.verifier.clone(),
        Some(DevelopmentCandidateInput {
            manifest_path: std::fs::canonicalize(&root.root.release_manifest_path).unwrap(),
            max_manifest_bytes: 65536,
            max_executable_bytes: 1024 * 1024 * 1024,
        }),
    )
}
fn handover_control(
    root: &RootFixture,
    custodians: &Custodians,
    app: &ConsensusApplication,
    anchor: &Info,
    action: handover::Action,
) -> Vec<u8> {
    let state = handover_state(&app.storage, &app.config).unwrap().unwrap();
    let policy = app.config.release_handover.as_ref().unwrap();
    let payload = handover::Payload {
        schema: 2,
        chain_id: CHAIN.into(),
        genesis_sha256: app.config.app_state_sha256.clone(),
        policy_sha256: policy.sha256().unwrap(),
        source_release_sha512: state.active_release_sha512().into(),
        authority_epoch: 1,
        sequence: state.next_sequence(),
        parent_height: anchor.height,
        parent_app_hash: anchor.app_hash.clone(),
        target_height: anchor.height + 1,
        action,
        v2: Some(handover::PayloadV2 {
            not_before_height: anchor.height + 1,
            not_after_height: anchor.height + 3,
        }),
    };
    let artifact = handover::artifact_bytes(&payload).unwrap();
    let signatures = signatures(
        root,
        custodians,
        &artifact,
        payload.sequence,
        anchor.height + 1,
        anchor.height + 3,
    );
    serde_json::to_vec(&handover::Control {
        kind: handover::CONTROL_KIND_V2.into(),
        payload,
        signatures,
    })
    .unwrap()
}

#[test]
#[ignore = "Requires pinned real SLH helper and disposable fixture signers"]
fn schema_two_handover_pairs_with_the_migration_after_notice() {
    let mut f = Fixture::new();
    let (root, custodians) = handover_root(&mut f);
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("state");
    let mut app = open_candidate(&root, &f, &db).unwrap();
    app.init_chain(CHAIN, 1, &f.genesis, &f.config.validators)
        .unwrap();
    commit(&mut app, 1, vec![]);
    let anchor = app.info().unwrap();
    let up = upgrade_control(
        &root,
        &custodians,
        &app,
        &anchor,
        upgrade::v2::Action::Admit { plan: plan(&app) },
    );
    let release = handover::ReleasePlan {
        target_release_sha512: "ab".repeat(64),
        transition: handover::Transition::ReceiptIndexV1 {
            migration_sha256: upgrade::v2::migration_sha256(),
        },
        authorization_sha256: "31".repeat(32),
    };
    let hand = handover_control(
        &root,
        &custodians,
        &app,
        &anchor,
        handover::Action::Admit { plan: release },
    );
    assert_admitted(app.check_tx(&hand));
    commit(&mut app, 2, vec![up, hand]);
    // Both admissions replay through the real helper on restart.
    drop(app);
    let mut app = open_candidate(&root, &f, &db).unwrap();
    assert_eq!(app.info().unwrap().height, 2);
    let up_pending = upgrade_v2(&app).pending().unwrap().clone();
    let hand_pending = handover_state(&app.storage, &app.config)
        .unwrap()
        .unwrap()
        .pending()
        .unwrap()
        .clone();
    let pair = |app: &ConsensusApplication| {
        let anchor = app.info().unwrap();
        let up = upgrade_control(
            &root,
            &custodians,
            app,
            &anchor,
            upgrade::v2::Action::Activate {
                plan: up_pending.plan.clone(),
                admission_receipt_sha256: up_pending.admission_receipt_sha256.clone(),
                emergency_receipt_sha256: None,
                evidence_sha256: "33".repeat(32),
            },
        );
        let hand = handover_control(
            &root,
            &custodians,
            app,
            &anchor,
            handover::Action::Activate {
                plan: hand_pending.plan.clone(),
                admission_receipt_sha256: hand_pending.admission_receipt_sha256.clone(),
                emergency_receipt_sha256: None,
                evidence_sha256: "34".repeat(32),
                upgrade_activation_sha256: Some(hex::encode(Sha256::digest(&up))),
            },
        );
        (up, hand)
    };
    // Admitted at 2 with notice 3: the handover cannot activate at 3.
    let (_, early) = pair(&app);
    let refused = app.check_tx(&early);
    assert_ne!(refused.code, 0);
    assert!(refused.log.contains("notice"), "{}", refused.log);
    commit(&mut app, 3, vec![]);
    commit(&mut app, 4, vec![]);
    let (up, hand) = pair(&app);
    assert_admitted(app.check_tx(&hand));
    // Either control alone is refused; the pair commits in one batch.
    assert!(!app.process_proposal(block(5, vec![hand.clone()])).unwrap());
    commit(&mut app, 5, vec![up, hand]);
    let status = app.query().unwrap();
    assert_eq!(
        status["release_handover"]["active_release_sha512"],
        "ab".repeat(64)
    );
    assert_eq!(status["release_handover"]["active_schema"], 1);
    assert_eq!(upgrade_v2(&app).active_schema(), 1);
    let anchors = control_anchors(&app.storage, &app.config).unwrap();
    assert!(anchors.contains(&1) && anchors.contains(&4), "{anchors:?}");
    // The committed release now requires another executable.
    drop(app);
    let error = open_candidate(&root, &f, &db).err().unwrap().to_string();
    assert!(error.contains("manifest digest"), "{error}");
}
