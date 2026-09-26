//! Actual signed control planning and one common RocksDB activation batch.
use super::emergency_tests::RootFixture;
use super::*;
use crate::{emergency_freeze as emergency, release_handover as handover, upgrade};
use sha2::Sha512;
use std::{path::Path, process::Command};

fn signed(
    root: &Path,
    artifact: &[u8],
    sequence: u64,
    height: u64,
    key: u8,
    action: &str,
) -> serde_json::Value {
    let input = root.join("handover-artifact");
    let output = root.join("handover-signature.json");
    std::fs::write(&input, artifact).unwrap();
    let result = Command::new(std::env::var("DYT_UPGRADE_TEST_SIGNER").unwrap())
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
            &height.to_string(),
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
    root: &RootFixture,
    artifact: &[u8],
    sequence: u64,
    height: u64,
    key: u8,
    id: &str,
    action: &str,
) -> emergency::ControlSignature {
    let value = signed(
        root._directory.path(),
        artifact,
        sequence,
        height,
        key,
        action,
    );
    emergency::ControlSignature {
        key_id: id.into(),
        signature_hex: hex::encode(
            B64.decode(value["Request"]["Signature"].as_str().unwrap())
                .unwrap(),
        ),
    }
}
fn setup(f: &mut Fixture) -> RootFixture {
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
    RootFixture::with_release_manifest(f, &serde_json::to_vec(&manifest).unwrap(), |f, path| {
        f.config.max_tx_bytes = 262_144;
        let value = signed(path, b"public-key-fixture", 1, 1, 31, "upgrade");
        let authority = emergency::AuthorityPolicy {
            threshold: 1,
            keys: vec![emergency::AuthorityKey {
                key_id: "upgrade-31".into(),
                public_key_hex: hex::encode(
                    B64.decode(value["PublicKey"].as_str().unwrap()).unwrap(),
                ),
            }],
        };
        let release = f.config.emergency.as_ref().unwrap().release_sha512.clone();
        f.config.upgrade = Some(upgrade::Policy {
            schema: 1,
            development_only: true,
            chain_id: CHAIN.into(),
            genesis_sha256: f.config.app_state_sha256.clone(),
            source_release_sha512: release.clone(),
            authority_epoch: 1,
            authority: authority.clone(),
            initial_sequence: 1,
            max_control_bytes: 262_144,
            max_signatures: 1,
            migration_bounds: upgrade::MigrationBounds {
                max_receipts: 16,
                max_receipt_bytes: 4 * 1024 * 1024,
                max_write_bytes: 65536,
            },
        });
        f.config.release_handover = Some(handover::Policy {
            schema: 1,
            development_only: true,
            chain_id: CHAIN.into(),
            genesis_sha256: f.config.app_state_sha256.clone(),
            initial_release_sha512: release,
            initial_schema: 0,
            authority_epoch: 1,
            authority,
            initial_sequence: 1,
            max_control_bytes: 262_144,
            max_signatures: 1,
        });
    })
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
fn start(root: &RootFixture, f: &Fixture, path: &Path) -> ConsensusApplication {
    let mut app = open(root, f, path).unwrap();
    app.init_chain(CHAIN, 1, &f.genesis, &f.config.validators)
        .unwrap();
    app
}
fn emergency_control(
    root: &RootFixture,
    app: &ConsensusApplication,
    action: emergency::Action,
) -> Vec<u8> {
    let info = app.info().unwrap();
    let state = emergency_state(&app.storage, &app.config).unwrap().unwrap();
    let policy = app.config.emergency.as_ref().unwrap();
    let payload = emergency::Payload {
        schema: 1,
        chain_id: CHAIN.into(),
        release_sha512: policy.release_sha512.clone(),
        action,
        sequence: state.next_sequence(),
        parent_height: info.height,
        parent_app_hash: info.app_hash,
        target_height: info.height + 1,
        v2: None,
    };
    let (key, id) = if action == emergency::Action::Freeze {
        (11, "freeze-key")
    } else {
        (12, "resume-key")
    };
    let signatures = vec![signature(
        root,
        &emergency::artifact_bytes(&payload).unwrap(),
        payload.sequence,
        payload.target_height,
        key,
        id,
        "emergency",
    )];
    serde_json::to_vec(&emergency::Control {
        kind: emergency::CONTROL_KIND.into(),
        payload,
        signatures,
    })
    .unwrap()
}
fn handover_control(
    root: &RootFixture,
    app: &ConsensusApplication,
    action: handover::Action,
) -> Vec<u8> {
    let info = app.info().unwrap();
    let state = handover_state(&app.storage, &app.config).unwrap().unwrap();
    let policy = app.config.release_handover.as_ref().unwrap();
    let payload = handover::Payload {
        schema: 1,
        chain_id: CHAIN.into(),
        genesis_sha256: app.config.app_state_sha256.clone(),
        policy_sha256: policy.sha256().unwrap(),
        source_release_sha512: state.active_release_sha512().into(),
        authority_epoch: 1,
        sequence: state.next_sequence(),
        parent_height: info.height,
        parent_app_hash: info.app_hash,
        target_height: info.height + 1,
        action,
    };
    let signatures = vec![signature(
        root,
        &handover::artifact_bytes(&payload).unwrap(),
        payload.sequence,
        payload.target_height,
        31,
        "upgrade-31",
        "upgrade",
    )];
    serde_json::to_vec(&handover::Control {
        kind: handover::CONTROL_KIND.into(),
        payload,
        signatures,
    })
    .unwrap()
}
fn upgrade_control(
    root: &RootFixture,
    app: &ConsensusApplication,
    action: upgrade::Action,
) -> Vec<u8> {
    let info = app.info().unwrap();
    let state = upgrade_state(&app.storage, &app.config).unwrap().unwrap();
    let policy = app.config.upgrade.as_ref().unwrap();
    let payload = upgrade::Payload {
        schema: 1,
        chain_id: CHAIN.into(),
        genesis_sha256: app.config.app_state_sha256.clone(),
        source_release_sha512: policy.source_release_sha512.clone(),
        authority_epoch: 1,
        sequence: state.next_sequence(),
        parent_height: info.height,
        parent_app_hash: info.app_hash,
        target_height: info.height + 1,
        action,
    };
    let signatures = vec![signature(
        root,
        &upgrade::artifact_bytes(&payload).unwrap(),
        payload.sequence,
        payload.target_height,
        31,
        "upgrade-31",
        "upgrade",
    )];
    serde_json::to_vec(&upgrade::Control {
        kind: upgrade::CONTROL_KIND.into(),
        payload,
        signatures,
    })
    .unwrap()
}
fn release_plan() -> handover::ReleasePlan {
    handover::ReleasePlan {
        target_release_sha512: "ab".repeat(64),
        transition: handover::Transition::ReceiptIndexV1 {
            migration_sha256: upgrade::migration_sha256(),
        },
        authorization_sha256: "31".repeat(32),
    }
}
fn upgrade_plan(app: &ConsensusApplication) -> upgrade::MigrationPlan {
    let policy = app.config.upgrade.as_ref().unwrap();
    upgrade::MigrationPlan {
        target_release_sha512: policy.source_release_sha512.clone(),
        migration_id: upgrade::MIGRATION_ID.into(),
        migration_sha256: upgrade::migration_sha256(),
        source_schema: 0,
        target_schema: 1,
        bounds: policy.migration_bounds.clone(),
        authorization_sha256: "32".repeat(32),
    }
}
#[test]
fn handover_configuration_omission_and_explicit_null_are_distinct() {
    let f = Fixture::new();
    let bytes = serde_json::to_vec(&f.config).unwrap();
    assert!(!String::from_utf8(bytes.clone())
        .unwrap()
        .contains("release_handover"));
    assert_eq!(
        serde_json::to_vec(&serde_json::from_slice::<ConsensusConfig>(&bytes).unwrap()).unwrap(),
        bytes
    );
    let mut value = serde_json::to_value(&f.config).unwrap();
    value["release_handover"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<ConsensusConfig>(value).is_err());
}
#[test]
#[ignore = "requires explicit disposable SLH signing helper"]
fn handover_pair_commits_once_and_source_stops_after_acknowledgement_loss() {
    let mut f = Fixture::new();
    let mut genesis: serde_json::Value = serde_json::from_slice(&f.genesis).unwrap();
    genesis["adaptive_issuance"]["epoch_blocks"] = serde_json::json!(3);
    f.genesis = serde_json::to_vec(&genesis).unwrap();
    let digest: [u8; 32] = Sha256::digest(&f.genesis).into();
    f.config.app_state_sha256 = hex::encode(digest);
    for account in f.config.recovery.as_mut().unwrap().accounts.values_mut() {
        account.recovery.domain.genesis_digest = digest;
    }
    f.config.max_txs = 3;
    let root = setup(&mut f);
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state");
    assert!(root.open(&f, &db).is_err());
    assert!(!db.exists());
    let mut app = start(&root, &f, &db);
    let freeze = emergency_control(&root, &app, emergency::Action::Freeze);
    commit(&mut app, 1, vec![freeze]);
    let resume = emergency_control(&root, &app, emergency::Action::Resume);
    commit(&mut app, 2, vec![resume]);
    let up = upgrade_control(
        &root,
        &app,
        upgrade::Action::Admit {
            plan: upgrade_plan(&app),
        },
    );
    let hand = handover_control(
        &root,
        &app,
        handover::Action::Admit {
            plan: release_plan(),
        },
    );
    assert_eq!(app.check_tx(&hand).code, 0);
    commit(&mut app, 3, vec![up, hand]);
    let upstate = upgrade_state(&app.storage, &app.config).unwrap().unwrap();
    let pending = upstate.pending().unwrap();
    let clearance = emergency_state(&app.storage, &app.config)
        .unwrap()
        .unwrap()
        .last_receipt_sha256()
        .map(str::to_owned);
    let up = upgrade_control(
        &root,
        &app,
        upgrade::Action::Activate {
            plan: pending.plan.clone(),
            admission_receipt_sha256: pending.admission_receipt_sha256.clone(),
            emergency_receipt_sha256: clearance.clone(),
            evidence_sha256: "33".repeat(32),
        },
    );
    let handstate = handover_state(&app.storage, &app.config).unwrap().unwrap();
    let pending = handstate.pending().unwrap();
    let hand = handover_control(
        &root,
        &app,
        handover::Action::Activate {
            plan: pending.plan.clone(),
            admission_receipt_sha256: pending.admission_receipt_sha256.clone(),
            emergency_receipt_sha256: clearance,
            evidence_sha256: "34".repeat(32),
            upgrade_activation_sha256: Some(hex::encode(Sha256::digest(&up))),
        },
    );
    assert_eq!(app.check_tx(&hand).code, 0);
    let observation = serde_json::to_vec(&WireTransaction::EpochObservation {
        observation: EpochObservation {
            epoch: 0,
            utilization_ppm: 500000,
            volatility_ppm: 0,
            first_height: 1,
            last_height: 3,
            parent_hash: block(3, vec![]).hash,
        },
    })
    .unwrap();
    assert!(!app
        .process_proposal(block(4, vec![observation.clone(), up.clone()]))
        .unwrap());
    assert!(!app
        .process_proposal(block(4, vec![observation.clone(), hand.clone()]))
        .unwrap());
    assert!(!app
        .process_proposal(block(4, vec![up.clone(), hand.clone()]))
        .unwrap());
    let candidates = vec![hand.clone(), up.clone(), observation.clone()];
    let full_size = candidates.iter().map(Vec::len).sum::<usize>() as u64;
    assert_eq!(
        app.prepare_proposal(4, 40, 0, candidates.clone(), full_size - 1)
            .unwrap(),
        vec![observation.clone()]
    );
    assert_eq!(
        app.prepare_proposal(4, 40, 0, candidates, full_size)
            .unwrap(),
        vec![observation.clone(), up.clone(), hand.clone()]
    );
    let over_count = block(
        4,
        vec![observation.clone(), up.clone(), hand.clone(), up.clone()],
    );
    let before_over_count = data(&app);
    assert!(!app.process_proposal(over_count.clone()).unwrap());
    assert_eq!(
        app.finalize_block(over_count).unwrap_err().to_string(),
        "Too many block transactions"
    );
    assert_eq!(data(&app), before_over_count);
    let freeze = emergency_control(&root, &app, emergency::Action::Freeze);
    assert_eq!(
        app.prepare_proposal(
            4,
            40,
            0,
            vec![
                up.clone(),
                hand.clone(),
                freeze.clone(),
                observation.clone()
            ],
            4 * 1024 * 1024
        )
        .unwrap(),
        vec![observation.clone(), freeze.clone()]
    );
    assert!(app
        .process_proposal(block(4, vec![observation.clone(), freeze, hand.clone()]))
        .unwrap());
    let input = block(4, vec![observation, up, hand]);
    let before = data(&app);
    let result = app.finalize_block(input.clone()).unwrap();
    assert_eq!(data(&app), before);
    assert!(app
        .commit_with(|_, _| anyhow::bail!("bounded prewrite failure"))
        .is_err());
    assert_eq!(data(&app), before);
    assert!(app
        .commit_with(|storage, batch| {
            write_sync(storage, batch)?;
            anyhow::bail!("acknowledgement lost")
        })
        .is_err());
    let info = app.commit().unwrap();
    assert_eq!(info.app_hash, result.app_hash);
    assert_eq!(app.finalize_block(input).unwrap(), result);
    assert!(app.finalize_block(block(5, vec![])).is_err());
    assert!(app.process_proposal(block(5, vec![])).is_err());
    assert!(app.prepare_proposal(5, 50, 0, vec![], 1024).is_err());
    assert!(app.check_tx_result(b"{}").is_err());
    let query = app.query().unwrap();
    assert_eq!(
        query["release_handover"]["active_release_sha512"],
        "ab".repeat(64)
    );
    assert_eq!(query["release_handover"]["active_schema"], 1);
    assert_eq!(query["emergency_control"]["blocks_upgrade"], true);
    let receipt = emergency_history(&app.storage, &app.config).unwrap()[0]
        .0
        .sha256()
        .unwrap();
    assert_eq!(
        app.query_emergency_receipt(&receipt).unwrap()["status"],
        "committed_receipt_reported"
    );
    drop(app);
    assert!(open(&root, &f, &db)
        .err()
        .unwrap()
        .to_string()
        .contains("manifest digest"));
}
#[test]
#[ignore = "requires explicit disposable SLH signing helper"]
fn handover_cancellation_and_freeze_priority_preserve_history() {
    let mut f = Fixture::new();
    let root = setup(&mut f);
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state");
    let mut app = start(&root, &f, &db);
    let admit = handover_control(
        &root,
        &app,
        handover::Action::Admit {
            plan: release_plan(),
        },
    );
    commit(&mut app, 1, vec![admit]);
    let state = handover_state(&app.storage, &app.config).unwrap().unwrap();
    let pending = state.pending().unwrap();
    let cancel = handover_control(
        &root,
        &app,
        handover::Action::Cancel {
            plan_sha256: pending.plan.sha256().unwrap(),
            admission_receipt_sha256: pending.admission_receipt_sha256.clone(),
        },
    );
    commit(&mut app, 2, vec![cancel]);
    let state = handover_state(&app.storage, &app.config).unwrap().unwrap();
    assert!(state.pending().is_none());
    assert_eq!(state.next_sequence(), 3);
    let admit = handover_control(
        &root,
        &app,
        handover::Action::Admit {
            plan: release_plan(),
        },
    );
    let freeze = emergency_control(&root, &app, emergency::Action::Freeze);
    for txs in [
        vec![freeze.clone(), admit.clone(), admit.clone()],
        vec![admit.clone(), admit.clone(), freeze.clone()],
    ] {
        assert!(app.process_proposal(block(3, txs)).unwrap());
    }
    let result = commit(&mut app, 3, vec![admit.clone(), freeze, admit]);
    assert_eq!(result.tx_results[0].code, 1);
    assert_eq!(result.tx_results[1].code, 0);
    assert_eq!(result.tx_results[2].code, 1);
    assert_eq!(
        handover_state(&app.storage, &app.config).unwrap().unwrap(),
        state
    );
    drop(app);
    let app = open(&root, &f, &db).unwrap();
    assert_eq!(app.info().unwrap().height, 3);
}

#[test]
#[ignore = "requires explicit disposable SLH signing helper"]
fn handover_startup_rejects_missing_verifier_and_structurally_consistent_bad_signature() {
    let mut f = Fixture::new();
    let root = setup(&mut f);
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state");
    let mut app = start(&root, &f, &db);
    let admit = handover_control(
        &root,
        &app,
        handover::Action::Admit {
            plan: release_plan(),
        },
    );
    commit(&mut app, 1, vec![admit]);
    let before = data(&app);
    drop(app);
    let mut missing = root.verifier.clone();
    missing.helper_path = root._directory.path().join("missing-verifier");
    let error = ConsensusApplication::open_with_development_candidate(
        &db,
        f.config.clone(),
        f.genesis.clone(),
        &root.source,
        root.root.clone(),
        missing,
        Some(DevelopmentCandidateInput {
            manifest_path: std::fs::canonicalize(&root.root.release_manifest_path).unwrap(),
            max_manifest_bytes: 65536,
            max_executable_bytes: 1024 * 1024 * 1024,
        }),
    )
    .err()
    .unwrap();
    assert!(
        crate::emergency_verifier::is_infrastructure_error(&error),
        "{error:#}"
    );
    let app = open(&root, &f, &db).unwrap();
    assert_eq!(data(&app), before);
    // Make only this disposable database structurally self-consistent with a
    // wrong historical signature. Startup must still verify actual signatures.
    let policy = app.config.release_handover.as_ref().unwrap();
    let key = handover::receipt_key(1);
    let mut receipt =
        handover::decode_receipt(policy, &app.storage.db.get(&key).unwrap().unwrap()).unwrap();
    let first = &receipt.control.signatures[0].signature_hex[0..2];
    let changed = u8::from_str_radix(first, 16).unwrap() ^ 1;
    receipt.control.signatures[0]
        .signature_hex
        .replace_range(0..2, &format!("{changed:02x}"));
    let state = handover::replay_record(
        policy,
        &handover::State::new(policy).unwrap(),
        &receipt,
        &receipt.context,
        None,
        None,
    )
    .unwrap()
    .state;
    let mut writes = Writes::new();
    writes.insert(
        key.into_bytes(),
        handover::encode_receipt(&receipt).unwrap(),
    );
    writes.insert(
        handover::STATE_KEY.as_bytes().to_vec(),
        handover::encode_state(&state).unwrap(),
    );
    let mut record: BlockRecord =
        decode(&app.storage.db.get(record_key(1)).unwrap().unwrap()).unwrap();
    record.input.txs[0] = serde_json::to_vec(&receipt.control).unwrap();
    record.head.anchor.input_digest = digest(b"dytallix-cometbft-input-v1", &record.input).unwrap();
    record.head.state_digest =
        state_digest(&app.storage, &writes, app.config.governance.is_some()).unwrap();
    record.head.app_hash = app_hash(&record.head.state_digest, &record.head.anchor).unwrap();
    record.result.app_hash = record.head.app_hash.clone();
    let mut batch = WriteBatch::default();
    for (key, value) in writes {
        batch.put(key, value);
    }
    batch.put(record_key(1), serde_json::to_vec(&record).unwrap());
    batch.put(HEAD_KEY, serde_json::to_vec(&record.head).unwrap());
    write_sync(&app.storage, batch).unwrap();
    verify_recovery(&app.storage).unwrap();
    let changed = data(&app);
    drop(app);
    let error = open(&root, &f, &db).err().unwrap();
    assert!(
        error.to_string().contains("Handover signature rejected"),
        "{error:#}"
    );
    let storage = Storage::open(db).unwrap();
    let after = storage
        .db
        .iterator(rocksdb::IteratorMode::Start)
        .map(|item| {
            let (k, v) = item.unwrap();
            (k.to_vec(), v.to_vec())
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(after, changed);
}

#[path = "history_validation_tests.rs"]
mod history_validation_tests;
