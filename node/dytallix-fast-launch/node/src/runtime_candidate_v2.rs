//! Development V2 startup binding. Authority comes from verified chain history.
//! Member files and current-process maps do not prove helper role execution,
//! continuous loading control, bootstrap approval, or production acceptance.
use crate::consensus_settlement::{DevelopmentCandidateInput, VerifiedReleaseAuthority};
use crate::emergency_verifier::EmergencyVerifierConfig;
use crate::root_genesis::DevelopmentRootGenesis;
use anyhow::{ensure, Context, Result};
use dytallix_release_runtime::{component_candidate as catalog, observation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};
use std::collections::BTreeSet;
use std::fs::{File, Metadata};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogBoundsInput {
    pub max_manifest_bytes: usize,
    pub max_mapping_bytes: usize,
    pub max_members: usize,
    pub max_roles: usize,
    pub max_runtime_profiles: usize,
    pub max_references: usize,
    pub max_id_bytes: usize,
    pub max_path_bytes: usize,
    pub max_member_bytes: u64,
    pub max_total_member_bytes: u64,
}
impl CatalogBoundsInput {
    pub fn validation_bounds(&self) -> catalog::ValidationBounds {
        catalog::ValidationBounds {
            max_manifest_bytes: self.max_manifest_bytes,
            max_mapping_bytes: self.max_mapping_bytes,
            max_members: self.max_members,
            max_roles: self.max_roles,
            max_runtime_profiles: self.max_runtime_profiles,
            max_references: self.max_references,
            max_id_bytes: self.max_id_bytes,
            max_path_bytes: self.max_path_bytes,
            max_member_bytes: self.max_member_bytes,
            max_total_member_bytes: self.max_total_member_bytes,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationBoundsInput {
    pub max_stat_bytes: usize,
    pub max_maps_bytes: usize,
    pub max_map_entries: usize,
    pub max_path_bytes: usize,
    pub max_unique_files: usize,
    pub max_file_bytes: u64,
    pub max_total_file_bytes: u64,
    pub max_elapsed_millis: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DevelopmentCandidateV2Input {
    pub config_schema: u16,
    pub manifest_path: PathBuf,
    pub mapping_path: PathBuf,
    pub bounds: CatalogBoundsInput,
    pub observation: ObservationBoundsInput,
    pub allowed_owner_uids: Vec<u32>,
}
impl DevelopmentCandidateV2Input {
    fn file_policy(&self) -> Result<catalog::FilePolicy> {
        ensure!(
            self.config_schema == 2,
            "Unsupported V2 candidate configuration schema"
        );
        ensure!(
            !self.allowed_owner_uids.is_empty()
                && self.allowed_owner_uids.windows(2).all(|w| w[0] < w[1]),
            "Candidate owner UID policy must be nonempty, sorted, and unique"
        );
        Ok(catalog::FilePolicy {
            allowed_owner_uids: self.allowed_owner_uids.iter().copied().collect(),
        })
    }
    pub fn observation_bounds(&self) -> Result<observation::Bounds> {
        let policy = self.file_policy()?;
        let b = &self.observation;
        ensure!(
            [
                b.max_stat_bytes,
                b.max_maps_bytes,
                b.max_map_entries,
                b.max_path_bytes,
                b.max_unique_files
            ]
            .iter()
            .all(|n| *n > 0)
                && b.max_file_bytes > 0
                && b.max_total_file_bytes > 0
                && b.max_elapsed_millis > 0,
            "All observation bounds must be explicit and nonzero"
        );
        Ok(observation::Bounds {
            max_stat_bytes: b.max_stat_bytes,
            max_maps_bytes: b.max_maps_bytes,
            max_map_entries: b.max_map_entries,
            max_path_bytes: b.max_path_bytes,
            max_unique_files: b.max_unique_files,
            max_file_bytes: b.max_file_bytes,
            max_total_file_bytes: b.max_total_file_bytes,
            max_elapsed: Duration::from_millis(b.max_elapsed_millis),
            allowed_owner_uids: policy.allowed_owner_uids,
        })
    }
}

/// Existing V1 JSON has no schema selector. Both variants reject unknown fields;
/// V2 requires config_schema and cannot be accepted as a legacy configuration.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum RuntimeCandidateInput {
    V1(DevelopmentCandidateInput),
    V2(DevelopmentCandidateV2Input),
}

#[derive(Clone, Debug)]
pub struct VerifiedCandidateV2 {
    files: catalog::VerifiedMemberFiles,
    application: observation::Snapshot,
}
impl VerifiedCandidateV2 {
    pub fn manifest_sha512(&self) -> &str {
        self.files.candidate().sha512()
    }
    pub fn files(&self) -> &catalog::VerifiedMemberFiles {
        &self.files
    }
    pub fn application_snapshot(&self) -> &observation::Snapshot {
        &self.application
    }
}
#[derive(Clone, Debug)]
pub enum VerifiedRuntimeCandidate {
    V1(crate::runtime_candidate::VerifiedCandidate),
    V2(VerifiedCandidateV2),
}
impl VerifiedRuntimeCandidate {
    pub fn manifest_sha512(&self) -> &str {
        match self {
            Self::V1(v) => v.manifest_sha512(),
            Self::V2(v) => v.manifest_sha512(),
        }
    }
}

fn safe_helper_metadata(m: &Metadata, limit: u64, owners: &BTreeSet<u32>) -> Result<()> {
    ensure!(
        m.is_file() && m.len() > 0 && m.len() <= limit,
        "Bootstrap helper must be a nonempty bounded regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            owners.contains(&m.uid()),
            "Bootstrap helper owner is not permitted"
        );
        ensure!(
            m.mode() & 0o6022 == 0 && m.mode() & 0o111 != 0 && m.nlink() == 1,
            "Bootstrap helper permissions or link count are unsafe"
        );
    }
    #[cfg(not(unix))]
    anyhow::bail!("Bootstrap helper identity requires Unix");
    Ok(())
}
fn same_metadata(a: &Metadata, b: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        a.dev() == b.dev()
            && a.ino() == b.ino()
            && a.len() == b.len()
            && a.mode() == b.mode()
            && a.uid() == b.uid()
            && a.gid() == b.gid()
            && a.nlink() == b.nlink()
            && a.mtime() == b.mtime()
            && a.mtime_nsec() == b.mtime_nsec()
            && a.ctime() == b.ctime()
            && a.ctime_nsec() == b.ctime_nsec()
    }
    #[cfg(not(unix))]
    {
        let _ = (a, b);
        false
    }
}

/// The caller supplies the original independent root configuration. The target
/// catalog cannot choose this path or its trusted SHA256 digest.
fn bootstrap_file(
    path: &Path,
    expected_sha256: &str,
    limit: u64,
    owners: &BTreeSet<u32>,
) -> Result<(catalog::FileDigest, Metadata, File)> {
    ensure!(limit > 0, "Bootstrap helper byte limit must be explicit");
    ensure!(
        expected_sha256.len() == 64
            && expected_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Independent bootstrap SHA256 pin is malformed"
    );
    ensure!(
        path.is_absolute() && std::fs::canonicalize(path)?.as_os_str() == path.as_os_str(),
        "Bootstrap helper path must be canonical without aliases"
    );
    let before = std::fs::symlink_metadata(path)?;
    safe_helper_metadata(&before, limit, owners)?;
    let mut file = File::open(path)?;
    let opened = file.metadata()?;
    safe_helper_metadata(&opened, limit, owners)?;
    ensure!(
        same_metadata(&before, &opened),
        "Bootstrap helper changed during open"
    );
    let mut sha256 = Sha256::new();
    let mut sha512 = Sha512::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        // Read at most one byte past the declared bound; do not hash excess data.
        let remaining = limit.saturating_sub(total);
        let read_bound = remaining.saturating_add(1).min(buffer.len() as u64) as usize;
        let count = file.read(&mut buffer[..read_bound])?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .context("Bootstrap helper byte overflow")?;
        ensure!(total <= limit, "Bootstrap helper byte limit exceeded");
        sha256.update(&buffer[..count]);
        sha512.update(&buffer[..count]);
    }
    let digest = catalog::FileDigest {
        bytes: total,
        sha256: hex::encode(sha256.finalize()),
        sha512: hex::encode(sha512.finalize()),
    };
    ensure!(
        digest.sha256 == expected_sha256,
        "Bootstrap helper differs from independent SHA256 pin"
    );
    ensure!(
        total == before.len()
            && same_metadata(&before, &file.metadata()?)
            && same_metadata(&before, &std::fs::symlink_metadata(path)?),
        "Bootstrap helper changed while hashing"
    );
    Ok((digest, before, file))
}

