//! Kit replacement through the consensus adapter, with real SLH signatures
//! and RocksDB batches (root kit replacement v1, K2b).
use super::emergency_tests::RootFixture;
use super::*;
use crate::emergency_freeze::AuthorityKey;
use crate::{emergency_freeze as emergency, kit_replacement as replacement};
use crate::{release_handover as handover, upgrade};
use std::path::Path;
use std::process::Command;

/// Blocks from admission to the effect height, in this fixture.
const NOTICE: u64 = 3;

/// One disposable SLH signature by fixture key `key` for heights
/// `first..=last`.
fn sign(
    path: &Path,
    artifact: &[u8],
    action: &str,
    (sequence, first, last): (u64, u64, u64),
    key: u8,
) -> serde_json::Value {
    let input = path.join("replacement-artifact.bin");
    let output = path.join("replacement-public.json");
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
fn signature(
    path: &Path,
    artifact: &[u8],
    action: &str,
    window: (u64, u64, u64),
    key: u8,
) -> Vec<u8> {
    let signed = sign(path, artifact, action, window, key);
    B64.decode(signed["Request"]["Signature"].as_str().unwrap())
        .unwrap()
}
/// Fixture key `key`, under its SHA-256 key ID as in production.
fn public_key(path: &Path, key: u8) -> AuthorityKey {
    let signed = sign(path, b"disposable-public-key", "upgrade", (1, 1, 1), key);
    let public = B64.decode(signed["PublicKey"].as_str().unwrap()).unwrap();
    AuthorityKey {
        key_id: hex::encode(Sha256::digest(&public)),
        public_key_hex: hex::encode(public),
    }
}
/// Five fixture keys from `first`, three needed.
fn set(path: &Path, first: u8) -> (emergency::AuthorityPolicy, BTreeMap<u8, AuthorityKey>) {
    let keys: BTreeMap<u8, AuthorityKey> = (first..first + 5)
        .map(|key| (key, public_key(path, key)))
        .collect();
    let mut sorted: Vec<_> = keys.values().cloned().collect();
    sorted.sort_by(|a, b| a.key_id.cmp(&b.key_id));
    (
        emergency::AuthorityPolicy {
            keys: sorted,
            threshold: 3,
        },
        keys,
    )
}

/// Every fixture key the test signs with, by number.
struct Kits(BTreeMap<u8, AuthorityKey>);
impl Kits {
    fn key(&self, number: u8) -> &AuthorityKey {
        &self.0[&number]
    }
}

/// Emergency, upgrade and handover policies at schema 2, as in production:
/// freeze keys 11 to 15, resume keys 21 to 25, upgrade keys 41 to 45. A
/// handover policy needs a release manifest for this test executable.
fn root(f: &mut Fixture) -> (RootFixture, Kits) {
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
    let manifest = serde_json::to_vec(&manifest).unwrap();
    let mut chosen = None;
    let root = RootFixture::with_release_manifest(f, &manifest, |f, path| {
        f.config.max_tx_bytes = 262_144;
        let (freeze, mut keys) = set(path, 11);
        let (resume, resume_keys) = set(path, 21);
        let (authority, upgrade_keys) = set(path, 41);
        keys.extend(resume_keys);
        keys.extend(upgrade_keys);
        for key in [61, 62, 63, 64] {
            keys.insert(key, public_key(path, key));
        }
        let genesis_sha256 = f.config.app_state_sha256.clone();
        let emergency = f.config.emergency.as_mut().unwrap();
        emergency.schema = 2;
        emergency.max_control_bytes = 262_144;
        emergency.max_signatures = 3;
        emergency.freeze_authority = freeze;
        emergency.resume_authority = resume;
        emergency.v2 = Some(emergency::PolicyV2 {
            genesis_sha256: genesis_sha256.clone(),
            authority_epoch: 1,
            max_validity_blocks: 4,
            max_anchor_age_blocks: 4,
        });
        let release = emergency.release_sha512.clone();
        f.config.upgrade = Some(upgrade::Policy::V2(upgrade::v2::Policy {
            schema: 2,
            chain_id: CHAIN.into(),
            genesis_sha256: genesis_sha256.clone(),
            initial_release_sha512: release.clone(),
            authority_epoch: 1,
            authority: authority.clone(),
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
        }));
        f.config.release_handover = Some(handover::Policy {
            schema: 2,
            development_only: true,
            chain_id: CHAIN.into(),
            genesis_sha256,
            initial_release_sha512: release,
            initial_schema: 0,
            authority_epoch: 1,
            authority,
            initial_sequence: 1,
            max_control_bytes: 262_144,
            max_signatures: 5,
            v2: Some(handover::PolicyV2 {
                min_notice_blocks: NOTICE,
                max_validity_blocks: 4,
                max_anchor_age_blocks: 4,
            }),
        });
        chosen = Some(Kits(keys));
    });
    (root, chosen.unwrap())
}

fn open(root: &RootFixture, f: &Fixture, path: &Path) -> anyhow::Result<ConsensusApplication> {
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

/// A replacement anchored at `anchor` for the next three blocks: the seat
/// `old` (upgrade, freeze and resume fixture keys) leaves for `new`, signed
/// by the upgrade keys `signers`.
fn replacement(
    root: &RootFixture,
    kits: &Kits,
    app: &ConsensusApplication,
    anchor: &Info,
    signers: &[u8],
    old: [u8; 3],
    new: [u8; 3],
) -> Vec<u8> {
    let policy = app.config.upgrade.as_ref().unwrap().as_v2().unwrap();
    let id = |key| kits.key(key).key_id.clone();
    let payload = replacement::Payload {
        schema: 1,
        chain_id: CHAIN.into(),
        genesis_sha256: app.config.app_state_sha256.clone(),
        policy_sha256: policy.sha256().unwrap(),
        authority_epoch: 1,
        sequence: 2,
        anchor_height: anchor.height,
        anchor_app_hash: anchor.app_hash.clone(),
        not_before_height: anchor.height + 1,
        not_after_height: anchor.height + 3,
        replaced: replacement::Roles {
            upgrade: id(old[0]),
            freeze: id(old[1]),
            resume: id(old[2]),
        },
        keys: replacement::Roles {
            upgrade: kits.key(new[0]).clone(),
            freeze: kits.key(new[1]).clone(),
            resume: kits.key(new[2]).clone(),
        },
    };
    let artifact = replacement::artifact_bytes(&payload).unwrap();
    let window = (2, anchor.height + 1, anchor.height + 3);
    let path = root._directory.path();
    let signed = |key| B64.encode(signature(path, &artifact, "upgrade", window, key));
    let mut signatures: Vec<_> = signers
        .iter()
        .map(|&key| replacement::Signature {
            key_id: id(key),
            signature_base64: signed(key),
        })
        .collect();
    signatures.sort_by(|a, b| a.key_id.cmp(&b.key_id));
    serde_json::to_vec(&replacement::Control {
        kind: replacement::CONTROL_KIND.into(),
        payload,
        signatures,
        proofs: replacement::Roles {
            upgrade: signed(new[0]),
            freeze: signed(new[1]),
            resume: signed(new[2]),
        },
    })
    .unwrap()
}

/// A freeze naming `epoch`, anchored at `anchor` for the next three blocks,
/// signed by the freeze keys `signers`.
fn freeze(
    root: &RootFixture,
    kits: &Kits,
    app: &ConsensusApplication,
    anchor: &Info,
    epoch: u64,
    signers: &[u8],
) -> Vec<u8> {
    let action = emergency::Action::Freeze;
    emergency_control(root, kits, app, anchor, (epoch, action), signers)
}
/// An emergency action naming `epoch`, as `freeze`.
fn emergency_control(
    root: &RootFixture,
    kits: &Kits,
    app: &ConsensusApplication,
    anchor: &Info,
    (epoch, action): (u64, emergency::Action),
    signers: &[u8],
) -> Vec<u8> {
    let policy = app.config.emergency.as_ref().unwrap();
    let state = emergency_state(&app.storage, &app.config).unwrap().unwrap();
    let (first, last) = (anchor.height + 1, anchor.height + 3);
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
            genesis_sha256: app.config.app_state_sha256.clone(),
            authority_epoch: epoch,
            policy_sha256: policy.sha256().unwrap(),
            not_before_height: first,
            not_after_height: last,
            incident_sha256: "33".repeat(32),
            resume: (action == emergency::Action::Resume).then(|| emergency::ResumeBinding {
                freeze_receipt_sha256: state.freeze_receipt_sha256().unwrap().into(),
                restored_state_sha256: anchor.app_hash.clone(),
                readiness_evidence_sha256: "44".repeat(32),
            }),
        }),
    };
    let artifact = emergency::artifact_bytes(&payload).unwrap();
    let path = root._directory.path();
    let mut signatures: Vec<_> = signers
        .iter()
        .map(|&key| emergency::ControlSignature {
            key_id: kits.key(key).key_id.clone(),
            signature_hex: hex::encode(signature(
                path,
                &artifact,
                "emergency",
                (payload.sequence, first, last),
                key,
            )),
        })
        .collect();
    signatures.sort_by(|a, b| a.key_id.cmp(&b.key_id));
    serde_json::to_vec(&emergency::Control {
        kind: emergency::CONTROL_KIND_V2.into(),
        payload,
        signatures,
    })
    .unwrap()
}

