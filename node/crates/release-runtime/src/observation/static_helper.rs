//! Independent static-helper file and owned-process checks.
//! Results describe one observation. They do not grant release authority,
//! prove helper behavior, or prevent mappings from changing after observation.
use super::*;

#[derive(Clone, Debug)]
pub struct StaticHelperPolicy {
    pub max_file_bytes: u64,
    pub max_path_bytes: usize,
    pub max_program_headers: usize,
    pub max_section_headers: usize,
    pub max_elapsed: Duration,
}
impl StaticHelperPolicy {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.max_file_bytes >= 64
                && self.max_path_bytes > 0
                && self.max_program_headers > 0
                && self.max_section_headers > 0
                && !self.max_elapsed.is_zero(),
            "Static helper bounds must be explicit and nonzero"
        );
        Ok(())
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"max_file_bytes":self.max_file_bytes,"max_path_bytes":self.max_path_bytes,
            "max_program_headers":self.max_program_headers,"max_section_headers":self.max_section_headers,
            "max_elapsed_millis":self.max_elapsed.as_millis(),"root_owned":true,
            "read_only_mount":true,"elf_profile":"linux-x86_64-static-et-exec-v1"})
    }
}

/// Only verify_file constructs this object. The retained handle stays read-only.
#[derive(Debug)]
pub struct VerifiedStaticHelperFile {
    file: File,
    path: PathBuf,
    metadata: Metadata,
    digest: FileDigest,
    policy: StaticHelperPolicy,
}
impl VerifiedStaticHelperFile {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn digest(&self) -> &FileDigest {
        &self.digest
    }
    pub fn device(&self) -> u64 {
        file_identity(&self.metadata).device
    }
    pub fn inode(&self) -> u64 {
        file_identity(&self.metadata).inode
    }
    pub fn file(&self) -> &File {
        &self.file
    }
    pub fn policy(&self) -> &StaticHelperPolicy {
        &self.policy
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"path":self.path,"device":self.device(),"inode":self.inode(),
            "bytes":self.digest.bytes,"sha256":self.digest.sha256,"sha512":self.digest.sha512,
            "policy":self.policy.report(),"release_authority":false,"production_qualified":false})
    }
}

fn elapsed(start: Instant, policy: &StaticHelperPolicy) -> Result<()> {
    ensure!(
        start.elapsed() <= policy.max_elapsed,
        "Static helper verification duration exceeded"
    );
    Ok(())
}
fn sha256_text(value: &str) -> Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Independent helper SHA256 must be lowercase hexadecimal"
    );
    Ok(())
}
fn range(bytes: &[u8], start: u64, len: u64) -> Result<&[u8]> {
    let end = start.checked_add(len).context("ELF range overflow")?;
    ensure!(end <= bytes.len() as u64, "ELF range exceeds file bytes");
    Ok(&bytes[usize::try_from(start)?..usize::try_from(end)?])
}
fn u16le(bytes: &[u8], pos: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(range(bytes, pos as u64, 2)?.try_into()?))
}
fn u32le(bytes: &[u8], pos: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(range(bytes, pos as u64, 4)?.try_into()?))
}
fn u64le(bytes: &[u8], pos: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(range(bytes, pos as u64, 8)?.try_into()?))
}