/// Verify all catalog files without asserting that this process is the app.
/// The supervisor uses this API after obtaining opaque verified chain authority.
pub fn verify_catalog(
    input: &DevelopmentCandidateV2Input,
    authority: &VerifiedReleaseAuthority,
    root: &DevelopmentRootGenesis,
    live: &EmergencyVerifierConfig,
) -> Result<catalog::VerifiedMemberFiles> {
    let policy = input.file_policy()?;
    input.observation_bounds()?;
    ensure!(
        root.enabled,
        "V2 requires independent root bootstrap configuration"
    );
    ensure!(
        root.helper_path.as_os_str().len() <= input.bounds.max_path_bytes,
        "Bootstrap helper path exceeds candidate path bound"
    );
    let helper_limit = u64::try_from(root.max_helper_bytes)?.min(input.bounds.max_member_bytes);
    let (bootstrap, before, handle) = bootstrap_file(
        &root.helper_path,
        &root.helper_sha256,
        helper_limit,
        &policy.allowed_owner_uids,
    )?;
    let chain = authority.expected_candidate();
    let expected = catalog::ExpectedRelease {
        manifest_sha512: chain.manifest_sha512.clone(),
        chain_id: chain.chain_id.clone(),
        app_genesis_sha256: chain.app_genesis_sha256.clone(),
        migration_registry_sha256: chain.migration_registry_sha256.clone(),
        target: catalog::Target {
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            abi: if cfg!(target_env = "musl") {
                "musl"
            } else if cfg!(target_env = "gnu") {
                "gnu"
            } else {
                "unsupported"
            }
            .into(),
        },
        bootstrap_verifier: bootstrap,
    };
    let files = catalog::verify_member_files(
        &input.manifest_path,
        &input.mapping_path,
        &expected,
        &input.bounds.validation_bounds(),
        &policy,
    )?;
    verify_helper_role_bindings(&files, &root.helper_path, &before, live)?;
    ensure!(
        same_metadata(&before, &handle.metadata()?)
            && same_metadata(&before, &std::fs::symlink_metadata(&root.helper_path)?),
        "Bootstrap helper changed during catalog verification"
    );
    Ok(files)
}

