// Included in the existing private test module. Reuses its synthetic catalog
// builder without changing any existing fixture or owned-child test.
#[test]
fn current_observation_requires_explicit_bounds_and_linux() {
    let f = Fixture::new();
    let mut b = bounds();
    b.max_stat_bytes = 0;
    assert!(observe_current_process("consensus_stdio", &f.catalog, &b).is_err());
    #[cfg(not(target_os = "linux"))]
    assert!(
        observe_current_process("consensus_stdio", &f.catalog, &bounds())
            .unwrap_err()
            .to_string()
            .contains("requires Linux")
    );
    #[cfg(target_os = "linux")]
    {
        assert!(observe_current_process("missing", &f.catalog, &bounds()).is_err());
        assert!(
            observe_current_process("service_supervisor", &f.catalog, &bounds())
                .unwrap_err()
                .to_string()
                .contains("must be executable")
        );
        assert!(observe_current_process("consensus_stdio", &f.catalog, &bounds()).is_err());
    }
}

#[cfg(all(target_os = "linux", feature = "qualification-fixtures"))]
mod current_process_linux {
    use super::*;

    fn limits() -> Bounds {
        let mut b = bounds();
        b.max_maps_bytes = 128 << 10;
        b.max_map_entries = 1024;
        b.max_unique_files = 64;
        b.max_file_bytes = 256 << 20;
        b.max_total_file_bytes = 512 << 20;
        b.max_elapsed = Duration::from_secs(30);
        b.allowed_owner_uids.insert(0);
        b
    }

    // Synthetic test authority describes this test executable and its current
    // libraries. It is never used by the public observation API to infer trust.
    fn fixture(libraries: bool, copy_executable: bool) -> Fixture {
        let mut f = Fixture::new();
        let mut manifest = f.catalog.candidate().manifest().clone();
        manifest.service_profile = DEVELOPMENT_NATIVE_PROFILE.into();
        manifest.target.arch = std::env::consts::ARCH.into();
        manifest.target.abi = if cfg!(target_env = "musl") {
            "musl"
        } else {
            "gnu"
        }
        .into();
        let actual = std::env::current_exe().unwrap().canonicalize().unwrap();
        let actual_identity = file_identity(&fs::metadata(&actual).unwrap());
        let program = if copy_executable {
            fs::copy(&actual, &f.program).unwrap();
            f.program.clone()
        } else {
            actual
        };
        let make = |id: String, path: &Path, kind: MemberKind| {
            let bytes = fs::read(path).unwrap();
            Member {
                id,
                kind,
                bytes: bytes.len() as u64,
                sha256: hex::encode(Sha256::digest(&bytes)),
                sha512: hex::encode(Sha512::digest(&bytes)),
            }
        };
        let executable = make("program".into(), &program, MemberKind::Executable);
        let bootstrap = FileDigest {
            bytes: executable.bytes,
            sha256: executable.sha256.clone(),
            sha512: executable.sha512.clone(),
        };
        manifest.members = vec![executable];
        manifest
            .roles
            .retain(|r| r.role != "supervisor_interpreter");
        for role in &mut manifest.roles {
            role.member_id = "program".into();
            role.runtime_profile_id = "native".into();
        }
        manifest.runtime_profiles.retain(|p| p.id == "native");
        let mut mapping = LocalMapping {
            schema: 1,
            members: vec![MemberPath {
                id: "program".into(),
                path: program.clone(),
            }],
        };
        if libraries {
            let maps = parse_maps(&fs::read("/proc/self/maps").unwrap(), &limits()).unwrap();
            let mut seen = BTreeSet::new();
            for map in maps {
                if let MappingKind::File(path) = map.kind {
                    let path = path.canonicalize().unwrap();
                    let identity = file_identity(&fs::metadata(&path).unwrap());
                    if identity == actual_identity
                        || !seen.insert((identity.device, identity.inode))
                    {
                        continue;
                    }
                    let id = format!("library{:03}", seen.len());
                    manifest
                        .members
                        .push(make(id.clone(), &path, MemberKind::SharedLibrary));
                    manifest.runtime_profiles[0].member_ids.push(id.clone());
                    mapping.members.push(MemberPath { id, path });
                }
            }
        }
        manifest.members.sort_by(|a, b| a.id.cmp(&b.id));
        mapping.members.sort_by(|a, b| a.id.cmp(&b.id));
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let manifest_path = f.directory.path().join("current-manifest.json");
        fs::write(&manifest_path, &bytes).unwrap();
        let mapping_path = f.directory.path().join("current-mapping.json");
        fs::write(&mapping_path, serde_json::to_vec(&mapping).unwrap()).unwrap();
        let expected = ExpectedRelease {
            manifest_sha512: hex::encode(Sha512::digest(&bytes)),
            chain_id: manifest.chain_id.clone(),
            app_genesis_sha256: manifest.app_genesis_sha256.clone(),
            migration_registry_sha256: manifest.migration_registry_sha256.clone(),
            target: manifest.target.clone(),
            bootstrap_verifier: bootstrap,
        };
        let validation = ValidationBounds {
            max_manifest_bytes: 65536,
            max_mapping_bytes: 65536,
            max_members: 64,
            max_roles: 16,
            max_runtime_profiles: 16,
            max_references: 256,
            max_id_bytes: 64,
            max_path_bytes: 4096,
            max_member_bytes: 256 << 20,
            max_total_member_bytes: 512 << 20,
        };
        f.catalog = verify_member_files(
            &manifest_path,
            &mapping_path,
            &expected,
            &validation,
            &FilePolicy {
                allowed_owner_uids: limits().allowed_owner_uids,
            },
        )
        .unwrap();
        f.program = program;
        f
    }

    #[test]
    fn current_process_observation_binds_both_native_roles_without_child_handle() {
        let f = fixture(true, false);
        for role in ["consensus_stdio", "service_supervisor"] {
            let snapshot = observe_current_process(role, &f.catalog, &limits()).unwrap();
            assert_eq!(snapshot.start().pid, std::process::id());
            assert_eq!(snapshot.role(), role);
            assert_eq!(snapshot.scope(), "CURRENT_PROCESS_AND_MAPPINGS_SNAPSHOT");
            assert_eq!(snapshot.candidate_sha512(), f.catalog.candidate().sha512());
            assert!(snapshot.files().iter().any(|f| f.member_id == "program"));
            assert_eq!(snapshot.report()["continuous_enforcement"], false);
        }
    }

    #[test]
    fn current_process_refuses_identical_bytes_at_another_inode() {
        let f = fixture(true, true);
        let error = observe_current_process("consensus_stdio", &f.catalog, &limits()).unwrap_err();
        assert!(error.to_string().contains("inode changed"), "{error:#}");
    }

    #[test]
    fn current_process_refuses_unlisted_file_mapping() {
        let f = fixture(true, false);
        // This different synthetic regular file is deliberately absent from the
        // profile. Even a read-only non-code mapping must fail current policy.
        let path = f.directory.path().join("unlisted-map");
        fs::write(&path, vec![0u8; 4096]).unwrap();
        let extra = File::open(&path).unwrap();
        use std::os::fd::AsRawFd;
        let pointer = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                4096,
                libc::PROT_READ,
                libc::MAP_PRIVATE,
                extra.as_raw_fd(),
                0,
            )
        };
        assert_ne!(pointer, libc::MAP_FAILED);
        let result = observe_current_process("consensus_stdio", &f.catalog, &limits());
        assert_eq!(unsafe { libc::munmap(pointer, 4096) }, 0);
        let error = result.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("absent from declared runtime profile"),
            "{error:#}"
        );
    }
}