// A bounded structural profile. Absence of dynamic metadata does not prove
// absence of code that could call mmap or other system calls at runtime.
fn validate_elf(bytes: &[u8], policy: &StaticHelperPolicy) -> Result<()> {
    policy.validate()?;
    ensure!(
        bytes.len() >= 64 && bytes.len() as u64 <= policy.max_file_bytes,
        "ELF byte bound"
    );
    ensure!(
        &bytes[..7] == b"\x7fELF\x02\x01\x01"
            && matches!(bytes[7], 0 | 3)
            && bytes[8..16].iter().all(|b| *b == 0),
        "Static helper requires Linux ELF64 little-endian header"
    );
    ensure!(
        u16le(bytes, 16)? == 2
            && u16le(bytes, 18)? == 62
            && u32le(bytes, 20)? == 1
            && u32le(bytes, 48)? == 0
            && u16le(bytes, 52)? == 64,
        "Only x86_64 ET_EXEC static ELF is supported"
    );
    let phoff = u64le(bytes, 32)?;
    let phnum = usize::from(u16le(bytes, 56)?);
    ensure!(
        u16le(bytes, 54)? == 56
            && phoff >= 64
            && phnum > 0
            && phnum < 0xffff
            && phnum <= policy.max_program_headers,
        "ELF program header bound or encoding"
    );
    let table = range(
        bytes,
        phoff,
        (phnum as u64)
            .checked_mul(56)
            .context("ELF program header overflow")?,
    )?;
    let entry = u64le(bytes, 24)?;
    let mut entry_seen = false;
    let mut executable_load = false;
    for ph in table.chunks_exact(56) {
        let kind = u32le(ph, 0)?;
        ensure!(
            kind != 2 && kind != 3,
            "Static helper refuses PT_DYNAMIC and PT_INTERP"
        );
        ensure!(
            matches!(
                kind,
                0 | 1 | 4 | 6 | 7 | 0x6474e550 | 0x6474e551 | 0x6474e552 | 0x6474e553
            ),
            "Unsupported static helper program header"
        );
        let flags = u32le(ph, 4)?;
        ensure!(
            flags & !7 == 0 && !(flags & 1 != 0 && flags & 2 != 0),
            "Unsafe ELF segment permissions"
        );
        let offset = u64le(ph, 8)?;
        let address = u64le(ph, 16)?;
        let filesz = u64le(ph, 32)?;
        let memsz = u64le(ph, 40)?;
        let align = u64le(ph, 48)?;
        range(bytes, offset, filesz)?;
        ensure!(
            align <= 1 || align.is_power_of_two(),
            "ELF segment alignment"
        );
        if kind == 1 {
            ensure!(
                filesz <= memsz && memsz > 0 && (align <= 1 || address % align == offset % align),
                "ELF load range or alignment"
            );
            let end = address
                .checked_add(filesz)
                .context("ELF load address overflow")?;
            address
                .checked_add(memsz)
                .context("ELF memory range overflow")?;
            if flags & 1 != 0 {
                ensure!(
                    flags & 4 != 0 && filesz > 0,
                    "Executable ELF load must be readable and file-backed"
                );
                executable_load = true;
                entry_seen |= entry >= address && entry < end;
            }
        }
        if kind == 0x6474e551 {
            ensure!(flags & 1 == 0, "Executable GNU stack refused");
        }
    }
    ensure!(
        executable_load && entry_seen,
        "ELF entry is outside executable file-backed load"
    );
    let shoff = u64le(bytes, 40)?;
    let shnum = usize::from(u16le(bytes, 60)?);
    let shstr = usize::from(u16le(bytes, 62)?);
    if shoff == 0 {
        ensure!(
            shnum == 0 && shstr == 0,
            "ELF absent section table encoding"
        );
    } else {
        ensure!(
            shoff >= 64
                && u16le(bytes, 58)? == 64
                && shnum > 0
                && shnum < 0xff00
                && shnum <= policy.max_section_headers
                && shstr < shnum,
            "ELF section header bound or extended encoding"
        );
        let sections = range(
            bytes,
            shoff,
            (shnum as u64)
                .checked_mul(64)
                .context("ELF section overflow")?,
        )?;
        for sh in sections.chunks_exact(64) {
            let kind = u32le(sh, 4)?;
            ensure!(
                !matches!(kind, 6 | 11),
                "Static helper refuses dynamic section metadata"
            );
            if kind != 8 {
                range(bytes, u64le(sh, 24)?, u64le(sh, 32)?)?;
            }
        }
    }
    Ok(())
}

