use anyhow::{ensure, Context, Result};
use dytallix_native_supervisor::processes::{is_graceful_cancellation, with_owned_cleanup};
use dytallix_native_supervisor::service::NativeService;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
fn main() -> std::process::ExitCode {
    // Keep the inherited diagnostic socket free of the default blocking panic hook.
    // Panic propagation, unwinding and nonzero termination remain unchanged.
    std::panic::set_hook(Box::new(|_| {}));
    // Owned cleanup and bounded diagnostics run before this nonprinting exit.
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(_) => std::process::ExitCode::FAILURE,
    }
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 2 && args[0] == "--development-service-config",
        "Usage: dytallix-native-supervisor --development-service-config /absolute/config.json"
    );
    dytallix_release_runtime::ownership::install_cancellation()?;
    let sink = ReportSink::stdout()?;
    let mut service = NativeService::prepare(Path::new(&args[1]))?;
    let flag = service.cancellation_handle();
    let result = (|| -> Result<()> {
        service.start()?;
        sink.emit(&service.report(), true)?;
        loop {
            // Check child failures before classifying the cancellation signal.
            service.monitor_tick()?;
            let deadline = std::time::Instant::now() + service.monitor_interval();
            while !flag.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
                std::thread::sleep(
                    std::time::Duration::from_millis(25)
                        .min(deadline.saturating_duration_since(std::time::Instant::now())),
                );
            }
        }
    })();
    if result.is_err() { service.capture_failure(); }
    let cleanup = service.stop();
    let final_stop_succeeded = cleanup.is_ok();
    let finished = finish_shutdown(result, cleanup);
    // No owned child cleanup is delayed by the final diagnostic output.
    let mut report = service.final_timing_report();
    report["final_stop_succeeded"] = final_stop_succeeded.into();
    report["service_exit_success"] = finished.is_ok().into();
    if let Err(error) = &finished {
        report["failure"] = dytallix_native_supervisor::startup_diagnostic::record(error);
    }
    let output = sink.emit(&report, false);
    match (finished, output) {
        (result, Ok(())) => result,
        (Ok(()), Err(error)) => Err(error.context("Final timing report failed")),
        (Err(error), Err(output)) => {
            Err(error.context(format!("Final timing report failed: {output:#}")))
        }
    }
}

