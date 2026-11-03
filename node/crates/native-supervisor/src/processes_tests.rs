use super::*;
fn process_bounds() -> ProcessBounds {
    ProcessBounds {
        startup_timeout: Duration::from_secs(3),
        stop_timeout: Duration::from_millis(200),
        kill_timeout: Duration::from_secs(2),
        poll_interval: Duration::from_millis(5),
        max_argument_bytes: 4096,
        max_environment_bytes: 4096,
        max_probe_request_bytes: 65536,
        max_probe_response_bytes: 65536,
    }
}
#[test]
fn all_process_bounds_are_explicit() {
    for index in 0..8 {
        let mut b = process_bounds();
        match index {
            0 => b.startup_timeout = Duration::ZERO,
            1 => b.stop_timeout = Duration::ZERO,
            2 => b.kill_timeout = Duration::ZERO,
            3 => b.poll_interval = Duration::ZERO,
            4 => b.max_argument_bytes = 0,
            5 => b.max_environment_bytes = 0,
            6 => b.max_probe_request_bytes = 0,
            _ => b.max_probe_response_bytes = 0,
        }
        assert!(b.validate().is_err());
    }
    let mut b = process_bounds();
    b.poll_interval = Duration::from_secs(61);
    assert!(b.validate().is_err());
}
#[test]
fn environment_never_admits_loader_proxy_or_runtime_override_fallbacks() {
    for key in [
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "PYTHONPATH",
        "HTTP_PROXY",
        "PATH",
        "GODEBUG",
    ] {
        assert!(validate_environment(
            &[(key.into(), "fixture".into())].into_iter().collect(),
            4096
        )
        .is_err());
    }
    assert!(
        validate_environment(&[("LANG".into(), "C".into())].into_iter().collect(), 4096).is_ok()
    );
    assert!(
        validate_environment(&[("LANG".into(), "C\0".into())].into_iter().collect(), 4096).is_err()
    );
    assert!(validate_arguments(&[OsString::from("abcd")], 4).is_err());
    assert!(validate_arguments(&[OsString::from("a\0")], 4096).is_err());
}

#[cfg(unix)]
#[test]
fn endpoint_metadata_refuses_wrong_type_and_owner() {
    let directory = tempfile::tempdir().unwrap();
    let regular = directory.path().join("regular");
    std::fs::write(&regular, b"fixture").unwrap();
    assert!(
        endpoint_identity(&std::fs::metadata(regular).unwrap(), unsafe {
            libc::geteuid()
        })
        .is_err()
    );
    let socket = directory.path().join("socket");
    let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let metadata = std::fs::metadata(socket).unwrap();
    assert!(endpoint_identity(&metadata, metadata.uid().wrapping_add(1)).is_err());
    assert!(endpoint_identity(&metadata, metadata.uid()).is_ok());
}