fn mount_policy(read_only: bool, noexec: bool) -> Result<()> {
    ensure!(
        read_only && !noexec,
        "Static helper requires read-only execution-capable mount"
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn require_read_only(file: &File) -> Result<()> {
    use std::os::fd::AsRawFd;
    let mut value = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    ensure!(
        unsafe { libc::fstatvfs(file.as_raw_fd(), value.as_mut_ptr()) } == 0,
        "Static helper mount query failed: {}",
        std::io::Error::last_os_error()
    );
    let value = unsafe { value.assume_init() };
    mount_policy(
        value.f_flag & libc::ST_RDONLY != 0,
        value.f_flag & libc::ST_NOEXEC != 0,
    )?;
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn require_read_only(_: &File) -> Result<()> {
    anyhow::bail!("Static helper verification requires Linux")
}

fn file_policy(
    regular: bool,
    len: u64,
    uid: u32,
    links: u64,
    mode: u32,
    policy: &StaticHelperPolicy,
) -> Result<()> {
    ensure!(
        regular
            && len >= 64
            && len <= policy.max_file_bytes
            && uid == 0
            && links == 1
            && mode & 0o7022 == 0
            && mode & 0o111 != 0,
        "Static helper must be bounded root-owned regular executable without unsafe modes or links"
    );
    Ok(())
}
#[cfg(unix)]
fn safe_root_file(metadata: &Metadata, policy: &StaticHelperPolicy) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    file_policy(
        metadata.is_file(),
        metadata.len(),
        metadata.uid(),
        metadata.nlink(),
        metadata.mode(),
        policy,
    )
}
#[cfg(not(unix))]
fn safe_root_file(_: &Metadata, _: &StaticHelperPolicy) -> Result<()> {
    anyhow::bail!("Static helper requires Unix metadata")
}
fn canonical_path(path: &Path, policy: &StaticHelperPolicy) -> Result<()> {
    use std::path::Component;
    ensure!(
        path.is_absolute()
            && path
                .to_str()
                .is_some_and(|v| !v.is_empty() && v.len() <= policy.max_path_bytes)
            && path
                .components()
                .all(|c| matches!(c, Component::RootDir | Component::Normal(_))),
        "Static helper path bound or form"
    );
    ensure!(
        std::fs::canonicalize(path)?.as_os_str() == path.as_os_str(),
        "Static helper path alias refused"
    );
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(
        !metadata.file_type().is_symlink(),
        "Static helper symlink refused"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        for parent in path.ancestors().skip(1) {
            let m = std::fs::symlink_metadata(parent)?;
            ensure!(
                m.is_dir() && !m.file_type().is_symlink() && m.uid() == 0 && m.mode() & 0o022 == 0,
                "Static helper ancestor must be a root-owned directory without group/other write"
            );
        }
    }
    Ok(())
}
#[cfg(unix)]
fn open_regular(path: &Path) -> Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    Ok(OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)?)
}
#[cfg(not(unix))]
fn open_regular(_: &Path) -> Result<File> {
    anyhow::bail!("Static helper requires Unix")
}

fn validate_image(
    bytes: &[u8],
    independent_sha256: &str,
    policy: &StaticHelperPolicy,
) -> Result<FileDigest> {
    sha256_text(independent_sha256)?;
    ensure!(
        bytes.len() as u64 <= policy.max_file_bytes,
        "Static helper image byte bound"
    );
    let digest = FileDigest {
        bytes: bytes.len() as u64,
        sha256: hex::encode(Sha256::digest(bytes)),
        sha512: hex::encode(Sha512::digest(bytes)),
    };
    ensure!(
        digest.sha256 == independent_sha256,
        "Static helper independent hash mismatch"
    );
    validate_elf(bytes, policy)?;
    Ok(digest)
}

