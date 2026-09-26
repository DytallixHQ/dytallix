//! Linux ownership of native service children. Observations are snapshots, not
//! release authority, protocol readiness, or continuous mapping enforcement.
use anyhow::{bail, ensure, Context, Result};
use crate::startup_diagnostic::{phase, Stage, OwnedChildExit, ExitObservation, EndpointViolation};
use dytallix_release_runtime::{component_candidate::VerifiedMemberFiles, observation, ownership};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

pub type ExplicitEnvironment = BTreeMap<String, String>;

/// Intentional signal or caller cancellation. This is distinct from a child,
/// observation, or cleanup failure, even if they occur after a signal arrives.
#[derive(Debug)]
pub struct OperationCancelled;
impl std::fmt::Display for OperationCancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Owned process operation cancelled")
    }
}
impl std::error::Error for OperationCancelled {}

#[derive(Debug)]
struct CleanupFailure {
    phase: &'static str,
    error: anyhow::Error,
}
impl std::fmt::Display for CleanupFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} cleanup failed: {:#}", self.phase, self.error)
    }
}
impl std::error::Error for CleanupFailure {}

/// Preserve cleanup failure across repeated cleanup calls. A later successful
/// stop cannot convert an earlier failed cleanup into graceful cancellation.
pub fn with_owned_cleanup<T>(
    operation: Result<T>,
    cleanup: Result<()>,
    phase: &'static str,
) -> Result<T> {
    match (operation, cleanup) {
        (result, Ok(())) => result,
        (Ok(_), Err(error)) => Err(CleanupFailure { phase, error }.into()),
        (Err(operation), Err(error)) => Err(operation.context(CleanupFailure { phase, error })),
    }
}
pub fn has_cleanup_failure(error: &anyhow::Error) -> bool { error.is::<CleanupFailure>() }
pub fn is_graceful_cancellation(error: &anyhow::Error) -> bool {
    error.is::<OperationCancelled>() && !error.is::<CleanupFailure>()
}

#[derive(Clone, Debug)]
pub struct ProcessBounds {
    pub startup_timeout: Duration,
    pub stop_timeout: Duration,
    pub kill_timeout: Duration,
    pub poll_interval: Duration,
    pub max_argument_bytes: usize,
    pub max_environment_bytes: usize,
    pub max_probe_request_bytes: usize,
    pub max_probe_response_bytes: usize,
}
impl ProcessBounds {
    fn validate(&self) -> Result<()> {
        ensure!(
            [
                self.startup_timeout,
                self.stop_timeout,
                self.kill_timeout,
                self.poll_interval
            ]
            .iter()
            .all(|v| !v.is_zero() && *v <= Duration::from_secs(60)),
            "Process time bounds must be explicit and within 60 seconds"
        );
        ensure!(
            self.poll_interval <= self.startup_timeout
                && self.poll_interval <= self.stop_timeout
                && self.poll_interval <= self.kill_timeout,
            "Poll interval exceeds process deadlines"
        );
        ensure!(
            [
                self.max_argument_bytes,
                self.max_environment_bytes,
                self.max_probe_request_bytes,
                self.max_probe_response_bytes
            ]
            .iter()
            .all(|v| *v > 0),
            "Process byte bounds must be explicit and nonzero"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Application,
    Bridge,
    Engine,
    Adapter,
}
impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Application => "consensus_stdio",
            Self::Bridge => "consensus_bridge",
            Self::Engine => "consensus_engine",
            Self::Adapter => "http_adapter",
        }
    }
}

