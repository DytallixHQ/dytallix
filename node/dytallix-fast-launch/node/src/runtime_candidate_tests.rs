use super::*;
use std::fs;
use tempfile::TempDir;

struct Fixture {
    _dir: TempDir,
    path: PathBuf,
    executable: PathBuf,
    manifest: ManifestV1,
    expected: ExpectedCandidate,
    bounds: Bounds,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let executable = base.join("candidate");
        let contents = b"bounded executable fixture\0";
        fs::write(&executable, contents).unwrap();
        let manifest = ManifestV1 {
            schema: 1,
            chain_id: "candidate-test-1".into(),
            app_genesis_sha256: "11".repeat(32),
            target: Target {
                os: std::env::consts::OS.into(),
                arch: std::env::consts::ARCH.into(),
            },
            consensus_stdio: Executable {
                bytes: contents.len() as u64,
                sha256: hex::encode(Sha256::digest(contents)),
                sha512: hex::encode(Sha512::digest(contents)),
            },
            migration_registry_sha256: "22".repeat(32),
        };
        let path = base.join("manifest.json");
        let expected = ExpectedCandidate {
            manifest_sha512: String::new(),
            chain_id: manifest.chain_id.clone(),
            app_genesis_sha256: manifest.app_genesis_sha256.clone(),
            migration_registry_sha256: manifest.migration_registry_sha256.clone(),
        };
        let mut f = Self {
            _dir: dir,
            path,
            executable,
            manifest,
            expected,
            bounds: Bounds {
                max_manifest_bytes: 4096,
                max_executable_bytes: 4096,
            },
        };
        f.write_manifest();
        f
    }
    fn write_bytes(&mut self, bytes: &[u8]) {
        fs::write(&self.path, bytes).unwrap();
        self.expected.manifest_sha512 = hex::encode(Sha512::digest(bytes));
    }
    fn write_manifest(&mut self) {
        self.write_bytes(&serde_json::to_vec(&self.manifest).unwrap());
    }
    fn check(&self) -> Result<VerifiedCandidate> {
        verify_impl(
            &self.path,
            &self.executable,
            &self.expected,
            self.bounds,
            false,
        )
    }
    fn refuses(&self, text: &str) {
        let error = format!("{:#}", self.check().unwrap_err());
        assert!(error.contains(text), "{error}");
    }
}

#[test]
fn runtime_candidate_valid_binding_and_opaque_getters() {
    let f = Fixture::new();
    let checked = f.check().unwrap();
    assert_eq!(checked.manifest(), &f.manifest);
    assert_eq!(checked.manifest_sha512(), f.expected.manifest_sha512);
    assert_eq!(checked.chain_id(), f.expected.chain_id);
    assert_eq!(checked.app_genesis_sha256(), f.expected.app_genesis_sha256);
    assert_eq!(
        checked.migration_registry_sha256(),
        f.expected.migration_registry_sha256
    );
    assert_eq!(checked.executable_path(), f.executable);
    assert_eq!(checked.executable(), &f.manifest.consensus_stdio);
    assert!(!checked.kernel_image_identity_checked());
}

#[test]
fn runtime_candidate_trusted_manifest_hash_required() {
    let mut f = Fixture::new();
    f.expected.manifest_sha512 = "00".repeat(64);
    f.refuses("manifest digest mismatch");
}

#[test]
fn runtime_candidate_wrong_executable_bytes_rejected() {
    let f = Fixture::new();
    fs::write(
        &f.executable,
        vec![b'x'; f.manifest.consensus_stdio.bytes as usize],
    )
    .unwrap();
    f.refuses("SHA-256 mismatch");
}

#[test]
fn runtime_candidate_both_executable_digests_required() {
    let mut f = Fixture::new();
    f.manifest.consensus_stdio.sha512 = "00".repeat(64);
    f.write_manifest();
    f.refuses("SHA-512 mismatch");
}

#[test]
fn runtime_candidate_chain_genesis_registry_and_target_bound() {
    let mut f = Fixture::new();
    f.manifest.chain_id = "different".into();
    f.write_manifest();
    f.refuses("chain ID mismatch");
    let mut f = Fixture::new();
    f.manifest.app_genesis_sha256 = "33".repeat(32);
    f.write_manifest();
    f.refuses("genesis mismatch");
    let mut f = Fixture::new();
    f.manifest.migration_registry_sha256 = "33".repeat(32);
    f.write_manifest();
    f.refuses("registry mismatch");
    let mut f = Fixture::new();
    f.manifest.target.os = "unsupported".into();
    f.write_manifest();
    f.refuses("target mismatch");
    let mut f = Fixture::new();
    f.manifest.target.arch = "unsupported".into();
    f.write_manifest();
    f.refuses("target mismatch");
}

#[test]
fn runtime_candidate_noncanonical_and_malformed_json_refused() {
    for kind in 0..6 {
        let mut f = Fixture::new();
        let mut bytes = serde_json::to_vec(&f.manifest).unwrap();
        match kind {
            0 => bytes.push(b'\n'),
            1 => bytes = serde_json::to_vec_pretty(&f.manifest).unwrap(),
            2 => bytes = b"{\"schema\":1,\"schema\":1}".to_vec(),
            3 => bytes = b"{broken".to_vec(),
            4 => {
                bytes.pop();
                bytes.extend_from_slice(b",\"unknown\":true}");
            }
            _ => {
                bytes = String::from_utf8(bytes)
                    .unwrap()
                    .replace("\"schema\":1", "\"schema\":1.0")
                    .into_bytes()
            }
        }
        f.write_bytes(&bytes);
        assert!(f.check().is_err(), "kind {kind}");
    }
}

