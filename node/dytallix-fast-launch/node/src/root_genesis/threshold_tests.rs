//! Root genesis three of five (production activation v1, step A2). The unit
//! tests use fixture keys and a fake helper; the signed test uses the real
//! offline signer and verifier, on development and production builds.
use super::*;
use crate::consensus_settlement::{ConsensusApplication, ConsensusConfig};
use crate::storage::state::Storage;

const CHAIN: &str = "root-threshold-fixture";
const APP: &[u8] = b"{\"app\":1}";
const CONFIG: &[u8] = b"{\"config\":1}";
const ENGINE: &[u8] = b"{\"scope\":\"engine-genesis-fixture\"}";
const RELEASE: &[u8] = b"{\"scope\":\"release-manifest-fixture\"}";

fn fixture_keys() -> Vec<GenesisKey> {
    let mut keys: Vec<GenesisKey> = (1..=5u8)
        .map(|i| GenesisKey {
            key_id: hex::encode(Sha256::digest([0xa0 + i; 64])),
            public_key_hex: hex::encode([0xa0 + i; 64]),
        })
        .collect();
    keys.sort_by(|a, b| a.key_id.cmp(&b.key_id));
    keys
}

fn fixture_policy() -> GenesisPolicy {
    GenesisPolicy {
        schema: 1,
        chain_id: CHAIN.into(),
        authority: GenesisAuthority {
            keys: fixture_keys(),
            threshold: 3,
        },
    }
}

fn bundle_sha512() -> String {
    let artifact = bundle(
        APP,
        CONFIG,
        &Sha512::digest(ENGINE),
        &Sha512::digest(RELEASE),
    )
    .unwrap();
    hex::encode(Sha512::digest(artifact))
}

fn fixture_signatures(count: usize) -> GenesisSignatures {
    GenesisSignatures {
        schema: 1,
        chain_id: CHAIN.into(),
        bundle_sha512: bundle_sha512(),
        signatures: fixture_keys()[..count]
            .iter()
            .map(|key| GenesisSignature {
                key_id: key.key_id.clone(),
                signature_hex: "ab".repeat(29_792),
            })
            .collect(),
    }
}

/// A root configuration around a fake helper script, run by the test launcher.
fn fixture_root(directory: &Path, script: &[u8]) -> RootGenesis {
    use std::os::unix::fs::PermissionsExt;
    let helper = directory.join("helper");
    std::fs::write(&helper, script).unwrap();
    let scratch = directory.join("scratch");
    std::fs::create_dir(&scratch).unwrap();
    std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(directory.join("engine.json"), ENGINE).unwrap();
    std::fs::write(directory.join("release.json"), RELEASE).unwrap();
    RootGenesis {
        helper_path: helper,
        helper_scratch_path: Some(scratch),
        helper_execution: None,
        helper_sha256: hex::encode(Sha256::digest(script)),
        max_helper_bytes: 4096,
        max_request_bytes: 1 << 20,
        timeout_ms: 5000,
        policy_json: serde_json::to_vec(&fixture_policy()).unwrap(),
        signatures_json: serde_json::to_vec(&fixture_signatures(3)).unwrap(),
        engine_genesis_path: directory.join("engine.json"),
        max_engine_genesis_bytes: 1024,
        engine_genesis_sha512: hex::encode(Sha512::digest(ENGINE)),
        release_manifest_path: directory.join("release.json"),
        max_release_manifest_bytes: 1024,
        release_manifest_sha512: hex::encode(Sha512::digest(RELEASE)),
    }
}

fn prepare_error(root: &RootGenesis) -> String {
    format!(
        "{:#}",
        root.prepare(CHAIN, APP, CONFIG)
            .err()
            .expect("prepare refused")
    )
}

