use super::*;
fn policy() -> StaticHelperPolicy {
    StaticHelperPolicy {
        max_file_bytes: 1024 * 1024,
        max_path_bytes: 4096,
        max_program_headers: 32,
        max_section_headers: 128,
        max_elapsed: Duration::from_secs(5),
    }
}
fn bounds() -> Bounds {
    Bounds {
        max_stat_bytes: 65536,
        max_maps_bytes: 1024 * 1024,
        max_map_entries: 4096,
        max_path_bytes: 4096,
        max_unique_files: 1,
        max_file_bytes: 32 * 1024 * 1024,
        max_total_file_bytes: 32 * 1024 * 1024,
        max_elapsed: Duration::from_secs(10),
        allowed_owner_uids: [0].into_iter().collect(),
    }
}
fn put16(b: &mut [u8], p: usize, v: u16) {
    b[p..p + 2].copy_from_slice(&v.to_le_bytes());
}
fn put32(b: &mut [u8], p: usize, v: u32) {
    b[p..p + 4].copy_from_slice(&v.to_le_bytes());
}
fn put64(b: &mut [u8], p: usize, v: u64) {
    b[p..p + 8].copy_from_slice(&v.to_le_bytes());
}
fn elf() -> Vec<u8> {
    let mut b = vec![0; 256];
    b[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    put16(&mut b, 16, 2);
    put16(&mut b, 18, 62);
    put32(&mut b, 20, 1);
    put64(&mut b, 24, 0x400080);
    put64(&mut b, 32, 64);
    put16(&mut b, 52, 64);
    put16(&mut b, 54, 56);
    put16(&mut b, 56, 1);
    put32(&mut b, 64, 1);
    put32(&mut b, 68, 5);
    put64(&mut b, 80, 0x400000);
    put64(&mut b, 96, 256);
    put64(&mut b, 104, 256);
    put64(&mut b, 112, 4096);
    b
}
#[test]
fn static_elf_accepts_bounded_static_exec_only() {
    validate_elf(&elf(), &policy()).unwrap();
}
#[test]
fn static_elf_refuses_wrong_format_arch_and_pie() {
    for (offset, value) in [
        (0, 0),
        (4, 1),
        (5, 2),
        (7, 9),
        (8, 1),
        (16, 3),
        (18, 183),
        (20, 2),
        (52, 63),
    ] {
        let mut b = elf();
        b[offset] = value;
        assert!(validate_elf(&b, &policy()).is_err(), "offset {offset}");
    }
}
#[test]
fn static_elf_refuses_interp_dynamic_and_unknown_program_headers() {
    for kind in [2, 3, 5, 0x70000001] {
        let mut b = elf();
        put32(&mut b, 64, kind);
        assert!(validate_elf(&b, &policy()).is_err());
    }
}
#[test]
fn static_elf_checks_program_table_bounds_and_extended_numbering() {
    for (pos, value) in [(32, u64::MAX), (32, 240)] {
        let mut b = elf();
        put64(&mut b, pos, value);
        assert!(validate_elf(&b, &policy()).is_err());
    }
    for num in [0, 33, 0xffff] {
        let mut b = elf();
        put16(&mut b, 56, num);
        assert!(validate_elf(&b, &policy()).is_err());
    }
    let mut p = policy();
    p.max_file_bytes = 255;
    assert!(validate_elf(&elf(), &p).is_err());
    let mut b = elf();
    put16(&mut b, 54, 55);
    assert!(validate_elf(&b, &policy()).is_err());
}
#[test]
fn static_elf_checks_load_overflow_alignment_and_entry() {
    for (pos, value) in [
        (72, 255),
        (80, u64::MAX),
        (96, 257),
        (104, 255),
        (112, 3),
        (24, 0x500000),
    ] {
        let mut b = elf();
        put64(&mut b, pos, value);
        assert!(validate_elf(&b, &policy()).is_err(), "offset {pos}");
    }
    for flags in [1, 3, 7, 8] {
        let mut b = elf();
        put32(&mut b, 68, flags);
        assert!(validate_elf(&b, &policy()).is_err());
    }
}
#[test]
fn static_elf_refuses_dynamic_sections_and_section_overflow() {
    for kind in [6, 11] {
        let mut b = elf();
        b.resize(384, 0);
        put64(&mut b, 40, 256);
        put16(&mut b, 58, 64);
        put16(&mut b, 60, 1);
        put32(&mut b, 260, kind);
        assert!(validate_elf(&b, &policy()).is_err());
    }
    let mut b = elf();
    put64(&mut b, 40, u64::MAX);
    put16(&mut b, 58, 64);
    put16(&mut b, 60, 1);
    assert!(validate_elf(&b, &policy()).is_err());
    let mut b = elf();
    put16(&mut b, 60, 1);
    assert!(validate_elf(&b, &policy()).is_err());
    let mut b = elf();
    b.resize(384, 0);
    put64(&mut b, 40, 256);
    put16(&mut b, 58, 64);
    put16(&mut b, 60, 1);
    put16(&mut b, 62, 1);
    assert!(validate_elf(&b, &policy()).is_err());
}
#[test]
fn static_helper_requires_every_policy_bound_and_canonical_digest() {
    for n in 0..5 {
        let mut p = policy();
        match n {
            0 => p.max_file_bytes = 0,
            1 => p.max_path_bytes = 0,
            2 => p.max_program_headers = 0,
            3 => p.max_section_headers = 0,
            _ => p.max_elapsed = Duration::ZERO,
        };
        assert!(p.validate().is_err());
    }
    sha256_text(&"a".repeat(64)).unwrap();
    for text in [
        "A".repeat(64),
        "a".repeat(63),
        "g".repeat(64),
        " ".repeat(64),
    ] {
        assert!(sha256_text(&text).is_err());
    }
}
#[test]
fn static_helper_file_policy_requires_root_links_mode_and_length() {
    file_policy(true, 256, 0, 1, 0o100555, &policy()).unwrap();
    for (regular, len, uid, links, mode) in [
        (false, 256, 0, 1, 0o555),
        (true, 63, 0, 1, 0o555),
        (true, 1048577, 0, 1, 0o555),
        (true, 256, 1000, 1, 0o555),
        (true, 256, 0, 2, 0o555),
        (true, 256, 0, 1, 0o577),
        (true, 256, 0, 1, 0o4555),
        (true, 256, 0, 1, 0o2555),
        (true, 256, 0, 1, 0o1555),
        (true, 256, 0, 1, 0o444),
    ] {
        assert!(file_policy(regular, len, uid, links, mode, &policy()).is_err());
    }
}
#[test]
fn static_helper_mount_must_be_readonly_and_executable() {
    mount_policy(true, false).unwrap();
    for (ro, nx) in [(false, false), (false, true), (true, true)] {
        assert!(mount_policy(ro, nx).is_err());
    }
}
#[test]
fn static_helper_refuses_path_aliases_before_use() {
    assert!(canonical_path(Path::new("relative"), &policy()).is_err());
    assert!(canonical_path(Path::new("/../"), &policy()).is_err());
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("helper");
    std::fs::write(&file, elf()).unwrap();
    #[cfg(unix)]
    {
        let alias = temp.path().join("alias");
        std::os::unix::fs::symlink(&file, &alias).unwrap();
        assert!(canonical_path(&alias, &policy()).is_err());
    }
}
fn digest() -> FileDigest {
    FileDigest {
        bytes: 256,
        sha256: "a".repeat(64),
        sha512: "b".repeat(128),
    }
}
#[test]
fn static_helper_maps_accept_only_its_inode_and_parser_classes() {
    let maps=parse_maps(b"1000-2000 r-xp 00000000 00:01 42 /opt/helper\n2000-3000 rw-p 00000000 00:00 0 [heap]\n3000-4000 r-xp 00000000 00:00 0 [vdso]\n",&bounds()).unwrap();
    let (kernel, anonymous) = static_mappings(
        &maps,
        &FileIdentity {
            device: 1,
            inode: 42,
        },
        &digest(),
    )
    .unwrap();
    assert_eq!(kernel.len(), 1);
    assert_eq!(anonymous, 1);
    assert!(static_mappings(
        &maps,
        &FileIdentity {
            device: 1,
            inode: 43
        },
        &digest()
    )
    .is_err());
}
#[test]
fn static_helper_maps_refuse_extra_file_even_if_not_executable() {
    let maps=parse_maps(b"1000-2000 r-xp 00000000 00:01 42 /opt/helper\n2000-3000 r--p 00000000 00:01 43 /opt/data\n",&bounds()).unwrap();
    assert!(static_mappings(
        &maps,
        &FileIdentity {
            device: 1,
            inode: 42
        },
        &digest()
    )
    .is_err());
}
#[test]
fn static_helper_maps_refuse_bad_offsets_deleted_and_anonymous_code() {
    let maps = parse_maps(b"1000-2000 r-xp 00000100 00:01 42 /opt/helper\n", &bounds()).unwrap();
    assert!(static_mappings(
        &maps,
        &FileIdentity {
            device: 1,
            inode: 42
        },
        &digest()
    )
    .is_err());
    for raw in [
        b"1000-2000 r-xp 00000000 00:00 0\n".as_slice(),
        b"1000-2000 r-xp 00000000 00:01 42 /opt/helper (deleted)\n".as_slice(),
    ] {
        assert!(parse_maps(raw, &bounds()).is_err());
    }
    let maps = parse_maps(b"1000-2000 r--p 00000000 00:01 42 /opt/helper\n", &bounds()).unwrap();
    assert!(static_mappings(
        &maps,
        &FileIdentity {
            device: 1,
            inode: 42
        },
        &digest()
    )
    .is_err());
}
#[test]
fn static_helper_image_refuses_wrong_pin_and_changed_bytes() {
    let original = elf();
    let hash = hex::encode(Sha256::digest(&original));
    let digest = validate_image(&original, &hash, &policy()).unwrap();
    assert_eq!(digest.bytes, 256);
    assert!(validate_image(&original, &"0".repeat(64), &policy()).is_err());
    let mut changed = original.clone();
    changed[255] = 1;
    assert!(validate_image(&changed, &hash, &policy()).is_err());
    let bad = vec![0; 256];
    assert!(validate_image(&bad, &hex::encode(Sha256::digest(&bad)), &policy()).is_err());
}
#[test]
fn static_helper_maps_refuse_writable_executable_file_permissions() {
    for permissions in ["rwxp", "rwxs", "-wxp", "-wxs"] {
        let line = format!("1000-2000 {permissions} 00000000 00:01 42 /opt/helper\n");
        let maps = parse_maps(line.as_bytes(), &bounds()).unwrap();
        let error = static_mappings(
            &maps,
            &FileIdentity {
                device: 1,
                inode: 42,
            },
            &digest(),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Writable executable static helper mapping refused"
        );
    }
    // Writable data stays valid when it does not have execute permission.
    let maps = parse_maps(b"1000-2000 r-xp 00000000 00:01 42 /opt/helper\n2000-3000 rw-p 00000000 00:01 42 /opt/helper\n", &bounds()).unwrap();
    static_mappings(
        &maps,
        &FileIdentity {
            device: 1,
            inode: 42,
        },
        &digest(),
    )
    .unwrap();
}
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
#[test]
fn static_helper_actual_verification_refuses_other_platforms() {
    assert!(verify_file(
        Path::new("/unopened-static-helper"),
        &"a".repeat(64),
        &policy()
    )
    .is_err());
}

#[cfg(all(
    target_os = "linux",
    target_arch = "x86_64",
    feature = "qualification-fixtures"
))]
#[test]
#[ignore = "Requires pinned immutable static helper and public policy arguments on native Linux"]
fn static_helper_native_ready_owned_snapshot() -> Result<()> {
    use std::os::fd::AsRawFd;
    let path = PathBuf::from(std::env::var("DYT_STATIC_HELPER_FIXTURE")?);
    let hash = std::env::var("DYT_STATIC_HELPER_SHA256")?;
    let raw = std::env::var("DYT_STATIC_HELPER_ARGS_JSON")?;
    ensure!(raw.len() <= 8192, "Fixture argument byte bound");
    let args: Vec<String> = serde_json::from_str(&raw)?;
    ensure!(
        args.len() <= 16
            && args
                .iter()
                .all(|s| !s.is_empty() && !s.contains(['\0', '\n', '\r']))
            && args
                .windows(2)
                .filter(|p| p[0] == "--execution-profile"
                    && p[1] == "linux-immutable-observed-helper-v1")
                .count()
                == 1,
        "Explicit observed-helper fixture profile and bounded arguments required"
    );
    let mut p = policy();
    p.max_file_bytes = 32 * 1024 * 1024;
    p.max_elapsed = Duration::from_secs(10);
    let file = verify_file(&path, &hash, &p)?;
    struct Cleanup(Child);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut owned = Cleanup(
        Command::new(file.path())
            .args(args)
            .env_clear()
            .env("LANG", "C")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let stdout = owned.0.stdout.as_mut().context("Fixture stdout missing")?;
    let fd = stdout.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    ensure!(
        flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == 0,
        "Fixture pipe nonblocking mode"
    );
    let ready = b"DYTALLIX-ROOT-READY-v1\n";
    let mut observed = Vec::new();
    let until = Instant::now() + Duration::from_secs(10);
    while observed.len() < ready.len() {
        ensure!(Instant::now() < until, "Fixture READY deadline");
        let mut byte = [0];
        match stdout.read(&mut byte) {
            Ok(1) => observed.push(byte[0]),
            Ok(0) => anyhow::bail!("Fixture exited before READY"),
            Ok(_) => unreachable!(),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5))
            }
            Err(e) => return Err(e.into()),
        }
    }
    ensure!(observed == ready, "Exact fixture READY required");
    let identity = capture_owned_child(&mut owned.0, &file, &bounds())?;
    let snapshot = observe_owned_child(&mut owned.0, &identity, &file, &bounds())?;
    ensure!(
        snapshot.digest() == file.digest()
            && snapshot.inode() == file.inode()
            && snapshot.start().pid == owned.0.id(),
        "Native static helper snapshot binding"
    );
    ensure!(
        snapshot.report()["release_authority"] == false,
        "Snapshot cannot grant release authority"
    );
    // No request enters the pipe. Only the owned test handle is stopped/reaped.
    Ok(())
}
