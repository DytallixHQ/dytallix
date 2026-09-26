//! Cooperative lifecycle exclusion. Locks remain on disk after release.
use anyhow::{ensure, Context, Result};
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

/// Holds both locks until all owned processes have stopped. The process owner
/// inherits duplicate descriptors so abrupt owner exit does not release early.
pub struct LifecycleLease {
    service: File,
    state: File,
    paths: [PathBuf; 2],
}

fn lock(path: &Path) -> Result<File> {
    ensure!(path.is_absolute(), "Lease path must be absolute");
    let parent = path.parent().context("Lease parent required")?;
    ensure!(
        std::fs::canonicalize(parent)? == parent,
        "Lease parent has an alias"
    );
    let directory = std::fs::symlink_metadata(parent)?;
    ensure!(
        directory.is_dir()
            && directory.uid() == unsafe { libc::geteuid() }
            && directory.mode() & 0o7777 == 0o700,
        "Lease parent must be current-user mode 0700"
    );
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file()
            && metadata.nlink() == 1
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o7777 == 0o600,
        "Unsafe lifecycle lease file"
    );
    ensure!(
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "Lifecycle lease is already held or unavailable"
    );
    let current = std::fs::symlink_metadata(path)?;
    ensure!(
        current.dev() == metadata.dev() && current.ino() == metadata.ino(),
        "Lifecycle lease path changed while locking"
    );
    Ok(file)
}

impl LifecycleLease {
    pub fn acquire(service: &Path, state: &Path) -> Result<Self> {
        ensure!(
            service != state,
            "Service and state require distinct lease paths"
        );
        let service_file = lock(service)?;
        let state_file = lock(state)?;
        ensure!(
            (
                service_file.metadata()?.dev(),
                service_file.metadata()?.ino()
            ) != (state_file.metadata()?.dev(), state_file.metadata()?.ino()),
            "Service and state leases share one inode"
        );
        Ok(Self {
            service: service_file,
            state: state_file,
            paths: [service.to_path_buf(), state.to_path_buf()],
        })
    }
    pub fn files(&self) -> (&File, &File) {
        (&self.service, &self.state)
    }
    pub fn recheck(&self) -> Result<()> {
        for (path, file) in self.paths.iter().zip([&self.service, &self.state]) {
            let current = std::fs::symlink_metadata(path)?;
            let held = file.metadata()?;
            ensure!(
                current.is_file()
                    && current.nlink() == 1
                    && current.uid() == unsafe { libc::geteuid() }
                    && current.mode() & 0o7777 == 0o600
                    && current.dev() == held.dev()
                    && current.ino() == held.ino(),
                "Lifecycle lease changed during startup"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    fn directory() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }
    #[test]
    fn two_locks_exclude_competitor_and_remain_after_release() {
        let dir = directory();
        let root = dir.path().canonicalize().unwrap();
        let a = root.join("service.lock");
        let b = root.join("state.lock");
        let held = LifecycleLease::acquire(&a, &b).unwrap();
        assert!(LifecycleLease::acquire(&a, &b).is_err());
        held.recheck().unwrap();
        drop(held);
        assert!(a.is_file() && b.is_file());
        LifecycleLease::acquire(&a, &b).unwrap();
    }
    #[test]
    fn replacement_and_symlink_refused() {
        let dir = directory();
        let root = dir.path().canonicalize().unwrap();
        let a = root.join("service.lock");
        let b = root.join("state.lock");
        let held = LifecycleLease::acquire(&a, &b).unwrap();
        std::fs::rename(&a, root.join("old.lock")).unwrap();
        symlink(root.join("old.lock"), &a).unwrap();
        assert!(held.recheck().is_err());
        assert!(LifecycleLease::acquire(&a, &b).is_err());
    }
    #[test]
    fn second_lock_failure_releases_first() {
        let dir = directory();
        let root = dir.path().canonicalize().unwrap();
        let a = root.join("service.lock");
        let b = root.join("state.lock");
        let second = lock(&b).unwrap();
        assert!(LifecycleLease::acquire(&a, &b).is_err());
        let first = lock(&a).unwrap();
        drop(first);
        drop(second);
        LifecycleLease::acquire(&a, &b).unwrap();
    }
}
