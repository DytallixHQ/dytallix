use super::*;
use crate::component_candidate::*;
use std::fs;

fn bounds() -> Bounds {
    Bounds {
        max_stat_bytes: 4096,
        max_maps_bytes: 16384,
        max_map_entries: 64,
        max_path_bytes: 4096,
        max_unique_files: 16,
        max_file_bytes: 1 << 20,
        max_total_file_bytes: 4 << 20,
        max_elapsed: Duration::from_secs(5),
        allowed_owner_uids: [unsafe { libc::geteuid() }].into_iter().collect(),
    }
}
fn stat(pid: u32, name: &str, state: &str, start: &str) -> Vec<u8> {
    let mut fields = vec!["0"; 24];
    fields[0] = state;
    fields[19] = start;
    format!("{pid} ({name}) {}\n", fields.join(" ")).into_bytes()
}
const MAPS:&str="1000-2000 r-xp 00000000 08:01 100 /fixture/app\n2000-3000 r--p 00001000 08:01 100 /fixture/app\n3000-4000 rw-p 00000000 00:00 0 [heap]\n4000-5000 r--p 00000000 00:00 0 [vvar]\n5000-6000 r-xp 00000000 00:00 0 [vdso]\n";

#[test]
fn stat_parser_binds_pid_and_start_with_spaces_and_parentheses() {
    let b = bounds();
    assert_eq!(
        parse_stat(&stat(42, "name ) with (spaces)", "S", "12345"), 42, &b).unwrap(),
        StartIdentity {
            pid: 42,
            start_ticks: 12345
        }
    );
}
#[test]
fn stat_refuses_wrong_pid_exited_state_missing_fields_and_bad_time() {
    let b = bounds();
    for raw in [
        stat(43, "name", "S", "1"),
        stat(42, "name", "Z", "1"),
        stat(42, "name", "S", "0"),
        stat(42, "name", "S", "-1"),
        b"42 (name) S 0\n".to_vec(),
        b"42 name S 0\n".to_vec(),
    ] {
        assert!(parse_stat(&raw, 42, &b).is_err());
    }
}
#[test]
fn stat_and_maps_byte_and_count_bounds_are_applied() {
    let mut b = bounds();
    b.max_stat_bytes = 1;
    assert!(parse_stat(&stat(42, "name", "S", "1"), 42, &b).is_err());
    let mut b = bounds();
    b.max_maps_bytes = 1;
    assert!(parse_maps(MAPS.as_bytes(), &b).is_err());
    let mut b = bounds();
    b.max_map_entries = 1;
    assert!(parse_maps(MAPS.as_bytes(), &b).is_err());
    let mut b = bounds();
    b.max_path_bytes = 1;
    assert!(parse_maps(MAPS.as_bytes(), &b).is_err());
}
#[test]
fn maps_parser_preserves_kernel_and_noncode_regions_separately() {
    let entries = parse_maps(MAPS.as_bytes(), &bounds()).unwrap();
    assert_eq!(entries.len(), 5);
    assert_eq!(entries[0].kind, MappingKind::File("/fixture/app".into()));
    assert!(entries[0].executable());
    assert_eq!(entries[2].kind, MappingKind::Anonymous);
    assert_eq!(entries[3].kind, MappingKind::Kernel("[vvar]".into()));
    assert_eq!(entries[4].kind, MappingKind::Kernel("[vdso]".into()));
}
#[test]
fn maps_path_with_spaces_is_retained_without_process_discovery() {
    let m = parse_maps(
        b"1000-2000 r-xp 0 08:01 12 /private/path with spaces\n",
        &bounds(),
    )
    .unwrap();
    assert_eq!(
        m[0].kind,
        MappingKind::File("/private/path with spaces".into())
    );
}
#[test]
fn maps_refuse_deleted_unknown_executable_anonymous_and_unverifiable_names() {
    for raw in [
        "1000-2000 r-xp 0 08:01 12 /a (deleted)\n",
        "1000-2000 r-xp 0 00:00 0\n",
        "1000-2000 r-xp 0 00:00 0 [anon:claimed-vdso]\n",
        "1000-2000 r-xp 0 08:01 12\n",
        "1000-2000 r-xp 0 08:01 12 /a\\012b\n",
        "1000-2000 r--p 0 00:00 0 [made-up]\n",
    ] {
        assert!(parse_maps(raw.as_bytes(), &bounds()).is_err(), "{raw}");
    }
}
#[test]
fn maps_require_nonoverlapping_order_valid_fields_and_complete_snapshot() {
    for raw in [
        "2000-1000 r-xp 0 08:01 12 /a\n",
        "1000-2000 r-xp 0 08:01 12 /a\n1800-3000 r--p 0 08:01 12 /a\n",
        "1000-2000 rwxx 0 08:01 12 /a\n",
        "1000-2000 r-xp 0 08 12 /a\n",
        "1000-2000 r-xp 0 08:01 -1 /a\n",
        "1000-2000 r-xp 0 08:01 12 /a",
        "10000000000000000-20000000000000000 r-xp 0 08:01 12 /a\n",
    ] {
        assert!(parse_maps(raw.as_bytes(), &bounds()).is_err(), "{raw}");
    }
}
#[test]
fn kernel_mapping_exceptions_have_exact_permissions_and_identity() {
    for raw in [
        "1000-2000 rwxp 0 00:00 0 [vdso]\n",
        "1000-2000 r-xp 0 00:00 0 [vvar]\n",
        "1000-2000 r-xp 1 00:00 0 [vdso]\n",
        "1000-2000 r-xp 0 08:01 0 [vdso]\n",
    ] {
        assert!(parse_maps(raw.as_bytes(), &bounds()).is_err());
    }
    assert!(parse_maps(b"1000-2000 --xp 0 00:00 0 [vsyscall]\n", &bounds()).is_ok());
}
#[test]
fn anonymous_nonexecuting_names_are_not_kernel_evidence() {
    for path in [
        "",
        "[heap]",
        "[stack]",
        "[anon:cache]",
        "[anon_shmem:cache]",
        "[stack:42]",
    ] {
        let m = parse_maps(
            format!("1000-2000 rw-p 0 00:00 0 {path}\n").as_bytes(),
            &bounds(),
        )
        .unwrap();
        assert_eq!(m[0].kind, MappingKind::Anonymous);
    }
}
#[test]
fn device_identity_includes_extended_major_minor_bits() {
    let major = 0x12345u64;
    let minor = 0x123456u64;
    let dev = ((major & 0xfff) << 8)
        | ((major & !0xfff) << 32)
        | (minor & 0xff)
        | ((minor & !0xff) << 12);
    assert_eq!(device_parts(dev), (major, minor));
    let m = parse_maps(
        format!("1000-2000 r-xp 0 {major:x}:{minor:x} 123 /a\n").as_bytes(),
        &bounds(),
    )
    .unwrap();
    assert!(map_identity(
        &m[0],
        &FileIdentity {
            device: dev,
            inode: 123
        }
    ));
    assert!(!map_identity(
        &m[0],
        &FileIdentity {
            device: dev,
            inode: 124
        }
    ));
}
#[test]
fn all_observation_limits_and_owner_policy_are_explicit() {
    for index in 0..9 {
        let mut b = bounds();
        match index {
            0 => b.max_stat_bytes = 0,
            1 => b.max_maps_bytes = 0,
            2 => b.max_map_entries = 0,
            3 => b.max_path_bytes = 0,
            4 => b.max_unique_files = 0,
            5 => b.max_file_bytes = 0,
            6 => b.max_total_file_bytes = 0,
            7 => b.max_elapsed = Duration::ZERO,
            _ => b.allowed_owner_uids.clear(),
        };
        assert!(b.validate().is_err());
    }
}

