//! Actual RocksDB and helper qualification under an ordinary-enabled fixture.
//! Synthetic keys and ContinueExisting are not production policy or custody.
use super::*;
use crate::emergency_freeze as emergency;
use crate::emergency_verifier::EmergencyVerifierConfig;
use crate::root_genesis::DevelopmentRootGenesis;
use sha2::Sha512;
use std::path::{Path, PathBuf};
use std::process::Command;

const RELEASE: &[u8] = b"{\"scope\":\"emergency-development-fixture\"}";
const ENGINE: &[u8] = b"{\"scope\":\"development-engine-fixture\"}";

fn private_directory(path: &Path) {
    std::fs::create_dir(path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}
fn sign_artifact(
    directory: &Path,
    artifact: &[u8],
    sequence: u64,
    height: u64,
    key: u8,
) -> serde_json::Value {
    let input = directory.join("artifact.bin");
    let output = directory.join("public-control.json");
    std::fs::write(&input, artifact).unwrap();
    let result = Command::new(
        std::env::var("DYT_EMERGENCY_TEST_SIGNER").expect("explicit test-only emergency signer"),
    )
    .arg("--artifact")
    .arg(input)
    .arg("--output")
    .arg(&output)
    .arg("--chain")
    .arg(CHAIN)
    .arg("--sequence")
    .arg(sequence.to_string())
    .arg("--height")
    .arg(height.to_string())
    .arg("--fixture-key")
    .arg(key.to_string())
    .output()
    .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap()
}
fn authority(directory: &Path, key: u8, id: &str) -> emergency::AuthorityPolicy {
    let signed = sign_artifact(directory, b"public-key-fixture", 1, 1, key);
    emergency::AuthorityPolicy {
        threshold: 1,
        keys: vec![emergency::AuthorityKey {
            key_id: id.into(),
            public_key_hex: hex::encode(B64.decode(signed["PublicKey"].as_str().unwrap()).unwrap()),
        }],
    }
}
pub(super) struct RootFixture {
    pub(super) _directory: tempfile::TempDir,
    pub(super) source: Vec<u8>,
    pub(super) root: DevelopmentRootGenesis,
    pub(super) verifier: EmergencyVerifierConfig,
}
impl RootFixture {
    fn new(fixture: &mut Fixture) -> Self {
        Self::with_config(fixture, |_, _| {})
    }
    pub(super) fn with_config(
        fixture: &mut Fixture,
        configure: impl FnOnce(&mut Fixture, &Path),
    ) -> Self {
        Self::with_release_manifest(fixture, RELEASE, configure)
    }
    pub(super) fn with_release_manifest(
        fixture: &mut Fixture,
        release: &[u8],
        configure: impl FnOnce(&mut Fixture, &Path),
    ) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let freeze_authority = authority(directory.path(), 11, "freeze-key");
        let resume_authority = authority(directory.path(), 12, "resume-key");
        fixture.config.emergency = Some(emergency::Policy {
            schema: 1,
            development_only: true,
            chain_id: CHAIN.into(),
            release_sha512: hex::encode(Sha512::digest(release)),
            initial_sequence: 1,
            freeze_authority,
            resume_authority,
            max_control_bytes: 65_536,
            max_signatures: 1,
            automatic_transition_policy: emergency::AutomaticTransitionPolicy::ContinueExisting,
            v2: None,
        });
        configure(fixture, directory.path());
        fixture.config.validate().unwrap();
        let source = serde_json::to_vec(&fixture.config).unwrap();
        std::fs::write(directory.path().join("engine-genesis.json"), ENGINE).unwrap();
        std::fs::write(directory.path().join("release-manifest.json"), release).unwrap();
        std::fs::write(directory.path().join("genesis.json"), &fixture.genesis).unwrap();
        std::fs::write(directory.path().join("consensus.json"), &source).unwrap();
        let status = Command::new(
            std::env::var("DYT_ROOT_TEST_SIGNER").expect("explicit genesis fixture signer"),
        )
        .arg("-test.run=^TestExportDevelopmentGenesis$")
        .arg("-test.count=1")
        .env("DYT_ROOT_PUBLIC_FIXTURE_DIR", directory.path())
        .output()
        .unwrap();
        assert!(
            status.status.success(),
            "{}",
            String::from_utf8_lossy(&status.stderr)
        );
        let helper =
            PathBuf::from(std::env::var("DYT_ROOT_VERIFIER").expect("explicit real SLH verifier"));
        let helper_sha256 = hex::encode(Sha256::digest(std::fs::read(&helper).unwrap()));
        let scratch = directory.path().join("root-scratch");
        private_directory(&scratch);
        let root = DevelopmentRootGenesis {
            enabled: true,
            profile: "SLH-DSA-SHAKE-256s".into(),
            helper_path: helper.clone(),
            helper_scratch_path: Some(scratch),
            helper_execution: None,
            helper_sha256: helper_sha256.clone(),
            max_helper_bytes: 32 * 1024 * 1024,
            max_request_bytes: 8 * 1024 * 1024,
            timeout_ms: 5000,
            policy_json: std::fs::read(directory.path().join("policy.json")).unwrap(),
            request_json: std::fs::read(directory.path().join("request.json")).unwrap(),
            engine_genesis_path: directory.path().join("engine-genesis.json"),
            max_engine_genesis_bytes: 1024,
            engine_genesis_sha512: hex::encode(Sha512::digest(ENGINE)),
            release_manifest_path: directory.path().join("release-manifest.json"),
            max_release_manifest_bytes: 65_536,
            release_manifest_sha512: hex::encode(Sha512::digest(release)),
        };
        let scratch = directory.path().join("control-scratch");
        private_directory(&scratch);
        let verifier = EmergencyVerifierConfig {
            helper_path: helper,
            helper_scratch_path: scratch,
            helper_execution: None,
            helper_sha256,
            max_helper_bytes: 32 * 1024 * 1024,
            max_request_bytes: 1024 * 1024,
            timeout_ms: 5000,
        };
        Self {
            _directory: directory,
            source,
            root,
            verifier,
        }
    }
    pub(super) fn open(&self, f: &Fixture, path: &Path) -> anyhow::Result<ConsensusApplication> {
        ConsensusApplication::open_with_development_emergency(
            path,
            f.config.clone(),
            f.genesis.clone(),
            &self.source,
            self.root.clone(),
            self.verifier.clone(),
        )
    }
    pub(super) fn initialized(&self, f: &Fixture, path: &Path) -> ConsensusApplication {
        let mut app = self.open(f, path).unwrap();
        app.init_chain(CHAIN, 1, &f.genesis, &f.config.validators)
            .unwrap();
        app
    }
    fn control(
        &self,
        f: &Fixture,
        app: &ConsensusApplication,
        action: emergency::Action,
    ) -> Vec<u8> {
        let info = app.info().unwrap();
        let state = emergency_state(&app.storage, &app.config).unwrap().unwrap();
        let payload = emergency::Payload {
            schema: 1,
            chain_id: CHAIN.into(),
            release_sha512: f.config.emergency.as_ref().unwrap().release_sha512.clone(),
            action,
            sequence: state.next_sequence(),
            parent_height: info.height,
            parent_app_hash: info.app_hash,
            target_height: info.height + 1,
            v2: None,
        };
        let (key, id) = match action {
            emergency::Action::Freeze => (11, "freeze-key"),
            emergency::Action::Resume => (12, "resume-key"),
        };
        let signed = sign_artifact(
            self._directory.path(),
            &emergency::artifact_bytes(&payload).unwrap(),
            payload.sequence,
            payload.target_height,
            key,
        );
        serde_json::to_vec(&emergency::Control {
            kind: emergency::CONTROL_KIND.into(),
            payload,
            signatures: vec![emergency::ControlSignature {
                key_id: id.into(),
                signature_hex: hex::encode(
                    B64.decode(signed["Request"]["Signature"].as_str().unwrap())
                        .unwrap(),
                ),
            }],
        })
        .unwrap()
    }
}
fn principal_state(app: &ConsensusApplication) -> BTreeMap<Vec<u8>, Vec<u8>> {
    data(app)
        .into_iter()
        .filter(|(key, _)| {
            key.starts_with(b"acct:")
                || key.starts_with(b"execution:v1:receipt:")
                || key.starts_with(b"tx:")
                || key.starts_with(b"rcpt:")
        })
        .collect()
}
fn ordinary_send(f: &Fixture, app: &ConsensusApplication) -> Vec<u8> {
    f.ordinary_wire(
        &f.ordinary(
            &current(app, &f.payer),
            &f.payer,
            vec![send(f.secondary.id(), 100)],
            f.config
                .ordinary
                .as_ref()
                .unwrap()
                .fee_profile
                .max_transaction_gas,
        ),
    )
}