/// An upgrade admission naming `epoch`, anchored at `anchor` for the next
/// three blocks, signed by the upgrade keys `signers`.
fn upgrade_admission(
    root: &RootFixture,
    kits: &Kits,
    app: &ConsensusApplication,
    anchor: &Info,
    epoch: u64,
    signers: &[u8],
) -> Vec<u8> {
    let policy = app.config.upgrade.as_ref().unwrap().as_v2().unwrap();
    let state = upgrade_state(&app.storage, &app.config).unwrap().unwrap();
    let plan = upgrade::v2::MigrationPlan {
        target_release_sha512: policy.initial_release_sha512.clone(),
        migration_id: upgrade::MIGRATION_ID.into(),
        migration_sha256: upgrade::v2::migration_sha256(),
        source_schema: 0,
        target_schema: 1,
        bounds: policy.migration_bounds.clone(),
        authorization_sha256: "55".repeat(32),
    };
    let (first, last) = (anchor.height + 1, anchor.height + 3);
    let payload = upgrade::v2::Payload {
        schema: 2,
        chain_id: CHAIN.into(),
        genesis_sha256: app.config.app_state_sha256.clone(),
        policy_sha256: policy.sha256().unwrap(),
        source_release_sha512: policy.initial_release_sha512.clone(),
        authority_epoch: epoch,
        sequence: state.next_sequence(),
        anchor_height: anchor.height,
        anchor_app_hash: anchor.app_hash.clone(),
        not_before_height: first,
        not_after_height: last,
        action: upgrade::v2::Action::Admit { plan },
    };
    let artifact = upgrade::v2::artifact_bytes(&payload).unwrap();
    let path = root._directory.path();
    let mut signatures: Vec<_> = signers
        .iter()
        .map(|&key| emergency::ControlSignature {
            key_id: kits.key(key).key_id.clone(),
            signature_hex: hex::encode(signature(
                path,
                &artifact,
                "upgrade",
                (payload.sequence, first, last),
                key,
            )),
        })
        .collect();
    signatures.sort_by(|a, b| a.key_id.cmp(&b.key_id));
    serde_json::to_vec(&upgrade::v2::Control {
        kind: upgrade::v2::CONTROL_KIND.into(),
        payload,
        signatures,
    })
    .unwrap()
}

