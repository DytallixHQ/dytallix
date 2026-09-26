//! Bounded development root consumer checks. No production key or approval.
use super::*;
use crate::consensus_settlement::{ConsensusApplication, ConsensusConfig};
use crate::crypto::{ActivePQC, PQC};
use crate::storage::state::Storage;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use fips204::{
    ml_dsa_65,
    traits::{KeyGen, SerDes},
};
use rocksdb::IteratorMode;
use sha2::{Digest, Sha256};
const CHAIN: &str = "root-development-fixture";
const INITIAL_DRT: u128 = 1_000_000;
struct Inputs {
    config: ConsensusConfig,
    genesis: Vec<u8>,
    owner: String,
    secret: Vec<u8>,
    public: Vec<u8>,
}

impl Inputs {
    fn new() -> Self {
        let (secret, public) = ActivePQC::keypair();
        let owner = crate::addr::initial_address(
            crate::addr::AddressNetwork::Development,
            CHAIN,
            crate::addr::OriginKeyAlgorithm::MlDsa65,
            &public,
        )
        .unwrap();
        let (validator, _) = ml_dsa_65::KG::try_keygen_with_rng(&mut rand_core::OsRng).unwrap();
        let genesis = serde_json::to_vec(&serde_json::json!({
            "chain_id": CHAIN,
            "accounts": [{"address": owner,
                "balances": {"udgt": "1000", "udrt": INITIAL_DRT.to_string()},
                "vesting": {"kind": "unlocked"}}],
            "staking": {"delegations": [{"delegator": owner, "amount_udgt": "100"}]},
            "reward_v2": {"version": 2, "activation_height": 1, "decimals": 6,
                "profile": "development", "max_validators": 4, "max_positions": 8,
                "validators": [{"address": "validator-one", "active": true, "jailed": false}],
                "positions": [{"owner": owner, "validator": "validator-one", "amount_udgt": "100"}]},
            "adaptive_issuance": {"version": 1, "profile": "development", "decimals": 6,
                "epoch_blocks": 2, "initial_epoch_budget_udrt": "1001", "max_recorded_epochs": 8,
                "controller": {"target_ppm": 500000, "shock_threshold_ppm": 100000,
                    "volatility_threshold_ppm": 1000000, "window_samples": 2,
                    "integral_min": -2000000, "integral_max": 2000000,
                    "soft": {"proportional": 0, "integral": 0, "derivative": 0},
                    "hard": {"proportional": 0, "integral": 0, "derivative": 0},
                    "base_udrt": 1001, "min_udrt": 1001, "max_udrt": 1001}}
        })).unwrap();
        let config = serde_json::from_value(serde_json::json!({
            "profile": "cometbft-local-qualification", "engine": "cometbft-v0.40.0",
            "chain_id": CHAIN, "app_state_sha256": hex::encode(Sha256::digest(&genesis)),
            "gas_price": 1, "max_tx_bytes": 65536, "max_block_bytes": 1048576, "max_txs": 64,
            "validators": [{"pubkey_type": "ml_dsa_65", "pubkey_base64": B64.encode(validator.into_bytes()),
                "power": 10, "reward_address": "validator-one"}]
        })).unwrap();
        Self {
            config,
            genesis,
            owner,
            secret,
            public,
        }
    }
}

#[test]
fn development_bundle_exact_bytes_and_explicit_digests() {
    let a = bundle(b"app", b"config", &[0; 64], &[0; 64]).unwrap();
    let b = bundle(b"app ", b"config", &[0; 64], &[0; 64]).unwrap();
    assert_ne!(a, b);
    assert!(bundle(b"app", b"config", &[0; 63], &[0; 64]).is_err());
    assert!(digest_bytes(&"AA".repeat(64), 64).is_err());
    assert!(check_receipt(Some(b"unapproved"), None).is_err());
}

