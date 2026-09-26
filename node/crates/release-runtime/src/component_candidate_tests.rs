use super::*;
use std::fs;
use tempfile::TempDir;

struct Fixture {
    _dir: TempDir,
    manifest: ManifestV2,
    expected: ExpectedRelease,
    mapping: LocalMapping,
    manifest_path: PathBuf,
    mapping_path: PathBuf,
    bounds: ValidationBounds,
    policy: FilePolicy,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let kinds = [
            ("app", MemberKind::Executable),
            ("bridge", MemberKind::Executable),
            ("engine", MemberKind::Executable),
            ("helper", MemberKind::Executable),
            ("interpreter", MemberKind::Executable),
            ("library", MemberKind::SharedLibrary),
            ("module", MemberKind::InterpretedModule),
            ("script", MemberKind::Script),
        ];
        let mut members = Vec::new();
        let mut paths = Vec::new();
        for (id, kind) in kinds {
            // Deliberately synthetic file bytes. No executable-format or role
            // execution claim is made by this catalog/file harness.
            let bytes = format!("bounded synthetic fixture for {id}\n").into_bytes();
            let path = base.join(id);
            fs::write(&path, &bytes).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(
                    &path,
                    fs::Permissions::from_mode(if kind == MemberKind::Executable {
                        0o700
                    } else {
                        0o600
                    }),
                )
                .unwrap();
            }
            members.push(Member {
                id: id.into(),
                kind,
                bytes: bytes.len() as u64,
                sha256: hex::encode(Sha256::digest(&bytes)),
                sha512: hex::encode(Sha512::digest(&bytes)),
            });
            paths.push(MemberPath {
                id: id.into(),
                path,
            });
        }
        let role = |name: &str, id: &str, profile: &str| Role {
            role: name.into(),
            member_id: id.into(),
            runtime_profile_id: profile.into(),
        };
        let manifest = ManifestV2 {
            schema: 2,
            chain_id: "development-components-1".into(),
            app_genesis_sha256: "11".repeat(32),
            target: Target {
                os: "linux".into(),
                arch: "x86_64".into(),
                abi: "gnu".into(),
            },
            migration_registry_sha256: "22".repeat(32),
            service_profile: DEVELOPMENT_PROFILE.into(),
            members,
            roles: vec![
                role("consensus_bridge", "bridge", "native"),
                role("consensus_engine", "engine", "native"),
                role("consensus_stdio", "app", "native"),
                role("control_verifier", "helper", "native"),
                role("genesis_bootstrap_verifier", "helper", "native"),
                role("service_supervisor", "script", "python"),
                role("supervisor_interpreter", "interpreter", "python"),
            ],
            runtime_profiles: vec![
                RuntimeProfile {
                    id: "native".into(),
                    member_ids: vec!["library".into()],
                    mapping_policy: OBSERVED_CODE_POLICY.into(),
                    interpreted_member_ids: vec![],
                },
                RuntimeProfile {
                    id: "python".into(),
                    member_ids: vec!["library".into()],
                    mapping_policy: OBSERVED_CODE_POLICY.into(),
                    interpreted_member_ids: vec!["module".into(), "script".into()],
                },
            ],
        };
        let bootstrap_verifier =
            member_digest(manifest.members.iter().find(|m| m.id == "helper").unwrap());
        let expected = ExpectedRelease {
            manifest_sha512: String::new(),
            chain_id: manifest.chain_id.clone(),
            app_genesis_sha256: manifest.app_genesis_sha256.clone(),
            migration_registry_sha256: manifest.migration_registry_sha256.clone(),
            target: manifest.target.clone(),
            bootstrap_verifier,
        };
        #[cfg(unix)]
        let owner = {
            use std::os::unix::fs::MetadataExt;
            fs::metadata(&base).unwrap().uid()
        };
        #[cfg(not(unix))]
        let owner = 0;
        let mut f = Self {
            _dir: dir,
            manifest,
            expected,
            mapping: LocalMapping {
                schema: 1,
                members: paths,
            },
            manifest_path: base.join("manifest.json"),
            mapping_path: base.join("mapping.json"),
            bounds: ValidationBounds {
                max_manifest_bytes: 32768,
                max_mapping_bytes: 32768,
                max_members: 32,
                max_roles: 16,
                max_runtime_profiles: 16,
                max_references: 64,
                max_id_bytes: 64,
                max_path_bytes: 4096,
                max_member_bytes: 4096,
                max_total_member_bytes: 32768,
            },
            policy: FilePolicy {
                allowed_owner_uids: [owner].into_iter().collect(),
            },
        };
        f.write();
        f
    }
    fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&self.manifest).unwrap()
    }
    fn write(&mut self) {
        let b = self.bytes();
        self.expected.manifest_sha512 = hex::encode(Sha512::digest(&b));
        fs::write(&self.manifest_path, b).unwrap();
        fs::write(
            &self.mapping_path,
            serde_json::to_vec(&self.mapping).unwrap(),
        )
        .unwrap();
    }
    fn validate(&mut self) -> Result<ValidatedManifest> {
        self.expected.manifest_sha512 = hex::encode(Sha512::digest(self.bytes()));
        validate_manifest_bytes(&self.bytes(), &self.expected, &self.bounds)
    }
    fn check(&mut self) -> Result<VerifiedMemberFiles> {
        self.write();
        verify_member_files(
            &self.manifest_path,
            &self.mapping_path,
            &self.expected,
            &self.bounds,
            &self.policy,
        )
    }
    fn member(&mut self, id: &str) -> &mut Member {
        self.manifest
            .members
            .iter_mut()
            .find(|m| m.id == id)
            .unwrap()
    }
    fn path(&self, id: &str) -> PathBuf {
        self.mapping
            .members
            .iter()
            .find(|m| m.id == id)
            .unwrap()
            .path
            .clone()
    }
    fn refusal(&mut self, part: &str) {
        let e = format!("{:#}", self.validate().unwrap_err());
        assert!(e.contains(part), "{e}");
    }
}