const MAX_REPORT_BYTES: usize = 1024 * 1024;
const REPORT_DEADLINE: Duration = Duration::from_secs(1);
struct ReportBytes(Vec<u8>);
impl Write for ReportBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > (MAX_REPORT_BYTES - 1).saturating_sub(self.0.len()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Service report exceeds byte bound",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn report_bytes(value: &serde_json::Value) -> Result<Vec<u8>> {
    let mut bytes = ReportBytes(Vec::new());
    serde_json::to_writer(&mut bytes, value)?;
    bytes.0.push(b'\n');
    Ok(bytes.0)
}
struct ReportSink {
    fd: OwnedFd,
}
impl ReportSink {
    fn stdout() -> Result<Self> {
        let fd = unsafe { libc::fcntl(libc::STDOUT_FILENO, libc::F_DUPFD_CLOEXEC, 9) };
        ensure!(fd >= 9, "Cannot retain report descriptor");
        Self::admit(unsafe { OwnedFd::from_raw_fd(fd) })
    }
    fn admit(fd: OwnedFd) -> Result<Self> {
        let mut metadata: libc::stat = unsafe { std::mem::zeroed() };
        ensure!(
            unsafe { libc::fstat(fd.as_raw_fd(), &mut metadata) } == 0,
            "Cannot inspect report descriptor"
        );
        let kind = metadata.st_mode & libc::S_IFMT;
        ensure!(
            kind == libc::S_IFIFO || kind == libc::S_IFSOCK,
            "Report output requires a nonblocking pipe or socket; regular files are refused"
        );
        if kind == libc::S_IFSOCK {
            let mut kind: libc::c_int = 0;
            let mut bytes = std::mem::size_of_val(&kind) as libc::socklen_t;
            ensure!(
                unsafe {
                    libc::getsockopt(
                        fd.as_raw_fd(),
                        libc::SOL_SOCKET,
                        libc::SO_TYPE,
                        (&mut kind as *mut libc::c_int).cast(),
                        &mut bytes,
                    )
                } == 0
                    && bytes as usize == std::mem::size_of_val(&kind)
                    && kind == libc::SOCK_STREAM,
                "Report socket must be a stream"
            );
        }
        let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
        ensure!(
            flags >= 0
                && (flags & libc::O_ACCMODE) != libc::O_RDONLY
                && unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) }
                    == 0,
            "Cannot make report output nonblocking"
        );
        Ok(Self { fd })
    }
    fn emit(&self, value: &serde_json::Value, cancellable: bool) -> Result<()> {
        let deadline = Instant::now()
            .checked_add(REPORT_DEADLINE)
            .context("Report deadline overflow")?;
        let bytes = report_bytes(value)?;
        self.write_bounded(&bytes, deadline, || {
            if cancellable {
                dytallix_release_runtime::ownership::check_cancellation()?;
            }
            Ok(())
        })
    }
    fn write_bounded(
        &self,
        bytes: &[u8],
        deadline: Instant,
        check: impl Fn() -> Result<()>,
    ) -> Result<()> {
        ensure!(
            !bytes.is_empty() && bytes.len() <= MAX_REPORT_BYTES,
            "Report frame size invalid"
        );
        let mut offset = 0;
        loop {
            check()?;
            ensure!(Instant::now() < deadline, "Report output deadline exceeded");
            if offset == bytes.len() {
                return Ok(());
            }
            let count = unsafe {
                libc::write(
                    self.fd.as_raw_fd(),
                    bytes[offset..].as_ptr().cast(),
                    (bytes.len() - offset).min(16 * 1024),
                )
            };
            if count > 0 {
                offset = offset
                    .checked_add(count as usize)
                    .context("Report offset overflow")?;
                continue;
            }
            ensure!(count != 0, "Report output closed during write");
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            if error.kind() != std::io::ErrorKind::WouldBlock {
                return Err(error.into());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            ensure!(!remaining.is_zero(), "Report output deadline exceeded");
            let millis = remaining.as_millis().clamp(1, 25) as libc::c_int;
            let mut poll = libc::pollfd {
                fd: self.fd.as_raw_fd(),
                events: libc::POLLOUT,
                revents: 0,
            };
            let rc = unsafe { libc::poll(&mut poll, 1, millis) };
            if rc < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                return Err(std::io::Error::last_os_error().into());
            }
            ensure!(
                poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) == 0,
                "Report output endpoint failed"
            );
        }
    }
}

