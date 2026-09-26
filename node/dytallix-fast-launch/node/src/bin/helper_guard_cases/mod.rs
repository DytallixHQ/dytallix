//! Optional Linux qualification fixture. Never selected by a production binary.
use anyhow::{bail, ensure, Context, Result};
use dytallix_fast_node::root_genesis::{
    DevelopmentRootGenesis, ObservedHelperPolicy, OBSERVED_HELPER_PROFILE,
};
use dytallix_release_runtime::{
    observation::{self, static_helper},
    ownership::{self, Bootstrap, OwnerThread, Role},
};
use std::{
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::process::{CommandExt, ExitStatusExt},
    },
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

struct Owned(Child, bool);
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.1 {
            let _ = self.0.kill();
            let _ = self.0.try_wait();
        }
    }
}
fn remaining(end: Instant) -> Result<Duration> {
    let d = end.saturating_duration_since(Instant::now());
    ensure!(!d.is_zero(), "Fixture absolute deadline exceeded");
    Ok(d)
}
fn bounds(p: &ObservedHelperPolicy, end: Instant) -> Result<observation::Bounds> {
    Ok(observation::Bounds {
        max_stat_bytes: p.max_stat_bytes,
        max_maps_bytes: p.max_maps_bytes,
        max_map_entries: p.max_map_entries,
        max_path_bytes: p.max_path_bytes,
        max_unique_files: p.max_unique_files,
        max_file_bytes: p.max_file_bytes,
        max_total_file_bytes: p.max_total_file_bytes,
        max_elapsed: remaining(end)?.min(Duration::from_millis(p.max_elapsed_ms)),
        allowed_owner_uids: [0].into_iter().collect(),
    })
}
fn nonblocking(fd: i32) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    ensure!(
        flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == 0,
        "Cannot bound fixture pipe"
    );
    Ok(())
}
fn drain<R: Read>(pipe: &mut R, out: &mut Vec<u8>, limit: usize) -> Result<()> {
    let mut chunk = [0u8; 4096];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(n) => {
                ensure!(
                    out.len().checked_add(n).is_some_and(|x| x <= limit),
                    "Fixture helper output bound exceeded"
                );
                out.extend_from_slice(&chunk[..n]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
}
fn reap(child: &mut Owned, end: Instant) -> Result<ExitStatus> {
    if let Some(s) = child.0.try_wait()? {
        child.1 = true;
        return Ok(s);
    }
    child.0.kill()?;
    loop {
        if let Some(s) = child.0.try_wait()? {
            child.1 = true;
            return Ok(s);
        }
        std::thread::sleep(remaining(end)?.min(Duration::from_millis(1)));
    }
}
struct Sink {
    used: usize,
    limit: usize,
}
impl Sink {
    fn emit(&mut self, value: serde_json::Value) -> Result<()> {
        let mut raw = serde_json::to_vec(&value)?;
        raw.push(b'\n');
        self.used = self
            .used
            .checked_add(raw.len())
            .context("Sink length overflow")?;
        ensure!(self.used <= self.limit, "Fixture sink bound exceeded");
        // The outer harness must drain this sink and enforce its own absolute timeout.
        std::io::stdout().lock().write_all(&raw)?;
        Ok(())
    }
}
pub fn run(root: &DevelopmentRootGenesis, case: &str, output_bound: usize) -> Result<u8> {
    ensure!(
        [
            "missing-owner",
            "wrong-role",
            "no-go",
            "cancel",
            "owner-death"
        ]
        .contains(&case),
        "Unknown guard case"
    );
    let start = Instant::now();
    let owner = OwnerThread::new()?;
    let p = root
        .helper_execution
        .as_ref()
        .context("Observed helper policy required")?;
    ensure!(
        root.enabled
            && root.profile == "SLH-DSA-SHAKE-256s"
            && p.profile == OBSERVED_HELPER_PROFILE,
        "Fixture profile differs"
    );
    ensure!(
        p.cleanup_timeout_ms > 0 && p.cleanup_timeout_ms < root.timeout_ms,
        "Fixture cleanup reservation invalid"
    );
    let total = start
        .checked_add(Duration::from_millis(root.timeout_ms))
        .context("Fixture deadline overflow")?;
    let execution = total
        .checked_sub(Duration::from_millis(p.cleanup_timeout_ms))
        .context("Fixture reservation overflow")?;
    let security = p.owner_security.expected_child()?;
    let file = static_helper::verify_file(
        &root.helper_path,
        &root.helper_sha256,
        &static_helper::StaticHelperPolicy {
            max_file_bytes: u64::try_from(root.max_helper_bytes)?,
            max_path_bytes: p.max_path_bytes,
            max_program_headers: p.max_program_headers,
            max_section_headers: p.max_section_headers,
            max_elapsed: remaining(execution)?.min(Duration::from_millis(p.max_elapsed_ms)),
        },
    )?;
    ensure!(
        file.digest().bytes == p.helper_bytes && file.digest().sha512 == p.helper_sha512,
        "Independent helper pin differs"
    );
    let context = ownership::parse_context_sha512(&p.helper_sha512)?;
    let deadline = ownership::monotonic_deadline(remaining(execution)?)?;
    let mut bootstrap = if case == "missing-owner" {
        None
    } else {
        Some(Bootstrap::prepare(
            &owner,
            if case == "wrong-role" {
                Role::Engine
            } else {
                Role::Helper
            },
            context,
            deadline,
        )?)
    };
    let mut command = Command::new(file.path());
    command
        .env_clear()
        .arg("--profile")
        .arg(&root.profile)
        .arg("--execution-profile")
        .arg(OBSERVED_HELPER_PROFILE)
        .arg("--policy-json")
        .arg(std::str::from_utf8(&root.policy_json)?)
        .arg("--max-input-bytes")
        .arg(root.max_request_bytes.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let parent = owner.pid() as libc::pid_t;
    unsafe {
        command.pre_exec(move || {
            libc::umask(0o077);
            if libc::setpgid(0, 0) != 0 || libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(std::io::Error::from_raw_os_error(libc::ECHILD));
            }
            if libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 4u32) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = match bootstrap.as_mut() {
        Some(b) => b.spawn(&mut command)?,
        None => command.spawn()?,
    };
    let mut child = Owned(child, false);
    drop(command);
    let result = (|| {
        let mut stdout = child.0.stdout.take().context("Missing stdout")?;
        let mut stderr = child.0.stderr.take().context("Missing stderr")?;
        nonblocking(stdout.as_raw_fd())?;
        nonblocking(stderr.as_raw_fd())?;
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut sink = Sink {
            used: 0,
            limit: output_bound,
        };
        let mut admission = serde_json::Value::Null;
        if let Some(b) = bootstrap.as_mut() {
            if case == "wrong-role" {
                ensure!(
                    b.await_ready_with_cancel(ownership::check_cancellation)
                        .is_err(),
                    "Wrong role received READY"
                );
            } else {
                b.await_ready_with_cancel(ownership::check_cancellation)?;
                owner.check()?;
                let identity = static_helper::capture_owned_child(
                    &mut child.0,
                    &file,
                    &bounds(p, execution)?,
                )?;
                security.check_owned(identity.owned_identity())?;
                let snapshot = static_helper::observe_owned_child(
                    &mut child.0,
                    &identity,
                    &file,
                    &bounds(p, execution)?,
                )?;
                security.check_owned(identity.owned_identity())?;
                admission = snapshot.report();
                drain(&mut stdout, &mut out, p.max_output_bytes)?;
                ensure!(out.is_empty(), "Helper performed protocol work before GO");
                if case == "owner-death" {
                    sink.emit(serde_json::json!({"scope":"LOCAL_HELPER_GUARD_QUALIFICATION","event":"OWNER_DEATH_ARMED","owner_pid":owner.pid(),"helper_pid":child.0.id(),"helper_start_ticks":identity.start().start_ticks,"helper_sha512":p.helper_sha512,"owner_admission":admission,"go_sent":false,"production_qualified":false}))?;
                }
            }
        }
        if case == "cancel" {
            ensure!(
                unsafe { libc::kill(libc::getpid(), libc::SIGTERM) } == 0,
                "Cannot inject fixture cancellation"
            );
            ensure!(
                ownership::check_cancellation().is_err(),
                "Cancellation was not observed"
            );
            let status = reap(&mut child, total)?;
            drain(&mut stdout, &mut out, p.max_output_bytes)?;
            drain(&mut stderr, &mut err, p.max_output_bytes)?;
            ensure!(
                out.is_empty() && status.signal() == Some(libc::SIGKILL),
                "Cancellation cleanup outcome differs"
            );
            sink.emit(serde_json::json!({"scope":"LOCAL_HELPER_GUARD_QUALIFICATION","case":case,"status":"CANCELLED_AND_REAPED","owner_admission":admission,"go_sent":false,"helper_signal":status.signal(),"reaped":true,"production_qualified":false}))?;
            return Ok(0);
        }
        let status = loop {
            drain(&mut stdout, &mut out, p.max_output_bytes)?;
            drain(&mut stderr, &mut err, p.max_output_bytes)?;
            ensure!(out.is_empty(), "Helper emitted protocol output without GO");
            if let Some(status) = child.0.try_wait()? {
                child.1 = true;
                break status;
            }
            ownership::check_cancellation()?;
            std::thread::sleep(remaining(total)?.min(Duration::from_millis(1)));
        };
        drain(&mut stdout, &mut out, p.max_output_bytes)?;
        drain(&mut stderr, &mut err, p.max_output_bytes)?;
        ensure!(
            out.is_empty() && status.code() == Some(1),
            "Guard refusal lifecycle differs"
        );
        let text = std::str::from_utf8(&err)?;
        ensure!(
            text.contains("owner startup guard:"),
            "Failure did not originate from owner guard"
        );
        if case == "no-go" {
            ensure!(
                text.contains("deadline"),
                "No-GO refusal was not deadline-bound"
            )
        }
        if case == "owner-death" {
            bail!("Owner-death injector did not kill the immediate owner before its deadline")
        }
        sink.emit(serde_json::json!({"scope":"LOCAL_HELPER_GUARD_QUALIFICATION","case":case,"status":"GUARD_REFUSED_AND_REAPED","owner_pid":owner.pid(),"helper_pid":child.0.id(),"owner_admission":admission,"guard_error":text,"helper_exit_code":status.code(),"go_sent":false,"stdout_bytes":out.len(),"reaped":true,"elapsed_ms":start.elapsed().as_millis(),"production_qualified":false}))?;
        Ok(0)
    })();
    match result {
        Ok(v) => Ok(v),
        Err(e) => match reap(&mut child, total) {
            Ok(_) => Err(e),
            Err(cleanup) => Err(e.context(format!("Fixture cleanup failed: {cleanup:#}"))),
        },
    }
}
