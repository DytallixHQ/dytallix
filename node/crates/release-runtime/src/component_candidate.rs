//! Development-only candidate catalog and file checks. No process is launched.
//! A result does not prove role execution, mapped code, library closure or G35.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, Metadata};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const DEVELOPMENT_PROFILE: &str = "development-linux-python-service-v2";
pub const DEVELOPMENT_HTTP_PROFILE: &str = "development-linux-python-service-http-v2";
pub const DEVELOPMENT_NATIVE_PROFILE: &str = "development-linux-native-service-v2";
pub const DEVELOPMENT_NATIVE_HTTP_PROFILE: &str = "development-linux-native-service-http-v2";
pub const OBSERVED_CODE_POLICY: &str = "linux-observed-code-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub os: String,
    pub arch: String,
    pub abi: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberKind {
    Executable,
    SharedLibrary,
    Script,
    InterpretedModule,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub id: String,
    pub kind: MemberKind,
    pub bytes: u64,
    pub sha256: String,
    pub sha512: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    pub role: String,
    pub member_id: String,
    pub runtime_profile_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeProfile {
    pub id: String,
    pub member_ids: Vec<String>,
    pub mapping_policy: String,
    pub interpreted_member_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestV2 {
    pub schema: u16,
    pub chain_id: String,
    pub app_genesis_sha256: String,
    pub target: Target,
    pub migration_registry_sha256: String,
    pub service_profile: String,
    pub members: Vec<Member>,
    pub roles: Vec<Role>,
    pub runtime_profiles: Vec<RuntimeProfile>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDigest {
    pub bytes: u64,
    pub sha256: String,
    pub sha512: String,
}

/// Obtain release identity from verified genesis or committed handover state.
/// Obtain bootstrap identity from independent immutable bootstrap trust input.
/// This data is never decoded from the candidate or its local path mapping.
#[derive(Clone, Debug)]
pub struct ExpectedRelease {
    pub manifest_sha512: String,
    pub chain_id: String,
    pub app_genesis_sha256: String,
    pub migration_registry_sha256: String,
    pub target: Target,
    pub bootstrap_verifier: FileDigest,
}

#[derive(Clone, Debug)]
pub struct ValidationBounds {
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
impl ValidationBounds {
    fn validate(&self) -> Result<()> {
        ensure!(
            [
                self.max_manifest_bytes,
                self.max_mapping_bytes,
                self.max_members,
                self.max_roles,
                self.max_runtime_profiles,
                self.max_references,
                self.max_id_bytes,
                self.max_path_bytes
            ]
            .iter()
            .all(|v| *v > 0)
                && self.max_member_bytes > 0
                && self.max_total_member_bytes > 0,
            "All candidate bounds must be explicit and nonzero"
        );
        Ok(())
    }
}

/// UID policy is explicit local trusted input, not a manifest-selected exception.
#[derive(Clone, Debug)]
pub struct FilePolicy {
    pub allowed_owner_uids: BTreeSet<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberPath {
    pub id: String,
    pub path: PathBuf,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalMapping {
    pub schema: u16,
    pub members: Vec<MemberPath>,
}

/// Structurally valid catalog with an externally matched release digest.
#[derive(Clone, Debug)]
pub struct ValidatedManifest {
    manifest: ManifestV2,
    sha512: String,
}
impl ValidatedManifest {
    pub fn manifest(&self) -> &ManifestV2 {
        &self.manifest
    }
    pub fn sha512(&self) -> &str {
        &self.sha512
    }
    pub fn member_for_role(&self, role: &str) -> Option<&Member> {
        let r = self.manifest.roles.iter().find(|r| r.role == role)?;
        self.manifest.members.iter().find(|m| m.id == r.member_id)
    }
}

#[derive(Clone, Debug)]
pub struct VerifiedFile {
    id: String,
    path: PathBuf,
    digest: FileDigest,
    device: u64,
    inode: u64,
}
impl VerifiedFile {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn digest(&self) -> &FileDigest {
        &self.digest
    }
    pub fn device(&self) -> u64 {
        self.device
    }
    pub fn inode(&self) -> u64 {
        self.inode
    }
}

/// Immutable verification snapshot only. It grants no launch authority.
#[derive(Clone, Debug)]
pub struct VerifiedMemberFiles {
    candidate: ValidatedManifest,
    files: Vec<VerifiedFile>,
}
impl VerifiedMemberFiles {
    pub fn candidate(&self) -> &ValidatedManifest {
        &self.candidate
    }
    pub fn files(&self) -> &[VerifiedFile] {
        &self.files
    }
    pub fn role_file(&self, role: &str) -> Option<&VerifiedFile> {
        let member = self.candidate.member_for_role(role)?;
        self.files.iter().find(|f| f.id == member.id)
    }
    pub fn scope(&self) -> &'static str {
        "MEMBER_FILES_VERIFIED"
    }
}

fn digest(value: &str, bytes: usize) -> Result<()> {
    ensure!(
        value.len() == bytes * 2
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Noncanonical candidate digest"
    );
    Ok(())
}
fn identifier(value: &str, limit: usize) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= limit
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)),
        "Invalid candidate identifier"
    );
    Ok(())
}
fn sorted<'a>(values: impl Iterator<Item = &'a str>) -> Result<()> {
    let mut previous = None;
    for value in values {
        ensure!(
            previous.is_none_or(|p| p < value),
            "Candidate arrays must be strictly sorted and unique"
        );
        previous = Some(value);
    }
    Ok(())
}
fn canonical<T: for<'de> Deserialize<'de> + Serialize>(bytes: &[u8], bound: usize) -> Result<T> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= bound,
        "Candidate JSON byte bound"
    );
    let value: T = serde_json::from_slice(bytes).context("Decode candidate JSON")?;
    ensure!(
        serde_json::to_vec(&value)? == bytes,
        "Noncanonical candidate JSON"
    );
    Ok(value)
}
fn validate_target(target: &Target) -> Result<()> {
    ensure!(
        target.os == "linux"
            && matches!(target.arch.as_str(), "x86_64" | "aarch64")
            && matches!(target.abi.as_str(), "gnu" | "musl"),
        "Unsupported development candidate target"
    );
    Ok(())
}
fn member_digest(member: &Member) -> FileDigest {
    FileDigest {
        bytes: member.bytes,
        sha256: member.sha256.clone(),
        sha512: member.sha512.clone(),
    }
}