fn authorized_fixture(
    inputs: &Inputs,
    directory: &std::path::Path,
    consensus_source: &[u8],
) -> DevelopmentRootGenesis {
    let helper = std::path::PathBuf::from(
        std::env::var("DYT_ROOT_VERIFIER").expect("explicit built helper required"),
    );
    let signer =
        std::env::var("DYT_ROOT_TEST_SIGNER").expect("explicit test-only signer binary required");
    let config = consensus_source;
    let engine = b"{\"scope\":\"development-engine-fixture\"}";
    let release = b"{\"scope\":\"development-release-fixture\"}";
    std::fs::write(directory.join("engine-genesis.json"), engine).unwrap();
    std::fs::write(directory.join("release-manifest.json"), release).unwrap();
    std::fs::write(directory.join("genesis.json"), &inputs.genesis).unwrap();
    std::fs::write(directory.join("consensus.json"), &config).unwrap();
    let output = Command::new(signer)
        .arg("-test.run=^TestExportDevelopmentGenesis$")
        .arg("-test.count=1")
        .env("DYT_ROOT_PUBLIC_FIXTURE_DIR", directory)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "public fixture signer failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let scratch = directory.join("helper-scratch");
    std::fs::create_dir(&scratch).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    DevelopmentRootGenesis {
        enabled: true,
        profile: PROFILE.into(),
        helper_sha256: hex::encode(Sha256::digest(std::fs::read(&helper).unwrap())),
        helper_path: helper,
        helper_scratch_path: Some(scratch),
        helper_execution: None,
        max_helper_bytes: 32 * 1024 * 1024,
        max_request_bytes: 8 * 1024 * 1024,
        timeout_ms: 5000,
        policy_json: std::fs::read(directory.join("policy.json")).unwrap(),
        request_json: std::fs::read(directory.join("request.json")).unwrap(),
        engine_genesis_path: directory.join("engine-genesis.json"),
        max_engine_genesis_bytes: 1024,
        engine_genesis_sha512: hex::encode(Sha512::digest(engine)),
        release_manifest_path: directory.join("release-manifest.json"),
        max_release_manifest_bytes: 1024,
        release_manifest_sha512: hex::encode(Sha512::digest(release)),
    }
}