#[test]
fn runtime_candidate_schema_and_digest_encoding_refused() {
    let mut f = Fixture::new();
    f.manifest.schema = 2;
    f.write_manifest();
    f.refuses("Unsupported");
    let mut f = Fixture::new();
    f.manifest.consensus_stdio.sha256 = "AB".repeat(32);
    f.write_manifest();
    f.refuses("lowercase");
    let mut f = Fixture::new();
    f.expected.manifest_sha512 = "0".repeat(127);
    f.refuses("lowercase");
    let mut f = Fixture::new();
    f.expected.chain_id.clear();
    f.refuses("chain ID is invalid");
}

#[test]
fn runtime_candidate_explicit_size_bounds_enforced() {
    let mut f = Fixture::new();
    f.bounds.max_manifest_bytes = 0;
    f.refuses("nonzero bounds");
    let mut f = Fixture::new();
    f.bounds.max_executable_bytes = 0;
    f.refuses("nonzero bounds");
    let mut f = Fixture::new();
    f.bounds.max_manifest_bytes = 1;
    f.refuses("bounded regular file");
    let mut f = Fixture::new();
    f.bounds.max_executable_bytes = 1;
    f.refuses("length exceeds bound");
    let mut f = Fixture::new();
    f.manifest.consensus_stdio.bytes += 1;
    f.write_manifest();
    f.refuses("length mismatch");
    let mut f = Fixture::new();
    f.manifest.consensus_stdio.bytes = 0;
    f.write_manifest();
    f.refuses("length exceeds bound");
    let f = Fixture::new();
    fs::write(&f.executable, []).unwrap();
    f.refuses("bounded regular file");
}

#[test]
fn runtime_candidate_relative_and_lexical_aliases_refused() {
    let mut f = Fixture::new();
    f.path = PathBuf::from("manifest.json");
    f.refuses("must be absolute");
    let mut f = Fixture::new();
    f.path = f.path.parent().unwrap().join(".").join("manifest.json");
    f.refuses("lexical aliases");
    let mut f = Fixture::new();
    f.executable = f.executable.parent().unwrap().join(".").join("candidate");
    f.refuses("lexical aliases");
}

#[cfg(unix)]
#[test]
fn runtime_candidate_symlink_and_hardlink_aliases_refused() {
    use std::os::unix::fs::symlink;
    let mut f = Fixture::new();
    let alias = f.path.with_file_name("alias.json");
    symlink(&f.path, &alias).unwrap();
    f.path = alias;
    f.refuses("symlink or lexical aliases");
    let mut f = Fixture::new();
    let alias = f.executable.with_file_name("alias");
    symlink(&f.executable, &alias).unwrap();
    f.executable = alias;
    f.refuses("symlink or lexical aliases");
    let f = Fixture::new();
    fs::hard_link(&f.path, f.path.with_file_name("hardlink")).unwrap();
    f.refuses("hard-link aliases");
    let f = Fixture::new();
    fs::hard_link(&f.executable, f.path.with_file_name("hardlink")).unwrap();
    f.refuses("hard-link aliases");
    let mut f = Fixture::new();
    let alias = f.path.parent().unwrap().join("parent-alias");
    symlink(f.path.parent().unwrap(), &alias).unwrap();
    f.path = alias.join("manifest.json");
    f.refuses("symlink or lexical aliases");
}

#[cfg(unix)]
#[test]
fn runtime_candidate_unsafe_permissions_and_nonfiles_refused() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    fs::set_permissions(&f.path, fs::Permissions::from_mode(0o666)).unwrap();
    f.refuses("unsafe permissions");
    let f = Fixture::new();
    fs::set_permissions(&f.executable, fs::Permissions::from_mode(0o775)).unwrap();
    f.refuses("unsafe permissions");
    let mut f = Fixture::new();
    f.path = f.path.parent().unwrap().to_owned();
    f.refuses("bounded regular file");
}

#[test]
fn runtime_candidate_file_change_after_open_refused() {
    let f = Fixture::new();
    let (file, metadata) = open_checked(&f.path, 4096).unwrap();
    fs::write(&f.path, b"changed").unwrap();
    assert!(finish_checked(&f.path, &file, &metadata, 4096)
        .unwrap_err()
        .to_string()
        .contains("changed while reading"));
    let f = Fixture::new();
    let (file, metadata) = open_checked(&f.path, 4096).unwrap();
    let substitute = f.path.with_file_name("substitute");
    fs::write(&substitute, fs::read(&f.path).unwrap()).unwrap();
    fs::rename(&substitute, &f.path).unwrap();
    assert!(finish_checked(&f.path, &file, &metadata, 4096).is_err());
}

#[test]
fn runtime_candidate_public_api_cannot_select_fixture_executable() {
    let f = Fixture::new();
    assert!(verify_current_executable(&f.path, &f.expected, f.bounds).is_err());
}