#[test]
fn complete_catalog_and_file_binding_has_only_file_scope() {
    let mut f = Fixture::new();
    let result = f.check().unwrap();
    assert_eq!(result.scope(), "MEMBER_FILES_VERIFIED");
    assert_eq!(result.candidate().manifest(), &f.manifest);
    assert_eq!(result.candidate().sha512(), f.expected.manifest_sha512);
    assert_eq!(result.files().len(), 8);
    let helper = result.role_file("control_verifier").unwrap();
    assert_eq!(helper.id(), "helper");
    assert_eq!(helper.path(), f.path("helper"));
    assert_eq!(helper.digest(), &f.expected.bootstrap_verifier);
    assert!(helper.inode() > 0);
    assert_eq!(
        result
            .role_file("genesis_bootstrap_verifier")
            .unwrap()
            .inode(),
        helper.inode()
    );
    assert!(result.role_file("unknown").is_none());
}

#[test]
fn independent_bootstrap_pin_allows_different_live_verifier() {
    let mut f = Fixture::new();
    // Existing distinct bridge bytes are assigned to the live verifier role.
    f.manifest
        .roles
        .iter_mut()
        .find(|r| r.role == "control_verifier")
        .unwrap()
        .member_id = "bridge".into();
    let result = f.check().unwrap();
    assert_ne!(
        result.role_file("control_verifier").unwrap().digest(),
        result
            .role_file("genesis_bootstrap_verifier")
            .unwrap()
            .digest()
    );
    f.manifest
        .roles
        .iter_mut()
        .find(|r| r.role == "genesis_bootstrap_verifier")
        .unwrap()
        .member_id = "bridge".into();
    f.refusal("independent trusted pin");
}

#[test]
fn authoritative_digest_is_not_taken_from_manifest_or_mapping() {
    let f = Fixture::new();
    let mut expected = f.expected.clone();
    expected.manifest_sha512 = "00".repeat(64);
    assert!(validate_manifest_bytes(&f.bytes(), &expected, &f.bounds)
        .unwrap_err()
        .to_string()
        .contains("authority digest"));
    let mut f = Fixture::new();
    f.expected.bootstrap_verifier.sha512 = "00".repeat(64);
    f.refusal("independent trusted pin");
}

#[test]
fn chain_genesis_registry_target_and_abi_bindings_are_required() {
    for index in 0..5 {
        let mut f = Fixture::new();
        match index {
            0 => f.manifest.chain_id = "different".into(),
            1 => f.manifest.app_genesis_sha256 = "33".repeat(32),
            2 => f.manifest.migration_registry_sha256 = "33".repeat(32),
            3 => f.manifest.target.arch = "aarch64".into(),
            _ => f.manifest.target.abi = "musl".into(),
        };
        f.refusal("binding mismatch");
    }
    let mut f = Fixture::new();
    f.expected.target.os = "macos".into();
    f.refusal("Unsupported development candidate target");
}

