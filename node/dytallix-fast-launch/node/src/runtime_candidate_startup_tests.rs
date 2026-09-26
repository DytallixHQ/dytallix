//! Additive candidate dispatch preserves V1 and rejects caller-supplied authority.
use super::*;
use crate::runtime_candidate_v2::RuntimeCandidateInput;

#[test]
fn runtime_dispatch_preserves_v1_configuration_and_rejects_added_authority() {
    let original = serde_json::json!({
        "manifest_path": "/development/manifest.json",
        "max_manifest_bytes": 65536,
        "max_executable_bytes": 1048576
    });
    let old: DevelopmentCandidateInput = serde_json::from_value(original.clone()).unwrap();
    let dispatch: RuntimeCandidateInput = serde_json::from_value(original.clone()).unwrap();
    match dispatch {
        RuntimeCandidateInput::V1(input) => {
            assert_eq!(input.manifest_path, old.manifest_path);
            assert_eq!(input.max_manifest_bytes, old.max_manifest_bytes);
            assert_eq!(input.max_executable_bytes, old.max_executable_bytes);
        }
        RuntimeCandidateInput::V2(_) => panic!("Existing V1 input changed its dispatch"),
    }
    let mut changed = original;
    changed["expected_manifest_sha512"] = "11".repeat(64).into();
    assert!(serde_json::from_value::<RuntimeCandidateInput>(changed).is_err());
}

#[test]
#[ignore = "Requires explicitly supplied real SLH root fixture tools"]
fn runtime_dispatch_and_v1_wrapper_reject_same_unapproved_manifest_without_writes() {
    let mut fixture = Fixture::new();
    let root = RootFixture::with_config(&mut fixture, |_, _| {});
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("database");
    drop(root.initialized(&fixture, &database));
    let manifest = directory.path().join("wrong-candidate.json");
    std::fs::write(&manifest, b"{}").unwrap();
    let input = serde_json::json!({
        "manifest_path": std::fs::canonicalize(manifest).unwrap(),
        "max_manifest_bytes": 65536,
        "max_executable_bytes": 1073741824_u64
    });
    let before = file_bytes(&database);
    let legacy = ConsensusApplication::open_with_development_candidate(
        &database,
        fixture.config.clone(),
        fixture.genesis.clone(),
        &root.source,
        root.root.clone(),
        root.verifier.clone(),
        Some(serde_json::from_value(input.clone()).unwrap()),
    )
    .err()
    .expect("Legacy wrapper must reject candidate");
    assert_eq!(file_bytes(&database), before);
    let dispatched = ConsensusApplication::open_with_development_runtime_candidate(
        &database,
        fixture.config.clone(),
        fixture.genesis.clone(),
        &root.source,
        root.root.clone(),
        root.verifier.clone(),
        Some(serde_json::from_value(input).unwrap()),
    )
    .err()
    .expect("Runtime dispatch must reject candidate");
    assert_eq!(legacy.to_string(), dispatched.to_string());
    assert_eq!(file_bytes(&database), before);
}

#[test]
#[ignore = "Requires explicitly supplied real SLH root fixture tools"]
fn runtime_dispatch_invalid_v2_catalog_cannot_open_writable_state() {
    use std::os::unix::fs::MetadataExt;
    let mut fixture = Fixture::new();
    let root = RootFixture::with_config(&mut fixture, |_, _| {});
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("database");
    drop(root.initialized(&fixture, &database));
    let manifest = directory.path().join("unapproved-v2.json");
    let mapping = directory.path().join("mapping.json");
    std::fs::write(&manifest, b"{}").unwrap();
    std::fs::write(&mapping, b"{}").unwrap();
    let input = serde_json::json!({
        "config_schema": 2,
        "manifest_path": std::fs::canonicalize(manifest).unwrap(),
        "mapping_path": std::fs::canonicalize(mapping).unwrap(),
        "allowed_owner_uids": [std::fs::metadata(&root.root.helper_path).unwrap().uid()],
        "bounds": {
            "max_manifest_bytes": 65536, "max_mapping_bytes": 65536,
            "max_members": 32, "max_roles": 16, "max_runtime_profiles": 8,
            "max_references": 64, "max_id_bytes": 128, "max_path_bytes": 4096,
            "max_member_bytes": 67108864, "max_total_member_bytes": 268435456
        },
        "observation": {
            "max_stat_bytes": 65536, "max_maps_bytes": 524288, "max_map_entries": 4096,
            "max_path_bytes": 4096, "max_unique_files": 256,
            "max_file_bytes": 67108864, "max_total_file_bytes": 268435456,
            "max_elapsed_millis": 10000
        }
    });
    let candidate: RuntimeCandidateInput = serde_json::from_value(input).unwrap();
    assert!(matches!(&candidate, RuntimeCandidateInput::V2(_)));
    let before = file_bytes(&database);
    assert!(
        ConsensusApplication::open_with_development_runtime_candidate(
            &database,
            fixture.config.clone(),
            fixture.genesis.clone(),
            &root.source,
            root.root.clone(),
            root.verifier.clone(),
            Some(candidate),
        )
        .is_err()
    );
    assert_eq!(file_bytes(&database), before);
}
