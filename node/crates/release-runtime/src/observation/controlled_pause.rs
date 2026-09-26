//! Bounded retained-child pause, observation and continuation.
//! Callers must separately qualify exact signal isolation and explicit budgets.
use anyhow::{bail, ensure, Result};
use serde::Serialize;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Phase {
    Admission,
    StopRequest,
    StopWait,
    Observation,
    StoppedCheck,
    ContinueRequest,
    ContinueWait,
    RunningCheck,
    Complete,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    None,
    Stopped,
    Continued,
    Exited,
    Traced,
}
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    pub total: Duration,
    pub cleanup_reserve: Duration,
}
impl Budget {
    pub fn validate(self) -> Result<()> {
        self.work_limit().map(|_| ())
    }
    fn work_limit(self) -> Result<Duration> {
        ensure!(
            self.total <= Duration::from_secs(60)
                && !self.cleanup_reserve.is_zero()
                && self.cleanup_reserve < self.total,
            "Invalid qualification pause budget"
        );
        Ok(self.total - self.cleanup_reserve)
    }
}
#[derive(Debug)]
pub struct PauseCancelled(pub Phase);
impl std::fmt::Display for PauseCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Owned observation cancelled at {:?}", self.0)
    }
}
impl std::error::Error for PauseCancelled {}
#[derive(Debug)]
pub struct PauseCleanupFailure(pub anyhow::Error);
impl std::fmt::Display for PauseCleanupFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Owned observation cleanup failure: {:#}", self.0)
    }
}
impl std::error::Error for PauseCleanupFailure {}
pub fn is_clean_cancellation(error: &anyhow::Error) -> bool {
    error.is::<PauseCancelled>() && !error.is::<PauseCleanupFailure>()
}
#[derive(Clone, Copy)]
struct Admission {
    parent_matches: bool,
    role_matches: bool,
    member_matches: bool,
    executable: bool,
    digest_matches: bool,
    traced: bool,
    stopped: bool,
}
fn validate_admission(a: Admission) -> Result<()> {
    ensure!(a.parent_matches, "Owned child parent mismatch");
    ensure!(
        a.role_matches && a.member_matches && a.executable && a.digest_matches,
        "Retained candidate binding mismatch"
    );
    ensure!(!a.traced, "Traced child refused");
    ensure!(!a.stopped, "Pre-stopped child refused");
    Ok(())
}
#[derive(Debug, Serialize)]
pub struct Timing {
    pub total_micros: u128,
    pub stop_micros: u128,
    pub observation_micros: u128,
    pub release_micros: u128,
    pub confirmed_stop_to_final_check_micros: u128,
    pub pause_envelope_micros: u128,
}
trait Backend {
    type Observation;
    fn elapsed(&self) -> Duration;
    fn cancelled(&mut self, phase: Phase) -> bool;
    fn reject_terminal(&mut self) -> Result<()>;
    fn admission(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn event(&mut self) -> Result<Event>;
    fn idle(&mut self, remaining: Duration);
    fn observe(&mut self, remaining: Duration) -> Result<Self::Observation>;
    fn stopped(&mut self) -> Result<()>;
    fn resume(&mut self) -> Result<()>;
    fn running(&mut self) -> Result<()>;
    fn cleanup(&mut self, total: Duration) -> Result<()>;
}
fn checkpoint<B: Backend>(b: &mut B, phase: Phase, limit: Duration) -> Result<()> {
    if b.cancelled(phase) {
        // An already observable child failure must not become a graceful stop.
        b.reject_terminal()?;
        return Err(PauseCancelled(phase).into());
    }
    ensure!(
        b.elapsed() < limit,
        "Qualification pause deadline at {phase:?}"
    );
    Ok(())
}
fn wait<B: Backend>(b: &mut B, phase: Phase, want: Event, limit: Duration) -> Result<()> {
    loop {
        checkpoint(b, phase, limit)?;
        match b.event()? {
            Event::None => b.idle(limit.saturating_sub(b.elapsed())),
            event if event == want => return Ok(()),
            other => bail!("Unexpected owned-child event {other:?} at {phase:?}"),
        }
    }
}
struct CleanupGuard<'a, B: Backend> {
    backend: &'a mut B,
    total: Duration,
    armed: bool,
}
impl<B: Backend> CleanupGuard<'_, B> {
    fn cleanup(&mut self) -> Result<()> {
        self.armed = false; // One terminal cleanup attempt, including failure.
        self.backend.cleanup(self.total)
    }
}
impl<B: Backend> Drop for CleanupGuard<'_, B> {
    fn drop(&mut self) {
        if self.armed {
            // Unwind cannot report success. Never send CONT from Drop. The same
            // retained owner performs a bounded terminal cleanup attempt.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.cleanup()));
        }
    }
}
fn execute<B: Backend>(b: &mut B, budget: Budget) -> Result<(B::Observation, Timing)> {
    let limit = budget.work_limit()?;
    let mut guard = CleanupGuard {
        backend: b,
        total: budget.total,
        armed: false,
    };
    let result = (|| {
        checkpoint(guard.backend, Phase::Admission, limit)?;
        guard.backend.admission()?;
        checkpoint(guard.backend, Phase::StopRequest, limit)?;
        // A failed syscall can have an uncertain outcome. Retained-child cleanup
        // becomes mandatory before attempting the first signal.
        let stop_at = guard.backend.elapsed();
        guard.armed = true;
        guard.backend.stop()?;
        wait(guard.backend, Phase::StopWait, Event::Stopped, limit)?;
        let stopped_at = guard.backend.elapsed();
        let stop_micros = stopped_at.saturating_sub(stop_at).as_micros();
        checkpoint(guard.backend, Phase::Observation, limit)?;
        guard.backend.stopped()?;
        let observation_at = guard.backend.elapsed();
        let snapshot = guard
            .backend
            .observe(limit.saturating_sub(guard.backend.elapsed()))?;
        let observation_micros = guard
            .backend
            .elapsed()
            .saturating_sub(observation_at)
            .as_micros();
        checkpoint(guard.backend, Phase::StoppedCheck, limit)?;
        ensure!(
            guard.backend.event()? == Event::None,
            "Unexpected event during observation"
        );
        guard.backend.stopped()?;
        checkpoint(guard.backend, Phase::ContinueRequest, limit)?;
        let release_at = guard.backend.elapsed();
        guard.backend.resume()?;
        wait(guard.backend, Phase::ContinueWait, Event::Continued, limit)?;
        checkpoint(guard.backend, Phase::RunningCheck, limit)?;
        guard.backend.running()?;
        checkpoint(guard.backend, Phase::Complete, limit)?;
        Ok((
            snapshot,
            Timing {
                total_micros: guard.backend.elapsed().as_micros(),
                stop_micros,
                observation_micros,
                release_micros: guard
                    .backend
                    .elapsed()
                    .saturating_sub(release_at)
                    .as_micros(),
                confirmed_stop_to_final_check_micros: guard
                    .backend
                    .elapsed()
                    .saturating_sub(stopped_at)
                    .as_micros(),
                pause_envelope_micros: guard.backend.elapsed().saturating_sub(stop_at).as_micros(),
            },
        ))
    })();
    match result {
        Ok(value) => {
            guard.armed = false;
            Ok(value)
        }
        Err(primary) if guard.armed => {
            let cleanup = guard.cleanup();
            match cleanup {
                Ok(()) => Err(primary.context(
                    "Qualification observation refused; owned child terminated and reaped",
                )),
                Err(cleanup) => Err(primary.context(PauseCleanupFailure(cleanup))),
            }
        }
        Err(error) => Err(error), // No STOP was requested. Do not undo another actor's stop.
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::super::{
        observe_owned_child, proc_path, read_bounded, require_child, static_helper, Bounds,
        OwnedProcessIdentity, Snapshot,
    };
    use super::*;
    use crate::component_candidate::VerifiedMemberFiles;
    use anyhow::Context;
    use std::{
        os::fd::AsRawFd,
        process::Child,
        sync::atomic::{AtomicBool, Ordering},
        time::Instant,
    };

    enum Target<'a> {
        Member { role: &'a str, catalog: &'a VerifiedMemberFiles },
        StaticHelper { identity: &'a static_helper::OwnedStaticHelperIdentity,
                       file: &'a static_helper::VerifiedStaticHelperFile },
    }
    enum OwnedObservation {
        Member(Snapshot),
        StaticHelper(static_helper::StaticHelperSnapshot),
    }
    enum Cancellation<'a> {
        Flag(&'a AtomicBool),
        Check(fn() -> Result<()>),
    }
    struct Owned<'a> {
        child: &'a mut Child,
        identity: &'a OwnedProcessIdentity,
        target: Target<'a>,
        bounds: &'a Bounds,
        cancellation: Cancellation<'a>,
        started: Instant,
        cleanup_deadline: Instant,
    }
    impl Owned<'_> {
        fn signal(&self, signal: i32) -> Result<()> {
            // The opaque retained pidfd is the only signal target. There is no
            // numeric-PID or process-group fallback.
            let rc = unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    self.identity.pidfd.as_raw_fd(),
                    signal,
                    std::ptr::null::<libc::siginfo_t>(),
                    0,
                )
            };
            ensure!(
                rc == 0,
                "Retained pidfd signal failed: {}",
                std::io::Error::last_os_error()
            );
            Ok(())
        }
        fn state(&mut self) -> Result<u8> {
            require_child(self.child, self.identity, self.bounds)?;
            let raw = read_bounded(
                &proc_path(self.child.id(), "status"),
                self.bounds.max_stat_bytes,
            )?;
            let text = std::str::from_utf8(&raw)?;
            let field = |name: &str| -> Result<&str> {
                text.lines()
                    .find_map(|line| line.strip_prefix(name))
                    .map(str::trim)
                    .context("Missing process status field")
            };
            ensure!(
                field("TracerPid:")?.parse::<u32>()? == 0,
                "Traced child refused"
            );
            ensure!(
                field("PPid:")?.parse::<u32>()? == std::process::id(),
                "Owned child parent mismatch"
            );
            let state = *field("State:")?
                .as_bytes()
                .first()
                .context("Missing state")?;
            ensure!(
                state != b't' && state != b'Z' && state != b'X',
                "Invalid owned process state"
            );
            Ok(state)
        }
    }
    impl Backend for Owned<'_> {
        type Observation = OwnedObservation;
        fn elapsed(&self) -> Duration {
            self.started.elapsed()
        }
        fn cancelled(&mut self, _: Phase) -> bool {
            match self.cancellation {
                Cancellation::Flag(flag) => flag.load(Ordering::SeqCst),
                Cancellation::Check(check) => check().is_err(),
            }
        }
        fn reject_terminal(&mut self) -> Result<()> {
            ensure!(self.child.try_wait()?.is_none(),
                "Owned child exited before cancellation classification");
            Ok(())
        }
        fn admission(&mut self) -> Result<()> {
            self.bounds.validate()?;
            match &self.target {
                Target::Member { role, catalog } => {
                    let member = catalog.candidate().member_for_role(role).context("Unknown role")?;
                    let digest = catalog.role_file(role).context("Missing role file")?.digest();
                    validate_admission(Admission {
                        parent_matches: true,
                        role_matches: self.identity.role == *role,
                        member_matches: member.id == self.identity.member_id,
                        executable: member.kind == crate::component_candidate::MemberKind::Executable,
                        digest_matches: digest == &self.identity.expected,
                        traced: false,
                        stopped: false,
                    })?;
                }
                Target::StaticHelper { identity, file } => {
                    static_helper::validate_owned_identity(identity, file)?;
                    validate_admission(Admission {
                        parent_matches: true,
                        role_matches: self.identity.role == "independent_static_helper",
                        member_matches: self.identity.member_id == "independent_static_helper",
                        executable: true,
                        digest_matches: file.digest() == &self.identity.expected,
                        traced: false,
                        stopped: false,
                    })?;
                }
            }
            ensure!(self.state()? != b'T', "Pre-stopped child refused");
            ensure!(
                self.event()? == Event::None,
                "Pre-existing stop/continue event refused"
            );
            Ok(())
        }
        fn stop(&mut self) -> Result<()> {
            self.signal(libc::SIGSTOP)
        }
        fn event(&mut self) -> Result<Event> {
            // This exclusive mutable owner consumes STOP/CONT events. Exit is
            // reaped only through its retained Child handle, including observer checks.
            if self.child.try_wait()?.is_some() {
                return Ok(Event::Exited);
            }
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let rc = unsafe {
                libc::waitid(
                    libc::P_PIDFD,
                    self.identity.pidfd.as_raw_fd() as libc::id_t,
                    &mut info,
                    libc::WSTOPPED | libc::WCONTINUED | libc::WNOHANG,
                )
            };
            ensure!(
                rc == 0,
                "Retained pidfd event ownership failed: {}",
                std::io::Error::last_os_error()
            );
            if unsafe { info.si_pid() } == 0 {
                return Ok(Event::None);
            }
            ensure!(
                unsafe { info.si_pid() } as u32 == self.child.id(),
                "Wait event child mismatch"
            );
            Ok(match info.si_code {
                libc::CLD_STOPPED if unsafe { info.si_status() } == libc::SIGSTOP => Event::Stopped,
                libc::CLD_CONTINUED => Event::Continued,
                libc::CLD_TRAPPED | libc::CLD_STOPPED => Event::Traced,
                _ => Event::Exited,
            })
        }
        fn idle(&mut self, remaining: Duration) {
            std::thread::sleep(remaining.min(Duration::from_millis(1)));
        }
        fn observe(&mut self, remaining: Duration) -> Result<OwnedObservation> {
            ensure!(!remaining.is_zero(), "Observation budget exhausted");
            let mut bounds = self.bounds.clone();
            bounds.max_elapsed = bounds.max_elapsed.min(remaining);
            match &self.target {
                Target::Member { role, catalog } =>
                    observe_owned_child(self.child, self.identity, role, catalog, &bounds)
                        .map(OwnedObservation::Member),
                Target::StaticHelper { identity, file } =>
                    static_helper::observe_owned_child(self.child, identity, file, &bounds)
                        .map(OwnedObservation::StaticHelper),
            }
        }
        fn stopped(&mut self) -> Result<()> {
            ensure!(self.state()? == b'T', "Owned stop no longer held");
            Ok(())
        }
        fn resume(&mut self) -> Result<()> {
            self.signal(libc::SIGCONT)
        }
        fn running(&mut self) -> Result<()> {
            ensure!(self.state()? != b'T', "Owned continuation not complete");
            ensure!(
                self.event()? == Event::None,
                "Unexpected event after continuation"
            );
            Ok(())
        }
        fn cleanup(&mut self, _total: Duration) -> Result<()> {
            // The deadline is never reset. Even after budget expiry send the
            // retained kill once; do not report success without observed reaping.
            let deadline = self.cleanup_deadline;
            ensure!(self.child.try_wait()?.is_none(),
                "Owned child exited before terminal cleanup signal");
            self.signal(libc::SIGKILL)?;
            loop {
                if self.child.try_wait()?.is_some() {
                    return Ok(());
                }
                ensure!(
                    Instant::now() < deadline,
                    "Owned cleanup deadline; reaping unconfirmed"
                );
                std::thread::sleep(
                    Duration::from_millis(1)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
            }
        }
    }
    pub fn observe_paused(
        child: &mut Child,
        identity: &OwnedProcessIdentity,
        role: &str,
        catalog: &VerifiedMemberFiles,
        bounds: &Bounds,
        budget: Budget,
        cancelled: &AtomicBool,
    ) -> Result<(Snapshot, Timing)> {
        budget.work_limit()?;
        bounds.validate()?;
        let started = Instant::now();
        let cleanup_deadline = started
            .checked_add(budget.total)
            .context("Unrepresentable qualification deadline")?;
        let mut owner = Owned {
            child, identity, target: Target::Member { role, catalog }, bounds,
            cancellation: Cancellation::Flag(cancelled), started, cleanup_deadline,
        };
        let (snapshot, timing) = execute(&mut owner, budget)?;
        match snapshot {
            OwnedObservation::Member(snapshot) => Ok((snapshot, timing)),
            OwnedObservation::StaticHelper(_) => bail!("Unexpected static helper observation"),
        }
    }
    pub fn observe_paused_static_helper(
        child: &mut Child,
        identity: &static_helper::OwnedStaticHelperIdentity,
        file: &static_helper::VerifiedStaticHelperFile,
        bounds: &Bounds,
        budget: Budget,
        cancel_check: fn() -> Result<()>,
    ) -> Result<(static_helper::StaticHelperSnapshot, Timing)> {
        budget.work_limit()?;
        bounds.validate()?;
        let started = Instant::now();
        let cleanup_deadline = started.checked_add(budget.total)
            .context("Unrepresentable helper pause deadline")?;
        let mut owner = Owned {
            child, identity: identity.owned_identity(),
            target: Target::StaticHelper { identity, file }, bounds,
            cancellation: Cancellation::Check(cancel_check), started, cleanup_deadline,
        };
        let (snapshot, timing) = execute(&mut owner, budget)?;
        match snapshot {
            OwnedObservation::StaticHelper(snapshot) => Ok((snapshot, timing)),
            OwnedObservation::Member(_) => bail!("Unexpected member observation"),
        }
    }
}
#[cfg(target_os = "linux")]
pub use linux::{observe_paused, observe_paused_static_helper};
#[cfg(not(target_os = "linux"))]
pub fn observe_paused(
    _: &mut std::process::Child,
    _: &super::OwnedProcessIdentity,
    _: &str,
    _: &crate::component_candidate::VerifiedMemberFiles,
    _: &super::Bounds,
    _: Budget,
    _: &std::sync::atomic::AtomicBool,
) -> Result<(super::Snapshot, Timing)> {
    bail!("Controlled process observation requires Linux")
}
#[cfg(not(target_os = "linux"))]
pub fn observe_paused_static_helper(
    _: &mut std::process::Child,
    _: &super::static_helper::OwnedStaticHelperIdentity,
    _: &super::static_helper::VerifiedStaticHelperFile,
    _: &super::Bounds,
    _: Budget,
    _: fn() -> Result<()>,
) -> Result<(super::static_helper::StaticHelperSnapshot, Timing)> {
    bail!("Controlled static helper observation requires Linux")
}
#[cfg(test)]
#[path = "controlled_pause_tests.rs"]
mod tests;