fn verify_helper_role_bindings(
    files: &catalog::VerifiedMemberFiles,
    bootstrap_path: &Path,
    before: &Metadata,
    live: &EmergencyVerifierConfig,
) -> Result<()> {
    let bootstrap = files
        .role_file("genesis_bootstrap_verifier")
        .context("Bootstrap role missing")?;
    ensure!(
        bootstrap.path() == bootstrap_path,
        "Configured bootstrap helper path differs from candidate role"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            bootstrap.device() == before.dev() && bootstrap.inode() == before.ino(),
            "Bootstrap helper identity changed after independent verification"
        );
    }
    let verifier = files
        .role_file("control_verifier")
        .context("Live verifier role missing")?;
    ensure!(
        verifier.path() == live.helper_path && verifier.digest().sha256 == live.helper_sha256,
        "Configured live helper differs from candidate role"
    );
    ensure!(
        verifier.digest().bytes <= u64::try_from(live.max_helper_bytes)?,
        "Live helper exceeds configured byte bound"
    );
    Ok(())
}

pub fn verify_current_application(
    input: &DevelopmentCandidateV2Input,
    authority: &VerifiedReleaseAuthority,
    root: &DevelopmentRootGenesis,
    live: &EmergencyVerifierConfig,
) -> Result<VerifiedCandidateV2> {
    let files = verify_catalog(input, authority, root, live)?;
    let application = observation::observe_current_process(
        "consensus_stdio",
        &files,
        &input.observation_bounds()?,
    )?;
    Ok(VerifiedCandidateV2 { files, application })
}