#[test]
fn signer_policy_is_three_of_five_sorted_keys() {
    assert!(fixture_policy().validate(CHAIN).is_ok());
    assert!(fixture_policy().validate("another-chain").is_err());
    let cases: [(&str, fn(&mut GenesisPolicy)); 7] = [
        ("schema", |p| p.schema = 2),
        ("two of five", |p| p.authority.threshold = 2),
        ("four keys", |p| {
            p.authority.keys.pop();
        }),
        ("unsorted", |p| p.authority.keys.swap(0, 1)),
        ("duplicate", |p| {
            p.authority.keys[1] = p.authority.keys[0].clone()
        }),
        ("wrong key id", |p| {
            p.authority.keys[0].key_id = "00".repeat(32)
        }),
        ("uppercase key", |p| {
            p.authority.keys[0].public_key_hex = p.authority.keys[0].public_key_hex.to_uppercase()
        }),
    ];
    for (name, change) in cases {
        let mut policy = fixture_policy();
        change(&mut policy);
        assert!(policy.validate(CHAIN).is_err(), "accepted {name}");
    }
}

#[test]
fn signatures_are_three_to_five_sorted_signer_signatures_over_the_bundle() {
    let policy = fixture_policy();
    let bundle = bundle_sha512();
    for count in 3..=5 {
        assert!(fixture_signatures(count).validate(&policy, &bundle).is_ok());
    }
    assert!(fixture_signatures(2).validate(&policy, &bundle).is_err());
    let cases: [(&str, fn(&mut GenesisSignatures)); 6] = [
        ("other chain", |s| s.chain_id = "another-chain".into()),
        ("other bundle", |s| s.bundle_sha512 = "00".repeat(64)),
        ("unsorted", |s| s.signatures.swap(0, 1)),
        ("repeated signer", |s| {
            s.signatures[1] = s.signatures[0].clone()
        }),
        ("outside key", |s| s.signatures[0].key_id = "ff".repeat(32)),
        ("short signature", |s| {
            s.signatures[0].signature_hex.truncate(100)
        }),
    ];
    for (name, change) in cases {
        let mut signatures = fixture_signatures(3);
        change(&mut signatures);
        assert!(
            signatures.validate(&policy, &bundle).is_err(),
            "accepted {name}"
        );
    }
}

#[test]
fn public_records_and_configuration_are_exact() {
    let directory = tempfile::tempdir().unwrap();
    let root = fixture_root(directory.path(), b"#!/bin/sh\nexit 1\n");
    let mut spaced = root.clone();
    spaced.policy_json.push(b'\n');
    assert!(spaced.policy(CHAIN).is_err());
    let mut spaced = root.clone();
    spaced.signatures_json.insert(1, b' ');
    assert!(prepare_error(&spaced).contains("canonical"));
    // The configuration file names the profile and the two public records.
    std::fs::write(directory.path().join("policy.json"), &root.policy_json).unwrap();
    std::fs::write(
        directory.path().join("signatures.json"),
        &root.signatures_json,
    )
    .unwrap();
    let input = |profile: &str| {
        serde_json::json!({
            "profile": profile,
            "helper_path": root.helper_path, "helper_scratch_path": root.helper_scratch_path,
            "helper_sha256": root.helper_sha256, "max_helper_bytes": 4096,
            "max_request_bytes": 1 << 20, "timeout_ms": 5000,
            "policy_path": directory.path().join("policy.json"),
            "signatures_path": directory.path().join("signatures.json"),
            "engine_genesis_path": root.engine_genesis_path, "max_engine_genesis_bytes": 1024,
            "engine_genesis_sha512": root.engine_genesis_sha512,
            "release_manifest_path": root.release_manifest_path, "max_release_manifest_bytes": 1024,
            "release_manifest_sha512": root.release_manifest_sha512,
        })
    };
    let path = directory.path().join("root.json");
    std::fs::write(&path, serde_json::to_vec(&input(PROFILE)).unwrap()).unwrap();
    let loaded = RootGenesis::from_config(&path).unwrap();
    assert_eq!(loaded.policy_json, root.policy_json);
    assert_eq!(loaded.signatures_json, root.signatures_json);
    let path = directory.path().join("root-other-profile.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&input("SLH-DSA-SHA2-256s")).unwrap(),
    )
    .unwrap();
    assert!(RootGenesis::from_config(&path).is_err());
    let mut extra = input(PROFILE);
    extra["enabled"] = true.into();
    let path = directory.path().join("root-extra.json");
    std::fs::write(&path, serde_json::to_vec(&extra).unwrap()).unwrap();
    assert!(RootGenesis::from_config(&path).is_err());
}