pub fn validate_manifest_bytes(
    bytes: &[u8],
    expected: &ExpectedRelease,
    bounds: &ValidationBounds,
) -> Result<ValidatedManifest> {
    bounds.validate()?;
    ensure!(
        bytes.len() <= bounds.max_manifest_bytes,
        "Candidate manifest byte bound"
    );
    digest(&expected.manifest_sha512, 64)?;
    digest(&expected.app_genesis_sha256, 32)?;
    digest(&expected.migration_registry_sha256, 32)?;
    digest(&expected.bootstrap_verifier.sha256, 32)?;
    digest(&expected.bootstrap_verifier.sha512, 64)?;
    ensure!(
        expected.bootstrap_verifier.bytes > 0,
        "Bootstrap byte count required"
    );
    identifier(&expected.chain_id, 128)?;
    validate_target(&expected.target)?;
    let sha512 = hex::encode(Sha512::digest(bytes));
    ensure!(
        sha512 == expected.manifest_sha512,
        "Candidate manifest authority digest mismatch"
    );
    let manifest: ManifestV2 = canonical(bytes, bounds.max_manifest_bytes)?;
    ensure!(manifest.schema == 2, "Unsupported candidate schema");
    ensure!(
        manifest.chain_id == expected.chain_id
            && manifest.app_genesis_sha256 == expected.app_genesis_sha256
            && manifest.migration_registry_sha256 == expected.migration_registry_sha256
            && manifest.target == expected.target,
        "Candidate chain/genesis/registry/target binding mismatch"
    );
    let (http, interpreted_supervisor) = match manifest.service_profile.as_str() {
        DEVELOPMENT_PROFILE => (false, true),
        DEVELOPMENT_HTTP_PROFILE => (true, true),
        DEVELOPMENT_NATIVE_PROFILE => (false, false),
        DEVELOPMENT_NATIVE_HTTP_PROFILE => (true, false),
        _ => anyhow::bail!("Unsupported development service profile; production is not qualified"),
    };
    ensure!(
        !manifest.members.is_empty() && manifest.members.len() <= bounds.max_members,
        "Candidate member count bound"
    );
    ensure!(
        !manifest.roles.is_empty() && manifest.roles.len() <= bounds.max_roles,
        "Candidate role count bound"
    );
    ensure!(
        !manifest.runtime_profiles.is_empty()
            && manifest.runtime_profiles.len() <= bounds.max_runtime_profiles,
        "Candidate runtime profile count bound"
    );
    sorted(manifest.members.iter().map(|m| m.id.as_str()))?;
    sorted(manifest.roles.iter().map(|r| r.role.as_str()))?;
    sorted(manifest.runtime_profiles.iter().map(|r| r.id.as_str()))?;
    let mut members = BTreeMap::new();
    let mut contents = BTreeSet::new();
    let mut total = 0u64;
    for member in &manifest.members {
        identifier(&member.id, bounds.max_id_bytes)?;
        digest(&member.sha256, 32)?;
        digest(&member.sha512, 64)?;
        ensure!(
            member.bytes > 0 && member.bytes <= bounds.max_member_bytes,
            "Candidate per-member byte bound"
        );
        total = total
            .checked_add(member.bytes)
            .context("Candidate total byte overflow")?;
        ensure!(
            total <= bounds.max_total_member_bytes,
            "Candidate total member byte bound"
        );
        ensure!(
            contents.insert((member.bytes, &member.sha256, &member.sha512)),
            "Duplicate candidate content under different IDs"
        );
        members.insert(member.id.as_str(), member);
    }
    let mut used_members = BTreeSet::new();
    let mut profiles = BTreeMap::new();
    let mut references = 0usize;
    for profile in &manifest.runtime_profiles {
        identifier(&profile.id, bounds.max_id_bytes)?;
        ensure!(
            profile.mapping_policy == OBSERVED_CODE_POLICY,
            "Unknown mapping policy"
        );
        sorted(profile.member_ids.iter().map(String::as_str))?;
        sorted(profile.interpreted_member_ids.iter().map(String::as_str))?;
        for (ids, interpreted) in [
            (&profile.member_ids, false),
            (&profile.interpreted_member_ids, true),
        ] {
            references = references
                .checked_add(ids.len())
                .context("Candidate reference overflow")?;
            ensure!(
                references <= bounds.max_references,
                "Candidate reference count bound"
            );
            for id in ids {
                identifier(id, bounds.max_id_bytes)?;
                let member = members
                    .get(id.as_str())
                    .context("Runtime member reference missing")?;
                ensure!(
                    if interpreted {
                        matches!(
                            member.kind,
                            MemberKind::Script | MemberKind::InterpretedModule
                        )
                    } else {
                        member.kind == MemberKind::SharedLibrary
                    },
                    "Runtime member kind mismatch"
                );
                used_members.insert(id.as_str());
            }
        }
        profiles.insert(profile.id.as_str(), profile);
    }
    let mut required: BTreeSet<&str> = [
        "consensus_stdio",
        "consensus_bridge",
        "consensus_engine",
        "genesis_bootstrap_verifier",
        "control_verifier",
        "service_supervisor",
    ]
    .into_iter()
    .collect();
    if interpreted_supervisor {
        required.insert("supervisor_interpreter");
    }
    if http {
        required.insert("http_adapter");
    }
    ensure!(
        manifest
            .roles
            .iter()
            .map(|r| r.role.as_str())
            .collect::<BTreeSet<_>>()
            == required,
        "Missing or unknown required service roles"
    );
    let mut used_profiles = BTreeSet::new();
    for role in &manifest.roles {
        identifier(&role.role, bounds.max_id_bytes)?;
        identifier(&role.member_id, bounds.max_id_bytes)?;
        identifier(&role.runtime_profile_id, bounds.max_id_bytes)?;
        let member = members
            .get(role.member_id.as_str())
            .context("Role member reference missing")?;
        let profile = profiles
            .get(role.runtime_profile_id.as_str())
            .context("Role runtime profile missing")?;
        let supervisor = interpreted_supervisor && role.role == "service_supervisor";
        ensure!(
            member.kind
                == if supervisor {
                    MemberKind::Script
                } else {
                    MemberKind::Executable
                },
            "Role member kind mismatch"
        );
        if supervisor {
            ensure!(
                profile.interpreted_member_ids.contains(&role.member_id),
                "Supervisor script must appear in interpreted profile"
            );
        } else if role.role != "supervisor_interpreter" {
            ensure!(
                profile.interpreted_member_ids.is_empty(),
                "Native role cannot claim interpreted members"
            );
        }
        if role.role == "genesis_bootstrap_verifier" {
            ensure!(
                member_digest(member) == expected.bootstrap_verifier,
                "Candidate bootstrap differs from independent trusted pin"
            );
        }
        used_members.insert(role.member_id.as_str());
        used_profiles.insert(role.runtime_profile_id.as_str());
    }
    if interpreted_supervisor {
        let supervisor = manifest
            .roles
            .iter()
            .find(|r| r.role == "service_supervisor")
            .unwrap();
        let interpreter = manifest
            .roles
            .iter()
            .find(|r| r.role == "supervisor_interpreter")
            .unwrap();
        ensure!(
            supervisor.runtime_profile_id == interpreter.runtime_profile_id,
            "Supervisor and interpreter must share one runtime profile"
        );
    }
    ensure!(
        used_profiles.len() == profiles.len(),
        "Unused runtime profile"
    );
    ensure!(
        used_members.len() == members.len(),
        "Unused candidate member"
    );
    Ok(ValidatedManifest { manifest, sha512 })
}