struct Fixture {
    directory: tempfile::TempDir,
    catalog: VerifiedMemberFiles,
    program: PathBuf,
    scratch: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let base = directory.path().canonicalize().unwrap();
        let program = base.join("program");
        let script = base.join("script");
        let scratch = base.join("scratch");
        fs::create_dir(&scratch).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&scratch, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let contents = b"synthetic helper bytes; not a runnable binary\n";
        fs::write(&program, contents).unwrap();
        fs::write(&script, b"synthetic Python source\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let make = |id: &str, path: &Path, kind: MemberKind| {
            let bytes = fs::read(path).unwrap();
            Member {
                id: id.into(),
                kind,
                bytes: bytes.len() as u64,
                sha256: hex::encode(Sha256::digest(&bytes)),
                sha512: hex::encode(Sha512::digest(&bytes)),
            }
        };
        let executable = make("program", &program, MemberKind::Executable);
        let bootstrap = FileDigest {
            bytes: executable.bytes,
            sha256: executable.sha256.clone(),
            sha512: executable.sha512.clone(),
        };
        let role = |name: &str| Role {
            role: name.into(),
            member_id: if name == "service_supervisor" {
                "script"
            } else {
                "program"
            }
            .into(),
            runtime_profile_id:
                if name.starts_with("supervisor_") || name == "service_supervisor" {
                    "python"
                } else {
                    "native"
                }
                .into(),
        };
        let manifest = ManifestV2 {
            schema: 2,
            chain_id: "development-observer-1".into(),
            app_genesis_sha256: "11".repeat(32),
            target: Target {
                os: "linux".into(),
                arch: "x86_64".into(),
                abi: "gnu".into(),
            },
            migration_registry_sha256: "22".repeat(32),
            service_profile: DEVELOPMENT_PROFILE.into(),
            members: vec![executable, make("script", &script, MemberKind::Script)],
            roles: [
                "consensus_bridge",
                "consensus_engine",
                "consensus_stdio",
                "control_verifier",
                "genesis_bootstrap_verifier",
                "service_supervisor",
                "supervisor_interpreter",
            ]
            .into_iter()
            .map(role)
            .collect(),
            runtime_profiles: vec![
                RuntimeProfile {
                    id: "native".into(),
                    member_ids: vec![],
                    mapping_policy: OBSERVED_CODE_POLICY.into(),
                    interpreted_member_ids: vec![],
                },
                RuntimeProfile {
                    id: "python".into(),
                    member_ids: vec![],
                    mapping_policy: OBSERVED_CODE_POLICY.into(),
                    interpreted_member_ids: vec!["script".into()],
                },
            ],
        };
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let manifest_path = base.join("manifest.json");
        fs::write(&manifest_path, &bytes).unwrap();
        let mapping = LocalMapping {
            schema: 1,
            members: vec![
                MemberPath {
                    id: "program".into(),
                    path: program.clone(),
                },
                MemberPath {
                    id: "script".into(),
                    path: script,
                },
            ],
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
        let limits = ValidationBounds {
            max_manifest_bytes: 32768,
            max_mapping_bytes: 32768,
            max_members: 16,
            max_roles: 16,
            max_runtime_profiles: 16,
            max_references: 64,
            max_id_bytes: 64,
            max_path_bytes: 4096,
            max_member_bytes: 4096,
            max_total_member_bytes: 65536,
        };
        let policy = FilePolicy {
            allowed_owner_uids: [unsafe { libc::geteuid() }].into_iter().collect(),
        };
        let catalog =
            verify_member_files(&manifest_path, &mapping_path, &expected, &limits, &policy)
                .unwrap();
        Self {
            directory,
            catalog,
            program,
            scratch,
        }
    }
}
#[test]
fn private_snapshot_is_an_exact_new_owned_file_with_manifest_identity() {
    let f = Fixture::new();
    let snapshot =
        create_private_snapshot("control_verifier", &f.catalog, &f.scratch, &bounds()).unwrap();
    assert_eq!(snapshot.member_id(), "program");
    assert_eq!(
        snapshot.digest(),
        f.catalog.role_file("control_verifier").unwrap().digest()
    );
    assert_eq!(
        fs::read(snapshot.path()).unwrap(),
        fs::read(&f.program).unwrap()
    );
    assert_ne!(
        snapshot.identity,
        file_identity(&fs::metadata(&f.program).unwrap())
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(snapshot.path()).unwrap().permissions().mode() & 0o777,
            0o500
        );
        assert_eq!(
            fs::metadata(snapshot.path().parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    let path = snapshot.path().to_owned();
    drop(snapshot);
    assert!(!path.exists());
}
#[test]
fn snapshot_refuses_script_role_and_changed_catalog_source_bytes() {
    let f = Fixture::new();
    assert!(
        create_private_snapshot("service_supervisor", &f.catalog, &f.scratch, &bounds()).is_err()
    );
    let mut bytes = fs::read(&f.program).unwrap();
    bytes[0] ^= 1;
    fs::write(&f.program, bytes).unwrap();
    assert!(
        create_private_snapshot("control_verifier", &f.catalog, &f.scratch, &bounds())
            .unwrap_err()
            .to_string()
            .contains("digest mismatch")
    );
    assert_eq!(fs::read_dir(&f.scratch).unwrap().count(), 0);
}
#[test]
fn snapshot_refuses_replaced_catalog_inode_and_member_bounds() {
    let f = Fixture::new();
    let replacement = f.directory.path().join("replacement");
    fs::write(&replacement, fs::read(&f.program).unwrap()).unwrap();
    fs::rename(replacement, &f.program).unwrap();
    assert!(
        create_private_snapshot("control_verifier", &f.catalog, &f.scratch, &bounds()).is_err()
    );
    let f = Fixture::new();
    let mut b = bounds();
    b.max_file_bytes = 1;
    assert!(create_private_snapshot("control_verifier", &f.catalog, &f.scratch, &b).is_err());
}
#[cfg(unix)]
#[test]
fn snapshot_refuses_nonprivate_or_aliased_scratch() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let f = Fixture::new();
    fs::set_permissions(&f.scratch, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        create_private_snapshot("control_verifier", &f.catalog, &f.scratch, &bounds()).is_err()
    );
    let f = Fixture::new();
    let alias = f.directory.path().join("alias");
    symlink(&f.scratch, &alias).unwrap();
    assert!(create_private_snapshot("control_verifier", &f.catalog, &alias, &bounds()).is_err());
}
#[cfg(unix)]
#[test]
fn snapshot_reverification_refuses_mutated_bytes_before_any_spawn() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let snapshot =
        create_private_snapshot("control_verifier", &f.catalog, &f.scratch, &bounds()).unwrap();
    fs::set_permissions(snapshot.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let mut bytes = fs::read(snapshot.path()).unwrap();
    bytes[0] ^= 1;
    fs::write(snapshot.path(), bytes).unwrap();
    assert!(snapshot
        .spawn_observed(&[], &[], &bounds())
        .unwrap_err()
        .to_string()
        .contains("digest mismatch"));
}
#[test]
fn opened_file_hash_checks_owner_and_content_identity() {
    let f = Fixture::new();
    let expected = f.catalog.role_file("control_verifier").unwrap().digest();
    let (actual, _, _) = hash_file(
        File::open(&f.program).unwrap(),
        expected,
        Instant::now(),
        &bounds(),
    )
    .unwrap();
    assert_eq!(&actual, expected);
    let mut b = bounds();
    b.allowed_owner_uids = [unsafe { libc::geteuid() }.wrapping_add(1)]
        .into_iter()
        .collect();
    assert!(hash_file(
        File::open(&f.program).unwrap(),
        expected,
        Instant::now(),
        &b
    )
    .is_err());
}

#[cfg(all(target_os = "linux", feature = "qualification-fixtures"))]
mod linux_owned {
    use super::*;
    use std::io::{BufRead, BufReader};

    fn limits() -> Bounds {
        let mut b = bounds();
        b.max_file_bytes = 128 << 20;
        b.max_total_file_bytes = 512 << 20;
        b.max_elapsed = Duration::from_secs(15);
        b.allowed_owner_uids.insert(0);
        b
    }
    fn fixture(include_libraries: bool) -> Fixture {
        let mut f = Fixture::new();
        let program = PathBuf::from(
            std::env::var_os("DYT_OBSERVER_CHILD")
                .expect("Linux qualification requires exact child binary path"),
        )
        .canonicalize()
        .unwrap();
        let mut manifest = f.catalog.candidate().manifest().clone();
        manifest.target.arch = std::env::consts::ARCH.into();
        manifest.target.abi = if cfg!(target_env = "musl") {
            "musl"
        } else {
            "gnu"
        }
        .into();
        let bytes = fs::read(&program).unwrap();
        let member = manifest
            .members
            .iter_mut()
            .find(|m| m.id == "program")
            .unwrap();
        member.bytes = bytes.len() as u64;
        member.sha256 = hex::encode(Sha256::digest(&bytes));
        member.sha512 = hex::encode(Sha512::digest(&bytes));
        let bootstrap = FileDigest {
            bytes: member.bytes,
            sha256: member.sha256.clone(),
            sha512: member.sha512.clone(),
        };
        let mut mapping = LocalMapping {
            schema: 1,
            members: f
                .catalog
                .files()
                .iter()
                .map(|file| MemberPath {
                    id: file.id().into(),
                    path: if file.id() == "program" {
                        program.clone()
                    } else {
                        file.path().into()
                    },
                })
                .collect(),
        };
        if include_libraries {
            // Synthetic qualification catalog from this test process's own
            // backing files. This is test setup, not independent release approval.
            let current = file_identity(&fs::metadata(std::env::current_exe().unwrap()).unwrap());
            let own_maps = parse_maps(&fs::read("/proc/self/maps").unwrap(), &limits()).unwrap();
            let mut seen = BTreeSet::new();
            let mut library_ids = Vec::new();
            for entry in own_maps {
                if let MappingKind::File(path) = entry.kind {
                    let path = path.canonicalize().unwrap();
                    let identity = file_identity(&fs::metadata(&path).unwrap());
                    if identity == current || !seen.insert((identity.device, identity.inode)) {
                        continue;
                    }
                    let id = format!("library{:03}", library_ids.len());
                    let bytes = fs::read(&path).unwrap();
                    manifest.members.push(Member {
                        id: id.clone(),
                        kind: MemberKind::SharedLibrary,
                        bytes: bytes.len() as u64,
                        sha256: hex::encode(Sha256::digest(&bytes)),
                        sha512: hex::encode(Sha512::digest(&bytes)),
                    });
                    mapping.members.push(MemberPath {
                        id: id.clone(),
                        path,
                    });
                    library_ids.push(id);
                }
            }
            assert!(
                !library_ids.is_empty(),
                "Dynamic Linux qualification must observe its loader/library fixtures"
            );
            for profile in &mut manifest.runtime_profiles {
                profile.member_ids = library_ids.clone();
            }
        }
        manifest.members.sort_by(|a, b| a.id.cmp(&b.id));
        mapping.members.sort_by(|a, b| a.id.cmp(&b.id));
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let manifest_path = f.directory.path().join("linux-manifest.json");
        fs::write(&manifest_path, &bytes).unwrap();
        let mapping_path = f.directory.path().join("linux-mapping.json");
        fs::write(&mapping_path, serde_json::to_vec(&mapping).unwrap()).unwrap();
        let expected = ExpectedRelease {
            manifest_sha512: hex::encode(Sha512::digest(bytes)),
            chain_id: manifest.chain_id.clone(),
            app_genesis_sha256: manifest.app_genesis_sha256.clone(),
            migration_registry_sha256: manifest.migration_registry_sha256.clone(),
            target: manifest.target.clone(),
            bootstrap_verifier: bootstrap,
        };
        let vb = ValidationBounds {
            max_manifest_bytes: 65536,
            max_mapping_bytes: 65536,
            max_members: 64,
            max_roles: 16,
            max_runtime_profiles: 16,
            max_references: 256,
            max_id_bytes: 64,
            max_path_bytes: 4096,
            max_member_bytes: 128 << 20,
            max_total_member_bytes: 512 << 20,
        };
        f.catalog = verify_member_files(
            &manifest_path,
            &mapping_path,
            &expected,
            &vb,
            &FilePolicy {
                allowed_owner_uids: limits().allowed_owner_uids,
            },
        )
        .unwrap();
        f.program = program;
        f
    }
    struct Owned(Child);
    impl Drop for Owned {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn spawn(f: &Fixture) -> Owned {
        Owned(
            Command::new(&f.program)
                .env_clear()
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        )
    }
    fn ready(child: &mut Child) {
        let output = child.stdout.take().unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(output).read_line(&mut line).map(|_| line);
            let _ = send.send(result);
        });
        assert_eq!(
            receive
                .recv_timeout(Duration::from_secs(5))
                .expect("bounded child readiness")
                .unwrap(),
            "OBSERVER_CHILD_READY\n"
        );
    }
    fn stop(child: &mut Child) {
        child.stdin.take().unwrap().write_all(b"x").unwrap();
        assert!(child.wait().unwrap().success());
    }

    #[test]
    fn real_owned_child_executable_and_mappings_match_declared_files() {
        let f = fixture(true);
        let mut child = spawn(&f);
        ready(&mut child.0);
        let identity =
            capture_owned_child(&mut child.0, "control_verifier", &f.catalog, &limits()).unwrap();
        let snapshot = observe_owned_child(
            &mut child.0,
            &identity,
            "control_verifier",
            &f.catalog,
            &limits(),
        )
        .unwrap();
        assert_eq!(snapshot.scope(), "OWNED_PROCESS_AND_MAPPINGS_SNAPSHOT");
        assert!(snapshot.files.len() > 1);
        assert!(snapshot
            .kernel_mappings
            .iter()
            .any(|m| m.kind == MappingKind::Kernel("[vdso]".into())));
        assert_eq!(snapshot.start, *identity.start());
        assert!(!identity.uses_private_snapshot());
        assert!(observe_owned_child(
            &mut child.0,
            &identity,
            "consensus_engine",
            &f.catalog,
            &limits()
        )
        .is_err());
        stop(&mut child.0);
    }
    #[test]
    fn real_private_snapshot_keeps_manifest_digest_and_different_launched_inode() {
        let f = fixture(true);
        let private =
            create_private_snapshot("control_verifier", &f.catalog, &f.scratch, &limits()).unwrap();
        let path = private.path().to_owned();
        let (child, identity) = private.spawn_observed(&[], &[], &limits()).unwrap();
        let mut child = Owned(child);
        ready(&mut child.0);
        assert!(identity.uses_private_snapshot());
        assert_ne!(
            identity.identity.inode,
            f.catalog.role_file("control_verifier").unwrap().inode()
        );
        drop(private);
        assert!(path.exists(), "launch receipt retains private directory");
        let observed = observe_owned_child(
            &mut child.0,
            &identity,
            "control_verifier",
            &f.catalog,
            &limits(),
        )
        .unwrap();
        let executable = observed
            .files
            .iter()
            .find(|file| file.member_id == "program")
            .unwrap();
        assert_eq!(
            &executable.digest,
            f.catalog.role_file("control_verifier").unwrap().digest()
        );
        assert_eq!(executable.inode, identity.identity.inode);
        stop(&mut child.0);
        drop(identity);
        assert!(!path.exists());
    }
    #[test]
    fn undeclared_library_and_exited_child_are_not_qualified() {
        let f = fixture(false);
        let mut child = spawn(&f);
        ready(&mut child.0);
        let identity =
            capture_owned_child(&mut child.0, "control_verifier", &f.catalog, &limits()).unwrap();
        let error = observe_owned_child(
            &mut child.0,
            &identity,
            "control_verifier",
            &f.catalog,
            &limits(),
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("absent from declared runtime profile"));
        stop(&mut child.0);
        assert!(observe_owned_child(
            &mut child.0,
            &identity,
            "control_verifier",
            &f.catalog,
            &limits()
        )
        .is_err());
    }
    #[test]
    fn early_exit_helper_never_counts_as_reusable_process_observation() {
        let f = fixture(true);
        let private =
            create_private_snapshot("control_verifier", &f.catalog, &f.scratch, &limits()).unwrap();
        // Require actual execution. A noexec mount or other spawn failure must fail
        // this test rather than stand in for the intended exited-child refusal.
        let mut exited = Command::new(private.path())
            .arg("--early-exit")
            .env_clear()
            .spawn()
            .expect("private snapshot must actually execute");
        assert!(exited.wait().unwrap().success());
        let error = capture(
            &mut exited,
            private.role.clone(),
            private.member_id.clone(),
            private.digest.clone(),
            private.identity.clone(),
            Some(private.directory.clone()),
            &limits(),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "Owned child exited");

        // Also prove that an issued launch identity stops being usable after exit.
        let (child, identity) = private
            .spawn_observed(&[], &[], &limits())
            .expect("blocking private snapshot must execute and be captured");
        let mut child = Owned(child);
        ready(&mut child.0);
        stop(&mut child.0);
        let error = observe_owned_child(
            &mut child.0,
            &identity,
            "control_verifier",
            &f.catalog,
            &limits(),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Owned child exited or pidfd observation failed"
        );
    }
    #[test]
    fn pinned_receipt_cannot_transfer_to_another_owned_child() {
        let f = fixture(true);
        let mut first = spawn(&f);
        ready(&mut first.0);
        let identity =
            capture_owned_child(&mut first.0, "control_verifier", &f.catalog, &limits()).unwrap();
        stop(&mut first.0);
        let mut second = spawn(&f);
        ready(&mut second.0);
        assert!(observe_owned_child(
            &mut second.0,
            &identity,
            "control_verifier",
            &f.catalog,
            &limits()
        )
        .is_err());
        stop(&mut second.0);
    }
}