#[test]
#[ignore = "Requires pinned real SLH helper and disposable fixture signers"]
fn actual_kit_replacement_takes_effect_after_its_notice_and_replays() {
    let mut f = Fixture::new();
    let (root, kits) = root(&mut f);
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("state");
    let mut app = open(&root, &f, &db).unwrap();
    app.init_chain(CHAIN, 1, &f.genesis, &f.config.validators)
        .unwrap();
    commit(&mut app, 1, vec![]);
    let anchor = app.info().unwrap();
    // A new kit (fixture keys 61, 62 and 63) replaces the seat of keys 45,
    // 15 and 25, signed by three of the current upgrade keys.
    let control = replacement(
        &root,
        &kits,
        &app,
        &anchor,
        &[41, 42, 43],
        [45, 15, 25],
        [61, 62, 63],
    );
    // A proof made by another key does not show possession of the new one.
    let mut forged: replacement::Control = serde_json::from_slice(&control).unwrap();
    let artifact = replacement::artifact_bytes(&forged.payload).unwrap();
    forged.proofs.freeze = B64.encode(signature(
        root._directory.path(),
        &artifact,
        "upgrade",
        (2, 2, 4),
        64,
    ));
    let before = data(&app);
    let refused = app.check_tx(&serde_json::to_vec(&forged).unwrap());
    assert_ne!(refused.code, 0);
    assert!(
        refused.log.contains("proof of possession"),
        "{}",
        refused.log
    );
    assert_admitted(app.check_tx(&control));
    assert_eq!(data(&app), before, "Admission must not write state");
    // It takes a block of its own.
    assert_eq!(
        app.prepare_proposal(2, 20, 0, vec![control.clone()], 1_048_576)
            .unwrap(),
        vec![control.clone()]
    );
    assert!(app
        .process_proposal(block(2, vec![control.clone()]))
        .unwrap());
    let result = commit(&mut app, 2, vec![control.clone()]);
    assert_eq!(result.tx_results, vec![replacement_result()]);
    let record = root_record(&app.storage).unwrap().unwrap();
    assert_eq!(record.epochs.len(), 2);
    assert_eq!(
        (
            record.epochs[1].authority_epoch,
            record.epochs[1].from_height
        ),
        (2, 2 + NOTICE)
    );
    let status = app.query().unwrap();
    assert_eq!(status["root_authority"]["authority_epoch"], 1);
    assert_eq!(status["root_authority"]["pending"]["authority_epoch"], 2);
    assert_eq!(
        status["root_authority"]["pending"]["effect_height"],
        2 + NOTICE
    );
    // The same replacement is not applied twice, and no other is admitted
    // while it is pending.
    let refused = app.check_tx(&control);
    assert_ne!(refused.code, 0);
    assert!(refused.log.contains("pending"), "{}", refused.log);

    // Freezes anchored at block 3 for blocks 4 to 6: the replaced key 15
    // signs until the effect height (5), and the new key 62 from it.
    commit(&mut app, 3, vec![]);
    let anchor = app.info().unwrap();
    let old = freeze(&root, &kits, &app, &anchor, 1, &[11, 12, 15]);
    let early = freeze(&root, &kits, &app, &anchor, 1, &[11, 12, 62]);
    let stale = freeze(&root, &kits, &app, &anchor, 2, &[11, 12, 15]);
    let new = freeze(&root, &kits, &app, &anchor, 2, &[11, 12, 62]);
    assert_admitted(app.check_tx(&old));
    for refused in [&early, &stale, &new] {
        assert_ne!(app.check_tx(refused).code, 0);
    }
    commit(&mut app, 4, vec![]);
    assert_eq!(app.query().unwrap()["root_authority"]["authority_epoch"], 2);
    assert!(app.query().unwrap()["root_authority"]["pending"].is_null());
    for refused in [&old, &early, &stale] {
        assert_ne!(app.check_tx(refused).code, 0);
    }
    assert_admitted(app.check_tx(&new));
    commit(&mut app, 5, vec![new]);
    assert!(emergency_state(&app.storage, &app.config)
        .unwrap()
        .unwrap()
        .frozen());
    // While frozen, a replacement waits, as upgrades do (P01, 8 October 2026).
    let refused = app.check_tx(&control);
    assert_ne!(refused.code, 0);
    assert!(refused.log.contains(EMERGENCY_FROZEN), "{}", refused.log);
    assert!(app
        .prepare_proposal(6, 60, 0, vec![control.clone()], 1_048_576)
        .unwrap()
        .is_empty());

    // Restart replays the replacement and the freeze with the real helper,
    // each with the keys of its block.
    drop(app);
    let mut app = open(&root, &f, &db).unwrap();
    assert_eq!(app.info().unwrap().height, 5);
    assert_eq!(app.query().unwrap()["root_authority"]["authority_epoch"], 2);
    assert_eq!(root_record(&app.storage).unwrap().unwrap(), record);
    // The new kit resumes the chain, and an upgrade signed with the new
    // upgrade key is admitted: its emergency history spans both epochs, each
    // record checked with its own block's keys.
    let anchor = app.info().unwrap();
    let action = (2, emergency::Action::Resume);
    let resume = emergency_control(&root, &kits, &app, &anchor, action, &[21, 22, 63]);
    assert_admitted(app.check_tx(&resume));
    commit(&mut app, 6, vec![resume]);
    let anchor = app.info().unwrap();
    let admission = upgrade_admission(&root, &kits, &app, &anchor, 2, &[41, 42, 61]);
    assert_admitted(app.check_tx(&admission));
    commit(&mut app, 7, vec![admission]);
    assert!(app.query().unwrap()["upgrade"]["pending"].is_object());
    drop(app);
    let app = open(&root, &f, &db).unwrap();
    assert_eq!(app.info().unwrap().height, 7);
    // A record no committed replacement wrote is refused: it is outside
    // the committed state, and the replay would not recreate it.
    let mut forged = record.clone();
    forged.epochs[1].from_height += 1;
    app.storage
        .db
        .put(crate::root_authority::STATE_KEY, forged.encode().unwrap())
        .unwrap();
    let refused = app.check_tx(&control);
    assert!(
        refused.log.contains("Committed state tree root differs"),
        "{}",
        refused.log
    );
    drop(app);
    assert!(open(&root, &f, &db).is_err());
}
