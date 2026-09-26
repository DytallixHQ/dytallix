//! File-level binding between a trusted release manifest and this executable.
//!
//! The caller must obtain the expected manifest digest from verified genesis or
//! accepted upgrade state. This module does not authorize a release, inspect
//! loaded libraries, or prove production acceptance.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};
use std::fs::{File, Metadata};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub os: String,
    pub arch: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Executable {
    pub bytes: u64,
    pub sha256: String,
    pub sha512: String,
}

/// Canonical bytes are exactly `serde_json::to_vec(&manifest)`, with no newline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestV1 {
    pub schema: u16,
    pub chain_id: String,
    pub app_genesis_sha256: String,
    pub target: Target,
    pub consensus_stdio: Executable,
    pub migration_registry_sha256: String,
}

/// Trusted expectations. A CLI-supplied digest alone is not an authority source.
#[derive(Clone, Debug)]
pub struct ExpectedCandidate {
    pub manifest_sha512: String,
    pub chain_id: String,
    pub app_genesis_sha256: String,
    pub migration_registry_sha256: String,
}

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub max_manifest_bytes: usize,
    pub max_executable_bytes: u64,
}

/// A verified file-level binding. Fields and construction remain private.
#[derive(Clone, Debug)]
pub struct VerifiedCandidate {
    manifest: ManifestV1,
    manifest_sha512: String,
    executable_path: PathBuf,
    kernel_image_identity_checked: bool,
}
impl VerifiedCandidate {
    pub fn manifest(&self) -> &ManifestV1 {
        &self.manifest
    }
    pub fn manifest_sha512(&self) -> &str {
        &self.manifest_sha512
    }
    pub fn executable_path(&self) -> &Path {
        &self.executable_path
    }
    pub fn chain_id(&self) -> &str {
        &self.manifest.chain_id
    }
    pub fn app_genesis_sha256(&self) -> &str {
        &self.manifest.app_genesis_sha256
    }
    pub fn migration_registry_sha256(&self) -> &str {
        &self.manifest.migration_registry_sha256
    }
    pub fn executable(&self) -> &Executable {
        &self.manifest.consensus_stdio
    }
    /// Linux checks `/proc/self/exe`. Other Unix hosts only check path identity.
    pub fn kernel_image_identity_checked(&self) -> bool {
        self.kernel_image_identity_checked
    }
}

fn digest(value: &str, length: usize) -> Result<()> {
    ensure!(
        value.len() == length
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Candidate digest must use exact lowercase hexadecimal"
    );
    Ok(())
}

fn safe_metadata(metadata: &Metadata, limit: u64) -> Result<()> {
    ensure!(
        metadata.is_file() && metadata.len() > 0 && metadata.len() <= limit,
        "Candidate input must be a nonempty bounded regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            metadata.mode() & 0o6022 == 0,
            "Candidate input has unsafe permissions"
        );
        ensure!(
            metadata.nlink() == 1,
            "Candidate input must not have hard-link aliases"
        );
    }
    #[cfg(not(unix))]
    anyhow::bail!("Candidate file permission checks require Unix");
    #[allow(unreachable_code)]
    Ok(())
}

fn same_file(before: &Metadata, after: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        before.dev() == after.dev()
            && before.ino() == after.ino()
            && before.len() == after.len()
            && before.mode() == after.mode()
            && before.uid() == after.uid()
            && before.gid() == after.gid()
            && before.nlink() == after.nlink()
            && before.mtime() == after.mtime()
            && before.mtime_nsec() == after.mtime_nsec()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec()
    }
    #[cfg(not(unix))]
    {
        let _ = (before, after);
        false
    }
}

fn exact_path(path: &Path) -> Result<()> {
    ensure!(path.is_absolute(), "Candidate input path must be absolute");
    let resolved = std::fs::canonicalize(path).context("Resolve candidate input path")?;
    ensure!(
        resolved.as_os_str() == path.as_os_str(),
        "Candidate input path must be canonical without symlink or lexical aliases"
    );
    ensure!(
        !std::fs::symlink_metadata(path)?.file_type().is_symlink(),
        "Candidate input must not be a symlink"
    );
    Ok(())
}

fn open_checked(path: &Path, limit: u64) -> Result<(File, Metadata)> {
    exact_path(path)?;
    let before = std::fs::symlink_metadata(path)?;
    safe_metadata(&before, limit)?;
    let file = File::open(path).context("Open candidate input")?;
    let opened = file.metadata()?;
    safe_metadata(&opened, limit)?;
    ensure!(
        same_file(&before, &opened),
        "Candidate file changed before opening"
    );
    Ok((file, opened))
}

fn finish_checked(path: &Path, file: &File, opened: &Metadata, limit: u64) -> Result<()> {
    let after = file.metadata()?;
    safe_metadata(&after, limit)?;
    ensure!(
        same_file(opened, &after),
        "Candidate file changed while reading"
    );
    exact_path(path)?;
    let named = std::fs::symlink_metadata(path)?;
    safe_metadata(&named, limit)?;
    ensure!(
        same_file(opened, &named),
        "Candidate path changed while reading"
    );
    Ok(())
}