/// The digest comes from independent trusted configuration, never the helper
/// file or a local candidate manifest. This function creates no release authority.
pub fn verify_file(
    path: &Path,
    independent_sha256: &str,
    policy: &StaticHelperPolicy,
) -> Result<VerifiedStaticHelperFile> {
    policy.validate()?;
    sha256_text(independent_sha256)?;
    ensure!(
        cfg!(all(target_os = "linux", target_arch = "x86_64")),
        "Static helper requires native Linux x86_64"
    );
    let start = Instant::now();
    canonical_path(path, policy)?;
    let file = open_regular(path)?;
    let before = file.metadata()?;
    safe_root_file(&before, policy)?;
    require_read_only(&file)?;
    let mut bytes = Vec::new();
    let mut reader = file.try_clone()?;
    let mut buffer = [0u8; 65536];
    loop {
        elapsed(start, policy)?;
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let next = bytes
            .len()
            .checked_add(count)
            .context("Static helper read overflow")?;
        ensure!(
            next as u64 <= policy.max_file_bytes && next as u64 <= before.len(),
            "Static helper grew beyond byte bound"
        );
        bytes.extend_from_slice(&buffer[..count]);
    }
    elapsed(start, policy)?;
    ensure!(
        bytes.len() as u64 == before.len() && bytes.len() as u64 <= policy.max_file_bytes,
        "Static helper read bound or length changed"
    );
    ensure!(
        same_metadata(&before, &file.metadata()?),
        "Static helper changed while reading"
    );
    let digest = validate_image(&bytes, independent_sha256, policy)?;
    canonical_path(path, policy)?;
    ensure!(
        same_metadata(&before, &open_regular(path)?.metadata()?),
        "Static helper path changed after verification"
    );
    require_read_only(&file)?;
    elapsed(start, policy)?;
    Ok(VerifiedStaticHelperFile {
        file,
        path: path.into(),
        metadata: before,
        digest,
        policy: policy.clone(),
    })
}
fn retained(file: &VerifiedStaticHelperFile) -> Result<()> {
    safe_root_file(&file.file.metadata()?, &file.policy)?;
    ensure!(
        same_metadata(&file.metadata, &file.file.metadata()?),
        "Retained static helper file changed"
    );
    require_read_only(&file.file)?;
    canonical_path(&file.path, &file.policy)?;
    let installed = open_regular(&file.path)?;
    ensure!(
        same_metadata(&file.metadata, &installed.metadata()?),
        "Static helper installed identity changed"
    );
    require_read_only(&installed)?;
    Ok(())
}
fn observation_bounds(file: &VerifiedStaticHelperFile, bounds: &Bounds) -> Result<()> {
    bounds.validate()?;
    ensure!(
        bounds.allowed_owner_uids.contains(&0)
            && file.digest.bytes <= bounds.max_file_bytes
            && file.digest.bytes <= bounds.max_total_file_bytes,
        "Static helper observation owner or byte bound"
    );
    Ok(())
}

/// Captured only from a live Child handle. No external PID can construct it.
#[derive(Clone, Debug)]
pub struct OwnedStaticHelperIdentity {
    inner: OwnedProcessIdentity,
}
impl OwnedStaticHelperIdentity {
    pub fn owned_identity(&self) -> &OwnedProcessIdentity {
        &self.inner
    }
    pub fn start(&self) -> &StartIdentity {
        self.inner.start()
    }
}
pub fn validate_owned_identity(
    identity: &OwnedStaticHelperIdentity,
    file: &VerifiedStaticHelperFile,
) -> Result<()> {
    retained(file)?;
    ensure!(
        identity.inner.role == "independent_static_helper"
            && identity.inner.member_id == "independent_static_helper"
            && identity.inner.identity == file_identity(&file.metadata)
            && identity.inner.expected == file.digest,
        "Static helper identity belongs to another verified file"
    );
    Ok(())
}
pub fn capture_owned_child(
    child: &mut Child,
    file: &VerifiedStaticHelperFile,
    bounds: &Bounds,
) -> Result<OwnedStaticHelperIdentity> {
    observation_bounds(file, bounds)?;
    let started = Instant::now();
    retained(file)?;
    let inner = super::capture(
        child,
        "independent_static_helper".into(),
        "independent_static_helper".into(),
        file.digest.clone(),
        file_identity(&file.metadata),
        None,
        bounds,
    )?;
    retained(file)?;
    deadline(started, bounds)?;
    Ok(OwnedStaticHelperIdentity { inner })
}