#[test]
fn absent_emergency_config_preserves_legacy_bytes_and_null_is_rejected() {
    let f = Fixture::new();
    let bytes = serde_json::to_vec(&f.config).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value.get("emergency").is_none());
    let decoded: ConsensusConfig = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);
    value["emergency"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<ConsensusConfig>(value).is_err());
}

#[test]
#[ignore = "Requires pinned real SLH helper and disposable fixture signers"]
fn actual_ordinary_freeze_priority_atomic_commit_restart_resume_and_replay() {
    let mut f = Fixture::new();
    let root = RootFixture::new(&mut f);
    for control_first in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut app = root.initialized(&f, directory.path());
        let tx = ordinary_send(&f, &app);
        assert_admitted(app.check_tx(&tx));
        assert_eq!(app.ordinary_admission_count().unwrap(), 1);
        let freeze = root.control(&f, &app, emergency::Action::Freeze);
        assert_admitted(app.check_tx(&freeze));
        let before = data(&app);
        let principal = principal_state(&app);
        let candidates = if control_first {
            vec![freeze.clone(), tx.clone()]
        } else {
            vec![tx.clone(), freeze.clone()]
        };
        let selected = app
            .prepare_proposal(1, 10, 0, candidates.clone(), 1_048_576)
            .unwrap();
        assert_eq!(selected, vec![freeze.clone()]);
        assert!(app.process_proposal(block(1, candidates.clone())).unwrap());
        let finalized = app.finalize_block(block(1, candidates.clone())).unwrap();
        assert_eq!(
            data(&app),
            before,
            "Finalize must not write emergency state"
        );
        assert_eq!(
            finalized.tx_results[usize::from(!control_first)],
            emergency_result()
        );
        assert_eq!(
            finalized.tx_results[usize::from(control_first)],
            TxResult::invalid(EMERGENCY_FROZEN)
        );
        assert!(app
            .commit_with(|_, _| anyhow::bail!("injected pre-write failure"))
            .is_err());
        assert_eq!(data(&app), before);
        app.commit().unwrap();
        assert_eq!(principal_state(&app), principal);
        assert!(emergency_state(&app.storage, &app.config)
            .unwrap()
            .unwrap()
            .frozen());
        assert_eq!(app.ordinary_admission_count().unwrap(), 0);
        let frozen = data(&app);
        assert_ne!(app.check_tx(&tx).code, 0);
        assert_ne!(app.recheck_ordinary_admission(&tx).code, 0);
        assert_ne!(
            app.check_tx(&wire(&f.sponsored(f.enroll(), 0, GAS_LIMIT)))
                .code,
            0
        );
        assert_eq!(data(&app), frozen);
        assert!(!app
            .process_proposal(block(2, vec![freeze.clone()]))
            .unwrap());
        drop(app);
        let mut app = root.open(&f, directory.path()).unwrap();
        assert!(app.query().unwrap()["emergency_control"]["frozen"]
            .as_bool()
            .unwrap());
        assert_eq!(
            app.prepare_proposal(2, 20, 0, vec![tx.clone()], 1_048_576)
                .unwrap(),
            Vec::<Vec<u8>>::new()
        );
        let resume = root.control(&f, &app, emergency::Action::Resume);
        let wrong_key = {
            let mut value: emergency::Control = serde_json::from_slice(&resume).unwrap();
            value.signatures[0].key_id = "freeze-key".into();
            serde_json::to_vec(&value).unwrap()
        };
        assert_ne!(app.check_tx(&wrong_key).code, 0);
        let result = commit(&mut app, 2, vec![tx.clone(), resume]);
        assert_eq!(result.tx_results[0], TxResult::invalid(EMERGENCY_FROZEN));
        assert_eq!(principal_state(&app), principal);
        let state = emergency_state(&app.storage, &app.config).unwrap().unwrap();
        assert!(!state.frozen());
        assert!(state.blocks_upgrade());
        assert_eq!(state.next_sequence(), 3);
        assert_admitted(app.check_tx(&tx));
        assert_eq!(commit(&mut app, 3, vec![tx]).tx_results[0].code, 0);
        assert_eq!(ordinary_nonce(&app, &f.payer.address()), 1);
        drop(app);
        let app = root.open(&f, directory.path()).unwrap();
        assert!(app.query().unwrap()["emergency_control"]["blocks_upgrade"]
            .as_bool()
            .unwrap());
    }
}

