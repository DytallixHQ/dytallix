//! Pipe/framing lifecycle tests only. These shell fixtures never pass static
//! helper verification and do not provide cryptographic qualification.
use super::*;
fn fixture(script: &str, max_output: usize) -> (OwnedHelper, Channel) {
    let child = Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut helper = OwnedHelper {
        child,
        reaped: false,
    };
    let channel = Channel::new(&mut helper.child, max_output).unwrap();
    (helper, channel)
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}
#[test]
fn bounded_owned_channel_preserves_exact_binary_frame_and_ack_eof() {
    let (mut helper, mut channel) = fixture("exec /bin/cat", 8192);
    let request = [0, 0, 0, 3, 0, 0xff, 1];
    channel.write_all(&request, deadline()).unwrap();
    let mut actual = [0; 7];
    channel.read_exact(&mut actual, deadline()).unwrap();
    assert_eq!(actual, request);
    channel.write_all(ACK, deadline()).unwrap();
    let mut ack = vec![0; ACK.len()];
    channel.read_exact(&mut ack, deadline()).unwrap();
    assert_eq!(ack, ACK);
    channel.input.take();
    assert!(finish_protocol(&mut helper, &mut channel, deadline())
        .unwrap()
        .success());
    assert!(helper.reaped);
    assert!(channel.error_bytes.is_empty());
}
#[test]
fn incomplete_ready_and_output_overflow_fail_and_reap() {
    for (script, bound) in [("printf short", 8192), ("printf 123456789", 4)] {
        let (mut helper, mut channel) = fixture(script, bound);
        let mut ready = vec![0; READY.len()];
        assert!(channel.read_exact(&mut ready, deadline()).is_err());
        reap(&mut helper, deadline()).unwrap();
        assert!(helper.reaped);
    }
}
#[test]
fn trailing_stdout_and_stderr_share_the_total_output_bound() {
    let (mut helper, mut channel) = fixture("printf bad; printf 123456789 >&2", 4);
    assert!(finish_protocol(&mut helper, &mut channel, deadline()).is_err());
    reap(&mut helper, deadline()).unwrap();
    assert!(helper.reaped);
}
#[test]
fn blocked_read_and_natural_exit_deadlines_leave_bounded_reap_time() {
    for read_phase in [true, false] {
        let (mut helper, mut channel) = fixture("exec /bin/sleep 5", 8192);
        let started = Instant::now();
        let operation_deadline = started + Duration::from_millis(30);
        if read_phase {
            assert!(channel.read_exact(&mut [0], operation_deadline).is_err());
        } else {
            channel.input.take();
            assert!(finish_protocol(&mut helper, &mut channel, operation_deadline).is_err());
        }
        reap(&mut helper, started + Duration::from_secs(1)).unwrap();
        assert!(helper.reaped);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}

#[test]
fn cancellation_rejects_pipe_work_but_does_not_prevent_owned_reap() {
    for operation in 0..3 {
        let (mut helper, mut channel) = fixture("exec /bin/sleep 5", 8192);
        channel.check_cancel = || bail!("injected helper cancellation");
        let result = match operation {
            0 => channel.read_exact(&mut [0], deadline()),
            1 => channel.write_all(b"request", deadline()),
            _ => channel.drain_after_ack(deadline()),
        };
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("injected helper cancellation"));
        reap(&mut helper, deadline()).unwrap();
        assert!(helper.reaped);
    }
}
