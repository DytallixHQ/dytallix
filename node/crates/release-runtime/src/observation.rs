//! Bounded development observation of a caller-owned child or this process.
//! File/process/mapping evidence is a snapshot, not continuous enforcement.
pub mod static_helper;
pub mod controlled_pause;
mod mapping_diagnostic;
use anyhow::{ensure, Context, Result};
use crate::component_candidate::{FileDigest, MemberKind, VerifiedMemberFiles};
use sha2::{Digest, Sha256, Sha512};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(any(test, feature = "qualification-fixtures"))]
use std::ffi::OsString;
use std::fs::{File, Metadata, OpenOptions};
use std::io::Read;
#[cfg(any(test, feature = "qualification-fixtures"))]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Child;
#[cfg(any(test, feature = "qualification-fixtures"))]
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Bounds {
    pub max_stat_bytes: usize,
    pub max_maps_bytes: usize,
    pub max_map_entries: usize,
    pub max_path_bytes: usize,
    pub max_unique_files: usize,
    pub max_file_bytes: u64,
    pub max_total_file_bytes: u64,
    pub max_elapsed: Duration,
    pub allowed_owner_uids: BTreeSet<u32>,
}
impl Bounds {
    fn validate(&self) -> Result<()> {
        ensure!(
            [
                self.max_stat_bytes,
                self.max_maps_bytes,
                self.max_map_entries,
                self.max_path_bytes,
                self.max_unique_files
            ]
            .iter()
            .all(|v| *v > 0)
                && self.max_file_bytes > 0
                && self.max_total_file_bytes > 0
                && !self.max_elapsed.is_zero()
                && !self.allowed_owner_uids.is_empty(),
            "Observation bounds must be nonzero"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartIdentity {
    pub pid: u32,
    pub start_ticks: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MappingKind {
    File(PathBuf),
    Anonymous,
    Kernel(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mapping {
    pub start: u64,
    pub end: u64,
    pub permissions: String,
    pub offset: u64,
    pub device_major: u64,
    pub device_minor: u64,
    pub inode: u64,
    pub kind: MappingKind,
}
impl Mapping {
    pub fn executable(&self) -> bool {
        self.permissions.as_bytes()[2] == b'x'
    }
}

pub fn parse_stat(bytes: &[u8], expected_pid: u32, bounds: &Bounds) -> Result<StartIdentity> {
    bounds.validate()?;
    ensure!(
        !bytes.is_empty() && bytes.len() <= bounds.max_stat_bytes,
        "Stat byte bound"
    );
    let text = std::str::from_utf8(bytes)?;
    let open = text.find(" (").context("Stat command opening missing")?;
    let close = text.rfind(')').context("Stat command closing missing")?;
    ensure!(close > open, "Malformed stat command");
    let pid: u32 = text[..open].parse()?;
    ensure!(pid == expected_pid && pid > 0, "Stat PID mismatch");
    let suffix = text[close + 1..].trim();
    let fields: Vec<&str> = suffix.split_ascii_whitespace().collect();
    ensure!(
        fields.len() >= 20 && fields[0].len() == 1,
        "Stat fields missing"
    );
    ensure!(
        matches!(fields[0], "R" | "S" | "D" | "T" | "t" | "I"),
        "Child exited or unsupported task state"
    );
    let start_ticks: u64 = fields[19].parse()?;
    ensure!(start_ticks > 0, "Invalid process start ticks");
    Ok(StartIdentity { pid, start_ticks })
}
fn number_hex(s: &str) -> Result<u64> {
    ensure!(
        !s.is_empty() && s.len() <= 16 && s.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid mapping hex field"
    );
    Ok(u64::from_str_radix(s, 16)?)
}
fn token<'a>(line: &mut &'a str) -> Result<&'a str> {
    *line = line.trim_start_matches([' ', '\t']);
    let end = line.find([' ', '\t']).unwrap_or(line.len());
    ensure!(end > 0, "Missing map field");
    let value = &line[..end];
    *line = &line[end..];
    Ok(value)
}

pub fn parse_maps(bytes: &[u8], bounds: &Bounds) -> Result<Vec<Mapping>> {
    bounds.validate()?;
    ensure!(
        !bytes.is_empty() && bytes.len() <= bounds.max_maps_bytes,
        "Maps byte bound"
    );
    let text = std::str::from_utf8(bytes)?;
    ensure!(text.ends_with('\n'), "Incomplete maps snapshot");
    let mut output = Vec::new();
    let mut previous_end = 0;
    for raw in text.lines() {
        ensure!(
            !raw.is_empty() && output.len() < bounds.max_map_entries,
            "Empty map line or entry bound"
        );
        let mut line = raw;
        let range = token(&mut line)?;
        let (start, end) = range.split_once('-').context("Map address range missing")?;
        let start = number_hex(start)?;
        let end = number_hex(end)?;
        ensure!(
            start < end && start >= previous_end,
            "Unordered or overlapping map ranges"
        );
        previous_end = end;
        let permissions = token(&mut line)?.to_owned();
        let p = permissions.as_bytes();
        ensure!(
            p.len() == 4
                && matches!(p[0], b'r' | b'-')
                && matches!(p[1], b'w' | b'-')
                && matches!(p[2], b'x' | b'-')
                && matches!(p[3], b'p' | b's'),
            "Invalid map permissions"
        );
        let offset = number_hex(token(&mut line)?)?;
        let device = token(&mut line)?;
        let (major, minor) = device.split_once(':').context("Map device missing")?;
        let device_major = number_hex(major)?;
        let device_minor = number_hex(minor)?;
        let inode_text = token(&mut line)?;
        ensure!(
            inode_text.bytes().all(|b| b.is_ascii_digit()),
            "Invalid map inode"
        );
        let inode = inode_text.parse()?;
        let path = line.trim_start_matches([' ', '\t']);
        ensure!(
            path.len() <= bounds.max_path_bytes && !path.contains('\0'),
            "Map pathname bound"
        );
        ensure!(!path.ends_with(" (deleted)"), "Deleted mapping refused");
        // proc names are ambiguous for encoded newline characters; do not guess.
        ensure!(!path.contains("\\012"), "Ambiguous map pathname refused");
        let executable = p[2] == b'x';
        let kind = if inode > 0 {
            ensure!(
                path.starts_with('/'),
                "File mapping lacks absolute backing path"
            );
            MappingKind::File(path.into())
        } else if matches!(path, "[vdso]" | "[vvar]" | "[vvar_vclock]" | "[vsyscall]") {
            ensure!(
                device_major == 0 && device_minor == 0 && offset == 0,
                "Kernel mapping identity fields invalid"
            );
            ensure!(
                match path {
                    "[vdso]" => permissions == "r-xp",
                    "[vsyscall]" => permissions == "--xp",
                    _ => permissions == "r--p",
                },
                "Unsupported kernel mapping permissions"
            );
            MappingKind::Kernel(path.into())
        } else {
            ensure!(
                device_major == 0 && device_minor == 0,
                "Anonymous map has file device"
            );
            ensure!(!executable, "Unknown executable anonymous mapping");
            ensure!(
                path.is_empty()
                    || path == "[heap]"
                    || path == "[stack]"
                    || path.starts_with("[anon:")
                    || path.starts_with("[anon_shmem:")
                    || path.starts_with("[stack:"),
                "Unknown non-file mapping class"
            );
            MappingKind::Anonymous
        };
        output.push(Mapping {
            start,
            end,
            permissions,
            offset,
            device_major,
            device_minor,
            inode,
            kind,
        });
    }
    Ok(output)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}
#[cfg(unix)]
fn file_identity(m: &Metadata) -> FileIdentity {
    use std::os::unix::fs::MetadataExt;
    FileIdentity {
        device: m.dev(),
        inode: m.ino(),
    }
}
#[cfg(not(unix))]
fn file_identity(_: &Metadata) -> FileIdentity {
    FileIdentity {
        device: 0,
        inode: 0,
    }
}
fn same_metadata(a: &Metadata, b: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        file_identity(a) == file_identity(b)
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
fn safe_file(m: &Metadata, limit: u64, bounds: &Bounds) -> Result<()> {
    ensure!(
        m.is_file() && m.len() > 0 && m.len() <= limit,
        "Observed file bound or type failure"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            bounds.allowed_owner_uids.contains(&m.uid()),
            "Observed file owner not permitted"
        );
        ensure!(
            m.mode() & 0o6022 == 0 && m.nlink() == 1,
            "Observed file unsafe mode/link count"
        );
    }
    Ok(())
}
fn deadline(start: Instant, bounds: &Bounds) -> Result<()> {
    ensure!(
        start.elapsed() <= bounds.max_elapsed,
        "Observation duration exceeded"
    );
    Ok(())
}
fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(
            u64::try_from(limit)?
                .checked_add(1)
                .context("Read bound overflow")?,
        )
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "Proc input exceeded byte bound");
    Ok(bytes)
}
fn hash_file(
    mut file: File,
    expected: &FileDigest,
    start: Instant,
    bounds: &Bounds,
) -> Result<(FileDigest, Metadata, File)> {
    let before = file.metadata()?;
    safe_file(&before, bounds.max_file_bytes, bounds)?;
    ensure!(
        before.len() == expected.bytes,
        "Observed file size mismatch"
    );
    let mut h256 = Sha256::new();
    let mut h512 = Sha512::new();
    let mut buffer = [0u8; 65536];
    let mut total = 0u64;
    loop {
        deadline(start, bounds)?;
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total = total.checked_add(n as u64).context("Hash count overflow")?;
        ensure!(
            total <= expected.bytes && total <= bounds.max_file_bytes,
            "Observed file grew beyond bound"
        );
        h256.update(&buffer[..n]);
        h512.update(&buffer[..n]);
    }
    ensure!(
        same_metadata(&before, &file.metadata()?),
        "Observed file changed during hash"
    );
    let digest = FileDigest {
        bytes: total,
        sha256: hex::encode(h256.finalize()),
        sha512: hex::encode(h512.finalize()),
    };
    ensure!(&digest == expected, "Observed file digest mismatch");
    deadline(start, bounds)?;
    Ok((digest, before, file))
}
fn proc_path(pid: u32, file: &str) -> PathBuf {
    PathBuf::from(format!("/proc/{pid}/{file}"))
}
fn start_identity(child: &mut Child, bounds: &Bounds) -> Result<StartIdentity> {
    ensure!(
        cfg!(target_os = "linux"),
        "Actual process observation requires Linux"
    );
    ensure!(child.try_wait()?.is_none(), "Owned child exited");
    parse_stat(
        &read_bounded(&proc_path(child.id(), "stat"), bounds.max_stat_bytes)?,
        child.id(),
        bounds,
    )
}

/// Opaque start and launch binding. It cannot be decoded from a caller receipt.
#[derive(Clone, Debug)]
pub struct OwnedProcessIdentity {
    start: StartIdentity,
    role: String,
    member_id: String,
    expected: FileDigest,
    identity: FileIdentity,
    private_snapshot: Option<Arc<tempfile::TempDir>>,
    pidfd: Arc<File>,
}
impl OwnedProcessIdentity {
    pub fn start(&self) -> &StartIdentity {
        &self.start
    }
    pub fn member_id(&self) -> &str {
        &self.member_id
    }
    pub fn uses_private_snapshot(&self) -> bool {
        self.private_snapshot.is_some()
    }
}
fn capture(
    child: &mut Child,
    role: String,
    member_id: String,
    expected: FileDigest,
    identity: FileIdentity,
    private_snapshot: Option<Arc<tempfile::TempDir>>,
    bounds: &Bounds,
) -> Result<OwnedProcessIdentity> {
    bounds.validate()?;
    let start_time = Instant::now();
    let start = start_identity(child, bounds)?;
    let pidfd = Arc::new(open_pidfd(child.id())?);
    pidfd_alive(&pidfd)?;
    let (_, actual, _) = hash_file(
        File::open(proc_path(child.id(), "exe"))?,
        &expected,
        start_time,
        bounds,
    )?;
    ensure!(
        file_identity(&actual) == identity,
        "Launched executable inode mismatch"
    );
    ensure!(
        start_identity(child, bounds)? == start,
        "Child identity changed while capturing"
    );
    Ok(OwnedProcessIdentity {
        start,
        role,
        member_id,
        expected,
        identity,
        private_snapshot,
        pidfd,
    })
}
pub fn capture_owned_child(
    child: &mut Child,
    role: &str,
    catalog: &VerifiedMemberFiles,
    bounds: &Bounds,
) -> Result<OwnedProcessIdentity> {
    let member = catalog
        .candidate()
        .member_for_role(role)
        .context("Unknown candidate role")?;
    ensure!(
        member.kind == MemberKind::Executable,
        "Observe interpreter process separately from script role"
    );
    let file = catalog
        .role_file(role)
        .context("Role lacks verified file")?;
    capture(
        child,
        role.into(),
        file.id().into(),
        file.digest().clone(),
        FileIdentity {
            device: file.device(),
            inode: file.inode(),
        },
        None,
        bounds,
    )
}

/// Protected executable copy made from reverified catalog bytes. No external
/// pathname or serialized receipt can construct a valid snapshot object.
#[cfg(any(test, feature = "qualification-fixtures"))]
#[derive(Debug)]
pub struct PrivateSnapshot {
    role: String,
    directory: Arc<tempfile::TempDir>,
    path: PathBuf,
    member_id: String,
    digest: FileDigest,
    identity: FileIdentity,
}
#[cfg(any(test, feature = "qualification-fixtures"))]
impl PrivateSnapshot {
    pub fn member_id(&self) -> &str {
        &self.member_id
    }
    pub fn digest(&self) -> &FileDigest {
        &self.digest
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Child must stay alive for capture, normally through a bounded private
    /// protocol handshake. A fast exit is unqualified, never a skipped check.
    pub fn spawn_observed(
        &self,
        args: &[OsString],
        environment: &[(OsString, OsString)],
        bounds: &Bounds,
    ) -> Result<(Child, OwnedProcessIdentity)> {
        bounds.validate()?;
        let (_, before, _) = hash_file(
            File::open(&self.path)?,
            &self.digest,
            Instant::now(),
            bounds,
        )?;
        ensure!(
            file_identity(&before) == self.identity,
            "Private snapshot identity changed"
        );
        let mut command = Command::new(&self.path);
        command
            .args(args)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in environment {
            command.env(key, value);
        }
        let mut child = command.spawn()?;
        match capture(
            &mut child,
            self.role.clone(),
            self.member_id.clone(),
            self.digest.clone(),
            self.identity.clone(),
            Some(self.directory.clone()),
            bounds,
        ) {
            Ok(receipt) => Ok((child, receipt)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(error)
            }
        }
    }
}
#[cfg(any(test, feature = "qualification-fixtures"))]
pub fn create_private_snapshot(
    role: &str,
    catalog: &VerifiedMemberFiles,
    scratch: &Path,
    bounds: &Bounds,
) -> Result<PrivateSnapshot> {
    bounds.validate()?;
    ensure!(cfg!(unix), "Private snapshots require Unix");
    let started = Instant::now();
    ensure!(
        scratch.is_absolute()
            && scratch.as_os_str().len() <= bounds.max_path_bytes
            && std::fs::canonicalize(scratch)?.as_os_str() == scratch.as_os_str(),
        "Private scratch path must be canonical and bounded"
    );
    let parent = std::fs::symlink_metadata(scratch)?;
    ensure!(parent.is_dir(), "Private scratch must be a directory");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            parent.mode() & 0o077 == 0 && parent.uid() == unsafe { libc::geteuid() },
            "Private scratch must be owned by current user and private"
        );
    }
    let member = catalog
        .candidate()
        .member_for_role(role)
        .context("Unknown snapshot role")?;
    ensure!(
        member.kind == MemberKind::Executable,
        "Snapshot role must be executable"
    );
    let source = catalog.role_file(role).context("Missing snapshot member")?;
    ensure!(
        source.digest().bytes <= bounds.max_total_file_bytes,
        "Snapshot total byte bound"
    );
    let mut builder = tempfile::Builder::new();
    builder.prefix("dyt-verified-helper-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let directory = Arc::new(builder.tempdir_in(scratch)?);
    let parent_after = std::fs::symlink_metadata(scratch)?;
    ensure!(
        parent_after.is_dir() && file_identity(&parent) == file_identity(&parent_after),
        "Scratch directory changed"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            parent_after.mode() == parent.mode()
                && parent_after.uid() == parent.uid()
                && parent_after.gid() == parent.gid(),
            "Scratch authority changed"
        );
    }
    let path = directory.path().join("member");
    ensure!(
        std::fs::canonicalize(source.path())?.as_os_str() == source.path().as_os_str(),
        "Snapshot source path became an alias"
    );
    let mut input = File::open(source.path())?;
    let before = input.metadata()?;
    safe_file(&before, bounds.max_file_bytes, bounds)?;
    ensure!(
        file_identity(&before)
            == FileIdentity {
                device: source.device(),
                inode: source.inode()
            },
        "Catalog source identity changed"
    );
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o500);
    }
    let mut output = options.open(&path)?;
    let mut h256 = Sha256::new();
    let mut h512 = Sha512::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        deadline(started, bounds)?;
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .context("Snapshot size overflow")?;
        ensure!(
            total <= source.digest().bytes && total <= bounds.max_file_bytes,
            "Snapshot source grew beyond bound"
        );
        output.write_all(&buffer[..n])?;
        h256.update(&buffer[..n]);
        h512.update(&buffer[..n]);
    }
    ensure!(
        same_metadata(&before, &input.metadata()?),
        "Snapshot source changed during copy"
    );
    let copied = FileDigest {
        bytes: total,
        sha256: hex::encode(h256.finalize()),
        sha512: hex::encode(h512.finalize()),
    };
    ensure!(
        &copied == source.digest(),
        "Snapshot source digest mismatch"
    );
    output.sync_all()?;
    drop(output);
    let (digest, metadata, _) = hash_file(File::open(&path)?, source.digest(), started, bounds)?;
    Ok(PrivateSnapshot {
        role: role.into(),
        directory,
        path,
        member_id: source.id().into(),
        digest,
        identity: file_identity(&metadata),
    })
}