#[test]
fn noncanonical_missing_duplicate_null_and_unknown_fields_refused() {
    for index in 0..9 {
        let f = Fixture::new();
        let mut bytes = f.bytes();
        match index {
            0 => bytes.push(b'\n'),
            1 => bytes = serde_json::to_vec_pretty(&f.manifest).unwrap(),
            2 => {
                bytes = String::from_utf8(bytes)
                    .unwrap()
                    .replacen("\"schema\":2", "\"schema\":2,\"schema\":2", 1)
                    .into_bytes()
            }
            3 => {
                bytes = String::from_utf8(bytes)
                    .unwrap()
                    .replacen("\"schema\":2,", "", 1)
                    .into_bytes()
            }
            4 => {
                bytes = String::from_utf8(bytes)
                    .unwrap()
                    .replacen("\"schema\":2", "\"schema\":null", 1)
                    .into_bytes()
            }
            5 => {
                bytes.pop();
                bytes.extend_from_slice(b",\"unexpected\":false}");
            }
            6 => {
                bytes = String::from_utf8(bytes)
                    .unwrap()
                    .replacen("\"schema\":2", "\"schema\":2.0", 1)
                    .into_bytes()
            }
            7 => {
                bytes = String::from_utf8(bytes)
                    .unwrap()
                    .replacen("\"os\":\"linux\"", "\"os\":\"linux\",\"extra\":1", 1)
                    .into_bytes()
            }
            _ => bytes = b"{broken".to_vec(),
        }
        let mut expected = f.expected.clone();
        expected.manifest_sha512 = hex::encode(Sha512::digest(&bytes));
        assert!(
            validate_manifest_bytes(&bytes, &expected, &f.bounds).is_err(),
            "case {index}"
        );
    }
}

#[test]
fn nested_null_missing_and_unknown_catalog_fields_refused() {
    for index in 0..6 {
        let f = Fixture::new();
        let mut v = serde_json::to_value(&f.manifest).unwrap();
        match index {
            0 => v["members"][0]["sha512"] = serde_json::Value::Null,
            1 => {
                v["members"][0].as_object_mut().unwrap().remove("kind");
            }
            2 => v["roles"][0]["override"] = true.into(),
            3 => v["runtime_profiles"][0]["member_ids"] = serde_json::Value::Null,
            4 => v["runtime_profiles"][0]["exceptions"] = serde_json::json!([]),
            _ => v["members"] = serde_json::Value::Null,
        };
        let bytes = serde_json::to_vec(&v).unwrap();
        let mut expected = f.expected.clone();
        expected.manifest_sha512 = hex::encode(Sha512::digest(&bytes));
        assert!(validate_manifest_bytes(&bytes, &expected, &f.bounds).is_err());
    }
}

#[test]
fn sorted_unique_arrays_enforced() {
    for index in 0..5 {
        let mut f = Fixture::new();
        match index {
            0 => f.manifest.members.swap(0, 1),
            1 => f.manifest.roles.swap(0, 1),
            2 => f.manifest.runtime_profiles.swap(0, 1),
            3 => f.manifest.runtime_profiles[1]
                .interpreted_member_ids
                .reverse(),
            _ => f.manifest.runtime_profiles[0]
                .member_ids
                .push("library".into()),
        };
        f.refusal("sorted and unique");
    }
    let mut f = Fixture::new();
    f.manifest.members.insert(1, f.manifest.members[0].clone());
    f.refusal("sorted and unique");
}

#[test]
fn required_roles_and_http_profile_are_exact() {
    let mut f = Fixture::new();
    let roles = f.manifest.roles.clone();
    for role in roles {
        let mut f = Fixture::new();
        f.manifest.roles.retain(|r| r.role != role.role);
        f.refusal("required service roles");
    }
    f.manifest.roles.push(Role {
        role: "unknown".into(),
        member_id: "app".into(),
        runtime_profile_id: "native".into(),
    });
    f.refusal("required service roles");
    let mut f = Fixture::new();
    f.manifest.service_profile = DEVELOPMENT_HTTP_PROFILE.into();
    f.refusal("required service roles");
    f.manifest.roles.push(Role {
        role: "http_adapter".into(),
        member_id: "bridge".into(),
        runtime_profile_id: "native".into(),
    });
    f.manifest.roles.sort_by(|a, b| a.role.cmp(&b.role));
    assert!(f.check().is_ok());
    f.manifest.service_profile = DEVELOPMENT_PROFILE.into();
    f.refusal("required service roles");
}

