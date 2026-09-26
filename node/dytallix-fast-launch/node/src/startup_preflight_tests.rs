//! Read-only release selection before writable database startup.
use super::emergency_tests::RootFixture;
use super::*;

fn file_bytes(path: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, files: &mut BTreeMap<std::path::PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), files);
            } else {
                files.insert(
                    entry.path().strip_prefix(root).unwrap().to_path_buf(),
                    std::fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(path, path, &mut files);
    files
}

#[test]
fn read_only_missing_database_does_not_create_directory() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("absent");
    assert!(Storage::open_read_only(database.clone()).is_err());
    assert!(!database.exists());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn read_only_handle_reads_wal_and_refuses_all_database_writes() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("database");
    let writer = Storage::open(database.clone()).unwrap();
    let mut options = rocksdb::WriteOptions::default();
    options.set_sync(true);
    writer
        .db
        .put_opt(b"selected-release", b"committed", &options)
        .unwrap();
    // Keep the writer alive so the test requires reading its current WAL.
    let before = file_bytes(&database);
    let reader = Storage::open_read_only(database.clone()).unwrap();
    assert_eq!(
        reader.db.get(b"selected-release").unwrap().unwrap(),
        b"committed"
    );
    assert!(reader.db.put(b"selected-release", b"unapproved").is_err());
    assert!(reader.db.delete(b"selected-release").is_err());
    let mut batch = rocksdb::WriteBatch::default();
    batch.put(b"selected-release", b"unapproved");
    assert!(reader.db.write(batch).is_err());
    drop(reader);
    assert_eq!(file_bytes(&database), before);
    assert_eq!(
        writer.db.get(b"selected-release").unwrap().unwrap(),
        b"committed"
    );
}

fn preflight(
    fixture: &Fixture,
    root: &RootFixture,
    database: &Path,
) -> anyhow::Result<VerifiedReleaseAuthority> {
    ConsensusApplication::preflight_development_release(
        database,
        &fixture.config,
        &fixture.genesis,
        &root.source,
        &root.root,
        &root.verifier,
    )
}

#[test]
#[ignore = "Requires explicitly supplied real SLH root fixture tools"]
fn startup_preflight_bootstrap_and_committed_root_are_read_only() {
    let mut fixture = Fixture::new();
    let root = RootFixture::with_config(&mut fixture, |_, _| {});
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("database");
    let authority = preflight(&fixture, &root, &database).unwrap();
    assert_eq!(
        authority.expected_candidate().manifest_sha512,
        root.root.release_manifest_sha512
    );
    assert!(authority.committed_info().is_none());
    assert!(!database.exists());
    let app = root.initialized(&fixture, &database);
    let expected = app.info().unwrap();
    drop(app);
    let before = file_bytes(&database);
    let authority = preflight(&fixture, &root, &database).unwrap();
    assert_eq!(authority.committed_info(), Some(&expected));
    assert_eq!(
        authority.expected_candidate().manifest_sha512,
        root.root.release_manifest_sha512
    );
    assert_eq!(file_bytes(&database), before);
}

#[test]
#[ignore = "Requires explicitly supplied real SLH root fixture tools"]
fn startup_rejected_candidate_leaves_database_and_wal_bytes_unchanged() {
    let mut fixture = Fixture::new();
    let root = RootFixture::with_config(&mut fixture, |_, _| {});
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("database");
    drop(root.initialized(&fixture, &database));
    let manifest = directory.path().join("wrong-candidate.json");
    std::fs::write(&manifest, b"{}").unwrap();
    let before = file_bytes(&database);
    let error = ConsensusApplication::open_with_development_candidate(
        &database,
        fixture.config.clone(),
        fixture.genesis.clone(),
        &root.source,
        root.root.clone(),
        root.verifier.clone(),
        Some(DevelopmentCandidateInput {
            manifest_path: std::fs::canonicalize(manifest).unwrap(),
            max_manifest_bytes: 65_536,
            max_executable_bytes: 1024 * 1024 * 1024,
        }),
    )
    .err()
    .expect("Wrong candidate must fail");
    assert!(error.to_string().contains("Candidate"), "{error:#}");
    assert_eq!(file_bytes(&database), before);
}

#[test]
#[ignore = "Requires explicitly supplied real SLH root fixture tools"]
fn startup_failed_root_or_history_never_changes_database() {
    let mut fixture = Fixture::new();
    let root = RootFixture::with_config(&mut fixture, |_, _| {});
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("database");
    let mut bad_root = root.root.clone();
    bad_root.request_json = b"{}".to_vec();
    assert!(ConsensusApplication::preflight_development_release(
        &database,
        &fixture.config,
        &fixture.genesis,
        &root.source,
        &bad_root,
        &root.verifier
    )
    .is_err());
    assert!(!database.exists());
    drop(root.initialized(&fixture, &database));
    let writer = Storage::open(database.clone()).unwrap();
    writer.db.put(HEAD_KEY, b"invalid-head").unwrap();
    drop(writer);
    let before = file_bytes(&database);
    assert!(preflight(&fixture, &root, &database).is_err());
    assert_eq!(file_bytes(&database), before);
}

#[test]
#[ignore = "Requires explicitly supplied real SLH root fixture tools"]
fn startup_nonempty_unknown_directory_is_not_bootstrap_authority() {
    let mut fixture = Fixture::new();
    let root = RootFixture::with_config(&mut fixture, |_, _| {});
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("database");
    std::fs::create_dir(&database).unwrap();
    std::fs::write(database.join("unknown-state"), b"do not overwrite").unwrap();
    let before = file_bytes(&database);
    assert!(preflight(&fixture, &root, &database).is_err());
    assert_eq!(file_bytes(&database), before);
}

#[path = "runtime_candidate_startup_tests.rs"]
mod runtime_candidate_startup_tests;

#[path = "bootstrap_authority_order_tests.rs"]
mod bootstrap_authority_order_tests;