#[test]
#[ignore = "Requires pinned real SLH helper and disposable fixture signers"]
fn actual_emergency_helper_failure_and_corrupt_history_fail_closed() {
    let mut f = Fixture::new();
    let root = RootFixture::new(&mut f);
    let directory = tempfile::tempdir().unwrap();
    let mut wrong_release = root.root.clone();
    wrong_release.release_manifest_sha512 = "00".repeat(64);
    let error = ConsensusApplication::open_with_development_emergency(
        directory.path(),
        f.config.clone(),
        f.genesis.clone(),
        &root.source,
        wrong_release,
        root.verifier.clone(),
    )
    .err()
    .expect("Wrong release must fail");
    assert!(error.to_string().contains("Emergency release differs"));
    let mut app = root.initialized(&f, directory.path());
    let control = root.control(&f, &app, emergency::Action::Freeze);
    let duplicate = block(1, vec![control.clone(), control.clone()]);
    assert!(!app.process_proposal(duplicate.clone()).unwrap());
    assert!(app.finalize_block(duplicate).is_err());
    let before = data(&app);
    let mut bad_verifier = root.verifier.clone();
    bad_verifier.helper_path = root._directory.path().join("missing-helper");
    app.emergency_verifier =
        Some(crate::emergency_verifier::EmergencyVerifier::new(bad_verifier).unwrap());
    let error = app
        .process_proposal(block(1, vec![control.clone()]))
        .unwrap_err();
    assert!(crate::emergency_verifier::is_infrastructure_error(&error));
    assert!(app
        .prepare_proposal(1, 10, 0, vec![control.clone()], 1_048_576)
        .is_err());
    assert_ne!(app.check_tx(&control).code, 0);
    assert_eq!(data(&app), before);
    app.emergency_verifier =
        Some(crate::emergency_verifier::EmergencyVerifier::new(root.verifier.clone()).unwrap());
    let mut bad: emergency::Control = serde_json::from_slice(&control).unwrap();
    let replacement = if &bad.signatures[0].signature_hex[..2] == "00" {
        "01"
    } else {
        "00"
    };
    bad.signatures[0]
        .signature_hex
        .replace_range(..2, replacement);
    assert!(!app
        .process_proposal(block(1, vec![serde_json::to_vec(&bad).unwrap()]))
        .unwrap());
    commit(&mut app, 1, vec![control]);
    let receipt_key = emergency::receipt_key(1);
    let receipt = app.storage.db.get(&receipt_key).unwrap().unwrap();
    app.storage.db.delete(&receipt_key).unwrap();
    assert!(app.info().is_err());
    app.storage.db.put(&receipt_key, &receipt).unwrap();
    app.info().unwrap();
    app.storage
        .db
        .put("consensus:emergency:v1:unknown", b"unexpected")
        .unwrap();
    assert!(app.info().is_err());
    app.storage
        .db
        .delete("consensus:emergency:v1:unknown")
        .unwrap();
    let state = app.storage.db.get(emergency::STATE_KEY).unwrap().unwrap();
    let reset = emergency::encode_state(
        &emergency::State::new(f.config.emergency.as_ref().unwrap()).unwrap(),
    )
    .unwrap();
    app.storage.db.put(emergency::STATE_KEY, reset).unwrap();
    assert!(app.info().is_err());
    app.storage.db.put(emergency::STATE_KEY, state).unwrap();
    app.info().unwrap();
    drop(app);
    let mut invalid = root.verifier.clone();
    invalid.helper_path = root._directory.path().join("missing-helper");
    assert!(ConsensusApplication::open_with_development_emergency(
        directory.path(),
        f.config.clone(),
        f.genesis.clone(),
        &root.source,
        root.root.clone(),
        invalid
    )
    .is_err());
    assert!(
        ConsensusApplication::open(directory.path(), f.config.clone(), f.genesis.clone()).is_err()
    );
}