#[test]
fn unknown_and_production_profiles_and_schema_fail() {
    for profile in [
        "production-linux-python-service-v2",
        "development-native-v2",
        "",
    ] {
        let mut f = Fixture::new();
        f.manifest.service_profile = profile.into();
        f.refusal("Unsupported development service profile");
    }
    let mut f = Fixture::new();
    f.manifest.schema = 1;
    f.refusal("Unsupported candidate schema");
    let mut f = Fixture::new();
    f.manifest.runtime_profiles[0].mapping_policy = "ignore-unknown".into();
    f.refusal("Unknown mapping policy");
}

#[test]
fn role_and_runtime_member_kind_mismatches_fail() {
    for (id, kind) in [
        ("app", MemberKind::Script),
        ("script", MemberKind::Executable),
        ("library", MemberKind::Executable),
        ("module", MemberKind::SharedLibrary),
        ("interpreter", MemberKind::InterpretedModule),
    ] {
        let mut f = Fixture::new();
        f.member(id).kind = kind;
        f.refusal("kind mismatch");
    }
    let mut f = Fixture::new();
    f.manifest.runtime_profiles[0].interpreted_member_ids = vec!["module".into()];
    f.refusal("Native role");
}

#[test]
fn missing_references_unused_members_and_profiles_fail() {
    let mut f = Fixture::new();
    f.manifest.roles[0].member_id = "absent".into();
    f.refusal("Role member reference missing");
    let mut f = Fixture::new();
    f.manifest.roles[0].runtime_profile_id = "absent".into();
    f.refusal("Role runtime profile missing");
    let mut f = Fixture::new();
    f.manifest.runtime_profiles[0].member_ids = vec!["absent".into()];
    f.refusal("Runtime member reference missing");
    let mut f = Fixture::new();
    f.manifest.runtime_profiles[1]
        .interpreted_member_ids
        .retain(|id| id != "module");
    f.refusal("Unused candidate member");
    let mut f = Fixture::new();
    let mut p = f.manifest.runtime_profiles[0].clone();
    p.id = "unused".into();
    f.manifest.runtime_profiles.push(p);
    f.refusal("Unused runtime profile");
}

#[test]
fn interpreter_script_profile_is_bound_without_claiming_import_closure() {
    let mut f = Fixture::new();
    f.manifest.runtime_profiles[1].interpreted_member_ids = vec!["module".into()];
    f.refusal("Supervisor script");
    let mut f = Fixture::new();
    f.manifest
        .roles
        .iter_mut()
        .find(|r| r.role == "supervisor_interpreter")
        .unwrap()
        .runtime_profile_id = "native".into();
    f.refusal("share one runtime profile");
    let mut f = Fixture::new();
    f.manifest.runtime_profiles[1].interpreted_member_ids = vec!["script".into()];
    f.manifest.members.retain(|m| m.id != "module");
    assert!(f.validate().is_ok()); // file catalog cannot infer missing Python imports.
}

#[test]
fn content_aliases_digest_encoding_and_identifiers_fail() {
    let mut f = Fixture::new();
    let source = f.manifest.members[0].clone();
    let target = &mut f.manifest.members[1];
    target.bytes = source.bytes;
    target.sha256 = source.sha256;
    target.sha512 = source.sha512;
    f.refusal("Duplicate candidate content");
    let mut f = Fixture::new();
    f.member("app").sha256 = "AB".repeat(32);
    f.refusal("Noncanonical candidate digest");
    let mut f = Fixture::new();
    f.member("app").sha512 = "a".repeat(127);
    f.refusal("Noncanonical candidate digest");
    let mut f = Fixture::new();
    f.manifest.members[0].id = "bad/id".into();
    f.refusal("Invalid candidate identifier");
}