fn verify_impl(
    manifest_path: &Path,
    executable_path: &Path,
    expected: &ExpectedCandidate,
    bounds: Bounds,
    running_process: bool,
) -> Result<VerifiedCandidate> {
    ensure!(
        bounds.max_manifest_bytes > 0 && bounds.max_executable_bytes > 0,
        "Candidate verification requires explicit nonzero bounds"
    );
    digest(&expected.manifest_sha512, 128)?;
    digest(&expected.app_genesis_sha256, 64)?;
    digest(&expected.migration_registry_sha256, 64)?;
    ensure!(
        !expected.chain_id.is_empty()
            && expected.chain_id.len() <= 128
            && expected
                .chain_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)),
        "Candidate chain ID is invalid"
    );
    let limit = u64::try_from(bounds.max_manifest_bytes)?;
    let (mut file, opened) = open_checked(manifest_path, limit)?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(
            limit
                .checked_add(1)
                .context("Candidate manifest bound overflow")?,
        )
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= bounds.max_manifest_bytes && u64::try_from(bytes.len())? == opened.len(),
        "Candidate manifest length changed or exceeded bound"
    );
    finish_checked(manifest_path, &file, &opened, limit)?;
    let manifest_sha512 = hex::encode(Sha512::digest(&bytes));
    ensure!(
        manifest_sha512 == expected.manifest_sha512,
        "Candidate manifest digest mismatch"
    );
    let manifest: ManifestV1 =
        serde_json::from_slice(&bytes).context("Decode candidate manifest")?;
    ensure!(
        serde_json::to_vec(&manifest)? == bytes,
        "Candidate manifest is not canonical JSON"
    );
    ensure!(
        manifest.schema == 1,
        "Unsupported candidate manifest schema"
    );
    digest(&manifest.app_genesis_sha256, 64)?;
    digest(&manifest.migration_registry_sha256, 64)?;
    digest(&manifest.consensus_stdio.sha256, 64)?;
    digest(&manifest.consensus_stdio.sha512, 128)?;
    ensure!(
        manifest.chain_id == expected.chain_id,
        "Candidate chain ID mismatch"
    );
    ensure!(
        manifest.app_genesis_sha256 == expected.app_genesis_sha256,
        "Candidate genesis mismatch"
    );
    ensure!(
        manifest.migration_registry_sha256 == expected.migration_registry_sha256,
        "Candidate migration registry mismatch"
    );
    ensure!(
        manifest.target.os == std::env::consts::OS
            && manifest.target.arch == std::env::consts::ARCH,
        "Candidate target mismatch"
    );
    ensure!(
        manifest.consensus_stdio.bytes > 0
            && manifest.consensus_stdio.bytes <= bounds.max_executable_bytes,
        "Candidate executable length exceeds bound"
    );
    let (mut executable, identity) = open_checked(executable_path, bounds.max_executable_bytes)?;
    ensure!(
        identity.len() == manifest.consensus_stdio.bytes,
        "Candidate executable length mismatch"
    );
    let mut kernel_image_identity_checked = false;
    #[cfg(target_os = "linux")]
    if running_process {
        let running = File::open("/proc/self/exe").context("Open kernel running executable")?;
        ensure!(
            same_file(&identity, &running.metadata()?),
            "Candidate file does not match the running executable"
        );
        kernel_image_identity_checked = true;
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = running_process;
    }
    let mut sha256 = Sha256::new();
    let mut sha512 = Sha512::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = executable.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(count)?)
            .context("Candidate executable size overflow")?;
        ensure!(
            total <= manifest.consensus_stdio.bytes && total <= bounds.max_executable_bytes,
            "Candidate executable grew beyond bound"
        );
        sha256.update(&buffer[..count]);
        sha512.update(&buffer[..count]);
    }
    finish_checked(
        executable_path,
        &executable,
        &identity,
        bounds.max_executable_bytes,
    )?;
    ensure!(
        total == manifest.consensus_stdio.bytes,
        "Candidate executable was truncated"
    );
    ensure!(
        hex::encode(sha256.finalize()) == manifest.consensus_stdio.sha256,
        "Candidate executable SHA-256 mismatch"
    );
    ensure!(
        hex::encode(sha512.finalize()) == manifest.consensus_stdio.sha512,
        "Candidate executable SHA-512 mismatch"
    );
    Ok(VerifiedCandidate {
        manifest,
        manifest_sha512,
        executable_path: executable_path.into(),
        kernel_image_identity_checked,
    })
}

/// Verify this process's executable against an externally trusted manifest hash.
/// The caller cannot select an alternative executable. The result is a snapshot;
/// it does not prevent later file changes or verify dynamically loaded libraries.
pub fn verify_current_executable(
    manifest_path: &Path,
    expected: &ExpectedCandidate,
    bounds: Bounds,
) -> Result<VerifiedCandidate> {
    let path = std::env::current_exe().context("Locate running candidate executable")?;
    verify_impl(manifest_path, &path, expected, bounds, true)
}

#[cfg(test)]
#[path = "runtime_candidate_tests.rs"]
mod tests;
