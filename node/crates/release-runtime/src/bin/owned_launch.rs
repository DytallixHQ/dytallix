//! Qualification fixture: run one program as an owned application child (the
//! owner protocol) and wait for it. The child inherits this process's
//! standard streams, so a test talks to it through pipes (E04 gap 11, M-b).
//!
//! Linux only. It installs neither no_new_privs nor a seccomp filter; the
//! child's guard requires both, so run it where they already apply.
//!
//! `release-runtime-owned-launch --context-sha512 HEX -- PROGRAM [ARGS...]`
//! exits with the child's status, or 128 plus the signal that ended it.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("release-runtime-owned-launch runs only on Linux");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    match linux::run() {
        Ok(code) => std::process::ExitCode::from(code),
        Err(error) => {
            eprintln!("release-runtime-owned-launch: {error:#}");
            std::process::ExitCode::from(125)
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use anyhow::{ensure, Context, Result};
    use dytallix_release_runtime::ownership::{self, Bootstrap, OwnerThread, Role};
    use std::ffi::OsStr;
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::process::Command;
    use std::time::Duration;

    const USAGE: &str =
        "usage: release-runtime-owned-launch --context-sha512 HEX -- PROGRAM [ARGS...]";

    pub fn run() -> Result<u8> {
        let mut args = std::env::args_os().skip(1);
        ensure!(
            args.next().as_deref() == Some(OsStr::new("--context-sha512")),
            USAGE
        );
        let context = args.next().context(USAGE)?;
        let context = ownership::parse_context_sha512(context.to_str().context(USAGE)?)?;
        ensure!(args.next().as_deref() == Some(OsStr::new("--")), USAGE);
        let program = args.next().context(USAGE)?;
        let owner = OwnerThread::new()?;
        let mut command = Command::new(program);
        command.args(args);
        unsafe {
            command.pre_exec(|| {
                // Mark everything from 3 close-on-exec; the ownership
                // descriptors are installed after this.
                if libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 4u32) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let deadline = ownership::monotonic_deadline(Duration::from_secs(30))?;
        let mut bootstrap = Bootstrap::prepare(&owner, Role::Application, context, deadline)?;
        let mut child = bootstrap.spawn(&mut command)?;
        drop(command);
        if let Err(error) = bootstrap.await_ready().and_then(|()| bootstrap.release()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.context("Owned application not admitted"));
        }
        drop(bootstrap);
        let status = child.wait()?;
        Ok(match (status.code(), status.signal()) {
            (Some(code), _) => u8::try_from(code).unwrap_or(1),
            (None, Some(signal)) => u8::try_from(128 + signal).unwrap_or(255),
            _ => 1,
        })
    }
}