#[test]
fn explicit_zero_bounds_and_count_limits_fail() {
    for index in 0..10 {
        let mut f = Fixture::new();
        match index {
            0 => f.bounds.max_manifest_bytes = 0,
            1 => f.bounds.max_mapping_bytes = 0,
            2 => f.bounds.max_members = 0,
            3 => f.bounds.max_roles = 0,
            4 => f.bounds.max_runtime_profiles = 0,
            5 => f.bounds.max_references = 0,
            6 => f.bounds.max_id_bytes = 0,
            7 => f.bounds.max_path_bytes = 0,
            8 => f.bounds.max_member_bytes = 0,
            _ => f.bounds.max_total_member_bytes = 0,
        };
        f.refusal("explicit and nonzero");
    }
    for (index, part) in [
        (0, "manifest byte bound"),
        (1, "member count"),
        (2, "role count"),
        (3, "profile count"),
        (4, "reference count"),
        (5, "identifier"),
        (6, "per-member"),
        (7, "total member"),
    ] {
        let mut f = Fixture::new();
        match index {
            0 => f.bounds.max_manifest_bytes = 1,
            1 => f.bounds.max_members = 1,
            2 => f.bounds.max_roles = 1,
            3 => f.bounds.max_runtime_profiles = 1,
            4 => f.bounds.max_references = 1,
            5 => f.bounds.max_id_bytes = 1,
            6 => f.bounds.max_member_bytes = 1,
            _ => f.bounds.max_total_member_bytes = 1,
        };
        f.refusal(part);
    }
}

#[test]
fn byte_sum_overflow_and_zero_member_are_rejected() {
    let mut f = Fixture::new();
    f.member("app").bytes = 0;
    f.refusal("per-member");
    let mut f = Fixture::new();
    f.bounds.max_member_bytes = u64::MAX;
    f.bounds.max_total_member_bytes = u64::MAX;
    f.member("app").bytes = u64::MAX;
    f.refusal("total byte overflow");
}

#[test]
fn local_mapping_exact_ids_order_paths_and_schema_are_required() {
    for index in 0..6 {
        let mut f = Fixture::new();
        let valid = f.validate().unwrap();
        match index {
            0 => f.mapping.schema = 2,
            1 => {
                f.mapping.members.pop();
            }
            2 => f.mapping.members.swap(0, 1),
            3 => f.mapping.members[0].id = "absent".into(),
            4 => f.mapping.members[0].path = PathBuf::from("relative"),
            _ => f.mapping.members[1].path = f.mapping.members[0].path.clone(),
        };
        assert!(validate_mapping_bytes(
            &serde_json::to_vec(&f.mapping).unwrap(),
            &valid,
            &f.bounds
        )
        .is_err());
    }
    let mut f = Fixture::new();
    let valid = f.validate().unwrap();
    f.bounds.max_mapping_bytes = 1;
    assert!(
        validate_mapping_bytes(&serde_json::to_vec(&f.mapping).unwrap(), &valid, &f.bounds)
            .is_err()
    );
    let mut f = Fixture::new();
    let valid = f.validate().unwrap();
    f.bounds.max_path_bytes = 1;
    assert!(
        validate_mapping_bytes(&serde_json::to_vec(&f.mapping).unwrap(), &valid, &f.bounds)
            .is_err()
    );
}

#[test]
fn mapping_noncanonical_duplicate_unknown_and_null_fail() {
    let mut f = Fixture::new();
    let valid = f.validate().unwrap();
    let original = serde_json::to_vec(&f.mapping).unwrap();
    for index in 0..4 {
        let mut bytes = original.clone();
        match index {
            0 => bytes.push(b'\n'),
            1 => {
                bytes = String::from_utf8(bytes)
                    .unwrap()
                    .replacen("\"schema\":1", "\"schema\":1,\"schema\":1", 1)
                    .into_bytes()
            }
            2 => {
                bytes.pop();
                bytes.extend_from_slice(b",\"authority\":true}");
            }
            _ => bytes = b"{\"schema\":1,\"members\":null}".to_vec(),
        };
        assert!(validate_mapping_bytes(&bytes, &valid, &f.bounds).is_err());
    }
}

#[test]
fn changed_members_fail_both_hashes_and_sizes() {
    for id in [
        "app",
        "bridge",
        "engine",
        "helper",
        "interpreter",
        "library",
        "module",
        "script",
    ] {
        let mut f = Fixture::new();
        let path = f.path(id);
        let mut b = fs::read(&path).unwrap();
        b[0] ^= 1;
        fs::write(&path, &b).unwrap();
        assert!(
            f.check().unwrap_err().to_string().contains("hashes differ"),
            "{id}"
        );
    }
    let mut f = Fixture::new();
    f.member("app").sha512 = "00".repeat(64);
    assert!(f.check().unwrap_err().to_string().contains("hashes differ"));
    let mut f = Fixture::new();
    fs::write(f.path("library"), []).unwrap();
    assert!(f.check().is_err());
    let mut f = Fixture::new();
    f.member("app").bytes += 1;
    assert!(f
        .check()
        .unwrap_err()
        .to_string()
        .contains("length mismatch"));
}