#[derive(Clone, Debug)]
pub struct ObservedFile {
    pub member_id: String,
    pub device: u64,
    pub inode: u64,
    pub digest: FileDigest,
    pub backing_source: String,
}
#[derive(Clone, Copy, Debug)]
enum ObservationScope {
    OwnedChild,
    CurrentProcess,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    scope: ObservationScope,
    start: StartIdentity,
    role: String,
    candidate_sha512: String,
    files: Vec<ObservedFile>,
    kernel_mappings: Vec<Mapping>,
    anonymous_regions: usize,
    maps_sha256: String,
    elapsed: Duration,
}
impl Snapshot {
    pub fn scope(&self) -> &'static str {
        match self.scope {
            ObservationScope::OwnedChild => "OWNED_PROCESS_AND_MAPPINGS_SNAPSHOT",
            ObservationScope::CurrentProcess => "CURRENT_PROCESS_AND_MAPPINGS_SNAPSHOT",
        }
    }
    pub fn start(&self) -> &StartIdentity {
        &self.start
    }
    pub fn role(&self) -> &str {
        &self.role
    }
    pub fn candidate_sha512(&self) -> &str {
        &self.candidate_sha512
    }
    pub fn files(&self) -> &[ObservedFile] {
        &self.files
    }
    pub fn kernel_mappings(&self) -> &[Mapping] {
        &self.kernel_mappings
    }
    pub fn anonymous_regions(&self) -> usize {
        self.anonymous_regions
    }
    pub fn maps_sha256(&self) -> &str {
        &self.maps_sha256
    }
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }
    /// A serializable report, never an input authority or readiness token.
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({
            "scope": self.scope(), "pid": self.start.pid, "start_ticks": self.start.start_ticks,
            "role": self.role, "candidate_sha512": self.candidate_sha512,
            "files": self.files.iter().map(|f| serde_json::json!({"member_id":f.member_id,
                "device":f.device,"inode":f.inode,"bytes":f.digest.bytes,"sha256":f.digest.sha256,
                "sha512":f.digest.sha512,"backing_source":f.backing_source})).collect::<Vec<_>>(),
            "kernel_mappings": self.kernel_mappings.iter().map(|m| serde_json::json!({
                "start":m.start,"end":m.end,"permissions":m.permissions,
                "class":match &m.kind {MappingKind::Kernel(name)=>name.as_str(),_=>"invalid"}
            })).collect::<Vec<_>>(),
            "anonymous_regions":self.anonymous_regions,"maps_sha256":self.maps_sha256,
            "elapsed_millis":self.elapsed.as_millis(),"continuous_enforcement":false,
            "role_behavior_qualified":false,"production_qualified":false
        })
    }
}
impl serde::Serialize for Snapshot {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        self.report().serialize(serializer)
    }
}