pub fn validate_mapping_bytes(
    bytes: &[u8],
    candidate: &ValidatedManifest,
    bounds: &ValidationBounds,
) -> Result<LocalMapping> {
    bounds.validate()?;
    let mapping: LocalMapping = canonical(bytes, bounds.max_mapping_bytes)?;
    ensure!(mapping.schema == 1, "Unsupported local mapping schema");
    ensure!(
        mapping.members.len() == candidate.manifest.members.len()
            && mapping.members.len() <= bounds.max_members,
        "Local member mapping count mismatch"
    );
    sorted(mapping.members.iter().map(|m| m.id.as_str()))?;
    let mut paths = BTreeSet::new();
    for (row, member) in mapping.members.iter().zip(&candidate.manifest.members) {
        ensure!(row.id == member.id, "Local member mapping ID mismatch");
        check_path_syntax(&row.path, bounds.max_path_bytes)?;
        ensure!(
            paths.insert(&row.path),
            "Local path mapped to more than one member"
        );
    }
    Ok(mapping)
}

fn check_path_syntax(path: &Path, limit: usize) -> Result<()> {
    ensure!(
        path.is_absolute() && path.as_os_str().len() <= limit,
        "Candidate path must be absolute and bounded"
    );
    Ok(())
}
fn exact_path(path: &Path, limit: usize) -> Result<()> {
    check_path_syntax(path, limit)?;
    ensure!(
        std::fs::canonicalize(path)?.as_os_str() == path.as_os_str(),
        "Candidate path has symlink or lexical alias"
    );
    ensure!(
        std::fs::symlink_metadata(path)?.is_file(),
        "Candidate input is not a regular file"
    );
    Ok(())
}
fn metadata_ok(
    metadata: &Metadata,
    limit: u64,
    policy: &FilePolicy,
    executable: bool,
) -> Result<()> {
    ensure!(
        metadata.is_file() && metadata.len() > 0 && metadata.len() <= limit,
        "Candidate input byte bound or regular file failure"
    );
    ensure!(
        !policy.allowed_owner_uids.is_empty(),
        "Explicit trusted owner policy required"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            policy.allowed_owner_uids.contains(&metadata.uid()),
            "Candidate owner not allowed"
        );
        ensure!(
            metadata.mode() & 0o6022 == 0 && metadata.nlink() == 1,
            "Unsafe candidate permissions or hard-link alias"
        );
        ensure!(
            !executable || metadata.mode() & 0o111 != 0,
            "Executable permission required"
        );
    }
    #[cfg(not(unix))]
    anyhow::bail!("Candidate permission validation requires Unix");
    #[allow(unreachable_code)]
    Ok(())
}
fn identity(a: &Metadata, b: &Metadata) -> bool {
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
fn open_checked(
    path: &Path,
    limit: u64,
    bounds: &ValidationBounds,
    policy: &FilePolicy,
    executable: bool,
) -> Result<(File, Metadata)> {
    exact_path(path, bounds.max_path_bytes)?;
    let before = std::fs::symlink_metadata(path)?;
    metadata_ok(&before, limit, policy, executable)?;
    let file = File::open(path)?;
    let opened = file.metadata()?;
    ensure!(
        identity(&before, &opened),
        "Candidate identity changed before opening"
    );
    metadata_ok(&opened, limit, policy, executable)?;
    Ok((file, opened))
}
fn finish_checked(
    path: &Path,
    file: &File,
    opened: &Metadata,
    limit: u64,
    bounds: &ValidationBounds,
    policy: &FilePolicy,
    executable: bool,
) -> Result<()> {
    let after = file.metadata()?;
    metadata_ok(&after, limit, policy, executable)?;
    ensure!(
        identity(opened, &after),
        "Candidate file changed during read"
    );
    exact_path(path, bounds.max_path_bytes)?;
    let named = std::fs::symlink_metadata(path)?;
    metadata_ok(&named, limit, policy, executable)?;
    ensure!(
        identity(opened, &named),
        "Candidate path changed during read"
    );
    Ok(())
}
fn read_small(
    path: &Path,
    limit: usize,
    bounds: &ValidationBounds,
    policy: &FilePolicy,
) -> Result<Vec<u8>> {
    let limit64 = u64::try_from(limit)?;
    let (mut file, opened) = open_checked(path, limit64, bounds, policy, false)?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(
            limit64
                .checked_add(1)
                .context("Candidate byte bound overflow")?,
        )
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= limit && u64::try_from(bytes.len())? == opened.len(),
        "Candidate input length changed"
    );
    finish_checked(path, &file, &opened, limit64, bounds, policy, false)?;
    Ok(bytes)
}