#[test]
fn every_signature_must_verify_through_the_helper() {
    let directory = tempfile::tempdir().unwrap();
    let rejecting = fixture_root(
        directory.path(),
        b"#!/bin/sh\ncat >/dev/null\nprintf 'root verification rejected\\n' >&2\nexit 2\n",
    );
    assert!(prepare_error(&rejecting).contains("was rejected"));
    let other = tempfile::tempdir().unwrap();
    let unbound = fixture_root(
        other.path(),
        b"#!/bin/sh\ncat >/dev/null\nprintf '%s' '{\"Status\":\"VERIFIED\",\"RequestSHA256\":\"fixture\",\"ArtifactSHA512\":\"fixture\",\"ChainID\":\"root-threshold-fixture\",\"Action\":\"genesis\",\"Sequence\":1}'\n",
    );
    assert!(prepare_error(&unbound).contains("differs from request"));
    // Records are checked before any helper runs.
    let mut changed = unbound.clone();
    changed.release_manifest_sha512 = "00".repeat(64);
    assert!(prepare_error(&changed).contains("Release manifest"));
}

/// The rehearsal launch configuration and native genesis for this build,
/// with its root controls (production forms in a production build).
fn launch_inputs() -> (ConsensusConfig, Vec<u8>) {
    let mut config = crate::build_profile::tests::rehearsal_config();
    let mut genesis = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/mainnet-preparation/fixtures/genesis-rehearsal/native-genesis.json"),
    )
    .unwrap();
    if crate::build_profile::PRODUCTION {
        config = crate::build_profile::tests::production_named(config);
        let text = String::from_utf8(genesis).unwrap();
        assert_eq!(text.matches("\"profile\":\"development\"").count(), 2);
        genesis = text
            .replace("\"profile\":\"development\"", "\"profile\":\"production\"")
            .into_bytes();
        // The configuration binds the genesis digest in several places (the
        // recovery domains, governance, the source binding), as the builder
        // writes it: rebind every one.
        let (before, after) = (
            hex::decode(&config.app_state_sha256).unwrap(),
            Sha256::digest(&genesis).to_vec(),
        );
        let mut value = serde_json::to_value(&config).unwrap();
        rebind(&mut value, &before, &after);
        config = serde_json::from_value(value).unwrap();
        assert_eq!(config.app_state_sha256, hex::encode(&after));
    }
    (config, genesis)
}

/// Replace a digest wherever it appears, as hex or as a byte array.
fn rebind(value: &mut serde_json::Value, before: &[u8], after: &[u8]) {
    use serde_json::Value;
    let bytes = |values: &Vec<Value>| -> Option<Vec<u8>> {
        values
            .iter()
            .map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
            .collect()
    };
    match value {
        Value::String(text) if *text == hex::encode(before) => *text = hex::encode(after),
        Value::Array(values) if bytes(values).as_deref() == Some(before) => {
            *values = after.iter().map(|b| Value::from(*b)).collect();
        }
        Value::Array(values) => values.iter_mut().for_each(|v| rebind(v, before, after)),
        Value::Object(map) => map.values_mut().for_each(|v| rebind(v, before, after)),
        _ => {}
    }
}