pub fn verify_runtime_candidate(
    input: &RuntimeCandidateInput,
    authority: &VerifiedReleaseAuthority,
    root: &DevelopmentRootGenesis,
    live: &EmergencyVerifierConfig,
) -> Result<VerifiedRuntimeCandidate> {
    match input {
        RuntimeCandidateInput::V1(input) => Ok(VerifiedRuntimeCandidate::V1(
            crate::runtime_candidate::verify_current_executable(
                &input.manifest_path,
                authority.expected_candidate(),
                crate::runtime_candidate::Bounds {
                    max_manifest_bytes: input.max_manifest_bytes,
                    max_executable_bytes: input.max_executable_bytes,
                },
            )?,
        )),
        RuntimeCandidateInput::V2(input) => Ok(VerifiedRuntimeCandidate::V2(
            verify_current_application(input, authority, root, live)?,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn configuration() -> Value {
        json!({
            "config_schema":2,"manifest_path":"/fixture/manifest.json","mapping_path":"/fixture/mapping.json",
            "bounds":{"max_manifest_bytes":32768,"max_mapping_bytes":32768,"max_members":32,"max_roles":16,
                "max_runtime_profiles":16,"max_references":64,"max_id_bytes":64,"max_path_bytes":4096,
                "max_member_bytes":1048576,"max_total_member_bytes":4194304},
            "observation":{"max_stat_bytes":4096,"max_maps_bytes":32768,"max_map_entries":128,
                "max_path_bytes":4096,"max_unique_files":32,"max_file_bytes":1048576,
                "max_total_file_bytes":4194304,"max_elapsed_millis":5000},
            "allowed_owner_uids":[0,1000]
        })
    }
    #[test]
    fn runtime_v2_configuration_preserves_v1_dispatch_and_rejects_mixed_fields() {
        let legacy = br#"{"manifest_path":"/fixture/manifest.json","max_manifest_bytes":4096,"max_executable_bytes":1048576}"#;
        assert!(matches!(
            serde_json::from_slice::<RuntimeCandidateInput>(legacy).unwrap(),
            RuntimeCandidateInput::V1(_)
        ));
        assert!(matches!(
            serde_json::from_value::<RuntimeCandidateInput>(configuration()).unwrap(),
            RuntimeCandidateInput::V2(_)
        ));
        let mut value = configuration();
        value["max_manifest_bytes"] = json!(4096);
        assert!(serde_json::from_value::<RuntimeCandidateInput>(value).is_err());
        let mut value: Value = serde_json::from_slice(legacy).unwrap();
        value["config_schema"] = json!(2);
        assert!(serde_json::from_value::<RuntimeCandidateInput>(value).is_err());
    }
    #[test]
    fn runtime_v2_configuration_rejects_unknown_missing_null_duplicate_and_authority_fields() {
        for key in [
            "config_schema",
            "manifest_path",
            "mapping_path",
            "bounds",
            "observation",
            "allowed_owner_uids",
        ] {
            let mut value = configuration();
            value.as_object_mut().unwrap().remove(key);
            assert!(
                serde_json::from_value::<RuntimeCandidateInput>(value).is_err(),
                "missing {key}"
            );
            let mut value = configuration();
            value[key] = Value::Null;
            assert!(
                serde_json::from_value::<RuntimeCandidateInput>(value).is_err(),
                "null {key}"
            );
        }
        for key in [
            "manifest_sha512",
            "bootstrap_sha256",
            "expected_release",
            "unused",
        ] {
            let mut value = configuration();
            value[key] = json!("caller cannot select authority");
            assert!(serde_json::from_value::<RuntimeCandidateInput>(value).is_err());
        }
        let mut value = configuration();
        value["bounds"]["unknown"] = json!(1);
        assert!(serde_json::from_value::<RuntimeCandidateInput>(value).is_err());
        let mut value = configuration();
        value["observation"]["unknown"] = json!(1);
        assert!(serde_json::from_value::<RuntimeCandidateInput>(value).is_err());
        let mut bytes = serde_json::to_string(&configuration()).unwrap();
        bytes.insert_str(1, "\"config_schema\":2,");
        assert!(serde_json::from_str::<RuntimeCandidateInput>(&bytes).is_err());
    }
    #[test]
    fn runtime_v2_owner_and_observation_limits_remain_explicit() {
        for owners in [vec![], vec![1000, 0], vec![0, 0]] {
            let mut value = configuration();
            value["allowed_owner_uids"] = json!(owners);
            assert!(serde_json::from_value::<DevelopmentCandidateV2Input>(value)
                .unwrap()
                .observation_bounds()
                .is_err());
        }
        let value = configuration();
        for key in value["observation"].as_object().unwrap().keys() {
            let mut invalid = value.clone();
            invalid["observation"][key] = json!(0);
            assert!(
                serde_json::from_value::<DevelopmentCandidateV2Input>(invalid)
                    .unwrap()
                    .observation_bounds()
                    .is_err(),
                "{key}"
            );
        }
        let mut invalid = configuration();
        invalid["config_schema"] = json!(1);
        assert!(
            serde_json::from_value::<DevelopmentCandidateV2Input>(invalid)
                .unwrap()
                .observation_bounds()
                .is_err()
        );
        let valid = serde_json::from_value::<DevelopmentCandidateV2Input>(configuration())
            .unwrap()
            .observation_bounds()
            .unwrap();
        assert_eq!(valid.allowed_owner_uids, [0, 1000].into_iter().collect());
        assert_eq!(valid.max_elapsed, Duration::from_secs(5));
    }
    #[cfg(unix)]
    struct HelperFixture {
        _dir: tempfile::TempDir,
        path: PathBuf,
        bytes: Vec<u8>,
        owners: BTreeSet<u32>,
    }
    #[cfg(unix)]
    impl HelperFixture {
        fn new() -> Self {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().canonicalize().unwrap().join("bootstrap");
            let bytes = b"independently pinned bootstrap fixture".to_vec();
            std::fs::write(&path, &bytes).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500)).unwrap();
            let owners = [std::fs::metadata(&path).unwrap().uid()]
                .into_iter()
                .collect();
            Self {
                _dir: dir,
                path,
                bytes,
                owners,
            }
        }
        fn pin(&self) -> String {
            hex::encode(Sha256::digest(&self.bytes))
        }
    }
    #[cfg(unix)]
    #[test]
    fn runtime_v2_bootstrap_full_identity_derives_from_independent_sha256() {
        let f = HelperFixture::new();
        let (digest, metadata, _handle) =
            bootstrap_file(&f.path, &f.pin(), 1024, &f.owners).unwrap();
        assert_eq!(digest.bytes, f.bytes.len() as u64);
        assert_eq!(digest.sha512, hex::encode(Sha512::digest(&f.bytes)));
        assert_eq!(metadata.len(), digest.bytes);
        assert!(bootstrap_file(&f.path, &"aa".repeat(32), 1024, &f.owners)
            .unwrap_err()
            .to_string()
            .contains("independent SHA256 pin"));
        assert!(bootstrap_file(&f.path, &f.pin().to_uppercase(), 1024, &f.owners).is_err());
        assert!(bootstrap_file(&f.path, &f.pin(), 1, &f.owners).is_err());
        assert!(bootstrap_file(&f.path, &f.pin(), 1024, &BTreeSet::new()).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn runtime_v2_bootstrap_rejects_aliases_unsafe_modes_and_changed_bytes() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let f = HelperFixture::new();
        let alias = f.path.with_file_name("alias");
        symlink(&f.path, &alias).unwrap();
        assert!(bootstrap_file(&alias, &f.pin(), 1024, &f.owners).is_err());
        let hardlink = f.path.with_file_name("hardlink");
        std::fs::hard_link(&f.path, &hardlink).unwrap();
        assert!(bootstrap_file(&f.path, &f.pin(), 1024, &f.owners).is_err());
        std::fs::remove_file(&hardlink).unwrap();
        std::fs::set_permissions(&f.path, std::fs::Permissions::from_mode(0o522)).unwrap();
        assert!(bootstrap_file(&f.path, &f.pin(), 1024, &f.owners).is_err());
        std::fs::set_permissions(&f.path, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(&f.path, b"changed independently pinned bytes").unwrap();
        assert!(bootstrap_file(&f.path, &f.pin(), 1024, &f.owners)
            .unwrap_err()
            .to_string()
            .contains("independent SHA256 pin"));
    }
    #[cfg(unix)]
    #[test]
    fn runtime_v2_configured_helper_roles_match_verified_member_files() {
        let f = HelperFixture::new();
        let (digest, before, _handle) = bootstrap_file(&f.path, &f.pin(), 1024, &f.owners).unwrap();
        // Shared executable roles are legal in a synthetic catalog. This test
        // checks configured file binding only; it makes no role-execution claim.
        let manifest = catalog::ManifestV2 {
            schema: 2,
            chain_id: "development-helper-binding-1".into(),
            app_genesis_sha256: "11".repeat(32),
            migration_registry_sha256: "22".repeat(32),
            target: catalog::Target {
                os: "linux".into(),
                arch: "x86_64".into(),
                abi: "gnu".into(),
            },
            service_profile: catalog::DEVELOPMENT_NATIVE_PROFILE.into(),
            members: vec![catalog::Member {
                id: "helper".into(),
                kind: catalog::MemberKind::Executable,
                bytes: digest.bytes,
                sha256: digest.sha256.clone(),
                sha512: digest.sha512.clone(),
            }],
            roles: [
                "consensus_bridge",
                "consensus_engine",
                "consensus_stdio",
                "control_verifier",
                "genesis_bootstrap_verifier",
                "service_supervisor",
            ]
            .into_iter()
            .map(|role| catalog::Role {
                role: role.into(),
                member_id: "helper".into(),
                runtime_profile_id: "native".into(),
            })
            .collect(),
            runtime_profiles: vec![catalog::RuntimeProfile {
                id: "native".into(),
                member_ids: vec![],
                mapping_policy: catalog::OBSERVED_CODE_POLICY.into(),
                interpreted_member_ids: vec![],
            }],
        };
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let manifest_path = f.path.with_file_name("manifest.json");
        let mapping_path = f.path.with_file_name("mapping.json");
        std::fs::write(&manifest_path, &bytes).unwrap();
        std::fs::write(
            &mapping_path,
            serde_json::to_vec(&catalog::LocalMapping {
                schema: 1,
                members: vec![catalog::MemberPath {
                    id: "helper".into(),
                    path: f.path.clone(),
                }],
            })
            .unwrap(),
        )
        .unwrap();
        let expected = catalog::ExpectedRelease {
            manifest_sha512: hex::encode(Sha512::digest(&bytes)),
            chain_id: manifest.chain_id.clone(),
            app_genesis_sha256: manifest.app_genesis_sha256.clone(),
            migration_registry_sha256: manifest.migration_registry_sha256.clone(),
            target: manifest.target.clone(),
            bootstrap_verifier: digest,
        };
        let config: DevelopmentCandidateV2Input = serde_json::from_value(configuration()).unwrap();
        let files = catalog::verify_member_files(
            &manifest_path,
            &mapping_path,
            &expected,
            &config.bounds.validation_bounds(),
            &catalog::FilePolicy {
                allowed_owner_uids: f.owners.clone(),
            },
        )
        .unwrap();
        let live = EmergencyVerifierConfig {
            helper_path: f.path.clone(),
            helper_scratch_path: f.path.parent().unwrap().into(),
            helper_execution: None,
            helper_sha256: f.pin(),
            max_helper_bytes: 1024,
            max_request_bytes: 1024,
            timeout_ms: 1000,
        };
        verify_helper_role_bindings(&files, &f.path, &before, &live).unwrap();
        let mut wrong = live.clone();
        wrong.helper_path = f.path.with_file_name("other");
        assert!(verify_helper_role_bindings(&files, &f.path, &before, &wrong).is_err());
        let mut wrong = live.clone();
        wrong.helper_sha256 = "aa".repeat(32);
        assert!(verify_helper_role_bindings(&files, &f.path, &before, &wrong).is_err());
        let mut wrong = live.clone();
        wrong.max_helper_bytes = 1;
        assert!(verify_helper_role_bindings(&files, &f.path, &before, &wrong).is_err());
        assert!(verify_helper_role_bindings(
            &files,
            &f.path.with_file_name("other"),
            &before,
            &live
        )
        .is_err());
        let other = HelperFixture::new();
        assert!(verify_helper_role_bindings(
            &files,
            &f.path,
            &std::fs::metadata(&other.path).unwrap(),
            &live
        )
        .is_err());
    }
}
