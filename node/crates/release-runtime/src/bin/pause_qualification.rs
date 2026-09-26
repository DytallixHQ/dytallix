//! Local qualification fixture only. Never launches a supplied path or PID.
#[cfg(target_os = "linux")]
mod linux {
    use anyhow::{ensure, Context, Result};
    use dytallix_release_runtime::{
        component_candidate::*,
        observation::{
            self,
            controlled_pause::{observe_paused, Budget, Timing},
            Bounds, MappingKind,
        },
    };
    use serde_json::json;
    use sha2::{Digest, Sha256, Sha512};
    use std::{
        collections::BTreeSet,
        fs,
        io::{Read, Write},
        os::{
            fd::AsRawFd,
            unix::{fs::MetadataExt, process::CommandExt},
        },
        path::Path,
        process::{Child, Command, Stdio},
        sync::atomic::AtomicBool,
        time::{Duration, Instant},
    };

    fn digest(id: &str, kind: MemberKind, path: &Path) -> Result<Member> {
        let metadata = path.metadata()?;
        ensure!(
            metadata.len() > 0 && metadata.len() <= 128 << 20,
            "Fixture member size bound"
        );
        let bytes = fs::read(path)?;
        Ok(Member {
            id: id.into(),
            kind,
            bytes: bytes.len() as u64,
            sha256: hex::encode(Sha256::digest(&bytes)),
            sha512: hex::encode(Sha512::digest(&bytes)),
        })
    }
    fn bounds(max_elapsed: Duration) -> Bounds {
        Bounds {
            max_stat_bytes: 65536,
            max_maps_bytes: 2 << 20,
            max_map_entries: 4096,
            max_path_bytes: 4096,
            max_unique_files: 64,
            max_file_bytes: 128 << 20,
            max_total_file_bytes: 512 << 20,
            max_elapsed,
            allowed_owner_uids: [0, unsafe { libc::geteuid() }].into_iter().collect(),
        }
    }
    fn catalog(directory: &Path, program: &Path, bounds: &Bounds) -> Result<VerifiedMemberFiles> {
        let executable = digest("program", MemberKind::Executable, program)?;
        let bootstrap = FileDigest {
            bytes: executable.bytes,
            sha256: executable.sha256.clone(),
            sha512: executable.sha512.clone(),
        };
        let own = program.metadata()?;
        let mut members = vec![executable];
        let mut paths = vec![MemberPath {
            id: "program".into(),
            path: program.into(),
        }];
        let mut libraries = Vec::new();
        let mut seen = BTreeSet::new();
        // A synthetic fixture catalog binds this executable and its current
        // local backing libraries. It is not a production release catalog.
        for mapping in observation::parse_maps(&fs::read("/proc/self/maps")?, bounds)? {
            if let MappingKind::File(path) = mapping.kind {
                let path = path.canonicalize()?;
                let m = path.metadata()?;
                if (m.dev(), m.ino()) == (own.dev(), own.ino()) || !seen.insert((m.dev(), m.ino()))
                {
                    continue;
                }
                let id = format!("library{:03}", libraries.len());
                members.push(digest(&id, MemberKind::SharedLibrary, &path)?);
                paths.push(MemberPath {
                    id: id.clone(),
                    path,
                });
                libraries.push(id);
            }
        }
        ensure!(!libraries.is_empty(), "Dynamic fixture libraries missing");
        members.sort_by(|a, b| a.id.cmp(&b.id));
        paths.sort_by(|a, b| a.id.cmp(&b.id));
        let manifest = ManifestV2 {
            schema: 2,
            chain_id: "qualification-pause-1".into(),
            app_genesis_sha256: "11".repeat(32),
            migration_registry_sha256: "22".repeat(32),
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
            service_profile: DEVELOPMENT_NATIVE_PROFILE.into(),
            members,
            roles: [
                "consensus_bridge",
                "consensus_engine",
                "consensus_stdio",
                "control_verifier",
                "genesis_bootstrap_verifier",
                "service_supervisor",
            ]
            .into_iter()
            .map(|role| Role {
                role: role.into(),
                member_id: "program".into(),
                runtime_profile_id: "native".into(),
            })
            .collect(),
            runtime_profiles: vec![RuntimeProfile {
                id: "native".into(),
                member_ids: libraries,
                mapping_policy: OBSERVED_CODE_POLICY.into(),
                interpreted_member_ids: vec![],
            }],
        };
        let raw = serde_json::to_vec(&manifest)?;
        let mp = directory.join("manifest.json");
        fs::write(&mp, &raw)?;
        let mapping = directory.join("mapping.json");
        fs::write(
            &mapping,
            serde_json::to_vec(&LocalMapping {
                schema: 1,
                members: paths,
            })?,
        )?;
        let expected = ExpectedRelease {
            manifest_sha512: hex::encode(Sha512::digest(raw)),
            chain_id: manifest.chain_id.clone(),
            app_genesis_sha256: manifest.app_genesis_sha256.clone(),
            migration_registry_sha256: manifest.migration_registry_sha256.clone(),
            target: manifest.target,
            bootstrap_verifier: bootstrap,
        };
        verify_member_files(
            &mp,
            &mapping,
            &expected,
            &ValidationBounds {
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
            },
            &FilePolicy {
                allowed_owner_uids: bounds.allowed_owner_uids.clone(),
            },
        )
    }
    fn ready(child: &mut Child) -> Result<()> {
        let output = child.stdout.as_mut().context("Missing fixture stdout")?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            ensure!(Instant::now() < deadline, "Fixture readiness timeout");
            let mut p = libc::pollfd {
                fd: output.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let rc = unsafe { libc::poll(&mut p, 1, 100) };
            ensure!(rc >= 0, "Fixture readiness poll failed");
            if rc == 0 {
                continue;
            }
            ensure!(p.revents == libc::POLLIN, "Fixture readiness stream closed");
            let mut byte = [0];
            output.read_exact(&mut byte)?;
            ensure!(byte == [0x42], "Fixture readiness mismatch");
            return Ok(());
        }
    }
    fn finish(child: &mut Child) -> Result<std::process::ExitStatus> {
        // Closing only this child's private input causes the harmless fixture
        // to exit. No numeric PID signaling or generic process target exists.
        drop(child.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(status);
            }
            ensure!(
                Instant::now() < deadline,
                "Fixture terminal reaping unconfirmed"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    pub fn run() -> Result<()> {
        let args: Vec<_> = std::env::args().skip(1).collect();
        if args == ["--owned-harmless-child"] {
            std::io::stdout().write_all(&[0x42])?;
            std::io::stdout().flush()?;
            let mut byte = [0u8; 1];
            let _ = std::io::stdin().read(&mut byte)?;
            return Ok(());
        }
        ensure!((args.len()==6 || args.len()==8) && args[0]=="--samples" && args[2]=="--total-millis" && args[4]=="--cleanup-reserve-millis", "Usage: --samples N --total-millis N --cleanup-reserve-millis N [--mode paused|unpaused]");
        let mode = if args.len() == 8 {
            ensure!(
                args[6] == "--mode" && matches!(args[7].as_str(), "paused" | "unpaused"),
                "Invalid fixture mode"
            );
            args[7].as_str()
        } else {
            "paused"
        };
        let samples: usize = args[1].parse()?;
        let total: u64 = args[3].parse()?;
        let reserve: u64 = args[5].parse()?;
        ensure!(
            (1..=30).contains(&samples) && total <= 60000 && reserve > 0 && reserve < total,
            "Invalid local qualification limits"
        );
        let budget = Budget {
            total: Duration::from_millis(total),
            cleanup_reserve: Duration::from_millis(reserve),
        };
        let limits = bounds(budget.total - budget.cleanup_reserve);
        let program = std::env::current_exe()?.canonicalize()?;
        let directory = tempfile::tempdir()?;
        let verified = catalog(directory.path(), &program, &limits)?;
        let mut records = Vec::new();
        let cancelled = AtomicBool::new(false);
        for index in 0..samples {
            let parent = std::process::id();
            let mut command = Command::new(&program);
            command
                .arg("--owned-harmless-child")
                .env_clear()
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            unsafe {
                command.pre_exec(move || {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if libc::getppid() as u32 != parent {
                        return Err(std::io::Error::other("Fixture parent changed"));
                    }
                    Ok(())
                });
            }
            let mut child = command.spawn()?;
            let result = (|| {
                ready(&mut child)?;
                let identity = observation::capture_owned_child(
                    &mut child,
                    "consensus_stdio",
                    &verified,
                    &limits,
                )?;
                if mode == "paused" {
                    observe_paused(
                        &mut child,
                        &identity,
                        "consensus_stdio",
                        &verified,
                        &limits,
                        budget,
                        &cancelled,
                    )
                } else {
                    let started = Instant::now();
                    let snapshot = observation::observe_owned_child(
                        &mut child,
                        &identity,
                        "consensus_stdio",
                        &verified,
                        &limits,
                    )?;
                    let elapsed = started.elapsed().as_micros();
                    Ok((
                        snapshot,
                        Timing {
                            total_micros: elapsed,
                            stop_micros: 0,
                            observation_micros: elapsed,
                            release_micros: 0,
                            confirmed_stop_to_final_check_micros: 0,
                            pause_envelope_micros: 0,
                        },
                    ))
                }
            })();
            let cleanup = finish(&mut child);
            let cleanup_ok = cleanup.is_ok();
            let exit_status = cleanup.as_ref().ok().map(|status| status.to_string());
            let natural_exit = cleanup.as_ref().is_ok_and(|status| status.success());
            match result {
                Ok((_snapshot,timing)) if natural_exit => records.push(json!({"sample":index,"status":"PASS_QUALIFICATION_ONLY","timing":timing,"cleanup":"NATURAL_EXIT_AND_TERMINAL_REAP_CONFIRMED","exit_status":exit_status})),
                result => {
                    let error=result.err().map(|e|format!("{e:#}"));let cleanup_error=cleanup.err().map(|e|format!("{e:#}"));
                    records.push(json!({"sample":index,"status":"REFUSED","error":error,"cleanup_confirmed":cleanup_ok,"cleanup_error":cleanup_error,"natural_exit":natural_exit,"exit_status":exit_status}));
                    println!("{}",json!({"scope":"LOCAL_QUALIFICATION_ONLY","mode":mode,"status":"REFUSED","signal_isolation_enforced":false,"production_pause_limit_millis":null,"samples":records}));
                    anyhow::bail!("Qualification sample refused; no retry");
                }
            }
        }
        println!(
            "{}",
            json!({"scope":"LOCAL_QUALIFICATION_ONLY","mode":mode,"status":"PASS_QUALIFICATION_ONLY","signal_isolation_enforced":false,"production_pause_limit_millis":null,"sample_budget_millis":total,"cleanup_reserve_millis":reserve,"fixture_teardown_bound_millis":2000,"samples":records})
        );
        Ok(())
    }
}
#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    linux::run()
}
#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("Qualification fixture requires Linux")
}