/// This opaque value records one owned-helper process and maps observation.
#[derive(Clone, Debug)]
pub struct StaticHelperSnapshot {
    start: StartIdentity,
    path: PathBuf,
    device: u64,
    inode: u64,
    digest: FileDigest,
    kernel_mappings: Vec<Mapping>,
    anonymous_regions: usize,
    maps_sha256: String,
    elapsed: Duration,
}
impl StaticHelperSnapshot {
    pub fn start(&self) -> &StartIdentity {
        &self.start
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
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"scope":"OWNED_STATIC_HELPER_AND_MAPPINGS_SNAPSHOT","pid":self.start.pid,
            "start_ticks":self.start.start_ticks,"path":self.path,"device":self.device,"inode":self.inode,
            "bytes":self.digest.bytes,"sha256":self.digest.sha256,"sha512":self.digest.sha512,
            "maps_sha256":self.maps_sha256,"anonymous_regions":self.anonymous_regions,
            "kernel_mappings":self.kernel_mappings.iter().map(|m|serde_json::json!({"start":m.start,
                "end":m.end,"permissions":m.permissions,"class":match &m.kind {MappingKind::Kernel(v)=>v.as_str(),_=>"invalid"}})).collect::<Vec<_>>(),
            "elapsed_millis":self.elapsed.as_millis(),"continuous_enforcement":false,
            "role_behavior_qualified":false,"release_authority":false,"production_qualified":false})
    }
}
impl serde::Serialize for StaticHelperSnapshot {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        self.report().serialize(serializer)
    }
}

fn static_mappings(
    maps: &[Mapping],
    identity: &FileIdentity,
    digest: &FileDigest,
) -> Result<(Vec<Mapping>, usize)> {
    let mut kernel = Vec::new();
    let mut anonymous = 0;
    let mut executable = false;
    for map in maps {
        match &map.kind {
            MappingKind::File(_) => {
                ensure!(
                    !(map.executable() && map.permissions.as_bytes()[1] == b'w'),
                    "Writable executable static helper mapping refused"
                );
                ensure!(
                    map_identity(map, identity),
                    "Static helper has an undeclared file mapping"
                );
                ensure!(
                    map.offset < digest.bytes,
                    "Static helper map offset outside file"
                );
                executable |= map.executable();
            }
            MappingKind::Anonymous => anonymous += 1,
            MappingKind::Kernel(_) => kernel.push(map.clone()),
        }
    }
    ensure!(executable, "Static helper lacks its executable mapping");
    Ok((kernel, anonymous))
}
pub fn observe_owned_child(
    child: &mut Child,
    identity: &OwnedStaticHelperIdentity,
    file: &VerifiedStaticHelperFile,
    bounds: &Bounds,
) -> Result<StaticHelperSnapshot> {
    observation_bounds(file, bounds)?;
    let started = Instant::now();
    retained(file)?;
    super::require_child(child, &identity.inner, bounds)?;
    ensure!(
        identity.inner.identity == file_identity(&file.metadata)
            && identity.inner.expected == file.digest,
        "Static helper identity belongs to another verified file"
    );
    let (digest, exe, exe_handle) = hash_file(
        File::open(proc_path(child.id(), "exe"))?,
        &file.digest,
        started,
        bounds,
    )?;
    ensure!(
        same_metadata(&file.metadata, &exe),
        "Static helper executable differs from retained file"
    );
    let maps_bytes = read_bounded(&proc_path(child.id(), "maps"), bounds.max_maps_bytes)?;
    let maps = parse_maps(&maps_bytes, bounds)?;
    let (kernel_mappings, anonymous_regions) =
        static_mappings(&maps, &file_identity(&exe), &digest)?;
    ensure!(
        read_bounded(&proc_path(child.id(), "maps"), bounds.max_maps_bytes)? == maps_bytes,
        "Static helper mappings changed during observation"
    );
    ensure!(
        same_metadata(&exe, &exe_handle.metadata()?)
            && same_metadata(&exe, &File::open(proc_path(child.id(), "exe"))?.metadata()?),
        "Static helper executable changed during observation"
    );
    retained(file)?;
    super::require_child(child, &identity.inner, bounds)?;
    deadline(started, bounds)?;
    Ok(StaticHelperSnapshot {
        start: identity.inner.start.clone(),
        path: file.path.clone(),
        device: file.device(),
        inode: file.inode(),
        digest,
        kernel_mappings,
        anonymous_regions,
        maps_sha256: hex::encode(Sha256::digest(maps_bytes)),
        elapsed: started.elapsed(),
    })
}

#[cfg(test)]
mod tests;