/// Verify catalog and files only. Target expectation is explicit so a release
/// engineer can inspect Linux members on another host. No running-host claim.
pub fn verify_member_files(
    manifest_path: &Path,
    mapping_path: &Path,
    expected: &ExpectedRelease,
    bounds: &ValidationBounds,
    policy: &FilePolicy,
) -> Result<VerifiedMemberFiles> {
    bounds.validate()?;
    let manifest_bytes = read_small(manifest_path, bounds.max_manifest_bytes, bounds, policy)?;
    let candidate = validate_manifest_bytes(&manifest_bytes, expected, bounds)?;
    let mapping_bytes = read_small(mapping_path, bounds.max_mapping_bytes, bounds, policy)?;
    let mapping = validate_mapping_bytes(&mapping_bytes, &candidate, bounds)?;
    let mut files = Vec::with_capacity(candidate.manifest.members.len());
    for (member, row) in candidate.manifest.members.iter().zip(&mapping.members) {
        let executable = member.kind == MemberKind::Executable;
        let (mut file, opened) = open_checked(&row.path, member.bytes, bounds, policy, executable)?;
        ensure!(
            opened.len() == member.bytes,
            "Candidate member length mismatch"
        );
        let mut sha256 = Sha256::new();
        let mut sha512 = Sha512::new();
        let mut total = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            total = total
                .checked_add(u64::try_from(count)?)
                .context("Candidate read size overflow")?;
            ensure!(total <= member.bytes, "Candidate member grew beyond bound");
            sha256.update(&buffer[..count]);
            sha512.update(&buffer[..count]);
        }
        finish_checked(
            &row.path,
            &file,
            &opened,
            member.bytes,
            bounds,
            policy,
            executable,
        )?;
        ensure!(total == member.bytes, "Candidate member truncated");
        let actual = FileDigest {
            bytes: total,
            sha256: hex::encode(sha256.finalize()),
            sha512: hex::encode(sha512.finalize()),
        };
        ensure!(
            actual == member_digest(member),
            "Candidate member hashes differ"
        );
        #[cfg(unix)]
        let (device, inode) = {
            use std::os::unix::fs::MetadataExt;
            (opened.dev(), opened.ino())
        };
        #[cfg(not(unix))]
        let (device, inode) = (0, 0);
        files.push(VerifiedFile {
            id: member.id.clone(),
            path: row.path.clone(),
            digest: actual,
            device,
            inode,
        });
    }
    Ok(VerifiedMemberFiles { candidate, files })
}

#[cfg(test)]
#[path = "component_candidate_tests.rs"]
mod tests;
