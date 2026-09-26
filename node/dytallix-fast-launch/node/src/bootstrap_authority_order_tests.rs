//! Authority replay must not execute an unapproved candidate live helper.
use super::*;
use crate::runtime_candidate_v2::RuntimeCandidateInput;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

fn commit_signed_freeze(fixture: &Fixture, root: &RootFixture, app: &mut ConsensusApplication) {
    let info = app.info().unwrap();
    let state = emergency_state(&app.storage, &app.config).unwrap().unwrap();
    let payload = emergency::Payload {
        schema: 1,
        chain_id: CHAIN.into(),
        release_sha512: fixture
            .config
            .emergency
            .as_ref()
            .unwrap()
            .release_sha512
            .clone(),
        action: emergency::Action::Freeze,
        sequence: state.next_sequence(),
        parent_height: info.height,
        parent_app_hash: info.app_hash,
        target_height: info.height + 1,
        v2: None,
    };
    let artifact = root._directory.path().join("ordering-artifact.bin");
    let signed = root
        ._directory
        .path()
        .join("ordering-public-signature.json");
    std::fs::write(&artifact, emergency::artifact_bytes(&payload).unwrap()).unwrap();
    let result = std::process::Command::new(std::env::var("DYT_EMERGENCY_TEST_SIGNER").unwrap())
        .arg("--artifact")
        .arg(&artifact)
        .arg("--output")
        .arg(&signed)
        .arg("--chain")
        .arg(CHAIN)
        .arg("--sequence")
        .arg(payload.sequence.to_string())
        .arg("--height")
        .arg(payload.target_height.to_string())
        .arg("--fixture-key")
        .arg("11")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(signed).unwrap()).unwrap();
    let control = emergency::Control {
        kind: emergency::CONTROL_KIND.into(),
        payload,
        signatures: vec![emergency::ControlSignature {
            key_id: "freeze-key".into(),
            signature_hex: hex::encode(
                B64.decode(value["Request"]["Signature"].as_str().unwrap())
                    .unwrap(),
            ),
        }],
    };
    let result = commit(app, 1, vec![serde_json::to_vec(&control).unwrap()]);
    assert_eq!(result.tx_results[0].code, 0);
}

fn unapproved_live(root: &RootFixture) -> (EmergencyVerifierConfig, std::path::PathBuf) {
    let marker = root._directory.path().join("unapproved-helper-executed");
    let helper = root._directory.path().join("unapproved-live-helper");
    let escaped_marker = marker.to_str().unwrap().replace('\'', "'\\''");
    let script = format!("#!/bin/sh\nprintf executed > '{escaped_marker}'\nexit 97\n");
    std::fs::write(&helper, &script).unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut live = root.verifier.clone();
    live.helper_path = std::fs::canonicalize(helper).unwrap();
    live.helper_sha256 = hex::encode(Sha256::digest(script.as_bytes()));
    (live, marker)
}

fn invalid_v2(root: &RootFixture) -> RuntimeCandidateInput {
    let manifest = root._directory.path().join("unapproved-catalog.json");
    let mapping = root._directory.path().join("unapproved-mapping.json");
    std::fs::write(&manifest, b"{}").unwrap();
    std::fs::write(&mapping, b"{}").unwrap();
    serde_json::from_value(serde_json::json!({
        "config_schema": 2,
        "manifest_path": std::fs::canonicalize(manifest).unwrap(),
        "mapping_path": std::fs::canonicalize(mapping).unwrap(),
        "allowed_owner_uids": [std::fs::metadata(&root.root.helper_path).unwrap().uid()],
        "bounds": {"max_manifest_bytes":65536,"max_mapping_bytes":65536,
            "max_members":32,"max_roles":16,"max_runtime_profiles":8,"max_references":64,
            "max_id_bytes":128,"max_path_bytes":4096,"max_member_bytes":67108864,
            "max_total_member_bytes":268435456},
        "observation": {"max_stat_bytes":65536,"max_maps_bytes":524288,"max_map_entries":4096,
            "max_path_bytes":4096,"max_unique_files":256,"max_file_bytes":67108864,
            "max_total_file_bytes":268435456,"max_elapsed_millis":10000}
    }))
    .unwrap()
}

#[test]
#[ignore = "Requires explicitly supplied real SLH root fixture tools"]
fn bootstrap_authority_ignores_unapproved_live_helper_for_real_signed_history() {
    let mut fixture = Fixture::new();
    let root = RootFixture::with_config(&mut fixture, |_, _| {});
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("database");
    let mut app = root.initialized(&fixture, &database);
    commit_signed_freeze(&fixture, &root, &mut app);
    let expected = app.info().unwrap();
    drop(app);
    let (live, marker) = unapproved_live(&root);
    let before = file_bytes(&database);
    let authority = ConsensusApplication::preflight_development_release(
        &database,
        &fixture.config,
        &fixture.genesis,
        &root.source,
        &root.root,
        &live,
    )
    .unwrap();
    assert_eq!(authority.committed_info(), Some(&expected));
    assert!(
        !marker.exists(),
        "Preflight executed the unapproved live helper"
    );
    assert_eq!(file_bytes(&database), before);
    let candidate = invalid_v2(&root);
    assert!(
        ConsensusApplication::open_with_development_runtime_candidate(
            &database,
            fixture.config.clone(),
            fixture.genesis.clone(),
            &root.source,
            root.root.clone(),
            live,
            Some(candidate),
        )
        .is_err()
    );
    assert!(
        !marker.exists(),
        "Rejected V2 startup executed its unapproved live helper"
    );
    assert_eq!(file_bytes(&database), before);
}

#[test]
#[ignore = "Requires explicitly supplied real SLH root fixture tools"]
fn bootstrap_authority_requires_its_own_scratch_and_does_not_borrow_live_scratch() {
    let mut fixture = Fixture::new();
    let root = RootFixture::with_config(&mut fixture, |_, _| {});
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("database");
    drop(root.initialized(&fixture, &database));
    let (live, marker) = unapproved_live(&root);
    let before = file_bytes(&database);
    let mut independent = root.root.clone();
    independent.helper_scratch_path = None;
    // Even an independently pinned root helper must not execute before its
    // required scratch setting is validated. This marker makes that observable.
    independent.helper_path = live.helper_path.clone();
    independent.helper_sha256 = live.helper_sha256.clone();
    let error = ConsensusApplication::preflight_development_release(
        &database,
        &fixture.config,
        &fixture.genesis,
        &root.source,
        &independent,
        &live,
    )
    .err()
    .expect("Independent scratch must be explicit");
    assert!(
        error.to_string().contains("Independent root scratch"),
        "{error:#}"
    );
    assert!(!marker.exists());
    assert_eq!(file_bytes(&database), before);
    assert!(
        ConsensusApplication::open_with_development_runtime_candidate(
            &database,
            fixture.config.clone(),
            fixture.genesis.clone(),
            &root.source,
            independent,
            live,
            Some(invalid_v2(&root)),
        )
        .is_err()
    );
    assert!(
        !marker.exists(),
        "V2 executed a root helper before validating root scratch"
    );
    assert_eq!(file_bytes(&database), before);
}