#[test]
#[ignore = "Requires exact built Go verifier and test-only public fixture signer"]
fn development_genesis_actual_helper_atomic_restart_and_replay() {
    let inputs = Inputs::new();
    let fixtures = tempfile::tempdir().unwrap();
    let config_bytes = serde_json::to_vec(&inputs.config).unwrap();
    let authorization = authorized_fixture(&inputs, fixtures.path(), &config_bytes);
    let prepared = authorization
        .prepare(CHAIN, &inputs.genesis, &config_bytes)
        .unwrap();
    let failed = tempfile::tempdir().unwrap();
    let mut storage = Storage::open(failed.path().to_path_buf()).unwrap();
    let failure = crate::genesis::initialize_mode_with_root(
        &mut storage,
        CHAIN,
        Some(&inputs.genesis),
        Some(&config_bytes),
        Some(&prepared),
        |_, _| anyhow::bail!("injected pre-commit write failure"),
    );
    assert!(failure.is_err());
    assert!(storage.db.iterator(IteratorMode::Start).next().is_none());
    drop(storage);

    let root_path = tempfile::tempdir().unwrap();
    let mut app = ConsensusApplication::open_with_development_root(
        root_path.path(),
        inputs.config.clone(),
        inputs.genesis.clone(),
        &config_bytes,
        authorization.clone(),
    )
    .unwrap();
    let committed = app
        .init_chain(CHAIN, 1, &inputs.genesis, &inputs.config.validators)
        .unwrap();
    assert_eq!(committed.height, 0);
    assert_eq!(
        app.init_chain(CHAIN, 1, &inputs.genesis, &inputs.config.validators)
            .unwrap(),
        committed
    );
    drop(app);
    let storage = Storage::open(root_path.path().to_path_buf()).unwrap();
    assert_eq!(
        storage.db.get(STATE_KEY).unwrap().unwrap(),
        prepared.bytes()
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&storage.db.get(STATE_KEY).unwrap().unwrap()).unwrap();
    assert_eq!(receipt["consumed_sequence"], 1);
    drop(storage);
    assert!(ConsensusApplication::open(
        root_path.path(),
        inputs.config.clone(),
        inputs.genesis.clone()
    )
    .is_err());
    let mut restarted = ConsensusApplication::open_with_development_root(
        root_path.path(),
        inputs.config.clone(),
        inputs.genesis.clone(),
        &config_bytes,
        authorization.clone(),
    )
    .unwrap();
    assert_eq!(restarted.info().unwrap(), committed);
    assert_eq!(
        restarted
            .init_chain(CHAIN, 1, &inputs.genesis, &inputs.config.validators)
            .unwrap(),
        committed
    );
    drop(restarted);

    for path in [
        &authorization.engine_genesis_path,
        &authorization.release_manifest_path,
    ] {
        let original = std::fs::read(path).unwrap();
        std::fs::write(path, b"changed after initialization").unwrap();
        assert!(ConsensusApplication::open_with_development_root(
            root_path.path(),
            inputs.config.clone(),
            inputs.genesis.clone(),
            &config_bytes,
            authorization.clone()
        )
        .is_err());
        let stored = Storage::open(root_path.path().to_path_buf()).unwrap();
        assert_eq!(stored.db.get(STATE_KEY).unwrap().unwrap(), prepared.bytes());
        drop(stored);
        std::fs::write(path, original).unwrap();
        let mut restored = ConsensusApplication::open_with_development_root(
            root_path.path(),
            inputs.config.clone(),
            inputs.genesis.clone(),
            &config_bytes,
            authorization.clone(),
        )
        .unwrap();
        assert_eq!(restored.info().unwrap(), committed);
    }

    let legacy = tempfile::tempdir().unwrap();
    let mut ordinary =
        ConsensusApplication::open(legacy.path(), inputs.config.clone(), inputs.genesis.clone())
            .unwrap();
    let ordinary_info = ordinary
        .init_chain(CHAIN, 1, &inputs.genesis, &inputs.config.validators)
        .unwrap();
    assert_ne!(ordinary_info.app_hash, committed.app_hash);
    drop(ordinary);
    let ordinary =
        ConsensusApplication::open(legacy.path(), inputs.config.clone(), inputs.genesis.clone())
            .unwrap();
    assert_eq!(ordinary.info().unwrap(), ordinary_info);
    drop(ordinary);
    assert!(ConsensusApplication::open_with_development_root(
        legacy.path(),
        inputs.config.clone(),
        inputs.genesis.clone(),
        &config_bytes,
        authorization.clone()
    )
    .is_err());

    let spaced = [b" ".as_slice(), config_bytes.as_slice(), b"\n".as_slice()].concat();
    let untouched = tempfile::tempdir().unwrap();
    assert!(ConsensusApplication::open_with_development_root(
        untouched.path().join("not-created"),
        inputs.config.clone(),
        inputs.genesis.clone(),
        &spaced,
        authorization.clone()
    )
    .is_err());
    assert!(!untouched.path().join("not-created").exists());
    let spaced_fixtures = tempfile::tempdir().unwrap();
    let spaced_authorization = authorized_fixture(&inputs, spaced_fixtures.path(), &spaced);
    let spaced_database = tempfile::tempdir().unwrap();
    let mut spaced_app = ConsensusApplication::open_with_development_root(
        spaced_database.path(),
        inputs.config.clone(),
        inputs.genesis.clone(),
        &spaced,
        spaced_authorization,
    )
    .unwrap();
    spaced_app
        .init_chain(CHAIN, 1, &inputs.genesis, &inputs.config.validators)
        .unwrap();
    drop(spaced_app);

    for case in [
        "disabled",
        "wrong-key",
        "wrong-helper",
        "changed-artifact",
        "changed-engine-file",
        "changed-release-file",
        "changed-engine-file-and-digest",
        "changed-release-file-and-digest",
        "missing-engine-file",
        "missing-release-file",
        "engine-bound",
        "release-bound",
        "trailing-request",
    ] {
        let empty = tempfile::tempdir().unwrap();
        let database = empty.path().join("not-created");
        let mut bad = authorization.clone();
        match case {
            "disabled" => bad.enabled = false,
            "wrong-key" => {
                let mut p: Policy = serde_json::from_slice(&bad.policy_json).unwrap();
                p.public_key = B64.encode([1u8; 64]);
                bad.policy_json = serde_json::to_vec(&p).unwrap();
            }
            "wrong-helper" => bad.helper_sha256 = "00".repeat(32),
            "changed-artifact" => bad.release_manifest_sha512 = "01".repeat(64),
            "changed-engine-file" | "changed-engine-file-and-digest" => {
                bad.engine_genesis_path = empty.path().join("engine.json");
                std::fs::write(&bad.engine_genesis_path, b"changed engine bytes").unwrap();
                if case.ends_with("and-digest") {
                    bad.engine_genesis_sha512 =
                        hex::encode(Sha512::digest(b"changed engine bytes"));
                }
            }
            "changed-release-file" | "changed-release-file-and-digest" => {
                bad.release_manifest_path = empty.path().join("release.json");
                std::fs::write(&bad.release_manifest_path, b"changed release bytes").unwrap();
                if case.ends_with("and-digest") {
                    bad.release_manifest_sha512 =
                        hex::encode(Sha512::digest(b"changed release bytes"));
                }
            }
            "missing-engine-file" => bad.engine_genesis_path = empty.path().join("absent-engine"),
            "missing-release-file" => {
                bad.release_manifest_path = empty.path().join("absent-release")
            }
            "engine-bound" => bad.max_engine_genesis_bytes = 1,
            "release-bound" => bad.max_release_manifest_bytes = 1,
            _ => bad.request_json.push(b'\n'),
        }
        assert!(
            ConsensusApplication::open_with_development_root(
                &database,
                inputs.config.clone(),
                inputs.genesis.clone(),
                &config_bytes,
                bad
            )
            .is_err(),
            "accepted {case}"
        );
        assert!(
            !database.exists(),
            "verification failure created database for {case}"
        );
    }
}

