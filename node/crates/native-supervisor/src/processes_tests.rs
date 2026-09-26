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

#[cfg(all(target_os = "linux", feature = "qualification-fixtures"))]
mod linux {
    use super::*;
    use dytallix_release_runtime::component_candidate::*;
    use sha2::{Digest, Sha256, Sha512};
    use std::collections::BTreeSet;
    use std::fs::{self, OpenOptions};
    use std::io::BufRead;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;

    fn observation_bounds() -> observation::Bounds {
        observation::Bounds {
            max_stat_bytes: 65536,
            max_maps_bytes: 1048576,
            max_map_entries: 4096,
            max_path_bytes: 4096,
            max_unique_files: 128,
            max_file_bytes: 128 * 1024 * 1024,
            max_total_file_bytes: 512 * 1024 * 1024,
            max_elapsed: Duration::from_secs(15),
            allowed_owner_uids: [0, unsafe { libc::geteuid() }].into_iter().collect(),
        }
    }
    struct Fixture {
        directory: tempfile::TempDir,
        catalog: VerifiedMemberFiles,
        program: PathBuf,
        service: File,
        state: File,
    }
    impl Fixture {
        fn new(libraries: bool) -> Self {
            let directory = tempfile::tempdir().unwrap();
            let base = directory.path().canonicalize().unwrap();
            fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
            let source = PathBuf::from(
                std::env::var_os("DYT_NATIVE_PROCESS_FIXTURE")
                    .expect("Linux process qualification requires DYT_NATIVE_PROCESS_FIXTURE"),
            );
            assert!(source.is_absolute());
            let program = base.join("program");
            fs::copy(source, &program).unwrap();
            fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
            let mut member_paths =
                vec![("app".to_owned(), program.clone(), MemberKind::Executable)];
            if libraries {
                let own_exe = std::env::current_exe().unwrap().canonicalize().unwrap();
                // Qualification fixture only: declare the test runner's actual
                // loader/library files in an explicitly synthetic catalog.
                let maps = observation::parse_maps(
                    &fs::read("/proc/self/maps").unwrap(),
                    &observation_bounds(),
                )
                .unwrap();
                let paths: BTreeSet<_> = maps
                    .iter()
                    .filter_map(|m| match &m.kind {
                        observation::MappingKind::File(p) if p != &own_exe => {
                            Some(p.canonicalize().unwrap())
                        }
                        _ => None,
                    })
                    .collect();
                for (index, path) in paths.into_iter().enumerate() {
                    member_paths.push((format!("lib{index:03}"), path, MemberKind::SharedLibrary));
                }
            }
            let members: Vec<_> = member_paths
                .iter()
                .map(|(id, path, kind)| {
                    let bytes = fs::read(path).unwrap();
                    Member {
                        id: id.clone(),
                        kind: *kind,
                        bytes: bytes.len() as u64,
                        sha256: hex::encode(Sha256::digest(&bytes)),
                        sha512: hex::encode(Sha512::digest(&bytes)),
                    }
                })
                .collect();
            let bootstrap = FileDigest {
                bytes: members[0].bytes,
                sha256: members[0].sha256.clone(),
                sha512: members[0].sha512.clone(),
            };
            let manifest = ManifestV2 {
                schema: 2,
                chain_id: "native-owner-fixture".into(),
                app_genesis_sha256: "11".repeat(32),
                target: Target {
                    os: "linux".into(),
                    arch: std::env::consts::ARCH.into(),
                    abi: if cfg!(target_env = "musl") {
                        "musl"
                    } else {
                        "gnu"
                    }
                    .into(),
                },
                migration_registry_sha256: "22".repeat(32),
                service_profile: DEVELOPMENT_NATIVE_HTTP_PROFILE.into(),
                members,
                roles: [
                    "consensus_bridge",
                    "consensus_engine",
                    "consensus_stdio",
                    "control_verifier",
                    "genesis_bootstrap_verifier",
                    "http_adapter",
                    "service_supervisor",
                ]
                .into_iter()
                .map(|role| dytallix_release_runtime::component_candidate::Role {
                    role: role.into(),
                    member_id: "app".into(),
                    runtime_profile_id: "native".into(),
                })
                .collect(),
                runtime_profiles: vec![RuntimeProfile {
                    id: "native".into(),
                    member_ids: member_paths
                        .iter()
                        .skip(1)
                        .map(|(id, _, _)| id.clone())
                        .collect(),
                    mapping_policy: OBSERVED_CODE_POLICY.into(),
                    interpreted_member_ids: vec![],
                }],
            };
            let bytes = serde_json::to_vec(&manifest).unwrap();
            let manifest_path = base.join("manifest.json");
            fs::write(&manifest_path, &bytes).unwrap();
            let mapping = LocalMapping {
                schema: 1,
                members: member_paths
                    .into_iter()
                    .map(|(id, path, _)| MemberPath { id, path })
                    .collect(),
            };
            let mapping_path = base.join("mapping.json");
            fs::write(&mapping_path, serde_json::to_vec(&mapping).unwrap()).unwrap();
            let expected = ExpectedRelease {
                manifest_sha512: hex::encode(Sha512::digest(bytes)),
                chain_id: manifest.chain_id.clone(),
                app_genesis_sha256: manifest.app_genesis_sha256.clone(),
                migration_registry_sha256: manifest.migration_registry_sha256.clone(),
                target: manifest.target.clone(),
                bootstrap_verifier: bootstrap,
            };
            let bounds = ValidationBounds {
                max_manifest_bytes: 1048576,
                max_mapping_bytes: 1048576,
                max_members: 128,
                max_roles: 16,
                max_runtime_profiles: 16,
                max_references: 128,
                max_id_bytes: 128,
                max_path_bytes: 4096,
                max_member_bytes: 128 * 1024 * 1024,
                max_total_member_bytes: 512 * 1024 * 1024,
            };
            let catalog = verify_member_files(
                &manifest_path,
                &mapping_path,
                &expected,
                &bounds,
                &FilePolicy {
                    allowed_owner_uids: observation_bounds().allowed_owner_uids,
                },
            )
            .unwrap();
            let lock = |name| {
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(base.join(name))
                    .unwrap()
            };
            let service = lock("service.lock");
            let state = lock("state.lock");
            Self {
                directory,
                catalog,
                program,
                service,
                state,
            }
        }
        fn owner(&self) -> ProcessOwner {
            let mut owner = ProcessOwner::new(
                self.catalog.clone(),
                observation_bounds(),
                process_bounds(),
                [("LANG".into(), "C".into())].into_iter().collect(),
            )
            .unwrap();
            owner
                .attach_lifecycle_leases(&self.service, &self.state)
                .unwrap();
            owner
        }
        fn socket(&self, name: &str) -> PathBuf {
            self.directory.path().join(name)
        }
    }
    fn assert_gone(pid: u32) {
        assert!(
            !PathBuf::from(format!("/proc/{pid}")).exists(),
            "Owned process remains: {pid}"
        );
    }
    fn probe(owner: &mut ProcessOwner) {
        assert_eq!(
            owner
                .exchange_application(b"probe\n", 128, Duration::from_secs(2))
                .unwrap(),
            b"probe\n"
        );
        owner.observe_application().unwrap();
    }
    #[test]
    fn owned_child_has_private_umask_and_creates_mode_0600_file() {
        let fixture = Fixture::new(true);
        let mut owner = fixture.owner();
        let created = fixture.socket("child-created.txt");
        owner
            .start_application(&[
                "--fixture-created-file".into(),
                created.as_os_str().to_owned(),
            ])
            .unwrap();
        let response = owner
            .exchange_application(b"umask\n", 128, Duration::from_secs(2))
            .unwrap();
        assert_eq!(
            std::str::from_utf8(&response)
                .unwrap()
                .trim()
                .parse::<u32>()
                .unwrap(),
            0o077
        );
        let metadata = fs::metadata(&created).unwrap();
        assert!(metadata.is_file());
        assert_eq!(metadata.mode() & 0o7777, 0o600);
        assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
        owner.shutdown().unwrap();
    }
    #[test]
    fn owned_application_probe_maps_and_exact_descriptor_environment_isolation() {
        let fixture = Fixture::new(true);
        let mut owner = fixture.owner();
        owner.start_application(&[]).unwrap();
        let descriptors = owner
            .exchange_application(b"fds\n", 128, Duration::from_secs(2))
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Vec<i32>>(&descriptors).unwrap(),
            vec![0, 1, 2, 5, 6]
        );
        let environment = owner
            .exchange_application(b"environment\n", 1024, Duration::from_secs(2))
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<BTreeMap<String, String>>(&environment).unwrap(),
            [("LANG".into(), "C".into())].into_iter().collect()
        );
        owner.observe_application().unwrap();
        assert_eq!(owner.snapshots().len(), 1);
        let pid = owner.owned_pids()[0].1;
        owner.shutdown().unwrap();
        assert_gone(pid);
    }
    #[test]
    fn bridge_inherits_owned_pipes_and_parent_retains_no_duplicate_endpoints() {
        let fixture = Fixture::new(true);
        let mut owner = fixture.owner();
        owner.start_application(&[]).unwrap();
        probe(&mut owner);
        let socket = fixture.socket("bridge.sock");
        owner.start_bridge(&socket).unwrap();
        assert!(owner.pipes.is_none());
        let mut stream = UnixStream::connect(&socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream.write_all(b"through-owned-bridge\n").unwrap();
        let mut response = String::new();
        std::io::BufReader::new(&stream)
            .read_line(&mut response)
            .unwrap();
        assert_eq!(response, "through-owned-bridge\n");
        let pids = owner.owned_pids();
        drop(owner);
        for (_, pid) in pids {
            assert_gone(pid);
        }
    }
    #[test]
    fn bridge_exit_delivers_application_eof_without_hidden_parent_duplicates() {
        let fixture = Fixture::new(true);
        let mut owner = fixture.owner();
        owner.start_application(&[]).unwrap();
        probe(&mut owner);
        owner.start_bridge(&fixture.socket("bridge.sock")).unwrap();
        signal_owned(&owner.children[1].child, libc::SIGKILL).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !exited(&owner.children[0].child).unwrap() {
            wait_tick(deadline, Duration::from_millis(5)).unwrap();
        }
        owner.shutdown().unwrap();
    }
    #[test]
    fn inherited_lease_descriptors_retain_locks_after_all_parent_copies_close() {
        let fixture = Fixture::new(true);
        let mut owner = fixture.owner();
        owner.start_application(&[]).unwrap();
        let competing = OpenOptions::new()
            .read(true)
            .write(true)
            .open(fixture.socket("service.lock"))
            .unwrap();
        owner.leases.take();
        drop(fixture.service);
        drop(fixture.state);
        assert_ne!(
            unsafe { libc::flock(competing.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        owner.shutdown().unwrap();
        assert_eq!(
            unsafe { libc::flock(competing.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
    }
    #[test]
    fn engine_adapter_start_after_delayed_final_socket_mode_and_all_are_reaped() {
        let fixture = Fixture::new(true);
        let mut owner = fixture.owner();
        owner.start_application(&[]).unwrap();
        probe(&mut owner);
        owner.start_bridge(&fixture.socket("bridge.sock")).unwrap();
        let socket = fixture.socket("engine.sock");
        owner
            .start_engine(
                &[
                    "--fixture-engine".into(),
                    socket.as_os_str().to_owned(),
                    "--fixture-delay-socket-mode".into(),
                ],
                &socket,
            )
            .unwrap();
        owner.start_adapter(&["--fixture-adapter".into()]).unwrap();
        assert_eq!(owner.snapshots().len(), 4);
        let pids = owner.owned_pids();
        owner.shutdown().unwrap();
        for (_, pid) in pids {
            assert_gone(pid);
        }
    }
    #[test]
    fn socket_mode_wait_is_bounded_and_refuses_inode_replacement() {
        use std::os::unix::net::UnixListener;
        let fixture = Fixture::new(true);
        let mut owner = fixture.owner();
        owner.bounds.startup_timeout = Duration::from_millis(50);
        let path = fixture.socket("never-ready.sock");
        let _listener = UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(owner
            .wait_endpoint(&path)
            .unwrap_err()
            .to_string()
            .contains("deadline"));
        owner.bounds.startup_timeout = Duration::from_secs(2);
        let path = fixture.socket("replace.sock");
        let _original = UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        let other = path.clone();
        let replace = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(75));
            fs::remove_file(&other).unwrap();
            let replacement = UnixListener::bind(&other).unwrap();
            fs::set_permissions(&other, fs::Permissions::from_mode(0o600)).unwrap();
            replacement
        });
        let error = owner.wait_endpoint(&path).unwrap_err();
        let _replacement = replace.join().unwrap();
        assert!(
            error.to_string().contains("inode changed")
                || error.to_string().contains("disappeared")
        );
    }
    #[test]
    fn socket_mode_wait_rechecks_private_parent() {
        use std::os::unix::net::UnixListener;
        let fixture = Fixture::new(true);
        let mut owner = fixture.owner();
        let path = fixture.socket("parent-mode.sock");
        let _listener = UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        let parent = fixture.directory.path().to_owned();
        let changed = parent.clone();
        let modify = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(75));
            fs::set_permissions(changed, fs::Permissions::from_mode(0o755)).unwrap();
        });
        let error = owner.wait_endpoint(&path).unwrap_err();
        modify.join().unwrap();
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(error.to_string().contains("service-owned and private"));
    }
    #[test]
    fn blocked_application_probe_deadline_stops_and_reaps_owned_child() {
        let fixture = Fixture::new(true);
        let mut owner = fixture.owner();
        owner.start_application(&[]).unwrap();
        let pid = owner.owned_pids()[0].1;
        let started = Instant::now();
        assert!(owner
            .exchange_application(b"hang\n", 128, Duration::from_millis(50))
            .is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_gone(pid);
        assert!(owner.start_application(&[]).is_err());
    }
    #[test]
    fn cancellation_interrupts_blocked_probe_and_reaps_owned_child() {
        let fixture = Fixture::new(true);
        let mut owner = fixture.owner();
        owner.start_application(&[]).unwrap();
        let pid = owner.owned_pids()[0].1;
        let flag = owner.cancellation_handle();
        let trigger = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            flag.store(true, Ordering::SeqCst);
        });
        let started = Instant::now();
        let error = owner
            .exchange_application(b"hang\n", 128, Duration::from_secs(2))
            .unwrap_err();
        trigger.join().unwrap();
        assert!(format!("{error:#}").contains("cancelled"));
        assert!(is_graceful_cancellation(&error));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_gone(pid);
    }
    #[test]
    fn detected_child_failure_is_not_suppressed_by_concurrent_cancellation() {
        let fixture = Fixture::new(true);
        let mut owner = fixture.owner();
        owner.start_application(&[]).unwrap();
        let pid = owner.owned_pids()[0].1;
        signal_owned(&owner.children[0].child, libc::SIGKILL).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !exited(&owner.children[0].child).unwrap() {
            wait_tick(deadline, Duration::from_millis(5)).unwrap();
        }
        owner.cancellation_handle().store(true, Ordering::SeqCst);
        let error = owner.check_alive().unwrap_err();
        assert!(!is_graceful_cancellation(&error));
        assert!(format!("{error:#}").contains("consensus_stdio exited"));
        assert_gone(pid);
    }
    #[test]
    fn child_exit_without_an_rpc_stops_all_owned_dependents() {
        let fixture = Fixture::new(true);
        let mut owner = fixture.owner();
        owner.start_application(&[]).unwrap();
        probe(&mut owner);
        owner.start_bridge(&fixture.socket("bridge.sock")).unwrap();
        let pids = owner.owned_pids();
        unsafe { libc::kill(pids[0].1 as i32, libc::SIGKILL) };
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if owner.check_alive().is_err() {
                break;
            }
            wait_tick(deadline, Duration::from_millis(5)).unwrap();
        }
        for (_, pid) in pids {
            assert_gone(pid);
        }
    }
    #[test]
    fn unknown_mapped_libraries_fail_without_relaxation_and_cleanup() {
        if cfg!(target_env = "musl") {
            return;
        }
        let fixture = Fixture::new(false);
        let mut owner = fixture.owner();
        owner.start_application(&[]).unwrap();
        let pid = owner.owned_pids()[0].1;
        owner
            .exchange_application(b"probe\n", 128, Duration::from_secs(2))
            .unwrap();
        let error = owner.observe_application().unwrap_err();
        assert!(format!("{error:#}").contains("absent from declared runtime profile"));
        assert_gone(pid);
    }
    #[test]
    fn wrong_start_order_or_stale_endpoint_is_terminal() {
        let fixture = Fixture::new(true);
        let mut owner = fixture.owner();
        assert!(owner.start_adapter(&[]).is_err());
        let mut owner = fixture.owner();
        owner.start_application(&[]).unwrap();
        probe(&mut owner);
        let pid = owner.owned_pids()[0].1;
        let socket = fixture.socket("stale.sock");
        fs::write(&socket, b"stale").unwrap();
        assert!(owner.start_bridge(&socket).is_err());
        assert_gone(pid);
        assert_eq!(fs::read(socket).unwrap(), b"stale");
    }
    #[test]
    fn leases_are_required_distinct_and_block_competing_opens() {
        let fixture = Fixture::new(true);
        let mut owner = ProcessOwner::new(
            fixture.catalog.clone(),
            observation_bounds(),
            process_bounds(),
            BTreeMap::new(),
        )
        .unwrap();
        assert!(owner.start_application(&[]).is_err());
        let mut owner = ProcessOwner::new(
            fixture.catalog.clone(),
            observation_bounds(),
            process_bounds(),
            BTreeMap::new(),
        )
        .unwrap();
        assert!(owner
            .attach_lifecycle_leases(&fixture.service, &fixture.service)
            .is_err());
        let mut owner = fixture.owner();
        owner.start_application(&[]).unwrap();
        let competing = OpenOptions::new()
            .read(true)
            .write(true)
            .open(fixture.socket("service.lock"))
            .unwrap();
        assert_ne!(
            unsafe { libc::flock(competing.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        let pid = owner.owned_pids()[0].1;
        owner.shutdown().unwrap();
        assert_gone(pid);
        // The external authority layer still owns its original lease handles.
        drop(owner);
        drop(fixture.service);
        drop(fixture.state);
        assert_eq!(
            unsafe { libc::flock(competing.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
    }
    #[test]
    fn parent_death_signal_terminates_child_without_pid_adoption() {
        let fixture = Fixture::new(true);
        let marker = fixture.socket("owned.pid");
        let mut launcher = Command::new(&fixture.program)
            .args([
                OsString::from("--fixture-parent-death"),
                marker.as_os_str().to_owned(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !marker.exists() {
            assert!(launcher.try_wait().unwrap().is_none());
            wait_tick(deadline, Duration::from_millis(5)).unwrap();
        }
        let pid: u32 = fs::read_to_string(marker).unwrap().parse().unwrap();
        assert!(PathBuf::from(format!("/proc/{pid}")).exists());
        launcher.kill().unwrap();
        launcher.wait().unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match fs::read_to_string(format!("/proc/{pid}/stat")) {
                Err(_) => break,
                Ok(stat)
                    if stat
                        .rsplit_once(')')
                        .unwrap()
                        .1
                        .trim_start()
                        .starts_with('Z') =>
                {
                    break
                }
                _ => wait_tick(deadline, Duration::from_millis(5)).unwrap(),
            }
        }
    }
}