/// Fixed-size summary of successful controlled observations. No samples grow in memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
struct TimingValues {
    total_micros: u64,
    stop_micros: u64,
    observation_micros: u64,
    release_micros: u64,
    confirmed_stop_to_final_check_micros: u64,
    pause_envelope_micros: u64,
}
impl TimingValues {
    fn from_timing(value: &observation::controlled_pause::Timing) -> Result<Self> {
        Ok(Self {
            total_micros: u64::try_from(value.total_micros)?,
            stop_micros: u64::try_from(value.stop_micros)?,
            observation_micros: u64::try_from(value.observation_micros)?,
            release_micros: u64::try_from(value.release_micros)?,
            confirmed_stop_to_final_check_micros: u64::try_from(value.confirmed_stop_to_final_check_micros)?,
            pause_envelope_micros: u64::try_from(value.pause_envelope_micros)?,
        })
    }
    fn maximum(self, other: Self) -> Self {
        Self {
            total_micros: self.total_micros.max(other.total_micros),
            stop_micros: self.stop_micros.max(other.stop_micros),
            observation_micros: self.observation_micros.max(other.observation_micros),
            release_micros: self.release_micros.max(other.release_micros),
            confirmed_stop_to_final_check_micros: self.confirmed_stop_to_final_check_micros.max(other.confirmed_stop_to_final_check_micros),
            pause_envelope_micros: self.pause_envelope_micros.max(other.pause_envelope_micros),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
struct RoleObservationTiming {
    role: &'static str,
    count: u64,
    latest: Option<TimingValues>,
    maximum: Option<TimingValues>,
}
impl RoleObservationTiming {
    fn new(role: Role) -> Self { Self { role: role.as_str(), count: 0, latest: None, maximum: None } }
    fn record(&mut self, timing: &observation::controlled_pause::Timing) -> Result<()> {
        let value = TimingValues::from_timing(timing)?;
        let count = self.count.checked_add(1).context("Observation timing count overflow")?;
        let maximum = self.maximum.map(|old| old.maximum(value)).unwrap_or(value);
        // Commit only after every fallible conversion and addition succeeds.
        self.count = count; self.latest = Some(value); self.maximum = Some(maximum);
        Ok(())
    }
}

struct OwnedChild {
    role: Role,
    child: Child,
    identity: Option<observation::OwnedProcessIdentity>,
    snapshot: Option<observation::Snapshot>,
}
struct ApplicationPipes {
    input: File,
    output: File,
}
struct OwnedRoleSecurity {
    application: dytallix_release_runtime::ownership_security::SecurityState,
    workload: dytallix_release_runtime::ownership_security::SecurityState,
}
impl OwnedRoleSecurity {
    fn for_role(&self, role: Role) -> &dytallix_release_runtime::ownership_security::SecurityState {
        match role {
            Role::Application => &self.application,
            Role::Bridge | Role::Engine | Role::Adapter => &self.workload,
        }
    }
}

/// The owner receives already verified catalog objects. It never decodes a
/// self-reported identity or reconstructs a Child from a PID.
pub struct ProcessOwner {
    creator: ownership::OwnerThread,
    admission_security: Option<OwnedRoleSecurity>,
    catalog: VerifiedMemberFiles,
    observation_bounds: observation::Bounds,
    observation_pause: Option<observation::controlled_pause::Budget>,
    observation_timings: [RoleObservationTiming; 4],
    first_failure_snapshot: Option<serde_json::Value>,
    bounds: ProcessBounds,
    environment: ExplicitEnvironment,
    children: Vec<OwnedChild>,
    pipes: Option<ApplicationPipes>,
    leases: Option<(OwnedFd, OwnedFd)>,
    application_probed: bool,
    cancelled: Arc<AtomicBool>,
    failed: bool,
    stopped: bool,
}
impl ProcessOwner {
    pub fn new(
        catalog: VerifiedMemberFiles,
        observation_bounds: observation::Bounds,
        bounds: ProcessBounds,
        environment: ExplicitEnvironment,
    ) -> Result<Self> {
        ensure!(
            cfg!(target_os = "linux"),
            "Native process ownership requires Linux"
        );
        bounds.validate()?;
        ensure!(
            [
                observation_bounds.max_stat_bytes,
                observation_bounds.max_maps_bytes,
                observation_bounds.max_map_entries,
                observation_bounds.max_path_bytes,
                observation_bounds.max_unique_files
            ]
            .iter()
            .all(|value| *value > 0)
                && observation_bounds.max_file_bytes > 0
                && observation_bounds.max_total_file_bytes > 0
                && !observation_bounds.max_elapsed.is_zero()
                && observation_bounds.max_elapsed <= Duration::from_secs(60)
                && !observation_bounds.allowed_owner_uids.is_empty(),
            "Observation bounds must be explicit before child creation"
        );
        validate_environment(&environment, bounds.max_environment_bytes)?;
        ensure!(
            matches!(
                catalog.candidate().manifest().service_profile.as_str(),
                "development-linux-native-service-v2" | "development-linux-native-service-http-v2"
            ),
            "Native owner requires an explicit native development catalog"
        );
        for role in [Role::Application, Role::Bridge, Role::Engine] {
            ensure!(
                catalog.role_file(role.as_str()).is_some(),
                "Required native role is absent"
            );
        }
        Ok(Self {
            creator: ownership::OwnerThread::new()?,
            admission_security: None,
            catalog,
            observation_bounds,
            observation_pause: None,
            observation_timings: [Role::Application, Role::Bridge, Role::Engine, Role::Adapter].map(RoleObservationTiming::new),
            bounds,
            environment,
            children: Vec::new(),
            pipes: None,
            leases: None,
            application_probed: false,
            cancelled: ownership::cancellation_handle(),
            first_failure_snapshot: None,
            failed: false,
            stopped: false,
        })
    }
    /// Bind exact role security state before any child is created.
    pub fn set_admission_security(&mut self,
        application: dytallix_release_runtime::ownership_security::SecurityState,
        workload: dytallix_release_runtime::ownership_security::SecurityState) -> Result<()> {
        self.active()?;
        ensure!(self.children.is_empty() && self.admission_security.is_none(),
            "Admission security must be bound once before startup");
        ensure!(application.uid == workload.uid && application.gid == workload.gid
            && application.mount_namespace == workload.mount_namespace
            && application.apparmor_label != workload.apparmor_label,
            "Distinct same-unit application and workload security required");
        self.admission_security = Some(OwnedRoleSecurity { application, workload });
        Ok(())
    }
    /// Bind an explicit reviewed pause budget once, before any child starts.
    pub fn set_observation_pause(&mut self, total: Duration, cleanup_reserve: Duration) -> Result<()> {
        self.active()?;
        ensure!(self.children.is_empty() && self.observation_pause.is_none(),
            "Observation pause must be bound once before startup");
        let budget = observation::controlled_pause::Budget { total, cleanup_reserve };
        budget.validate()?;
        self.observation_pause = Some(budget);
        Ok(())
    }
    /// Retain independent descriptors for the caller's actual flock files.
    /// No pathname or integer descriptor can substitute for these file handles.
    pub fn attach_lifecycle_leases(&mut self, service: &File, state: &File) -> Result<()> {
        let result = (|| {
            self.active()?;
            ensure!(
                self.children.is_empty() && self.leases.is_none(),
                "Attach leases exactly once before any child"
            );
            let mut identities = Vec::new();
            for file in [service, state] {
                let metadata = file.metadata()?;
                ensure!(
                    metadata.is_file()
                        && metadata.uid() == unsafe { libc::geteuid() }
                        && metadata.mode() & 0o7777 == 0o600
                        && metadata.nlink() == 1,
                    "Lifecycle lease requires a private owned regular file"
                );
                ensure!(
                    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
                    "Lifecycle lease is already held elsewhere"
                );
                identities.push((metadata.dev(), metadata.ino()));
            }
            ensure!(
                identities[0] != identities[1],
                "Lifecycle leases must use distinct files"
            );
            self.leases = Some((
                duplicate(service.as_raw_fd())?,
                duplicate(state.as_raw_fd())?,
            ));
            Ok(())
        })();
        self.finish(result)
    }
    /// A signal handler or another thread may request cancellation. Blocking
    /// probe and readiness loops check the flag before the next I/O attempt.
    pub fn cancellation_handle(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }
    pub fn observation_timings(&self) -> serde_json::Value {
        serde_json::json!({
            "scope":"SUCCESSFUL_CONTROLLED_OBSERVATIONS_FIXED_FOUR_ROLES",
            "roles":self.observation_timings,
            "configured_budget":self.observation_pause.map(|budget| serde_json::json!({
                "total_micros":budget.total.as_micros(),
                "cleanup_reserve_micros":budget.cleanup_reserve.as_micros(),
            })),
            "production_budget_approved":false,
            "failure_timings_included":false,
        })
    }
    pub fn snapshots(&self) -> Vec<&observation::Snapshot> {
        self.children
            .iter()
            .filter_map(|c| c.snapshot.as_ref())
            .collect()
    }
    /// PID values are report metadata only. They cannot be used to adopt a child.
    pub fn owned_pids(&self) -> Vec<(Role, u32)> {
        self.children
            .iter()
            .map(|c| (c.role, c.child.id()))
            .collect()
    }
    fn active(&self) -> Result<()> {
        self.creator.check()?;
        ensure!(!self.failed && !self.stopped, "Process owner is terminal");
        // A detected child failure takes priority over concurrent cancellation.
        for child in &self.children {
            if let Some(observation) = exit_observation(&child.child)? {
                return Err(OwnedChildExit { role: child.role, pid: child.child.id(), observation }.into());
            }
        }
        if self.cancelled.load(Ordering::SeqCst) {
            return Err(OperationCancelled.into());
        }
        Ok(())
    }
    /// Capture retained children before the first failure cleanup. Never wait or reap.
    pub fn capture_failure(&mut self) { capture_first_failure(&mut self.first_failure_snapshot, &self.children); }
    pub fn first_failure_snapshot(&self) -> Option<&serde_json::Value> { self.first_failure_snapshot.as_ref() }
    fn finish<T>(&mut self, result: Result<T>) -> Result<T> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                self.capture_failure();
                self.failed = true;
                with_owned_cleanup(Err(error), self.shutdown(), "Owned process")
            }
        }
    }
    fn spawn(
        &mut self,
        role: Role,
        args: &[OsString],
        stdin: Stdio,
        stdout: Stdio,
        bridge_fds: Option<(RawFd, RawFd)>,
    ) -> Result<()> {
        self.active()?;
        ensure!(
            !self.children.iter().any(|c| c.role == role),
            "Native role already started"
        );
        validate_arguments(args, self.bounds.max_argument_bytes)?;
        let security = self.admission_security.as_ref()
            .context("Exact owned-process security policy is required before spawn")?
            .for_role(role).clone();
        let path = self
            .catalog
            .role_file(role.as_str())
            .context("Role not in verified catalog")?
            .path();
        let mut command = Command::new(path);
        command
            .args(args)
            .env_clear()
            .envs(&self.environment)
            .stdin(stdin)
            .stdout(stdout)
            // Only the application can emit the bounded failed-helper record.
            // Preserve the exact inherited journal socket; other roles stay quiet.
            .stderr(if role == Role::Application { Stdio::inherit() } else { Stdio::null() });
        let leases = self
            .leases
            .as_ref()
            .context("Actual lifecycle lease handles required before spawn")?;
        // Reserve low descriptors before Rust allocates its private exec-error
        // pipe. Fixed bridge/lease targets must never overwrite that pipe.
        let reserved = reserve_low_descriptors()?;
        configure_child(
            &mut command,
            bridge_fds,
            Some((leases.0.as_raw_fd(), leases.1.as_raw_fd())),
        )?;
        let admission_role = match role {
            Role::Application => ownership::Role::Application,
            Role::Bridge => ownership::Role::Bridge,
            Role::Engine => ownership::Role::Engine,
            Role::Adapter => ownership::Role::Adapter,
        };
        let context = ownership::parse_context_sha512(self.catalog.candidate().sha512())?;
        if matches!(role, Role::Application | Role::Adapter) {
            command.arg("--release-manifest-sha512").arg(self.catalog.candidate().sha512());
        }
        let mut bootstrap = phase(ownership::Bootstrap::prepare(&self.creator, admission_role, context,
            ownership::monotonic_deadline(self.bounds.startup_timeout)?), role, Stage::GuardPrepare)?;
        let child = phase(bootstrap.spawn(&mut command)
            .with_context(|| format!("Start owned {}", role.as_str())), role, Stage::Spawn)?;
        // Insert the handle before any fallible capture. Failure must retain the
        // actual child for cleanup. Drop Command to close its duplicate pipe ends.
        drop(command);
        drop(reserved);
        self.children.push(OwnedChild {
            role,
            child,
            identity: None,
            snapshot: None,
        });
        phase(bootstrap.await_ready_with_cancel(|| self.active()), role, Stage::GuardReady)?;
        let owned = self.children.last_mut().unwrap();
        owned.identity = Some(phase(observation::capture_owned_child(
            &mut owned.child,
            role.as_str(),
            &self.catalog,
            &self.observation_bounds,
        ), role, Stage::CaptureIdentity)?);
        // Initial admission observes the blocked, pre-work process. Runtime
        // observation remains a separate phase with its own explicit bounds.
        let identity = owned.identity.as_ref().context("Owned admission identity absent")?;
        phase(security.check_owned(identity), role, Stage::SecurityBefore)?;
        owned.snapshot = Some(phase(observation::observe_owned_child(
            &mut owned.child, identity, role.as_str(), &self.catalog, &self.observation_bounds,
        ), role, Stage::ObserveInitial)?);
        phase(security.check_owned(identity), role, Stage::SecurityAfter)?;
        phase(bootstrap.release_with_cancel(|| self.active()), role, Stage::GuardRelease)?;
        Ok(())
    }
    pub fn start_application(&mut self, args: &[OsString]) -> Result<()> {
        let result = (|| {
            self.active()?;
            ensure!(self.children.is_empty(), "Application must start first");
            let (input_read, input_write) = pipe()?;
            let (output_read, output_write) = pipe()?;
            self.pipes = Some(ApplicationPipes {
                input: File::from(input_write),
                output: File::from(output_read),
            });
            self.spawn(
                Role::Application,
                args,
                Stdio::from(input_read),
                Stdio::from(output_write),
                None,
            )?;
            let pipes = self.pipes.as_ref().unwrap();
            nonblocking(pipes.input.as_raw_fd())?;
            nonblocking(pipes.output.as_raw_fd())?;
            Ok(())
        })();
        self.finish(phase(result, Role::Application, Stage::Start))
    }
    /// Bounded newline exchange before ownership of both endpoints transfers to
    /// the bridge. The caller must validate the response's protocol semantics.
    pub fn exchange_application(
        &mut self,
        request: &[u8],
        max_response_bytes: usize,
        timeout: Duration,
    ) -> Result<Vec<u8>> {
        let result = (|| {
            self.active()?;
            ensure!(
                timeout > Duration::ZERO && timeout <= self.bounds.startup_timeout,
                "Probe timeout exceeds startup bound"
            );
            ensure!(
                max_response_bytes > 0
                    && max_response_bytes <= self.bounds.max_probe_response_bytes,
                "Probe response bound invalid"
            );
            ensure!(
                !request.is_empty()
                    && request.len() <= self.bounds.max_probe_request_bytes
                    && request.last() == Some(&b'\n')
                    && !request[..request.len() - 1].contains(&b'\n'),
                "Probe must be one bounded newline record"
            );
            let deadline = Instant::now()
                .checked_add(timeout)
                .context("Probe deadline overflow")?;
            let mut written = 0;
            while written < request.len() {
                self.check_alive_inner()?;
                let input = &mut self
                    .pipes
                    .as_mut()
                    .context("Application endpoints already transferred")?
                    .input;
                match input.write(&request[written..]) {
                    Ok(0) => bail!("Application input closed"),
                    Ok(count) => written += count,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(error.into()),
                }
                if written < request.len() {
                    wait_tick(deadline, self.bounds.poll_interval)?;
                }
            }
            let mut response = Vec::new();
            loop {
                self.check_alive_inner()?;
                let output = &mut self
                    .pipes
                    .as_mut()
                    .context("Application endpoints already transferred")?
                    .output;
                let mut byte = [0; 1];
                match output.read(&mut byte) {
                    Ok(0) => bail!("Application output closed during probe"),
                    Ok(_) => {
                        ensure!(
                            response.len() < max_response_bytes,
                            "Application probe response exceeds bound"
                        );
                        response.push(byte[0]);
                        if byte[0] == b'\n' {
                            self.application_probed = true;
                            return Ok(response);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        wait_tick(deadline, self.bounds.poll_interval)?
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error.into()),
                }
                ensure!(
                    Instant::now() < deadline,
                    "Application probe deadline exceeded"
                );
            }
        })();
        self.finish(result)
    }
    fn observe(&mut self, role: Role) -> Result<observation::Snapshot> {
        self.check_alive_inner()?;
        let child = self
            .children
            .iter_mut()
            .find(|c| c.role == role)
            .context("Role has not started")?;
        let identity = child
            .identity
            .as_ref()
            .context("Owned role has no captured identity")?;
        let security = self.admission_security.as_ref().context("Owned security policy absent")?.for_role(role);
        security.check_owned(identity)?;
        let budget = self.observation_pause.context("Explicit observation pause budget absent")?;
        let (snapshot, timing) = observation::controlled_pause::observe_paused(
            &mut child.child, identity, role.as_str(), &self.catalog,
            &self.observation_bounds, budget, &self.cancelled,
        ).map_err(|error| {
            if observation::controlled_pause::is_clean_cancellation(&error) {
                anyhow::Error::new(OperationCancelled).context(error)
            } else {
                error
            }
        })?;
        security.check_owned(identity)?;
        self.observation_timings.iter_mut().find(|entry| entry.role == role.as_str())
            .context("Unknown observation timing role")?.record(&timing)?;
        child.snapshot = Some(snapshot.clone());
        Ok(snapshot)
    }
    pub fn observe_application(&mut self) -> Result<observation::Snapshot> {
        let result = (|| {
            self.active()?;
            ensure!(
                self.application_probed,
                "Probe application before mapping observation"
            );
            self.observe(Role::Application)
        })();
        self.finish(result)
    }
    pub fn start_bridge(&mut self, socket: &Path) -> Result<()> {
        let result = (|| {
            self.active()?;
            ensure!(
                self.application_probed
                    && self.children.len() == 1
                    && self.children[0].snapshot.is_some(),
                "Bridge requires the probed and observed owned application"
            );
            phase(endpoint_absent(socket), Role::Bridge, Stage::EndpointAbsent)?;
            let pipes = self
                .pipes
                .as_ref()
                .context("Application endpoints already transferred")?;
            // These independent high-numbered duplicates avoid dup2 source/target
            // collisions. Both remain CLOEXEC except explicit bridge endpoints.
            let input = duplicate(pipes.input.as_raw_fd())?;
            let output = duplicate(pipes.output.as_raw_fd())?;
            let args = vec![
                OsString::from("--socket"),
                OsString::from(format!("unix://{}", socket.display())),
                OsString::from("--application-channel=inherited-pipes-v1"),
                OsString::from("--application-input-fd=3"),
                OsString::from("--application-output-fd=4"),
            ];
            self.spawn(
                Role::Bridge,
                &args,
                Stdio::null(),
                Stdio::null(),
                Some((input.as_raw_fd(), output.as_raw_fd())),
            )?;
            drop(input);
            drop(output);
            self.pipes.take();
            phase(self.wait_endpoint(socket), Role::Bridge, Stage::EndpointReady)?;
            phase(self.observe(Role::Bridge), Role::Bridge, Stage::ControlledObservation)?;
            Ok(())
        })();
        self.finish(phase(result, Role::Bridge, Stage::Start))
    }
    pub fn start_engine(&mut self, args: &[OsString], ready_socket: &Path) -> Result<()> {
        let result = (|| {
            self.active()?;
            ensure!(
                self.children.len() == 2
                    && self.children[1].role == Role::Bridge
                    && self.children[1].snapshot.is_some(),
                "Engine requires observed bridge endpoint"
            );
            phase(endpoint_absent(ready_socket), Role::Engine, Stage::EndpointAbsent)?;
            self.spawn(Role::Engine, args, Stdio::null(), Stdio::null(), None)?;
            phase(self.wait_endpoint(ready_socket), Role::Engine, Stage::EndpointReady)?;
            phase(self.observe(Role::Engine), Role::Engine, Stage::ControlledObservation)?;
            Ok(())
        })();
        self.finish(phase(result, Role::Engine, Stage::Start))
    }
    pub fn start_adapter(&mut self, args: &[OsString]) -> Result<()> {
        let result = (|| {
            self.active()?;
            ensure!(
                self.children.len() == 3
                    && self.children[2].role == Role::Engine
                    && self.children[2].snapshot.is_some(),
                "Adapter requires observed engine endpoint"
            );
            self.spawn(Role::Adapter, args, Stdio::null(), Stdio::null(), None)?;
            phase(self.observe(Role::Adapter), Role::Adapter, Stage::ControlledObservation)?;
            Ok(())
        })();
        self.finish(phase(result, Role::Adapter, Stage::Start))
    }
    fn wait_endpoint(&mut self, path: &Path) -> Result<()> {
        let deadline = Instant::now()
            .checked_add(self.bounds.startup_timeout)
            .context("Startup deadline overflow")?;
        let mut identity = None;
        loop {
            self.check_alive_inner()?;
            endpoint_parent(path)?;
            match std::fs::symlink_metadata(path) {
                Ok(metadata) => {
                    let current = endpoint_identity(&metadata, unsafe { libc::geteuid() })?;
                    ensure!(
                        identity.is_none_or(|original| original == current),
                        EndpointViolation::InodeChanged
                    );
                    identity = Some(current);
                    // Bind precedes chmod in the real bridge. Its private
                    // parent protects this window; only the final 0600 mode
                    // permits a dependent process to start.
                    if metadata.mode() & 0o7777 == 0o600 {
                        return Ok(());
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    ensure!(
                        identity.is_none(),
                        EndpointViolation::Disappeared
                    );
                }
                Err(error) => return Err(error.into()),
            }
            wait_tick(deadline, self.bounds.poll_interval)?;
        }
    }
    fn check_alive_inner(&mut self) -> Result<()> {
        self.active()
    }
    pub fn check_alive(&mut self) -> Result<()> {
        let result = self.check_alive_inner();
        self.finish(result)
    }
    pub fn observe_all(&mut self) -> Result<Vec<observation::Snapshot>> {
        let result = (|| {
            self.active()?;
            let roles: Vec<_> = self.children.iter().map(|c| c.role).collect();
            roles.into_iter().map(|role| self.observe(role)).collect()
        })();
        self.finish(result)
    }
    /// Stop only retained child groups. A zombie leader pins its PID until the
    /// final group signal, so the owner never signals a recycled process group.
    pub fn shutdown(&mut self) -> Result<()> {
        self.creator.check()?;
        self.stopped = true;
        self.pipes.take();
        let mut errors = Vec::new();
        for child in self.children.iter().rev() {
            if let Err(error) = signal_owned(&child.child, libc::SIGTERM) {
                errors.push(error.to_string());
            }
        }
        let deadline = Instant::now() + self.bounds.stop_timeout;
        loop {
            let mut all = true;
            for child in &self.children {
                match exited(&child.child) {
                    Ok(value) => all &= value,
                    Err(error) => errors.push(error.to_string()),
                }
            }
            if all || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(
                self.bounds
                    .poll_interval
                    .min(deadline.saturating_duration_since(Instant::now())),
            );
        }
        for child in self.children.iter().rev() {
            if let Err(error) = signal_owned(&child.child, libc::SIGKILL) {
                errors.push(error.to_string());
            }
        }
        let deadline = Instant::now() + self.bounds.kill_timeout;
        loop {
            self.children
                .retain_mut(|child| match child.child.try_wait() {
                    Ok(Some(_)) => false,
                    Ok(None) => true,
                    Err(error) => {
                        errors.push(error.to_string());
                        true
                    }
                });
            if self.children.is_empty() || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(
                self.bounds
                    .poll_interval
                    .min(deadline.saturating_duration_since(Instant::now())),
            );
        }
        ensure!(
            self.children.is_empty(),
            "Owned children did not reap before deadline: {}",
            self.children.len()
        );
        self.leases.take();
        ensure!(
            errors.is_empty(),
            "Owned cleanup errors: {}",
            errors.join("; ")
        );
        Ok(())
    }
}
impl Drop for ProcessOwner {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn validate_arguments(args: &[OsString], limit: usize) -> Result<()> {
    let mut total = 0usize;
    for value in args {
        ensure!(
            !value.as_encoded_bytes().contains(&0),
            "Argument contains NUL"
        );
        total = total
            .checked_add(value.len() + 1)
            .context("Argument byte overflow")?;
        ensure!(total <= limit, "Argument byte bound exceeded");
    }
    Ok(())
}
fn validate_environment(environment: &ExplicitEnvironment, limit: usize) -> Result<()> {
    let mut total = 0usize;
    for (key, value) in environment {
        ensure!(
            matches!(
                key.as_str(),
                "HOME" | "TMPDIR" | "LANG" | "LC_ALL" | "TZ" | "GOMAXPROCS"
            ),
            "Environment key is not in explicit native allowlist"
        );
        ensure!(!value.as_bytes().contains(&0), "Environment contains NUL");
        total = total
            .checked_add(key.len())
            .and_then(|n| n.checked_add(value.len()))
            .and_then(|n| n.checked_add(2))
            .context("Environment byte overflow")?;
        ensure!(total <= limit, "Environment byte bound exceeded");
    }
    Ok(())
}
fn endpoint_absent(path: &Path) -> Result<()> {
    endpoint_parent(path)?;
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
        Ok(_) => bail!("Unix endpoint already exists; manual recovery required"),
    }
}
fn endpoint_identity(metadata: &std::fs::Metadata, expected_uid: u32) -> Result<(u64, u64)> {
    ensure!(
        metadata.file_type().is_socket() && metadata.uid() == expected_uid,
        EndpointViolation::TypeOrOwner
    );
    Ok((metadata.dev(), metadata.ino()))
}
fn endpoint_parent(path: &Path) -> Result<()> {
    ensure!(
        path.is_absolute(),
        EndpointViolation::NotAbsolute
    );
    let parent = path.parent().context(EndpointViolation::ParentMissing)?;
    ensure!(
        std::fs::canonicalize(parent)?.as_os_str() == parent.as_os_str(),
        EndpointViolation::ParentAlias
    );
    let metadata = std::fs::metadata(parent)?;
    ensure!(
        metadata.is_dir()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o077 == 0,
        EndpointViolation::ParentOwnershipOrMode
    );
    Ok(())
}
fn wait_tick(deadline: Instant, interval: Duration) -> Result<()> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    ensure!(!remaining.is_zero(), "Owned operation deadline exceeded");
    std::thread::sleep(interval.min(remaining));
    Ok(())
}
fn nonblocking(fd: RawFd) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    ensure!(flags >= 0, "Read pipe flags failed");
    ensure!(
        unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == 0,
        "Set pipe nonblocking failed"
    );
    Ok(())
}
fn duplicate(fd: RawFd) -> Result<OwnedFd> {
    let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 7) };
    ensure!(
        duplicate >= 0,
        "Duplicate owned pipe failed: {}",
        std::io::Error::last_os_error()
    );
    Ok(unsafe { OwnedFd::from_raw_fd(duplicate) })
}
fn reserve_low_descriptors() -> Result<Vec<File>> {
    let mut held = Vec::new();
    loop {
        let file = File::open("/dev/null")?;
        if file.as_raw_fd() > 6 {
            break;
        }
        held.push(file);
    }
    Ok(held)
}
#[cfg(target_os = "linux")]
fn pipe() -> Result<(OwnedFd, OwnedFd)> {
    let mut fds = [-1; 2];
    ensure!(
        unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } == 0,
        "Create owned pipe failed"
    );
    let read = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    let write = unsafe { OwnedFd::from_raw_fd(fds[1]) };
    ensure!(
        unsafe { libc::fchmod(read.as_raw_fd(), 0o600) } == 0,
        "Set private pipe mode failed"
    );
    Ok((read, write))
}
#[cfg(not(target_os = "linux"))]
fn pipe() -> Result<(OwnedFd, OwnedFd)> {
    bail!("Owned pipes require Linux")
}
#[cfg(target_os = "linux")]
fn configure_child(
    command: &mut Command,
    bridge_fds: Option<(RawFd, RawFd)>,
    leases: Option<(RawFd, RawFd)>,
) -> Result<()> {
    let parent = unsafe { libc::getpid() };
    // Hold each fixed target's original inode. If another parent thread closes
    // one while Command creates its exec-error pipe, refuse the changed target
    // before dup2. Never overwrite an unknown descriptor used by Rust's spawn.
    let mut target_guards = Vec::new();
    for target in 3..=6 {
        let held = File::from(duplicate(target)?);
        let metadata = held.metadata()?;
        target_guards.push((target, held, metadata.dev(), metadata.ino()));
    }
    // Only async-signal-safe operations occur after fork. close_range marks
    // unknown inherited descriptors CLOEXEC without closing Rust's error pipe.
    unsafe {
        command.pre_exec(move || {
            // Apply only in the forked child. Never change the multithreaded
            // parent's process-wide file-creation mask.
            libc::umask(0o077);
            for (target, _held, device, inode) in &target_guards {
                let mut metadata: libc::stat = std::mem::zeroed();
                if libc::fstat(*target, &mut metadata) != 0 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() == Some(libc::EBADF) {
                        continue;
                    }
                    return Err(error);
                }
                if metadata.st_dev != *device || metadata.st_ino != *inode {
                    return Err(std::io::Error::from_raw_os_error(libc::EBUSY));
                }
            }
            if libc::setpgid(0, 0) != 0 || libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(std::io::Error::from_raw_os_error(libc::ECHILD));
            }
            if libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 4u32) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if let Some((input, output)) = bridge_fds {
                if input < 7 || output < 7 || input == output {
                    return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
                }
                if libc::dup2(input, 3) != 3 || libc::dup2(output, 4) != 4 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            if let Some((service, state)) = leases {
                if service < 7 || state < 7 || service == state {
                    return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
                }
                if libc::dup2(service, 5) != 5 || libc::dup2(state, 6) != 6 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn configure_child(
    _: &mut Command,
    _: Option<(RawFd, RawFd)>,
    _: Option<(RawFd, RawFd)>,
) -> Result<()> {
    bail!("Native child isolation requires Linux")
}
#[cfg(target_os = "linux")]
fn exit_observation(child: &Child) -> Result<Option<ExitObservation>> {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let status = unsafe {
        libc::waitid(
            libc::P_PID,
            child.id(),
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if status != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ECHILD) {
            return Ok(Some(ExitObservation { observed_pid: None, si_code: None, si_status: None, observation_errno: Some(libc::ECHILD) }));
        }
        return Err(error.into());
    }
    let pid = unsafe { info.si_pid() };
    ensure!(pid == 0 || pid == child.id() as i32, "Wait status PID differs from owned child");
    Ok(if pid == 0 { None } else { Some(ExitObservation {
        observed_pid: Some(pid), si_code: Some(info.si_code), si_status: Some(unsafe { info.si_status() }), observation_errno: None,
    }) })
}
#[cfg(target_os = "linux")]
fn exited(child: &Child) -> Result<bool> { Ok(exit_observation(child)?.is_some()) }
#[cfg(not(target_os = "linux"))]
fn exit_observation(_: &Child) -> Result<Option<ExitObservation>> { bail!("Native child ownership requires Linux") }

#[cfg(not(target_os = "linux"))]
fn exited(_: &Child) -> Result<bool> {
    bail!("Owned wait requires Linux")
}
#[cfg(target_os = "linux")]
fn signal_owned(child: &Child, signal: i32) -> Result<()> {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    if unsafe {
        libc::waitid(
            libc::P_PID,
            child.id(),
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    } != 0
    {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ECHILD) {
            return Ok(());
        }
        return Err(error.into());
    }
    let pid = i32::try_from(child.id())?;
    // This group was established by pre_exec. The retained, unreaped child
    // prevents its ID from being reused by an unrelated process group.
    if unsafe { libc::kill(-pid, signal) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(error.into());
        }
    }
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn signal_owned(_: &Child, _: i32) -> Result<()> {
    bail!("Owned signal requires Linux")
}

#[cfg(test)]
#[path = "processes_tests.rs"]
mod tests;

/// Synthetic native fixture. It is absent from normal builds and has no release
/// authority. The qualification binary's only entry point calls this function.
#[cfg(all(not(target_os = "linux"), feature = "qualification-fixtures"))]
pub fn qualification_fixture_main() -> Result<()> {
    bail!("Native process qualification requires Linux")
}
#[cfg(all(target_os = "linux", feature = "qualification-fixtures"))]
pub fn qualification_fixture_main() -> Result<()> {
    use std::io::BufRead;
    use std::os::unix::net::UnixListener;
    let args: Vec<_> = std::env::args().collect();
    if args.iter().any(|a| a == "--fixture-parent-death") {
        let marker = args.get(2).context("Parent fixture marker missing")?;
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("--fixture-application")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .env_clear();
        let _reserved = reserve_low_descriptors()?;
        configure_child(&mut command, None, None)?;
        let mut child = command.spawn()?;
        std::fs::write(marker, child.id().to_string())?;
        loop {
            ensure!(child.try_wait()?.is_none(), "Fixture child exited");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    if args.iter().any(|a| a == "--fixture-exit") {
        return Ok(());
    }
    if let Some(index) = args.iter().position(|a| a == "--fixture-engine") {
        let path = args
            .get(index + 1)
            .context("Fixture engine endpoint missing")?;
        let _listener = UnixListener::bind(path)?;
        use std::os::unix::fs::PermissionsExt;
        if args.iter().any(|arg| arg == "--fixture-delay-socket-mode") {
            std::thread::sleep(Duration::from_millis(75));
        }
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        loop {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    if args.iter().any(|a| a == "--fixture-adapter") {
        loop {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    if args
        .iter()
        .any(|a| a == "--application-channel=inherited-pipes-v1")
    {
        ensure!(
            args.iter().any(|a| a == "--application-input-fd=3")
                && args.iter().any(|a| a == "--application-output-fd=4"),
            "Fixture bridge descriptors differ"
        );
        let endpoint = args
            .get(
                args.iter()
                    .position(|a| a == "--socket")
                    .context("Fixture socket missing")?
                    + 1,
            )
            .context("Fixture socket argument missing")?
            .strip_prefix("unix://")
            .context("Fixture socket scheme")?;
        let mut input = unsafe { File::from_raw_fd(3) };
        let mut output = unsafe { File::from_raw_fd(4) };
        ensure!(
            input.metadata()?.file_type().is_fifo() && output.metadata()?.file_type().is_fifo(),
            "Fixture inherited endpoints are not pipes"
        );
        ensure!(
            input.metadata()?.ino() != output.metadata()?.ino(),
            "Fixture endpoints share a pipe"
        );
        ensure!(
            unsafe { libc::fcntl(3, libc::F_GETFL) } & libc::O_ACCMODE == libc::O_WRONLY
                && unsafe { libc::fcntl(4, libc::F_GETFL) } & libc::O_ACCMODE == libc::O_RDONLY,
            "Fixture pipe directions differ"
        );
        let listener = UnixListener::bind(endpoint)?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(endpoint, std::fs::Permissions::from_mode(0o600))?;
        for stream in listener.incoming() {
            let mut stream = stream?;
            let mut request = String::new();
            std::io::BufReader::new(&mut stream).read_line(&mut request)?;
            input.write_all(request.as_bytes())?;
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut response = Vec::new();
            loop {
                let mut byte = [0; 1];
                match output.read(&mut byte) {
                    Ok(0) => bail!("Fixture application EOF"),
                    Ok(_) => {
                        response.push(byte[0]);
                        if byte[0] == b'\n' {
                            break;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        wait_tick(deadline, Duration::from_millis(1))?
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            stream.write_all(&response)?;
        }
        return Ok(());
    }
    if let Some(index) = args.iter().position(|a| a == "--fixture-created-file") {
        let path = args
            .get(index + 1)
            .context("Fixture created file path missing")?;
        // No chmod: the test must inspect the mode set by the launch-time mask.
        std::fs::write(path, b"synthetic child file\n")?;
    }
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        match line.as_str() {
            "umask" => {
                let mask = unsafe { libc::umask(0) };
                unsafe { libc::umask(mask) };
                println!("{mask}");
            }
            "exit" => return Ok(()),
            "hang" => loop {
                std::thread::sleep(Duration::from_secs(1));
            },
            "fds" => {
                let paths: Vec<_> = std::fs::read_dir("/proc/self/fd")?
                    .map(|r| r.map(|r| r.path()))
                    .collect::<std::io::Result<_>>()?;
                let mut fds: Vec<i32> = paths
                    .iter()
                    .filter(|p| std::fs::read_link(p).is_ok())
                    .filter_map(|p| p.file_name()?.to_str()?.parse().ok())
                    .collect();
                fds.sort();
                println!("{}", serde_json::to_string(&fds)?);
            }
            "environment" => println!(
                "{}",
                serde_json::to_string(&std::env::vars().collect::<BTreeMap<_, _>>())?
            ),
            _ => println!("{line}"),
        }
        std::io::stdout().flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod timing_summary_tests {
    use super::*;
    fn timing(value: u128) -> observation::controlled_pause::Timing {
        observation::controlled_pause::Timing { total_micros:value, stop_micros:value,
            observation_micros:value, release_micros:value,
            confirmed_stop_to_final_check_micros:value, pause_envelope_micros:value }
    }
    #[test]
    fn latest_and_component_maximum_stay_separate() {
        let mut summary = RoleObservationTiming::new(Role::Application);
        summary.record(&timing(10)).unwrap();
        let mut next = timing(5); next.release_micros=20;
        summary.record(&next).unwrap();
        assert_eq!(summary.count,2);
        assert_eq!(summary.latest.unwrap().total_micros,5);
        assert_eq!(summary.maximum.unwrap().total_micros,10);
        assert_eq!(summary.maximum.unwrap().release_micros,20);
    }
    #[test]
    fn overflow_refuses_without_partial_mutation() {
        let mut summary = RoleObservationTiming::new(Role::Adapter);
        summary.record(&timing(1)).unwrap();
        let before=summary;
        assert!(summary.record(&timing(u128::MAX)).is_err()); assert_eq!(summary,before);
        summary.count=u64::MAX; let before=summary;
        assert!(summary.record(&timing(2)).is_err()); assert_eq!(summary,before);
    }
    #[test]
    fn fixed_roles_start_with_no_measurement_and_do_not_grow() {
        let mut summaries=[Role::Application,Role::Bridge,Role::Engine,Role::Adapter].map(RoleObservationTiming::new);
        assert!(summaries.iter().all(|v| v.count==0 && v.latest.is_none() && v.maximum.is_none()));
        let bytes=std::mem::size_of_val(&summaries);
        for _ in 0..1000 { summaries[2].record(&timing(3)).unwrap(); }
        assert_eq!(summaries.len(),4); assert_eq!(std::mem::size_of_val(&summaries),bytes);
        assert_eq!(summaries[2].count,1000); assert_eq!(summaries[0].count,0);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod exit_diagnostic_tests {
    use super::*;
    #[test]
    fn owned_waitid_preserves_status_without_reaping() {
        let mut child=Command::new("/bin/sh").args(["-c","exit 37"]).spawn().unwrap();
        let deadline=Instant::now()+Duration::from_secs(3);
        let first=loop {
            if let Some(value)=exit_observation(&child).unwrap(){break value;}
            assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(first.observed_pid,Some(child.id() as i32));assert_eq!(first.si_code,Some(libc::CLD_EXITED));assert_eq!(first.si_status,Some(37));
        let again=exit_observation(&child).unwrap().unwrap();assert_eq!(again.si_status,Some(37));
        assert_eq!(child.wait().unwrap().code(),Some(37));
        let reaped=exit_observation(&child).unwrap().unwrap();assert_eq!(reaped.observation_errno,Some(libc::ECHILD));assert!(reaped.si_status.is_none());
    }
}

fn capture_first_failure(slot: &mut Option<serde_json::Value>, children: &[OwnedChild]) {
    if slot.is_some() { return; }
    let mut instant:libc::timespec=unsafe{std::mem::zeroed()};
    let monotonic_ns=if unsafe{libc::clock_gettime(libc::CLOCK_MONOTONIC,&mut instant)}==0 {
        (instant.tv_sec as u64).checked_mul(1_000_000_000).and_then(|v|v.checked_add(instant.tv_nsec as u64))
    } else { None };
    let rows:Vec<_>=children.iter().take(4).map(|child| {
        let mut row=serde_json::json!({"role":child.role.as_str(),"owned_pid":child.child.id(),"reaped_by_observation":false});
        match exit_observation(&child.child) {
            Ok(None)=>{row["state"]="RUNNING_AT_OBSERVATION".into();},
            Ok(Some(observation))=>{row["state"]="EXIT_OBSERVED".into();row["exit"]=crate::startup_diagnostic::child_exit_record(&OwnedChildExit{role:child.role,pid:child.child.id(),observation});},
            Err(error)=>{row["state"]="OBSERVATION_ERROR".into();row["observation_errno"]=error.downcast_ref::<std::io::Error>().and_then(|v|v.raw_os_error()).into();},
        }
        row
    }).collect();
    *slot=Some(serde_json::json!({"scope":"FIRST_FAILURE_OWNED_CHILDREN_V1","captured_before_cleanup":true,
        "monotonic_ns":monotonic_ns,"rows":rows,"inventory_overflow":children.len()>4,
        "ordering":"SEQUENTIAL_OBSERVATIONS_NO_CAUSAL_ORDER","production_qualified":false}));
}

#[cfg(all(test,target_os="linux"))]
mod first_failure_tests {
    use super::*;
    #[test]
    fn simultaneous_eof_and_bridge_failure_preserve_all_statuses_without_reap() {
        let app=Command::new("/bin/sh").args(["-c","cat >/dev/null"]).stdin(Stdio::piped()).stdout(Stdio::null()).spawn().unwrap();
        let bridge=Command::new("/bin/sh").args(["-c","exit 62"]).spawn().unwrap();
        let mut children=vec![OwnedChild{role:Role::Application,child:app,identity:None,snapshot:None},OwnedChild{role:Role::Bridge,child:bridge,identity:None,snapshot:None}];
        drop(children[0].child.stdin.take());
        let end=Instant::now()+Duration::from_secs(3);
        while children.iter().any(|c|exit_observation(&c.child).unwrap().is_none()) { assert!(Instant::now()<end);std::thread::sleep(Duration::from_millis(5)); }
        let mut slot=None;capture_first_failure(&mut slot,&children);let first=slot.clone();
        let rows=slot.as_ref().unwrap()["rows"].as_array().unwrap();assert_eq!(rows.len(),2);
        assert_eq!(rows[0]["exit"]["si_status"],0);assert_eq!(rows[1]["exit"]["si_status"],62);
        for (i,child) in children.iter_mut().enumerate() {assert_eq!(rows[i]["owned_pid"],child.child.id());assert_eq!(rows[i]["exit"]["observed_pid"],child.child.id());assert_eq!(child.child.wait().unwrap().code(),Some(if i==0{0}else{62}));}
        capture_first_failure(&mut slot,&children);assert_eq!(slot,first);
        assert_eq!(slot.unwrap()["ordering"],"SEQUENTIAL_OBSERVATIONS_NO_CAUSAL_ORDER");
    }
}