#[test]
fn root_presence_blocks_legacy_mutators_without_other_markers() {
    let directory = tempfile::tempdir().unwrap();
    let storage = Storage::open(directory.path().to_path_buf()).unwrap();
    storage
        .db
        .put(STATE_KEY, b"invalid development fixture record")
        .unwrap();
    assert!(crate::genesis::reject_consensus_state(&storage).is_err());
    assert!(crate::genesis::reject_qualified_state(&storage).is_err());
}

#[test]
fn external_artifact_files_are_bounded_exact_regular_inputs() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("artifact.json");
    let bytes = b"{\"fixture\":true}\n";
    std::fs::write(&path, bytes).unwrap();
    let digest = hex::encode(Sha512::digest(bytes));
    assert_eq!(
        verified_artifact_digest(&path, bytes.len(), &digest).unwrap(),
        Sha512::digest(bytes).to_vec()
    );
    assert!(verified_artifact_digest(&path, bytes.len() - 1, &digest).is_err());
    assert!(verified_artifact_digest(&path, 0, &digest).is_err());
    assert!(verified_artifact_digest(Path::new("artifact.json"), 1024, &digest).is_err());
    assert!(verified_artifact_digest(directory.path(), 1024, &digest).is_err());
    assert!(verified_artifact_digest(&directory.path().join("absent"), 1024, &digest).is_err());
    assert!(verified_artifact_digest(&path, 1024, &digest.to_uppercase()).is_err());
    std::fs::write(&path, b"{\"fixture\":true}").unwrap();
    assert!(verified_artifact_digest(&path, 1024, &digest).is_err());
    std::fs::write(&path, b"").unwrap();
    assert!(verified_artifact_digest(&path, 1024, &hex::encode(Sha512::digest(b""))).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::{symlink, PermissionsExt};
        std::fs::write(&path, bytes).unwrap();
        let link = directory.path().join("link.json");
        symlink(&path, &link).unwrap();
        assert!(verified_artifact_digest(&link, 1024, &digest).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert!(verified_artifact_digest(&path, 1024, &digest).is_err());
    }
}