fn device_parts(device: u64) -> (u64, u64) {
    (
        ((device >> 8) & 0xfff) | ((device >> 32) & 0xfffff000),
        (device & 0xff) | ((device >> 12) & 0xffffff00),
    )
}
fn map_identity(map: &Mapping, identity: &FileIdentity) -> bool {
    device_parts(identity.device) == (map.device_major, map.device_minor)
        && identity.inode == map.inode
}
#[cfg(target_os = "linux")]
fn open_pidfd(pid: u32) -> Result<File> {
    use std::os::fd::FromRawFd;
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    ensure!(
        fd >= 0,
        "Cannot pin owned child with pidfd: {}",
        std::io::Error::last_os_error()
    );
    Ok(unsafe { File::from_raw_fd(i32::try_from(fd)?) })
}
#[cfg(not(target_os = "linux"))]
fn open_pidfd(_: u32) -> Result<File> {
    anyhow::bail!("Owned child pidfd requires Linux")
}
#[cfg(target_os = "linux")]
fn pidfd_alive(file: &File) -> Result<()> {
    use std::os::fd::AsRawFd;
    let mut request = libc::pollfd {
        fd: file.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let result = unsafe { libc::poll(&mut request, 1, 0) };
    ensure!(
        result == 0 && request.revents == 0,
        "Owned child exited or pidfd observation failed"
    );
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn pidfd_alive(_: &File) -> Result<()> {
    anyhow::bail!("Owned child pidfd requires Linux")
}

fn require_child(
    child: &mut Child,
    identity: &OwnedProcessIdentity,
    bounds: &Bounds,
) -> Result<()> {
    pidfd_alive(&identity.pidfd)?;
    ensure!(
        child.id() == identity.start.pid && start_identity(child, bounds)? == identity.start,
        "Owned child start identity mismatch"
    );
    Ok(())
}

pub fn observe_owned_child(
    child: &mut Child,
    identity: &OwnedProcessIdentity,
    role: &str,
    catalog: &VerifiedMemberFiles,
    bounds: &Bounds,
) -> Result<Snapshot> {
    bounds.validate()?;
    let started = Instant::now();
    require_child(child, identity, bounds)?;
    let member = catalog
        .candidate()
        .member_for_role(role)
        .context("Unknown observed role")?;
    ensure!(
        member.kind == MemberKind::Executable
            && member.id == identity.member_id
            && identity.role == role,
        "Observed child role binding mismatch"
    );
    let expected = catalog
        .role_file(role)
        .context("Observed role file missing")?
        .digest();
    ensure!(
        expected == &identity.expected,
        "Observed member differs from candidate"
    );
    observe_process(
        child.id(),
        &identity.start,
        role,
        &identity.identity,
        ObservationScope::OwnedChild,
        catalog,
        bounds,
        started,
        || require_child(child, identity, bounds),
    )
}

fn current_start_identity(bounds: &Bounds) -> Result<StartIdentity> {
    ensure!(
        cfg!(target_os = "linux"),
        "Current process observation requires Linux"
    );
    let pid = std::process::id();
    parse_stat(
        &read_bounded(Path::new("/proc/self/stat"), bounds.max_stat_bytes)?,
        pid,
        bounds,
    )
}

/// Observe this executable and its mappings against the verified role catalog.
/// This does not construct child ownership, accept a PID, or prove continuous
/// enforcement. Exact before/after map equality can fail if this process changes
/// its own mappings during the bounded observation; callers must not suppress it.
pub fn observe_current_process(
    role: &str,
    catalog: &VerifiedMemberFiles,
    bounds: &Bounds,
) -> Result<Snapshot> {
    bounds.validate()?;
    let started = Instant::now();
    let start = current_start_identity(bounds)?;
    let member = catalog
        .candidate()
        .member_for_role(role)
        .context("Unknown observed role")?;
    ensure!(
        member.kind == MemberKind::Executable,
        "Current process role must be executable"
    );
    let file = catalog
        .role_file(role)
        .context("Observed role file missing")?;
    let identity = FileIdentity {
        device: file.device(),
        inode: file.inode(),
    };
    observe_process(
        start.pid,
        &start,
        role,
        &identity,
        ObservationScope::CurrentProcess,
        catalog,
        bounds,
        started,
        || {
            ensure!(
                current_start_identity(bounds)? == start,
                "Current process start identity changed"
            );
            Ok(())
        },
    )
}

// Only the two public owned/self entry points supply process identity. This
// shared private body keeps inode, byte, mapping and bound checks identical.
fn observe_process(
    pid: u32,
    start: &StartIdentity,
    role: &str,
    executable_identity: &FileIdentity,
    scope: ObservationScope,
    catalog: &VerifiedMemberFiles,
    bounds: &Bounds,
    started: Instant,
    finish_identity: impl FnOnce() -> Result<()>,
) -> Result<Snapshot> {
    let member = catalog
        .candidate()
        .member_for_role(role)
        .context("Unknown observed role")?;
    let expected = catalog
        .role_file(role)
        .context("Observed role file missing")?
        .digest();
    let role_row = catalog
        .candidate()
        .manifest()
        .roles
        .iter()
        .find(|r| r.role == role)
        .unwrap();
    let profile = catalog
        .candidate()
        .manifest()
        .runtime_profiles
        .iter()
        .find(|p| p.id == role_row.runtime_profile_id)
        .context("Observed runtime profile missing")?;
    let (digest, exe, exe_handle) = hash_file(
        File::open(proc_path(pid, "exe"))?,
        expected,
        started,
        bounds,
    )?;
    ensure!(
        file_identity(&exe) == *executable_identity,
        "Observed executable inode changed"
    );
    let maps_bytes = read_bounded(&proc_path(pid, "maps"), bounds.max_maps_bytes)?;
    let maps = parse_maps(&maps_bytes, bounds)?;
    let mut held_files = Vec::new();
    let mut files = BTreeMap::new();
    let key = (executable_identity.device, executable_identity.inode);
    files.insert(
        key,
        ObservedFile {
            member_id: member.id.clone(),
            device: key.0,
            inode: key.1,
            digest,
            backing_source: "proc_exe".into(),
        },
    );
    let mut total = expected.bytes;
    ensure!(
        total <= bounds.max_total_file_bytes,
        "Observed total file bytes exceeded"
    );
    let mut kernel_mappings = Vec::new();
    let mut anonymous_regions = 0;
    let mut executable_seen = false;
    for map in &maps {
        deadline(started, bounds)?;
        match &map.kind {
            MappingKind::Anonymous => {
                anonymous_regions += 1;
            }
            MappingKind::Kernel(name) => {
                ensure!(
                    name != "[vsyscall]" || catalog.candidate().manifest().target.arch == "x86_64",
                    "Unsupported architecture kernel mapping"
                );
                kernel_mappings.push(map.clone());
            }
            MappingKind::File(_) => {
                if map_identity(map, executable_identity) {
                    ensure!(
                        map.offset < expected.bytes,
                        "Executable map offset outside file"
                    );
                    executable_seen |= map.executable();
                    continue;
                }
                let matched=profile.member_ids.iter().filter_map(|id|catalog.files().iter().find(|f|f.id()==id)).find(|f|map_identity(map,&FileIdentity{device:f.device(),inode:f.inode()})).context("File mapping absent from declared runtime profile; data-mapping policy required for non-code files")?;
                ensure!(
                    map.offset < matched.digest().bytes,
                    "Mapping offset outside member file"
                );
                let key = (matched.device(), matched.inode());
                if files.contains_key(&key) {
                    continue;
                }
                ensure!(
                    files.len() < bounds.max_unique_files,
                    "Observed unique file bound"
                );
                total = total
                    .checked_add(matched.digest().bytes)
                    .context("Observed total byte overflow")?;
                ensure!(
                    total <= bounds.max_total_file_bytes,
                    "Observed total file bytes exceeded"
                );
                let proc_backing =
                    proc_path(pid, &format!("map_files/{:x}-{:x}", map.start, map.end));
                let (file, source) = match File::open(&proc_backing) {
                    Ok(file) => (file, "proc_map_files"),
                    Err(_) => {
                        ensure!(
                            std::fs::canonicalize(matched.path())?.as_os_str()
                                == matched.path().as_os_str(),
                            "Mapped fallback path is an alias"
                        );
                        (
                            File::open(matched.path())?,
                            "catalog_path_identity_fallback",
                        )
                    }
                };
                ensure!(
                    map_identity(map, &file_identity(&file.metadata()?)),
                    "Mapped backing identity mismatch"
                );
                let (digest, metadata, backing_handle) =
                    hash_file(file, matched.digest(), started, bounds)?;
                ensure!(
                    map_identity(map, &file_identity(&metadata)),
                    "Mapped file identity changed"
                );
                held_files.push((backing_handle, metadata));
                files.insert(
                    key,
                    ObservedFile {
                        member_id: matched.id().into(),
                        device: key.0,
                        inode: key.1,
                        digest,
                        backing_source: source.into(),
                    },
                );
            }
        }
    }
    for (file, before) in &held_files {
        ensure!(
            same_metadata(before, &file.metadata()?),
            "Mapped backing changed after hashing"
        );
    }
    ensure!(
        same_metadata(&exe, &exe_handle.metadata()?),
        "Executable backing changed after hashing"
    );
    ensure!(
        executable_seen,
        "Running executable has no matching executable mapping"
    );
    let second_maps_bytes = read_bounded(&proc_path(pid, "maps"), bounds.max_maps_bytes)?;
    if second_maps_bytes != maps_bytes {
        anyhow::bail!(
            "Mappings changed during observation; {}",
            mapping_diagnostic::summary(
                &maps_bytes, &second_maps_bytes, &maps, bounds, start, scope, role,
                started.elapsed(),
            )
        );
    }
    let current_exe = File::open(proc_path(pid, "exe"))?.metadata()?;
    ensure!(
        same_metadata(&exe, &current_exe),
        "Executable changed during observation"
    );
    finish_identity()?;
    deadline(started, bounds)?;
    Ok(Snapshot {
        scope,
        start: start.clone(),
        role: role.into(),
        candidate_sha512: catalog.candidate().sha512().into(),
        files: files.into_values().collect(),
        kernel_mappings,
        anonymous_regions,
        maps_sha256: hex::encode(Sha256::digest(maps_bytes)),
        elapsed: started.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    include!("observation_tests.rs");
    include!("observation_current_tests.rs");
    include!("observation_mapping_diagnostic_tests.rs");
}