#[cfg(unix)]
fn unsafe_mode_fixture(directory: &std::ffi::OsStr, mode: u32) -> PathBuf {
    use std::os::unix::fs::MetadataExt;
    let directory = PathBuf::from(directory);
    assert!(
        directory.is_absolute(),
        "Unsafe-mode fixture directory must be absolute"
    );
    assert_eq!(
        directory
            .canonicalize()
            .expect("Unsafe-mode fixture directory must exist"),
        directory,
        "Unsafe-mode fixture directory must be canonical"
    );
    let metadata = fs::symlink_metadata(&directory).expect("Unsafe-mode directory metadata");
    assert!(
        metadata.is_dir(),
        "Unsafe-mode fixture root must be a directory"
    );
    assert_eq!(
        metadata.uid(),
        0,
        "Unsafe-mode fixture directory must be root-owned"
    );
    assert_eq!(
        metadata.mode() & 0o7777,
        0o555,
        "Unsafe-mode fixture directory must have mode 0555"
    );
    let name = match mode {
        0o4755 => "app-setuid",
        0o2755 => "app-setgid",
        _ => panic!("Unsupported external unsafe-mode fixture"),
    };
    let path = directory.join(name);
    assert_eq!(
        path.canonicalize().expect("Unsafe-mode fixture must exist"),
        path,
        "Unsafe-mode fixture path must be canonical"
    );
    let metadata = fs::symlink_metadata(&path).expect("Unsafe-mode fixture metadata");
    assert!(
        metadata.is_file(),
        "Unsafe-mode fixture must be a regular file"
    );
    assert_eq!(metadata.uid(), 0, "Unsafe-mode fixture must be root-owned");
    assert_eq!(
        metadata.nlink(),
        1,
        "Unsafe-mode fixture must have exactly one link"
    );
    assert_eq!(
        metadata.mode() & 0o7777,
        mode,
        "Unsafe-mode fixture mode differs"
    );
    assert_eq!(
        metadata.len(),
        b"bounded synthetic fixture for app\n".len() as u64,
        "Unsafe-mode fixture size differs"
    );
    assert_eq!(
        fs::read(&path).expect("Unsafe-mode fixture read"),
        b"bounded synthetic fixture for app\n",
        "Unsafe-mode fixture bytes differ"
    );
    path
}

#[cfg(unix)]
#[test]
fn local_file_aliases_unsafe_modes_owner_and_nonfiles_fail() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    // Restricted units cannot create setuid/setgid files. An explicit fixture
    // input selects trusted precreated inert files; invalid input never falls
    // back to chmod or turns a failed setup into a verifier success.
    let external_modes = std::env::var_os("DYT_RUNTIME_UNSAFE_MODE_FIXTURES");
    for mode in [0o666, 0o4755, 0o2755, 0o440] {
        let mut f = Fixture::new();
        if let Some(directory) = external_modes.as_ref().filter(|_| mode & 0o6000 != 0) {
            let path = unsafe_mode_fixture(directory, mode);
            f.mapping
                .members
                .iter_mut()
                .find(|m| m.id == "app")
                .unwrap()
                .path = path;
            // This adds the fixture owner only to this synthetic test policy.
            f.policy.allowed_owner_uids.insert(0);
        } else {
            fs::set_permissions(f.path("app"), fs::Permissions::from_mode(mode)).unwrap();
        }
        if mode & 0o6000 != 0 {
            assert_eq!(
                f.check().unwrap_err().to_string(),
                "Unsafe candidate permissions or hard-link alias",
                "Special-mode fixture must reach the actual permission guard"
            );
        } else {
            assert!(f.check().is_err());
        }
    }
    let mut f = Fixture::new();
    f.policy.allowed_owner_uids.clear();
    assert!(f.check().is_err());
    let mut f = Fixture::new();
    let own = *f.policy.allowed_owner_uids.iter().next().unwrap();
    f.policy.allowed_owner_uids = [own.wrapping_add(1)].into_iter().collect();
    assert!(f.check().is_err());
    let mut f = Fixture::new();
    let p = f.path("app");
    let link = p.with_file_name("alias");
    symlink(&p, &link).unwrap();
    f.mapping.members[0].path = link;
    assert!(f.check().is_err());
    let mut f = Fixture::new();
    let p = f.path("app");
    fs::hard_link(&p, p.with_file_name("hard-alias")).unwrap();
    assert!(f.check().is_err());
    let mut f = Fixture::new();
    let p = f.path("app");
    f.mapping.members[0].path = p.parent().unwrap().join(".").join("app");
    assert!(f.check().is_err());
    let mut f = Fixture::new();
    f.mapping.members[0].path = f.path("app").parent().unwrap().to_owned();
    assert!(f.check().is_err());
}

