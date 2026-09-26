//! Versioned inherited ownership admission. This is not release authorization.
//! FD 3/4 remain application pipes; FD 5/6 remain lifecycle leases.
use anyhow::{ensure, Context, Result};
#[cfg(not(target_os = "linux"))]
use anyhow::bail;
use std::{marker::PhantomData, rc::Rc, time::Duration};

pub const CONTROL_FD: i32 = 7;
pub const PARENT_FD: i32 = 8;
pub const FRAME_BYTES: usize = 128;
const MAGIC: &[u8; 8] = b"DYTOWN01";
const READY: &[u8; 8] = b"DYTRDY01";
const GO: &[u8; 8] = b"DYTGO001";

/// Parse one exact lower-case SHA-512 context. A context is a binding, not approval.
pub fn parse_context_sha512(text: &str) -> Result<[u8;64]> {
    ensure!(text.len() == 128 && text.bytes().all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v)),
        "Ownership context requires 128 lower-case hex characters");
    let out: [u8;64] = hex::decode(text)?.try_into().map_err(|_| anyhow::anyhow!("Context length differs"))?;
    ensure!(out.iter().any(|v| *v != 0), "Ownership context is zero"); Ok(out)
}

static CANCELLED: std::sync::OnceLock<std::sync::Arc<std::sync::atomic::AtomicBool>> = std::sync::OnceLock::new();
static HANDLERS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
pub fn cancellation_handle() -> std::sync::Arc<std::sync::atomic::AtomicBool> {
    CANCELLED.get_or_init(|| std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false))).clone()
}
extern "C" fn cancel(_: libc::c_int) {
    if let Some(flag) = CANCELLED.get() { flag.store(true, std::sync::atomic::Ordering::SeqCst); }
}
pub fn install_cancellation() -> Result<()> {
    cancellation_handle();
    let installed = HANDLERS.get_or_init(|| {
        [libc::SIGINT,libc::SIGTERM].iter().all(|signal| unsafe {
            libc::signal(*signal, cancel as *const () as libc::sighandler_t) != libc::SIG_ERR
        })
    });
    ensure!(*installed, "Cannot install ownership cancellation handlers"); Ok(())
}
/// Causal cancellation marker. A later signal flag cannot convert another error.
#[derive(Debug)]
pub struct OwnershipCancelled;
impl std::fmt::Display for OwnershipCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Ownership operation cancelled")
    }
}
impl std::error::Error for OwnershipCancelled {}
pub fn check_cancellation() -> Result<()> {
    if cancellation_handle().load(std::sync::atomic::Ordering::SeqCst) {
        return Err(OwnershipCancelled.into());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Role { Application = 1, Bridge = 2, Engine = 3, Adapter = 4, Helper = 5 }

/// One OS process-leader thread owns children until reaping. Never transferable.
pub struct OwnerThread { pid: u32, _not_send_sync: PhantomData<Rc<()>> }
impl OwnerThread {
    pub fn new() -> Result<Self> {
        let pid = process_id()?;
        ensure!(pid > 1 && thread_id()? == pid, "Child ownership requires a non-init process leader thread");
        Ok(Self { pid, _not_send_sync: PhantomData })
    }
    pub fn check(&self) -> Result<()> {
        ensure!(process_id()? == self.pid && thread_id()? == self.pid,
            "Creator thread identity changed");
        Ok(())
    }
    pub fn pid(&self) -> u32 { self.pid }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Frame {
    role: Role, owner_pid: u32, uid: u32, deadline_ns: u64,
    device: u64, inode: u64, context_sha512: [u8; 64],
}
impl Frame {
    fn encode(&self) -> [u8; FRAME_BYTES] {
        let mut b = [0; FRAME_BYTES];
        b[..8].copy_from_slice(MAGIC);
        b[8..12].copy_from_slice(&(self.role as u32).to_be_bytes());
        b[12..16].copy_from_slice(&self.owner_pid.to_be_bytes());
        b[16..20].copy_from_slice(&self.owner_pid.to_be_bytes());
        b[20..24].copy_from_slice(&self.uid.to_be_bytes());
        b[24..32].copy_from_slice(&self.deadline_ns.to_be_bytes());
        b[32..40].copy_from_slice(&self.device.to_be_bytes());
        b[40..48].copy_from_slice(&self.inode.to_be_bytes());
        b[48..112].copy_from_slice(&self.context_sha512);
        b
    }
    fn decode(b: &[u8], expected: Role) -> Result<Self> {
        ensure!(b.len() == FRAME_BYTES && &b[..8] == MAGIC, "Ownership frame differs");
        let word = |i| u32::from_be_bytes(b[i..i+4].try_into().unwrap());
        let wide = |i| u64::from_be_bytes(b[i..i+8].try_into().unwrap());
        let pid = word(12);
        ensure!(word(8) == expected as u32 && pid > 1 && pid <= i32::MAX as u32
            && word(16) == pid && wide(24) > 0 && wide(24) <= i64::MAX as u64
            && b[48..112].iter().any(|v| *v != 0) && b[112..].iter().all(|v| *v == 0), "Ownership fields differ");
        Ok(Self { role: expected, owner_pid: pid, uid: word(20), deadline_ns: wide(24),
            device: wide(32), inode: wide(40), context_sha512: b[48..112].try_into().unwrap() })
    }
}

#[cfg(target_os = "linux")]
fn process_id() -> Result<u32> { Ok(unsafe { libc::getpid() } as u32) }
#[cfg(target_os = "linux")]
fn thread_id() -> Result<u32> { Ok(unsafe { libc::syscall(libc::SYS_gettid) } as u32) }
#[cfg(not(target_os = "linux"))]
fn process_id() -> Result<u32> { bail!("Ownership admission requires Linux") }
#[cfg(not(target_os = "linux"))]
fn thread_id() -> Result<u32> { bail!("Ownership admission requires Linux") }

pub fn monotonic_now_ns() -> Result<u64> {
    #[cfg(target_os = "linux")]
    {
        let mut t: libc::timespec = unsafe { std::mem::zeroed() };
        ensure!(unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t) } == 0
            && t.tv_sec >= 0 && (0..1_000_000_000).contains(&t.tv_nsec), "Monotonic clock failed");
        (t.tv_sec as u64).checked_mul(1_000_000_000).and_then(|n| n.checked_add(t.tv_nsec as u64))
            .filter(|n| *n <= i64::MAX as u64).context("Monotonic clock overflow")
    }
    #[cfg(not(target_os = "linux"))]
    bail!("Ownership admission requires Linux")
}
pub fn monotonic_deadline(duration: Duration) -> Result<u64> {
    ensure!(!duration.is_zero(), "Ownership deadline is empty");
    monotonic_now_ns()?.checked_add(u64::try_from(duration.as_nanos())?)
        .filter(|n| *n <= i64::MAX as u64).context("Ownership deadline overflow")
}
fn remaining(deadline: u64) -> Result<u64> {
    let now = monotonic_now_ns()?;
    ensure!(now < deadline && deadline <= i64::MAX as u64, "Ownership admission deadline expired");
    Ok(deadline - now)
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{fs::File, io::Read, os::fd::{AsRawFd, FromRawFd, RawFd},
        os::unix::{fs::MetadataExt, process::CommandExt}, process::{Child, Command}};

    fn high(fd: RawFd) -> Result<File> {
        let out = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 9) };
        ensure!(out >= 9, "Cannot retain ownership descriptor");
        Ok(unsafe { File::from_raw_fd(out) })
    }
    fn flags(fd: RawFd) -> Result<()> {
        ensure!(unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == 0,
            "Cannot mark admission descriptor close-on-exec");
        Ok(())
    }
    fn socket_type(fd: RawFd) -> Result<()> {
        let mut value: libc::c_int = 0;
        let mut length = std::mem::size_of_val(&value) as libc::socklen_t;
        ensure!(unsafe { libc::getsockopt(fd, libc::SOL_SOCKET, libc::SO_TYPE,
            (&mut value as *mut libc::c_int).cast(), &mut length) } == 0
            && length as usize == std::mem::size_of_val(&value) && value == libc::SOCK_SEQPACKET,
            "Ownership control is not a seqpacket socket");
        for peer_address in [false, true] {
            let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
            let mut size = std::mem::size_of_val(&address) as libc::socklen_t;
            let result = unsafe {
                if peer_address { libc::getpeername(fd, (&mut address as *mut libc::sockaddr_un).cast(), &mut size) }
                else { libc::getsockname(fd, (&mut address as *mut libc::sockaddr_un).cast(), &mut size) }
            };
            ensure!(result == 0 && address.sun_family as i32 == libc::AF_UNIX
                && size as usize == std::mem::size_of::<libc::sa_family_t>(),
                "Ownership channel is not anonymous AF_UNIX");
        }
        Ok(())
    }
    fn peer(fd: RawFd, frame: &Frame) -> Result<()> {
        let mut value: libc::ucred = unsafe { std::mem::zeroed() };
        let mut length = std::mem::size_of_val(&value) as libc::socklen_t;
        ensure!(unsafe { libc::getsockopt(fd, libc::SOL_SOCKET, libc::SO_PEERCRED,
            (&mut value as *mut libc::ucred).cast(), &mut length) } == 0
            && length as usize == std::mem::size_of_val(&value)
            && value.pid == frame.owner_pid as i32 && value.uid == frame.uid,
            "Ownership control peer differs");
        Ok(())
    }
    fn recv(fd: RawFd, maximum: usize) -> Result<Option<Vec<u8>>> {
        let mut b = vec![0; maximum + 1];
        let n = unsafe { libc::recv(fd, b.as_mut_ptr().cast(), b.len(), libc::MSG_DONTWAIT | libc::MSG_TRUNC) };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            if matches!(e.raw_os_error(), Some(libc::EAGAIN) | Some(libc::EINTR)) { return Ok(None); }
            return Err(e.into());
        }
        ensure!(n > 0 && n as usize <= maximum, "Ownership packet truncated or closed");
        b.truncate(n as usize); Ok(Some(b))
    }
    fn send(fd: RawFd, b: &[u8]) -> Result<bool> {
        let n = unsafe { libc::send(fd, b.as_ptr().cast(), b.len(), libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL) };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            if matches!(e.raw_os_error(), Some(libc::EAGAIN) | Some(libc::EINTR)) { return Ok(false); }
            return Err(e.into());
        }
        ensure!(n as usize == b.len(), "Ownership packet write was partial"); Ok(true)
    }
    fn tick(deadline: u64) -> Result<()> {
        let left = remaining(deadline)?;
        std::thread::sleep(Duration::from_nanos(left.min(1_000_000))); Ok(())
    }
    fn signal_value() -> Result<i32> {
        let mut value: libc::c_int = 0;
        ensure!(unsafe { libc::prctl(libc::PR_GET_PDEATHSIG, &mut value) } == 0,
            "Cannot read parent-death signal"); Ok(value)
    }
    fn live(fd: RawFd, frame: &Frame) -> Result<()> {
        ensure!(unsafe { libc::getppid() } == frame.owner_pid as i32, "Immediate parent changed");
        let mut request = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        ensure!(unsafe { libc::poll(&mut request, 1, 0) } == 0 && request.revents == 0,
            "Parent descriptor is not live"); Ok(())
    }
    fn parent_identity(file: &File, frame: &Frame) -> Result<()> {
        let m = file.metadata()?;
        ensure!(m.dev() == frame.device && m.ino() == frame.inode,
            "Parent descriptor inode differs");
        let fd = file.as_raw_fd();
        ensure!(std::fs::read_link(format!("/proc/self/fd/{fd}"))?.as_os_str() == "anon_inode:[pidfd]",
            "Parent descriptor is not a pidfd");
        let mut bytes = Vec::new();
        File::open(format!("/proc/self/fdinfo/{fd}"))?.take(4097).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 4096, "Parent descriptor info exceeds bound");
        let info = std::str::from_utf8(&bytes)?;
        let ids: Vec<_> = info.lines().filter_map(|l| l.strip_prefix("Pid:\t")).collect();
        ensure!(ids.len() == 1 && ids[0] == frame.owner_pid.to_string(), "Parent descriptor PID differs");
        live(fd, frame)
    }

    /// The caller must keep this object and its owner thread alive through spawn.
    /// After READY, inspect the retained Child before release. A READY packet is
    /// not independent mapping, label, mount, or release authorization evidence.
    pub struct Bootstrap {
        owner: u32, frame: Frame, control: File, child_control: Option<File>, parent: File,
        spawned: bool, ready: bool, released: bool, _not_send_sync: PhantomData<Rc<()>>,
    }
    impl Bootstrap {
        pub fn prepare(owner: &OwnerThread, role: Role, context_sha512: [u8;64], deadline_ns: u64) -> Result<Self> {
            owner.check()?; remaining(deadline_ns)?;
            ensure!(context_sha512.iter().any(|v| *v != 0), "Ownership context is zero");
            let p = unsafe { libc::syscall(libc::SYS_pidfd_open, owner.pid, 0) };
            ensure!(p >= 0, "Cannot open creator pidfd");
            let low = unsafe { File::from_raw_fd(i32::try_from(p)?) };
            let parent = high(low.as_raw_fd())?; drop(low);
            let mut pair = [-1; 2];
            ensure!(unsafe { libc::socketpair(libc::AF_UNIX,
                libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK, 0, pair.as_mut_ptr()) } == 0,
                "Cannot create ownership channel");
            let left = unsafe { File::from_raw_fd(pair[0]) };
            let right = unsafe { File::from_raw_fd(pair[1]) };
            let control = high(left.as_raw_fd())?; let child_control = high(right.as_raw_fd())?;
            drop((left,right));
            let m = parent.metadata()?;
            let frame = Frame { role, owner_pid: owner.pid, uid: unsafe { libc::geteuid() },
                deadline_ns, device: m.dev(), inode: m.ino(), context_sha512 };
            ensure!(send(control.as_raw_fd(), &frame.encode())?, "Cannot queue ownership bootstrap");
            Ok(Self { owner: owner.pid, frame, control, child_control: Some(child_control), parent,
                spawned: false, ready: false, released: false, _not_send_sync: PhantomData })
        }
        fn check(&self) -> Result<()> {
            ensure!(process_id()? == self.owner && thread_id()? == self.owner,
                "Ownership channel moved from creator thread");
            remaining(self.frame.deadline_ns)?; Ok(())
        }
        pub fn spawn(&mut self, command: &mut Command) -> Result<Child> {
            self.check()?; ensure!(!self.spawned, "Ownership bootstrap cannot be reused");
            // Fill low gaps before Rust creates its exec-error channel. Hold exact
            // target inodes; pre_exec must not overwrite a substituted descriptor.
            let mut reserved = Vec::new();
            loop { let f = File::open("/dev/null")?; if f.as_raw_fd() > PARENT_FD { break; } reserved.push(f); }
            let mut targets = Vec::new();
            for fd in [CONTROL_FD, PARENT_FD] {
                let held = high(fd)?; let m = held.metadata()?;
                targets.push((fd, held, m.dev(), m.ino()));
            }
            let child = high(self.child_control.as_ref().context("Child channel already consumed")?.as_raw_fd())?;
            let parent = high(self.parent.as_raw_fd())?;
            let pid = self.owner;
            // Install after the caller's pre_exec close_range so FD 7/8 survive.
            unsafe { command.pre_exec(move || {
                let _reserved = &reserved;
                for (fd, _held, dev, ino) in &targets {
                    let mut st: libc::stat = std::mem::zeroed();
                    if libc::fstat(*fd, &mut st) != 0 { return Err(std::io::Error::last_os_error()); }
                    if st.st_dev != *dev || st.st_ino != *ino { return Err(std::io::Error::from_raw_os_error(libc::EBUSY)); }
                }
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 { return Err(std::io::Error::last_os_error()); }
                if libc::getppid() != pid as i32 { return Err(std::io::Error::from_raw_os_error(libc::ECHILD)); }
                if libc::dup2(child.as_raw_fd(), CONTROL_FD) != CONTROL_FD
                    || libc::dup2(parent.as_raw_fd(), PARENT_FD) != PARENT_FD { return Err(std::io::Error::last_os_error()); }
                Ok(())
            }); }
            self.spawned = true;
            let result = command.spawn().context("Cannot spawn owned admission child");
            self.child_control.take();
            result
        }
        pub fn await_ready(&mut self) -> Result<()> { self.await_ready_with_cancel(|| Ok(())) }
        pub fn await_ready_with_cancel(&mut self, cancelled: impl Fn() -> Result<()>) -> Result<()> {
            ensure!(self.spawned && !self.ready && !self.released, "Ownership readiness state differs");
            loop {
                self.check()?; cancelled()?;
                if let Some(raw) = recv(self.control.as_raw_fd(), FRAME_BYTES)? {
                    let mut expected = self.frame.encode(); expected[..8].copy_from_slice(READY);
                    ensure!(raw == expected, "Owned READY differs from queued bootstrap");
                    self.check()?; cancelled()?; self.ready = true; return Ok(());
                }
                tick(self.frame.deadline_ns)?;
            }
        }
        pub fn release(&mut self) -> Result<()> { self.release_with_cancel(|| Ok(())) }
        pub fn release_with_cancel(&mut self, cancelled: impl Fn() -> Result<()>) -> Result<()> {
            ensure!(self.ready && !self.released, "Ownership release requires one READY");
            loop {
                self.check()?; cancelled()?;
                if send(self.control.as_raw_fd(), GO)? {
                    self.released = true; self.check()?; cancelled()?; return Ok(());
                }
                tick(self.frame.deadline_ns)?;
            }
        }
    }

    /// Keep this token on the admitted OS thread until process exit. Its pidfd
    /// supports later owner checks; it does not grant signing or release authority.
    pub struct Admission {
        pub context_sha512: [u8; 64], pub deadline_ns: u64, pub owner_pid: u32,
        frame: Frame, parent: File, tid: u32, _not_send_sync: PhantomData<Rc<()>>,
    }
    impl Admission {
        pub fn verify_context(&self, expected: &[u8; 64]) -> Result<()> {
            ensure!(&self.context_sha512 == expected, "Ownership release context differs"); self.check()
        }
        /// Admission deadline bounds startup, not the admitted service lifetime.
        pub fn check(&self) -> Result<()> {
            ensure!(thread_id()? == self.tid, "Admission guard moved from its OS thread");
            ensure!(signal_value()? == libc::SIGKILL, "Parent-death signal changed");
            live(self.parent.as_raw_fd(), &self.frame)
        }
    }
    pub fn admit(role: Role) -> Result<Admission> {
        ensure!(thread_id()? == process_id()?, "Rust admission requires the process leader thread");
        // Own the inherited fixed descriptors exactly once. Any failure terminates
        // startup; neither descriptor may be reused for a second admission.
        ensure!(unsafe { libc::fcntl(CONTROL_FD, libc::F_GETFD) } >= 0
            && unsafe { libc::fcntl(PARENT_FD, libc::F_GETFD) } >= 0, "Ownership descriptors absent");
        let control = unsafe { File::from_raw_fd(CONTROL_FD) };
        let parent = unsafe { File::from_raw_fd(PARENT_FD) };
        flags(CONTROL_FD)?; flags(PARENT_FD)?; socket_type(CONTROL_FD)?;
        let raw = recv(CONTROL_FD, FRAME_BYTES)?.context("Queued ownership bootstrap absent")?;
        let frame = Frame::decode(&raw, role)?;
        remaining(frame.deadline_ns)?;
        ensure!(unsafe { libc::geteuid() } == frame.uid && unsafe { libc::getuid() } == frame.uid,
            "Ownership UID differs");
        ensure!(unsafe { libc::prctl(libc::PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0) } == 1
            && unsafe { libc::prctl(libc::PR_GET_SECCOMP, 0, 0, 0, 0) } == 2,
            "Ownership requires NNP and filter seccomp");
        peer(CONTROL_FD, &frame)?; parent_identity(&parent, &frame)?;
        ensure!(unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) } == 0,
            "Cannot establish parent-death signal");
        ensure!(signal_value()? == libc::SIGKILL, "Parent-death signal differs");
        live(PARENT_FD, &frame)?;
        let mut ready = raw; ready[..8].copy_from_slice(READY);
        loop { remaining(frame.deadline_ns)?; live(PARENT_FD, &frame)?;
            if send(CONTROL_FD, &ready)? { break; } tick(frame.deadline_ns)?; }
        loop {
            remaining(frame.deadline_ns)?; live(PARENT_FD, &frame)?;
            if let Some(go) = recv(CONTROL_FD, GO.len())? { ensure!(go == GO, "Ownership GO differs"); break; }
            tick(frame.deadline_ns)?;
        }
        remaining(frame.deadline_ns)?; live(PARENT_FD, &frame)?;
        ensure!(signal_value()? == libc::SIGKILL, "Parent-death signal changed before GO");
        drop(control);
        Ok(Admission { context_sha512: frame.context_sha512, deadline_ns: frame.deadline_ns,
            owner_pid: frame.owner_pid, frame, parent, tid: thread_id()?, _not_send_sync: PhantomData })
    }
}
#[cfg(target_os = "linux")]
pub use linux::{admit, Admission, Bootstrap};