fn finish_shutdown(result: Result<()>, cleanup: Result<()>) -> Result<()> {
    match with_owned_cleanup(result, cleanup, "Service") {
        Err(error) if is_graceful_cancellation(&error) => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dytallix_native_supervisor::processes::OperationCancelled;

    fn socket_sink() -> (ReportSink, std::os::unix::net::UnixStream) {
        let (writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();
        (ReportSink::admit(writer.into()).unwrap(), reader)
    }
    #[test]
    fn report_serialization_has_exact_size_bound() {
        let exact = serde_json::Value::String("x".repeat(MAX_REPORT_BYTES - 3));
        assert_eq!(report_bytes(&exact).unwrap().len(), MAX_REPORT_BYTES);
        let oversized = serde_json::Value::String("x".repeat(MAX_REPORT_BYTES - 2));
        assert!(report_bytes(&oversized).is_err());
        let value = serde_json::json!({"escaped":"\n\"", "count":1});
        let bytes = report_bytes(&value).unwrap();
        assert_eq!(bytes.last(), Some(&b'\n'));
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            value
        );
    }
    #[test]
    fn regular_file_output_refuses_before_service_creation() {
        let file = tempfile::tempfile().unwrap();
        assert!(ReportSink::admit(file.into()).is_err());
    }
    #[test]
    fn short_writes_complete_exactly_with_a_bounded_reader() {
        use std::io::Read;
        let (sink, mut reader) = socket_sink();
        reader
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let bytes = vec![b'x'; 256 * 1024];
        let expected = bytes.clone();
        let receiver = std::thread::spawn(move || {
            let mut received = vec![0; expected.len()];
            reader.read_exact(&mut received).unwrap();
            assert_eq!(received, expected);
        });
        let result = sink.write_bounded(&bytes, Instant::now() + Duration::from_secs(1), || Ok(()));
        drop(sink);
        receiver.join().unwrap();
        result.unwrap();
    }
    #[test]
    fn full_socket_refuses_within_deadline_without_a_reader() {
        let (sink, _reader) = socket_sink();
        let chunk = [0u8; 16384];
        let mut total = 0;
        loop {
            let count =
                unsafe { libc::write(sink.fd.as_raw_fd(), chunk.as_ptr().cast(), chunk.len()) };
            if count < 0 {
                assert_eq!(
                    std::io::Error::last_os_error().kind(),
                    std::io::ErrorKind::WouldBlock
                );
                break;
            }
            assert!(count > 0);
            total += count as usize;
            assert!(total <= 16 * 1024 * 1024);
        }
        let start = Instant::now();
        let error = sink
            .write_bounded(b"bounded", start + Duration::from_millis(20), || Ok(()))
            .unwrap_err();
        assert!(error.to_string().contains("deadline"));
        assert!(start.elapsed() < Duration::from_secs(1));
    }
    #[test]
    fn cancellation_and_expiry_are_checked_after_the_last_write() {
        let (sink, _reader) = socket_sink();
        assert!(sink.write_bounded(b"x", Instant::now(), || Ok(())).is_err());
        let calls = std::cell::Cell::new(0);
        let error = sink
            .write_bounded(b"x", Instant::now() + Duration::from_secs(1), || {
                let count = calls.get() + 1;
                calls.set(count);
                ensure!(count < 2, "final check cancelled");
                Ok(())
            })
            .unwrap_err();
        assert!(error.to_string().contains("final check cancelled"));
        assert_eq!(calls.get(), 2);
        let calls = std::cell::Cell::new(0);
        let error = sink
            .write_bounded(b"x", Instant::now() + Duration::from_millis(20), || {
                let count = calls.get() + 1;
                calls.set(count);
                if count == 2 {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Ok(())
            })
            .unwrap_err();
        assert!(error.to_string().contains("deadline"));
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn intentional_cancellation_with_successful_cleanup_returns_success() {
        let cancellation = anyhow::Error::new(OperationCancelled).context("Readiness interrupted");
        assert!(finish_shutdown(Err(cancellation), Ok(())).is_ok());
        assert!(finish_shutdown(Ok(()), Ok(())).is_ok());
    }
    #[test]
    fn process_and_observation_failures_are_not_cancellation() {
        for message in [
            "Owned process operation cancelled",
            "Owned consensus_stdio exited",
            "Unknown executable mapping",
        ] {
            assert!(finish_shutdown(Err(anyhow::anyhow!(message)), Ok(())).is_err());
        }
    }
    #[test]
    fn cleanup_failure_is_not_erased_by_cancellation_or_a_later_success() {
        let earlier = with_owned_cleanup::<()>(
            Err(OperationCancelled.into()),
            Err(anyhow::anyhow!("Reap deadline exceeded")),
            "Owned process",
        );
        let error = finish_shutdown(earlier, Ok(())).unwrap_err();
        assert!(format!("{error:#}").contains("Reap deadline exceeded"));
        assert!(finish_shutdown(
            Err(OperationCancelled.into()),
            Err(anyhow::anyhow!("Final stop failed"))
        )
        .is_err());
        assert!(finish_shutdown(Ok(()), Err(anyhow::anyhow!("Final stop failed"))).is_err());
    }
}