#[test]
#[ignore = "Requires pinned real SLH helper and disposable fixture signers"]
fn required_epoch_observation_and_existing_automatic_work_continue_while_frozen() {
    let mut f = Fixture::new();
    let mut genesis: serde_json::Value = serde_json::from_slice(&f.genesis).unwrap();
    genesis["adaptive_issuance"]["epoch_blocks"] = serde_json::json!(2);
    f.genesis = serde_json::to_vec(&genesis).unwrap();
    let digest: [u8; 32] = Sha256::digest(&f.genesis).into();
    f.config.app_state_sha256 = hex::encode(digest);
    for account in f.config.recovery.as_mut().unwrap().accounts.values_mut() {
        account.recovery.domain.genesis_digest = digest;
    }
    let root = RootFixture::new(&mut f);
    let directory = tempfile::tempdir().unwrap();
    let mut app = root.initialized(&f, directory.path());
    let control = root.control(&f, &app, emergency::Action::Freeze);
    commit(&mut app, 1, vec![control]);
    commit(&mut app, 2, vec![]);
    let observation = serde_json::to_vec(&WireTransaction::EpochObservation {
        observation: EpochObservation {
            epoch: 0,
            utilization_ppm: 500000,
            volatility_ppm: 0,
            first_height: 1,
            last_height: 2,
            parent_hash: block(2, vec![]).hash,
        },
    })
    .unwrap();
    assert!(!app.process_proposal(block(3, vec![])).unwrap());
    assert_eq!(app.check_tx(&observation).code, 0);
    let tx = ordinary_send(&f, &app);
    assert_eq!(
        app.prepare_proposal(3, 30, 0, vec![tx.clone(), observation.clone()], 1_048_576)
            .unwrap(),
        vec![observation.clone()]
    );
    let result = commit(&mut app, 3, vec![observation, tx]);
    assert_eq!(result.tx_results[0].code, 0);
    assert_eq!(result.tx_results[1], TxResult::invalid(EMERGENCY_FROZEN));
    assert_eq!(timing(&app.storage).unwrap().last_height, 3);
    assert_eq!(book(&app).last_height, 3);
    assert!(emergency_state(&app.storage, &app.config)
        .unwrap()
        .unwrap()
        .frozen());
}