#[test]
fn manifest_and_mapping_paths_receive_same_identity_checks() {
    let f = Fixture::new();
    let relative = PathBuf::from("manifest.json");
    assert!(verify_member_files(
        &relative,
        &f.mapping_path,
        &f.expected,
        &f.bounds,
        &f.policy
    )
    .is_err());
    let mut bounds = f.bounds.clone();
    bounds.max_mapping_bytes = 1;
    assert!(verify_member_files(
        &f.manifest_path,
        &f.mapping_path,
        &f.expected,
        &bounds,
        &f.policy
    )
    .is_err());
    let mut bounds = f.bounds.clone();
    bounds.max_manifest_bytes = 1;
    assert!(verify_member_files(
        &f.manifest_path,
        &f.mapping_path,
        &f.expected,
        &bounds,
        &f.policy
    )
    .is_err());
}

#[test]
fn opened_file_mutation_and_path_replacement_are_observed() {
    let f = Fixture::new();
    let path = f.path("library");
    let (file, opened) = open_checked(&path, 4096, &f.bounds, &f.policy, false).unwrap();
    fs::write(&path, b"changed").unwrap();
    assert!(finish_checked(&path, &file, &opened, 4096, &f.bounds, &f.policy, false).is_err());
    let f = Fixture::new();
    let path = f.path("library");
    let (file, opened) = open_checked(&path, 4096, &f.bounds, &f.policy, false).unwrap();
    let replacement = path.with_file_name("replacement");
    fs::write(&replacement, fs::read(&path).unwrap()).unwrap();
    fs::rename(replacement, &path).unwrap();
    assert!(finish_checked(&path, &file, &opened, 4096, &f.bounds, &f.policy, false).is_err());
}

#[test]
fn identical_catalog_accepts_different_exact_local_installation_paths() {
    let mut a = Fixture::new();
    let mut b = Fixture::new();
    assert_eq!(a.bytes(), b.bytes());
    assert_ne!(a.path("app"), b.path("app"));
    let va = a.check().unwrap();
    let vb = b.check().unwrap();
    assert_eq!(va.candidate().sha512(), vb.candidate().sha512());
    assert_eq!(
        va.role_file("consensus_stdio").unwrap().digest(),
        vb.role_file("consensus_stdio").unwrap().digest()
    );
}

#[cfg(unix)]
#[test]
fn public_inputs_reject_manifest_symlink_mapping_hardlink_and_unsafe_modes() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let f = Fixture::new();
    let link = f.manifest_path.with_file_name("manifest-alias");
    symlink(&f.manifest_path, &link).unwrap();
    assert!(
        verify_member_files(&link, &f.mapping_path, &f.expected, &f.bounds, &f.policy).is_err()
    );
    let f = Fixture::new();
    fs::hard_link(
        &f.mapping_path,
        f.mapping_path.with_file_name("mapping-alias"),
    )
    .unwrap();
    assert!(verify_member_files(
        &f.manifest_path,
        &f.mapping_path,
        &f.expected,
        &f.bounds,
        &f.policy
    )
    .is_err());
    let f = Fixture::new();
    fs::set_permissions(&f.manifest_path, fs::Permissions::from_mode(0o666)).unwrap();
    assert!(verify_member_files(
        &f.manifest_path,
        &f.mapping_path,
        &f.expected,
        &f.bounds,
        &f.policy
    )
    .is_err());
}