#[cfg(unix)]
fn helper_process_fixture(directory: &Path, script: &[u8]) -> DevelopmentRootGenesis {
    use std::os::unix::fs::PermissionsExt;
    let helper = directory.join("helper");
    std::fs::write(&helper, script).unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o500)).unwrap();
    DevelopmentRootGenesis {
        enabled: true,
        profile: PROFILE.into(),
        helper_path: helper,
        helper_scratch_path: None,
        helper_execution: None,
        helper_sha256: hex::encode(Sha256::digest(script)),
        max_helper_bytes: 65536,
        max_request_bytes: 1024 * 1024,
        timeout_ms: 2000,
        policy_json: b"{}".to_vec(),
        request_json: b"{}\n".to_vec(),
        engine_genesis_path: directory.join("unused-engine"),
        max_engine_genesis_bytes: 1,
        engine_genesis_sha512: "00".repeat(64),
        release_manifest_path: directory.join("unused-release"),
        max_release_manifest_bytes: 1,
        release_manifest_sha512: "00".repeat(64),
    }
}

#[test]
#[cfg(unix)]
fn helper_scratch_selection_preserves_cleanup_and_default() {
    use std::os::unix::fs::PermissionsExt;
    let source = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    std::fs::set_permissions(scratch.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let script = b"#!/bin/sh\nIFS= read -r request\nprintf '%s' '{\"Status\":\"VERIFIED\",\"RequestSHA256\":\"fixture\",\"ArtifactSHA512\":\"fixture\",\"ChainID\":\"fixture\",\"Action\":\"genesis\",\"Sequence\":1,\"ProductionQualified\":false}'\n";
    let mut config = helper_process_fixture(source.path(), script);
    // This checks subprocess handling only, not cryptographic authorization.
    assert_eq!(config.run_helper().unwrap().sequence, 1);
    config.helper_scratch_path = Some(scratch.path().to_path_buf());
    assert_eq!(config.run_helper().unwrap().sequence, 1);
    assert_eq!(std::fs::read_dir(scratch.path()).unwrap().count(), 0);

    std::fs::set_permissions(scratch.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(config
        .run_helper()
        .err()
        .expect("helper must fail")
        .to_string()
        .contains("private directory"));
    std::fs::set_permissions(scratch.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let link = source.path().join("scratch-link");
    std::os::unix::fs::symlink(scratch.path(), &link).unwrap();
    config.helper_scratch_path = Some(link);
    assert!(config
        .run_helper()
        .err()
        .expect("helper must fail")
        .to_string()
        .contains("private directory"));
    config.helper_scratch_path = Some(PathBuf::from("relative-scratch"));
    assert!(config
        .run_helper()
        .err()
        .expect("helper must fail")
        .to_string()
        .contains("absolute"));
    config.helper_scratch_path = Some(source.path().join("missing"));
    assert!(config
        .run_helper()
        .err()
        .expect("helper must fail")
        .to_string()
        .contains("unavailable"));
}

#[test]
#[cfg(unix)]
fn helper_failure_reports_exit_status_and_removes_snapshot() {
    let source = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(scratch.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut config = helper_process_fixture(source.path(), b"#!/bin/sh\nexit 7\n");
    config.helper_scratch_path = Some(scratch.path().to_path_buf());
    config.request_json = vec![b'x'; 1024 * 1024];
    let error = config
        .run_helper()
        .err()
        .expect("helper must fail")
        .to_string();
    assert!(
        error.contains("Root verifier exited unsuccessfully"),
        "{error}"
    );
    assert!(error.contains('7'), "{error}");
    assert_eq!(std::fs::read_dir(scratch.path()).unwrap().count(), 0);
}

#[test]
fn development_config_rejects_legacy_digest_only_external_inputs() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("root-config.json");
    let policy = directory.path().join("policy.json");
    let request = directory.path().join("request.json");
    std::fs::write(&policy, b"{}").unwrap();
    std::fs::write(&request, b"{}").unwrap();
    let mut config = serde_json::json!({
        "enabled": true, "profile": PROFILE,
        "helper_path": directory.path().join("verifier"), "helper_sha256": "00".repeat(32),
        "max_helper_bytes": 1024, "max_request_bytes": 1024, "timeout_ms": 1000,
        "policy_path": policy, "request_path": request,
        "engine_genesis_sha512": "00".repeat(64), "release_manifest_sha512": "00".repeat(64),
    });
    std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(DevelopmentRootGenesis::from_development_config(&path).is_err());
    config["engine_genesis_path"] = directory
        .path()
        .join("engine.json")
        .to_str()
        .unwrap()
        .into();
    config["release_manifest_path"] = directory
        .path()
        .join("release.json")
        .to_str()
        .unwrap()
        .into();
    config["max_engine_genesis_bytes"] = 1024.into();
    config["max_release_manifest_bytes"] = 1024.into();
    std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(DevelopmentRootGenesis::from_development_config(&path).is_ok());
    for field in [
        "engine_genesis_path",
        "release_manifest_path",
        "max_engine_genesis_bytes",
        "max_release_manifest_bytes",
    ] {
        let mut missing = config.clone();
        missing.as_object_mut().unwrap().remove(field);
        std::fs::write(&path, serde_json::to_vec(&missing).unwrap()).unwrap();
        assert!(
            DevelopmentRootGenesis::from_development_config(&path).is_err(),
            "accepted missing {field}"
        );
    }
}

fn observed_policy_fixture(bytes: u64, sha512: String) -> ObservedHelperPolicy {
    ObservedHelperPolicy {
        profile: OBSERVED_HELPER_PROFILE.into(),
        helper_bytes: bytes,
        helper_sha512: sha512,
        owner_security: dytallix_release_runtime::ownership_security::HelperSecurityPolicy {
            supervisor_label: "dyt-role-0123456789abcdefabcd-node0-supervisor".into(),
            application_owner_label: "dyt-role-0123456789abcdefabcd-node0-application-owner//&dyt-role-0123456789abcdefabcd-node0-supervisor".into(),
            workload_label: "dyt-role-0123456789abcdefabcd-node0-supervisor//&dyt-role-0123456789abcdefabcd-node0-workload".into(),
            helper_label: "dyt-role-0123456789abcdefabcd-node0-application-owner//&dyt-role-0123456789abcdefabcd-node0-helper//&dyt-role-0123456789abcdefabcd-node0-supervisor".into(),
            uid: 62019,
            gid: 62019,
        },
        max_program_headers: 128,
        max_section_headers: 256,
        max_stat_bytes: 65536,
        max_maps_bytes: 1048576,
        max_map_entries: 4096,
        max_path_bytes: 4096,
        max_unique_files: 1,
        max_file_bytes: 32 * 1024 * 1024,
        max_total_file_bytes: 32 * 1024 * 1024,
        max_elapsed_ms: 1000,
        max_output_bytes: 8192,
        cleanup_timeout_ms: 100,
    }
}
#[test]
fn observed_helper_policy_requires_exact_profile_pins_and_explicit_bounds() {
    let policy = observed_policy_fixture(4096, "aa".repeat(64));
    policy.validate(32 * 1024 * 1024, 5000).unwrap();
    for field in [
        "helper_bytes",
        "max_program_headers",
        "max_section_headers",
        "max_stat_bytes",
        "max_maps_bytes",
        "max_map_entries",
        "max_path_bytes",
        "max_unique_files",
        "max_file_bytes",
        "max_total_file_bytes",
        "max_elapsed_ms",
        "max_output_bytes",
        "cleanup_timeout_ms",
    ] {
        let mut value = serde_json::to_value(&policy).unwrap();
        value[field] = serde_json::json!(0);
        let decoded: ObservedHelperPolicy = serde_json::from_value(value).unwrap();
        assert!(decoded.validate(32 * 1024 * 1024, 5000).is_err(), "{field}");
    }
    for (field, value) in [
        ("profile", serde_json::json!("historical")),
        ("helper_sha512", serde_json::json!("AA".repeat(64))),
        ("helper_bytes", serde_json::json!(32 * 1024 * 1024 + 1)),
        ("cleanup_timeout_ms", serde_json::json!(5000)),
    ] {
        let mut raw = serde_json::to_value(&policy).unwrap();
        raw[field] = value;
        assert!(
            serde_json::from_value::<ObservedHelperPolicy>(raw)
                .unwrap()
                .validate(32 * 1024 * 1024, 5000)
                .is_err(),
            "{field}"
        );
    }
}
#[test]
fn observed_helper_policy_rejects_missing_fields_owner_override_and_duplicates() {
    let policy = observed_policy_fixture(4096, "aa".repeat(64));
    let raw = serde_json::to_value(&policy).unwrap();
    for field in raw.as_object().unwrap().keys() {
        let mut changed = raw.clone();
        changed.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<ObservedHelperPolicy>(changed).is_err(),
            "{field}"
        );
    }
    let mut changed = raw;
    changed["allowed_owner_uids"] = serde_json::json!([1000]);
    assert!(serde_json::from_value::<ObservedHelperPolicy>(changed).is_err());
    let mut duplicate = serde_json::to_string(&policy).unwrap();
    duplicate.insert_str(1, "\"max_unique_files\":1,");
    assert!(serde_json::from_str::<ObservedHelperPolicy>(&duplicate).is_err());
}
#[test]
#[cfg(unix)]
fn observed_profile_never_falls_back_to_executable_scratch_for_a_script() {
    let source = tempfile::tempdir().unwrap();
    let marker = source.path().join("must-not-execute");
    let script = format!(
        "#!/bin/sh\nprintf touched > '{}'\nexit 1\n",
        marker.display()
    );
    let mut config = helper_process_fixture(source.path(), script.as_bytes());
    config.max_helper_bytes = 32 * 1024 * 1024;
    config.timeout_ms = 5000;
    config.helper_execution = Some(observed_policy_fixture(
        script.len() as u64,
        hex::encode(Sha512::digest(script.as_bytes())),
    ));
    let before = std::fs::read_dir(source.path()).unwrap().count();
    assert!(config.run_helper().is_err());
    assert!(!marker.exists());
    assert_eq!(std::fs::read_dir(source.path()).unwrap().count(), before);
}

#[test]
fn returned_helper_evidence_is_bounded_without_a_sink() {
    let report = serde_json::json!({"request_sha256":"11".repeat(32),"canonical_result_sha256":null,"result_status":"REJECTED","result_payload_bytes":0,"continuous_enforcement":false,"production_qualified":false});
    let expected = serde_json::to_vec(&report).unwrap();
    let evidence = HelperExecutionEvidence::encode(&report, expected.len()).unwrap();
    assert_eq!(evidence.bytes(), expected);
    assert!(HelperExecutionEvidence::encode(&report, expected.len() - 1).is_err());
    assert!(HelperExecutionEvidence::encode(&report, 0).is_err());
}
#[test]
#[cfg(unix)]
fn qualification_api_requires_observed_evidence_and_never_uses_old_test_launcher() {
    let source = tempfile::tempdir().unwrap();
    let config = helper_process_fixture(source.path(), b"#!/bin/sh\nexit 0\n");
    assert!(config
        .execute_helper_for_qualification()
        .unwrap_err()
        .to_string()
        .contains("explicit observed execution policy"));
}