#[cfg(not(target_os = "linux"))]
mod unsupported {
    use super::*;
    pub struct Bootstrap;
    impl Bootstrap {
        pub fn prepare(_: &OwnerThread, _: Role, _: [u8;64], _: u64) -> Result<Self> { bail!("Ownership requires Linux") }
        pub fn spawn(&mut self, _: &mut std::process::Command) -> Result<std::process::Child> { bail!("Ownership requires Linux") }
        pub fn await_ready(&mut self) -> Result<()> { bail!("Ownership requires Linux") }
        pub fn await_ready_with_cancel(&mut self, _: impl Fn() -> Result<()>) -> Result<()> { bail!("Ownership requires Linux") }
        pub fn release(&mut self) -> Result<()> { bail!("Ownership requires Linux") }
        pub fn release_with_cancel(&mut self, _: impl Fn() -> Result<()>) -> Result<()> { bail!("Ownership requires Linux") }
    }
    pub struct Admission { pub context_sha512: [u8;64], pub deadline_ns: u64, pub owner_pid: u32 }
    impl Admission {
        pub fn check(&self) -> Result<()> { bail!("Ownership requires Linux") }
        pub fn verify_context(&self, _: &[u8;64]) -> Result<()> { bail!("Ownership requires Linux") }
    }
    pub fn admit(_: Role) -> Result<Admission> { bail!("Ownership requires Linux") }
}
#[cfg(not(target_os = "linux"))]
pub use unsupported::{admit, Admission, Bootstrap};

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Frame { Frame { role: Role::Helper, owner_pid: 123, uid: 456,
        deadline_ns: 1234567, device: 8, inode: 99, context_sha512: [7;64] } }
    #[test]
    fn exact_wire_round_trip_and_role_binding() {
        let f=sample(); let b=f.encode(); assert_eq!(Frame::decode(&b,Role::Helper).unwrap(),f);
        assert!(Frame::decode(&b,Role::Application).is_err());
        assert_eq!(&b[24..32], &1234567u64.to_be_bytes());
    }
    #[test]
    fn malformed_frames_fail_closed() {
        let good=sample().encode();
        for length in [0,8,127,129] { let mut b=good.to_vec(); b.resize(length,0); assert!(Frame::decode(&b,Role::Helper).is_err()); }
        for index in [0,8,16,112,127] { let mut b=good; b[index]^=1; assert!(Frame::decode(&b,Role::Helper).is_err(),"index {index}"); }
        for pid in [0u32,1,i32::MAX as u32+1] { let mut b=good; b[12..16].copy_from_slice(&pid.to_be_bytes()); b[16..20].copy_from_slice(&pid.to_be_bytes()); assert!(Frame::decode(&b,Role::Helper).is_err()); }
        for deadline in [0u64,u64::MAX] { let mut b=good; b[24..32].copy_from_slice(&deadline.to_be_bytes()); assert!(Frame::decode(&b,Role::Helper).is_err()); }
    }
    #[cfg(target_os="linux")]
    #[test]
    fn worker_thread_cannot_be_an_owner() { assert!(std::thread::spawn(|| OwnerThread::new().is_err()).join().unwrap()); }
}