fn native_fixture(http: bool) -> Fixture {
    let mut f = Fixture::new();
    f.manifest.service_profile = if http {
        DEVELOPMENT_NATIVE_HTTP_PROFILE
    } else {
        DEVELOPMENT_NATIVE_PROFILE
    }
    .into();
    f.manifest
        .roles
        .retain(|r| r.role != "supervisor_interpreter");
    let supervisor = f
        .manifest
        .roles
        .iter_mut()
        .find(|r| r.role == "service_supervisor")
        .unwrap();
    supervisor.member_id = "interpreter".into(); // Synthetic executable, now used as native supervisor.
    supervisor.runtime_profile_id = "native".into();
    f.manifest.runtime_profiles.retain(|p| p.id == "native");
    f.manifest
        .members
        .retain(|m| m.id != "script" && m.id != "module");
    f.mapping
        .members
        .retain(|m| m.id != "script" && m.id != "module");
    if http {
        f.manifest.roles.push(Role {
            role: "http_adapter".into(),
            member_id: "bridge".into(),
            runtime_profile_id: "native".into(),
        });
        f.manifest.roles.sort_by(|a, b| a.role.cmp(&b.role));
    }
    f
}

#[test]
fn native_profiles_verify_complete_member_files_without_interpreted_roles() {
    for http in [false, true] {
        let mut f = native_fixture(http);
        let verified = f.check().unwrap();
        assert_eq!(verified.scope(), "MEMBER_FILES_VERIFIED");
        assert_eq!(
            verified.candidate().manifest().roles.len(),
            if http { 7 } else { 6 }
        );
        assert!(verified
            .candidate()
            .manifest()
            .runtime_profiles
            .iter()
            .all(|p| p.interpreted_member_ids.is_empty()));
    }
}

#[test]
fn native_profile_rejects_each_missing_required_role() {
    let names: Vec<String> = native_fixture(true)
        .manifest
        .roles
        .iter()
        .map(|r| r.role.clone())
        .collect();
    for name in names {
        let mut f = native_fixture(true);
        f.manifest.roles.retain(|r| r.role != name);
        assert!(
            f.validate()
                .unwrap_err()
                .to_string()
                .contains("required service roles"),
            "{name}"
        );
    }
}

#[test]
fn native_profile_cannot_silently_omit_python_or_import_members() {
    let mut f = Fixture::new();
    f.manifest.service_profile = DEVELOPMENT_NATIVE_PROFILE.into();
    assert!(f
        .validate()
        .unwrap_err()
        .to_string()
        .contains("required service roles"));
    let mut f = native_fixture(false);
    f.manifest.roles.push(Role {
        role: "supervisor_interpreter".into(),
        member_id: "interpreter".into(),
        runtime_profile_id: "native".into(),
    });
    assert!(f
        .validate()
        .unwrap_err()
        .to_string()
        .contains("required service roles"));
    let mut f = native_fixture(false);
    f.member("interpreter").kind = MemberKind::Script;
    assert!(f
        .validate()
        .unwrap_err()
        .to_string()
        .contains("Role member kind mismatch"));
    let mut f = native_fixture(false);
    f.member("library").kind = MemberKind::InterpretedModule;
    let profile = &mut f.manifest.runtime_profiles[0];
    profile.member_ids.clear();
    profile.interpreted_member_ids.push("library".into());
    assert!(f
        .validate()
        .unwrap_err()
        .to_string()
        .contains("Native role cannot claim interpreted members"));
}

#[test]
fn native_adapter_presence_requires_the_exact_http_profile() {
    let mut f = native_fixture(true);
    f.manifest.service_profile = DEVELOPMENT_NATIVE_PROFILE.into();
    assert!(f
        .validate()
        .unwrap_err()
        .to_string()
        .contains("required service roles"));
    let mut f = native_fixture(false);
    f.manifest.service_profile = DEVELOPMENT_NATIVE_HTTP_PROFILE.into();
    assert!(f
        .validate()
        .unwrap_err()
        .to_string()
        .contains("required service roles"));
}

#[test]
fn native_profile_preserves_independent_bootstrap_and_release_authority() {
    let mut f = native_fixture(false);
    f.expected.bootstrap_verifier.sha256 = "aa".repeat(32);
    assert!(f
        .validate()
        .unwrap_err()
        .to_string()
        .contains("independent trusted pin"));
    let mut f = native_fixture(false);
    f.write();
    f.expected.manifest_sha512 = "aa".repeat(64);
    assert!(validate_manifest_bytes(&f.bytes(), &f.expected, &f.bounds)
        .unwrap_err()
        .to_string()
        .contains("authority digest mismatch"));
}

#[test]
fn native_profile_does_not_accept_a_production_profile_name() {
    let mut f = native_fixture(false);
    f.manifest.service_profile = "production-linux-native-service-v2".into();
    assert!(f
        .validate()
        .unwrap_err()
        .to_string()
        .contains("production is not qualified"));
}