#[test]
#[ignore = "Requires the built root signer and verifier (scripts/run_signed_fixture_tests.py)"]
fn threshold_genesis_signed_three_of_five() {
    use std::os::unix::fs::PermissionsExt;
    let signer = PathBuf::from(std::env::var("DYT_ROOT_SIGNER").expect("built root signer"));
    let helper = PathBuf::from(std::env::var("DYT_ROOT_VERIFIER").expect("built verifier"));
    let (mut config, genesis) = launch_inputs();
    let chain = config.chain_id.clone();
    // The root controls name the release: a manifest for this executable, so
    // the handover's runtime candidate check passes (A4).
    let release = release_manifest(&config);
    let release_sha512 = hex::encode(Sha512::digest(&release));
    rebind_release(&mut config, &release_sha512);
    let config_bytes = serde_json::to_vec(&config).unwrap();
    let work = tempfile::tempdir().unwrap();
    let path = |name: &str| work.path().join(name);
    let run = |args: &[String]| {
        let output = Command::new(&signer).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "dytallix-root-sign {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    let arg = |name: &str| path(name).to_str().unwrap().to_owned();
    for i in 0..5 {
        run(&[
            "keygen".into(),
            "-private-key-out".into(),
            arg(&format!("key{i}.private")),
            "-public-key-out".into(),
            arg(&format!("key{i}.json")),
        ]);
    }
    let mut policy_args = vec![
        "policy".into(),
        "-chain-id".into(),
        chain.clone(),
        "-out".into(),
        arg("policy.json"),
    ];
    policy_args.extend((0..5).map(|i| arg(&format!("key{i}.json"))));
    run(&policy_args);
    std::fs::write(path("native-genesis.json"), &genesis).unwrap();
    std::fs::write(path("consensus.json"), &config_bytes).unwrap();
    std::fs::write(path("engine-genesis.json"), ENGINE).unwrap();
    std::fs::write(path("release-manifest.json"), &release).unwrap();
    let digests: serde_json::Value = serde_json::from_str(&run(&[
        "digest".into(),
        "-native-genesis".into(),
        arg("native-genesis.json"),
        "-config".into(),
        arg("consensus.json"),
        "-engine-genesis".into(),
        arg("engine-genesis.json"),
        "-release-manifest".into(),
        arg("release-manifest.json"),
    ]))
    .unwrap();
    let bundle = digests["bundle_sha512"].as_str().unwrap().to_owned();
    for i in 0..4 {
        run(&[
            "sign".into(),
            "-policy".into(),
            arg("policy.json"),
            "-private-key".into(),
            arg(&format!("key{i}.private")),
            "-bundle-sha512".into(),
            bundle.clone(),
            "-out".into(),
            arg(&format!("sig{i}.json")),
        ]);
    }
    for (name, count) in [("three.json", 3), ("four.json", 4)] {
        let mut args = vec![
            "combine".into(),
            "-policy".into(),
            arg("policy.json"),
            "-out".into(),
            arg(name),
        ];
        args.extend((0..count).map(|i| arg(&format!("sig{i}.json"))));
        run(&args);
    }
    for scratch in ["scratch", "control-scratch"] {
        std::fs::create_dir(path(scratch)).unwrap();
        std::fs::set_permissions(path(scratch), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let helper_sha256 = hex::encode(Sha256::digest(std::fs::read(&helper).unwrap()));
    let verifier = crate::emergency_verifier::EmergencyVerifierConfig {
        helper_path: helper.clone(),
        helper_scratch_path: path("control-scratch"),
        helper_execution: None,
        helper_sha256: helper_sha256.clone(),
        max_helper_bytes: 32 << 20,
        max_request_bytes: 1 << 20,
        timeout_ms: 30_000,
    };
    let candidate = || {
        crate::runtime_candidate_v2::RuntimeCandidateInput::V1(
            crate::consensus_settlement::DevelopmentCandidateInput {
                manifest_path: std::fs::canonicalize(path("release-manifest.json")).unwrap(),
                max_manifest_bytes: 65_536,
                max_executable_bytes: 1 << 30,
            },
        )
    };
    let root = |signatures: &[u8]| RootGenesis {
        helper_sha256: helper_sha256.clone(),
        helper_path: helper.clone(),
        helper_scratch_path: Some(path("scratch")),
        helper_execution: None,
        max_helper_bytes: 32 << 20,
        max_request_bytes: 16 << 20,
        timeout_ms: 30_000,
        policy_json: std::fs::read(path("policy.json")).unwrap(),
        signatures_json: signatures.to_vec(),
        engine_genesis_path: path("engine-genesis.json"),
        max_engine_genesis_bytes: 1024,
        engine_genesis_sha512: hex::encode(Sha512::digest(ENGINE)),
        release_manifest_path: path("release-manifest.json"),
        max_release_manifest_bytes: 65_536,
        release_manifest_sha512: release_sha512.clone(),
    };
    let three = std::fs::read(path("three.json")).unwrap();
    let four = std::fs::read(path("four.json")).unwrap();
    let open_with = |database: &Path, source: &[u8], root: RootGenesis| {
        ConsensusApplication::open_with_root_runtime(
            database,
            config.clone(),
            genesis.clone(),
            source,
            root,
            Some(verifier.clone()),
            Some(candidate()),
            None,
        )
    };
    let open =
        |database: &Path, signatures: &[u8]| open_with(database, &config_bytes, root(signatures));

    // Open, initialize and restart from three signatures.
    let database = tempfile::tempdir().unwrap();
    let mut app = open(database.path(), &three).unwrap();
    let committed = app
        .init_chain(&chain, 1, &genesis, &config.validators)
        .unwrap();
    drop(app);
    let storage = Storage::open(database.path().to_path_buf()).unwrap();
    let receipt: serde_json::Value =
        serde_json::from_slice(&storage.db.get(STATE_KEY).unwrap().unwrap()).unwrap();
    drop(storage);
    let signed: GenesisSignatures = serde_json::from_slice(&three).unwrap();
    assert_eq!(receipt["version"], 2);
    let scope = if crate::build_profile::PRODUCTION {
        "production"
    } else {
        "development"
    };
    assert_eq!(receipt["scope"], scope);
    assert_eq!(receipt["threshold"], 3);
    assert_eq!(receipt["consumed_sequence"], 1);
    assert_eq!(receipt["artifact_sha512"], bundle);
    assert_eq!(
        receipt["signing_key_ids"],
        serde_json::json!(signed
            .signatures
            .iter()
            .map(|s| &s.key_id)
            .collect::<Vec<_>>())
    );
    assert_eq!(
        receipt["policy_sha256"],
        hex::encode(Sha256::digest(std::fs::read(path("policy.json")).unwrap()))
    );
    assert_eq!(
        receipt["signatures_sha256"],
        hex::encode(Sha256::digest(&three))
    );
    let mut restarted = open(database.path(), &three).unwrap();
    assert_eq!(restarted.info().unwrap(), committed);
    assert_eq!(
        restarted
            .init_chain(&chain, 1, &genesis, &config.validators)
            .unwrap(),
        committed
    );
    drop(restarted);
    assert!(ConsensusApplication::open(database.path(), config.clone(), genesis.clone()).is_err());

    // Four signatures are another valid genesis with another receipt, so
    // another application hash: every node must read the same file.
    let database_four = tempfile::tempdir().unwrap();
    let mut app = open(database_four.path(), &four).unwrap();
    let committed_four = app
        .init_chain(&chain, 1, &genesis, &config.validators)
        .unwrap();
    assert_ne!(committed_four.app_hash, committed.app_hash);
    drop(app);
    assert!(open(database.path(), &four).is_err());

    // Refusals leave no database behind.
    let edit = |change: &dyn Fn(&mut GenesisSignatures)| {
        let mut file: GenesisSignatures = serde_json::from_slice(&three).unwrap();
        change(&mut file);
        serde_json::to_vec(&file).unwrap()
    };
    let flip = |hex: &mut String| {
        let byte = if hex.as_bytes()[200] == b'0' {
            "1"
        } else {
            "0"
        };
        hex.replace_range(200..201, byte);
    };
    let cases: Vec<(&str, Vec<u8>)> = vec![
        (
            "two signatures",
            edit(&|f| {
                f.signatures.pop();
            }),
        ),
        (
            "changed signature",
            edit(&|f| flip(&mut f.signatures[1].signature_hex)),
        ),
        (
            "signatures under other keys",
            edit(&|f| {
                let first = f.signatures[0].signature_hex.clone();
                f.signatures[0].signature_hex = f.signatures[1].signature_hex.clone();
                f.signatures[1].signature_hex = first;
            }),
        ),
        ("other bundle", edit(&|f| f.bundle_sha512 = "00".repeat(64))),
    ];
    for (name, signatures) in cases {
        let empty = tempfile::tempdir().unwrap();
        let target = empty.path().join("not-created");
        assert!(open(&target, &signatures).is_err(), "accepted {name}");
        assert!(!target.exists(), "{name} created a database");
    }
    let empty = tempfile::tempdir().unwrap();
    let target = empty.path().join("not-created");
    let spaced = [b" ".as_slice(), &config_bytes].concat();
    assert!(open_with(&target, &spaced, root(&three)).is_err());
    let mut other_engine = root(&three);
    other_engine.engine_genesis_path = path("release-manifest.json");
    other_engine.engine_genesis_sha512 = release_sha512.clone();
    assert!(open_with(&target, &config_bytes, other_engine).is_err());
    // The root controls need their verifier, and a handover its candidate.
    assert!(ConsensusApplication::open_with_root(
        &target,
        config.clone(),
        genesis.clone(),
        &config_bytes,
        root(&three)
    )
    .is_err());
    assert!(!target.exists());
}

/// A release manifest naming this executable, the chain and the registry.
fn release_manifest(config: &ConsensusConfig) -> Vec<u8> {
    let executable = std::fs::read(std::env::current_exe().unwrap()).unwrap();
    serde_json::to_vec(&crate::runtime_candidate::ManifestV1 {
        schema: 1,
        chain_id: config.chain_id.clone(),
        app_genesis_sha256: config.app_state_sha256.clone(),
        target: crate::runtime_candidate::Target {
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
        },
        consensus_stdio: crate::runtime_candidate::Executable {
            bytes: executable.len() as u64,
            sha256: hex::encode(Sha256::digest(&executable)),
            sha512: hex::encode(Sha512::digest(&executable)),
        },
        migration_registry_sha256: crate::upgrade::registry_sha256(),
    })
    .unwrap()
}

/// Point every root control at one release.
fn rebind_release(config: &mut ConsensusConfig, release_sha512: &str) {
    config.emergency.as_mut().unwrap().release_sha512 = release_sha512.into();
    match config.upgrade.as_mut().unwrap() {
        crate::upgrade::Policy::V1(policy) => policy.source_release_sha512 = release_sha512.into(),
        crate::upgrade::Policy::V2(policy) => policy.initial_release_sha512 = release_sha512.into(),
    }
    config
        .release_handover
        .as_mut()
        .unwrap()
        .initial_release_sha512 = release_sha512.into();
}

#[test]
fn genesis_signers_hold_no_other_role() {
    let (config, genesis) = launch_inputs();
    let directory = tempfile::tempdir().unwrap();
    let mut root = fixture_root(directory.path(), b"#!/bin/sh\nexit 1\n");
    // One genesis key is also an emergency freeze key.
    let shared = config.emergency.as_ref().unwrap().freeze_authority.keys[0].clone();
    let mut policy = fixture_policy();
    policy.chain_id = config.chain_id.clone();
    policy.authority.keys[0] = GenesisKey {
        key_id: shared.key_id,
        public_key_hex: shared.public_key_hex,
    };
    policy
        .authority
        .keys
        .sort_by(|a, b| a.key_id.cmp(&b.key_id));
    root.policy_json = serde_json::to_vec(&policy).unwrap();
    root.release_manifest_sha512 = config.emergency.as_ref().unwrap().release_sha512.clone();
    let target = directory.path().join("not-created");
    let error = ConsensusApplication::open_with_root(
        &target,
        config.clone(),
        genesis,
        &serde_json::to_vec(&config).unwrap(),
        root,
    )
    .err()
    .map(|e| format!("{e:#}"))
    .unwrap();
    assert!(error.contains("also holds"), "{error}");
    assert!(!target.exists());
}
