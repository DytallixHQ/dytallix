//! Explicit disposable service settings. Paths and limits do not grant release authority.
use crate::processes::ProcessBounds;
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{Metadata, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinnedInput {
    pub path: PathBuf,
    pub sha256: String,
    pub max_bytes: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessLimits {
    pub startup_millis: u64,
    pub stop_millis: u64,
    pub kill_millis: u64,
    pub poll_millis: u64,
    pub max_argument_bytes: usize,
    pub max_environment_bytes: usize,
    pub max_probe_request_bytes: usize,
    pub max_probe_response_bytes: usize,
}
impl ProcessLimits {
    pub fn bounds(&self) -> Result<ProcessBounds> {
        ensure!(
            [
                self.startup_millis,
                self.stop_millis,
                self.kill_millis,
                self.poll_millis
            ]
            .iter()
            .all(|v| (1..=60_000).contains(v)),
            "Invalid process time bound"
        );
        ensure!(
            self.poll_millis <= self.startup_millis
                && self.poll_millis <= self.stop_millis
                && self.poll_millis <= self.kill_millis,
            "Poll interval exceeds deadline"
        );
        ensure!(
            [
                self.max_argument_bytes,
                self.max_environment_bytes,
                self.max_probe_request_bytes,
                self.max_probe_response_bytes
            ]
            .iter()
            .all(|v| *v > 0),
            "Process byte bounds must be nonzero"
        );
        Ok(ProcessBounds {
            startup_timeout: Duration::from_millis(self.startup_millis),
            stop_timeout: Duration::from_millis(self.stop_millis),
            kill_timeout: Duration::from_millis(self.kill_millis),
            poll_interval: Duration::from_millis(self.poll_millis),
            max_argument_bytes: self.max_argument_bytes,
            max_environment_bytes: self.max_environment_bytes,
            max_probe_request_bytes: self.max_probe_request_bytes,
            max_probe_response_bytes: self.max_probe_response_bytes,
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeServiceConfig {
    pub schema: u16,
    pub mode: String,
    pub home: PathBuf,
    pub lock_directory: PathBuf,
    pub consensus_config: PinnedInput,
    pub application_genesis: PinnedInput,
    pub root_config: PinnedInput,
    pub root_public_inputs: Vec<PinnedInput>,
    pub emergency_verifier_config: PinnedInput,
    pub candidate_config: PinnedInput,
    pub process_admission: PinnedInput,
    pub observation_pause: Option<ObservationPause>,
    pub engine_inputs: Vec<PinnedInput>,
    pub validator_public_key_sha256: String,
    pub process: ProcessLimits,
    pub environment: BTreeMap<String, String>,
    pub monitor_interval_millis: u64,
    pub max_state_entries: usize,
    pub adapter_listen: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationPause {
    pub total_millis: u64,
    pub cleanup_reserve_millis: u64,
}
impl ObservationPause {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.cleanup_reserve_millis > 0
                && self.cleanup_reserve_millis < self.total_millis
                && self.total_millis <= 60_000,
            "Explicit bounded observation pause required"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionRole {
    pub role: String,
    pub member_id: String,
    pub label: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessAdmission {
    pub schema: u16,
    pub unit: String,
    pub policy_identity_sha256: String,
    pub catalog_sha512: String,
    pub uid: u32,
    pub gid: u32,
    pub supervisor_label: String,
    pub application_owner_label: String,
    pub workload_label: String,
    pub helper_label: String,
    pub no_new_privileges: u8,
    pub seccomp: u8,
    pub mount_namespace: String,
    pub roles: Vec<AdmissionRole>,
}
impl ProcessAdmission {
    pub fn validate_shape(&self) -> Result<()> {
        use dytallix_release_runtime::ownership_security::validate_four_role_labels;
        ensure!(
            self.schema == 2
                && self.uid > 0
                && self.gid > 0
                && self.no_new_privileges == 1
                && self.seccomp == 2
                && self.mount_namespace == "inherit-supervisor",
            "Required process admission controls absent"
        );
        for (text, size) in [
            (&self.policy_identity_sha256, 64),
            (&self.catalog_sha512, 128),
        ] {
            ensure!(
                text.len() == size
                    && text
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "Invalid admission digest"
            );
        }
        validate_four_role_labels(&self.supervisor_label, &self.application_owner_label,
            &self.workload_label, &self.helper_label)?;
        ensure!(
            self.supervisor_label
                == format!(
                    "dyt-role-{}-{}-supervisor",
                    &self.policy_identity_sha256[..20],
                    self.unit
                ),
            "Admission unit/policy label mismatch"
        );
        ensure!(
            !self.roles.is_empty() && self.roles.len() <= 7,
            "Admission role count invalid"
        );
        let mut names = BTreeSet::new();
        for row in &self.roles {
            ensure!(
                names.insert(&row.role)
                    && !row.member_id.is_empty()
                    && row.member_id.len() <= 128
                    && row
                        .member_id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)),
                "Admission role duplicate or member invalid"
            );
            let expected = match row.role.as_str() {
                "service_supervisor" => &self.supervisor_label,
                "consensus_stdio" => &self.application_owner_label,
                "genesis_bootstrap_verifier" | "control_verifier" => &self.helper_label,
                _ => &self.workload_label,
            };
            ensure!(
                &row.label == expected,
                "Admission role label differs"
            );
        }
        let required: BTreeSet<&str> = [
            "service_supervisor",
            "consensus_stdio",
            "consensus_bridge",
            "consensus_engine",
            "genesis_bootstrap_verifier",
            "control_verifier",
        ]
        .into_iter()
        .collect();
        let actual: BTreeSet<&str> = names.iter().map(|v| v.as_str()).collect();
        ensure!(
            required.is_subset(&actual)
                && actual
                    .iter()
                    .all(|v| required.contains(v) || *v == "http_adapter"),
            "Required admission roles absent or unknown"
        );
        Ok(())
    }
    pub fn bind_catalog(
        &self,
        catalog: &dytallix_release_runtime::component_candidate::VerifiedMemberFiles,
    ) -> Result<()> {
        self.validate_shape()?;
        ensure!(
            catalog.candidate().sha512() == self.catalog_sha512,
            "Admission catalog digest differs"
        );
        let expected: BTreeSet<_> = catalog
            .candidate()
            .manifest()
            .roles
            .iter()
            .map(|r| (&r.role, &r.member_id))
            .collect();
        let actual: BTreeSet<_> = self.roles.iter().map(|r| (&r.role, &r.member_id)).collect();
        ensure!(
            actual == expected && actual.len() == self.roles.len(),
            "Admission role/member set differs"
        );
        Ok(())
    }
    pub fn supervisor_state(
        &self,
    ) -> Result<dytallix_release_runtime::ownership_security::SecurityState> {
        self.validate_shape()?;
        self.require_enforced_profiles()?;
        let state = dytallix_release_runtime::ownership_security::capture_current()?;
        state.validate_exact(&self.supervisor_label, self.uid, self.gid)?;
        Ok(state)
    }
    pub fn require_enforced_profiles(&self) -> Result<()> {
        self.validate_shape()?;
        dytallix_release_runtime::ownership_security::require_enforced_profiles(&[
            &self.supervisor_label,
            &self.application_owner_label,
            &self.workload_label,
            &self.helper_label,
        ])
    }
}

fn safe_metadata(metadata: &Metadata) -> Result<()> {
    ensure!(
        metadata.is_file()
            && metadata.nlink() == 1
            && [0, unsafe { libc::geteuid() }].contains(&metadata.uid())
            && metadata.mode() & 0o6022 == 0,
        "Unsafe input file type, owner, mode or link count"
    );
    Ok(())
}

fn identity(metadata: &Metadata) -> (u64, u64, u64, i64, i64, i64, i64, u32, u32, u64) {
    (
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec(),
        metadata.mode(),
        metadata.uid(),
        metadata.nlink(),
    )
}

pub fn read_local(path: &Path, max_bytes: usize) -> Result<Vec<u8>> {
    ensure!(
        path.is_absolute() && max_bytes > 0 && std::fs::canonicalize(path)? == path,
        "Input needs an exact absolute path and nonzero limit"
    );
    let before = std::fs::symlink_metadata(path)?;
    safe_metadata(&before)?;
    ensure!(
        before.len() > 0 && before.len() <= u64::try_from(max_bytes)?,
        "Input size bound"
    );
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let opened = file.metadata()?;
    safe_metadata(&opened)?;
    ensure!(
        identity(&before) == identity(&opened),
        "Input changed before opening"
    );
    let mut bytes = Vec::new();
    (&mut file)
        .take(
            u64::try_from(max_bytes)?
                .checked_add(1)
                .context("Input bound overflow")?,
        )
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= max_bytes && bytes.len() as u64 == before.len(),
        "Input changed length"
    );
    ensure!(
        identity(&opened) == identity(&file.metadata()?)
            && identity(&opened) == identity(&std::fs::symlink_metadata(path)?),
        "Input changed during read"
    );
    Ok(bytes)
}
impl PinnedInput {
    pub fn read(&self) -> Result<Vec<u8>> {
        ensure!(
            self.sha256.len() == 64
                && self
                    .sha256
                    .bytes()
                    .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v)),
            "Noncanonical input hash"
        );
        let bytes = read_local(&self.path, self.max_bytes)?;
        ensure!(
            hex::encode(Sha256::digest(&bytes)) == self.sha256,
            "Pinned input hash mismatch"
        );
        Ok(bytes)
    }
}

pub fn private_directory(path: &Path) -> Result<()> {
    ensure!(
        path.is_absolute() && std::fs::canonicalize(path)? == path,
        "Directory path alias"
    );
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o7777 == 0o700,
        "Directory must be current-user mode 0700"
    );
    Ok(())
}
fn loopback(value: &str, prefix: &str) -> Result<()> {
    let port = value
        .strip_prefix(prefix)
        .context("Numeric loopback listener required")?;
    let parsed: u16 = port.parse()?;
    ensure!(
        parsed > 0 && parsed.to_string() == port,
        "Canonical nonzero port required"
    );
    Ok(())
}

impl NativeServiceConfig {
    pub fn admission(&self) -> Result<ProcessAdmission> {
        ensure!(
            self.process_admission.max_bytes <= 65_536,
            "Process admission record too large"
        );
        let admission: ProcessAdmission = serde_json::from_slice(&self.process_admission.read()?)?;
        admission.validate_shape()?;
        Ok(admission)
    }
    pub fn load(path: &Path) -> Result<Self> {
        let config: Self = serde_json::from_slice(&read_local(path, 65_536)?)?;
        config.validate()?;
        Ok(config)
    }
    pub fn database(&self) -> PathBuf {
        self.home.join("appdb")
    }
    pub fn bridge_socket(&self) -> PathBuf {
        self.home.join("abci/app.sock")
    }
    pub fn rpc_socket(&self) -> PathBuf {
        self.home.join("data/rpc.sock")
    }
    pub fn lock_paths(&self) -> (PathBuf, PathBuf) {
        // Match the existing supervisor's home and signing-identity leases.
        let home_digest = hex::encode(Sha256::digest(self.home.as_os_str().as_encoded_bytes()));
        (
            self.lock_directory.join(format!("home-{home_digest}.lock")),
            self.lock_directory
                .join(format!("signer-{}.lock", self.validator_public_key_sha256)),
        )
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            unsafe { libc::geteuid() } != 0,
            "Development service must run as a non-root user"
        );
        ensure!(
            self.schema == 1 && self.mode == "disposable-loopback-native",
            "Only disposable native mode is implemented; production remains disabled"
        );
        ensure!(
            (1..=60_000).contains(&self.monitor_interval_millis) && self.max_state_entries > 0,
            "Monitoring and state inventory bounds required"
        );
        self.process.bounds()?;
        self.observation_pause
            .as_ref()
            .context("Explicit observation pause budget required")?
            .validate()?;
        self.admission()?.validate_shape()?;
        private_directory(&self.home)?;
        private_directory(&self.lock_directory)?;
        for relative in ["config", "data", "abci", "appdb", "data/cs.wal"] {
            private_directory(&self.home.join(relative))?;
        }
        // A protected state tree is checked before read-only authority replay.
        let mut pending = vec![self.home.join("data"), self.database()];
        let mut entries = 0usize;
        while let Some(directory) = pending.pop() {
            for row in std::fs::read_dir(directory)? {
                let path = row?.path();
                entries = entries.checked_add(1).context("State entry overflow")?;
                ensure!(
                    entries <= self.max_state_entries,
                    "State inventory exceeds bound"
                );
                let metadata = std::fs::symlink_metadata(&path)?;
                ensure!(
                    !metadata.file_type().is_symlink()
                        && metadata.uid() == unsafe { libc::geteuid() }
                        && metadata.mode() & 0o077 == 0,
                    "Unsafe mutable state entry"
                );
                if metadata.is_dir() {
                    pending.push(path);
                } else {
                    ensure!(
                        metadata.is_file() && metadata.nlink() == 1,
                        "Unexpected mutable state type"
                    );
                }
            }
        }
        for input in [
            &self.consensus_config,
            &self.root_config,
            &self.emergency_verifier_config,
            &self.candidate_config,
        ] {
            ensure!(
                input.max_bytes <= 65_536,
                "Configuration bound exceeds protocol limit"
            );
            input.read()?;
        }
        ensure!(
            self.application_genesis.max_bytes <= 8 * 1024 * 1024,
            "Genesis bound exceeds pipe limit"
        );
        self.application_genesis.read()?;
        self.validate_root_inputs()?;
        self.validate_engine()?;
        if let Some(listen) = &self.adapter_listen {
            loopback(listen, "127.0.0.1:")?;
        }
        Ok(())
    }
    fn validate_root_inputs(&self) -> Result<()> {
        let root: serde_json::Value = serde_json::from_slice(&self.root_config.read()?)?;
        let expected: BTreeSet<PathBuf> = ["policy_path", "request_path"]
            .iter()
            .map(|key| {
                root.get(key)
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from)
                    .context("Root policy/request path missing")
            })
            .collect::<Result<_>>()?;
        ensure!(
            expected.len() == 2
                && self.root_public_inputs.len() == 2
                && self
                    .root_public_inputs
                    .iter()
                    .map(|p| p.path.clone())
                    .collect::<BTreeSet<_>>()
                    == expected,
            "Pin both exact root policy and request inputs"
        );
        for input in &self.root_public_inputs {
            input.read()?;
        }
        ensure!(
            root.get("engine_genesis_path").and_then(|v| v.as_str())
                == self.home.join("config/genesis.json").to_str(),
            "Root must bind the actual engine genesis"
        );
        Ok(())
    }
    fn validate_engine(&self) -> Result<()> {
        let expected: BTreeSet<PathBuf> = [
            "config.toml",
            "genesis.json",
            "node_key.json",
            "pqc_transport.json",
            "priv_validator_key.json",
        ]
        .iter()
        .map(|p| self.home.join("config").join(p))
        .collect();
        ensure!(
            self.engine_inputs.len() == expected.len()
                && self
                    .engine_inputs
                    .iter()
                    .map(|p| p.path.clone())
                    .collect::<BTreeSet<_>>()
                    == expected,
            "Pin every required engine configuration file exactly once"
        );
        let mut inputs = BTreeMap::new();
        for input in &self.engine_inputs {
            inputs.insert(input.path.clone(), input.read()?);
        }
        for name in ["node_key.json", "priv_validator_key.json"] {
            let metadata = std::fs::symlink_metadata(self.home.join("config").join(name))?;
            ensure!(
                metadata.mode() & 0o077 == 0,
                "Engine private key must not permit group or other access"
            );
        }
        let cfg: toml::Value = toml::from_str(std::str::from_utf8(
            &inputs[&self.home.join("config/config.toml")],
        )?)?;
        let proxy = format!("unix://{}", self.bridge_socket().display());
        for (key, value) in [
            ("proxy_app", proxy.as_str()),
            ("abci", "socket"),
            ("db_dir", "data"),
            ("genesis_file", "config/genesis.json"),
            ("node_key_file", "config/node_key.json"),
            ("priv_validator_key_file", "config/priv_validator_key.json"),
            (
                "priv_validator_state_file",
                "data/priv_validator_state.json",
            ),
            ("priv_validator_laddr", ""),
        ] {
            ensure!(
                cfg.get(key).and_then(toml::Value::as_str) == Some(value),
                "Engine path binding differs: {key}"
            );
        }
        ensure!(
            cfg.get("consensus")
                .and_then(|v| v.get("wal_file"))
                .and_then(toml::Value::as_str)
                == Some("data/cs.wal/wal"),
            "Consensus WAL path differs"
        );
        for section in ["p2p", "rpc"] {
            loopback(
                cfg.get(section)
                    .and_then(|v| v.get("laddr"))
                    .and_then(toml::Value::as_str)
                    .context("Engine loopback listener absent")?,
                "tcp://127.0.0.1:",
            )?;
        }
        for (section, key) in [
            ("p2p", "external_address"),
            ("p2p", "seeds"),
            ("rpc", "grpc_laddr"),
            ("rpc", "pprof_laddr"),
            ("rpc", "tls_cert_file"),
            ("rpc", "tls_key_file"),
        ] {
            ensure!(
                cfg.get(section)
                    .and_then(|v| v.get(key))
                    .and_then(toml::Value::as_str)
                    == Some(""),
                "Unsupported engine external route: {section}.{key}"
            );
        }
        for (section, key) in [
            ("p2p", "pex"),
            ("p2p", "seed_mode"),
            ("rpc", "unsafe"),
            ("statesync", "enable"),
            ("instrumentation", "prometheus"),
        ] {
            ensure!(
                cfg.get(section)
                    .and_then(|v| v.get(key))
                    .and_then(toml::Value::as_bool)
                    == Some(false),
                "Unsupported engine behavior: {section}.{key}"
            );
        }
        let genesis: serde_json::Value =
            serde_json::from_slice(&inputs[&self.home.join("config/genesis.json")])?;
        let chain = genesis
            .get("chain_id")
            .and_then(|v| v.as_str())
            .context("Engine chain ID required")?
            .to_lowercase();
        ensure!(
            !chain.is_empty() && !chain.contains("mainnet") && !chain.contains("production"),
            "Production chain identity forbidden"
        );
        let validator: serde_json::Value =
            serde_json::from_slice(&inputs[&self.home.join("config/priv_validator_key.json")])?;
        let public = validator
            .get("pub_key")
            .context("Validator public identity absent")?;
        ensure!(
            public.get("type").and_then(|v| v.as_str()) == Some("cometbft/PubKeyMlDsa65"),
            "ML-DSA-65 validator identity required"
        );
        let encoded = public
            .get("value")
            .and_then(|v| v.as_str())
            .context("Validator public key missing")?;
        let bytes = STANDARD.decode(encoded)?;
        ensure!(
            bytes.len() == 1952
                && STANDARD.encode(&bytes) == encoded
                && hex::encode(Sha256::digest(&bytes)) == self.validator_public_key_sha256,
            "Validator public identity pin mismatch"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    fn admission_record() -> serde_json::Value {
        let supervisor = format!("dyt-role-{}-node0-supervisor", "a".repeat(20));
        let application_owner = format!("dyt-role-{}-node0-application-owner//&{supervisor}", "a".repeat(20));
        let workload = format!("{supervisor}//&dyt-role-{}-node0-workload", "a".repeat(20));
        let helper = format!("dyt-role-{}-node0-application-owner//&dyt-role-{}-node0-helper//&{supervisor}", "a".repeat(20), "a".repeat(20));
        let roles = ["service_supervisor", "consensus_stdio", "consensus_bridge", "consensus_engine", "genesis_bootstrap_verifier", "control_verifier"].iter().map(|r|serde_json::json!({"role":r,"member_id":r,"label":match *r {"service_supervisor"=>&supervisor,"consensus_stdio"=>&application_owner,"genesis_bootstrap_verifier"|"control_verifier"=>&helper,_=>&workload}})).collect::<Vec<_>>();
        serde_json::json!({"schema":2,"unit":"node0","policy_identity_sha256":"a".repeat(64),"catalog_sha512":"b".repeat(128),"uid":42,"gid":43,"supervisor_label":supervisor,"application_owner_label":application_owner,"workload_label":workload,"helper_label":helper,"no_new_privileges":1,"seccomp":2,"mount_namespace":"inherit-supervisor","roles":roles})
    }
    #[test]
    fn role_admission_refuses_inconsistent_security_and_labels() {
        let original = admission_record();
        serde_json::from_value::<ProcessAdmission>(original.clone())
            .unwrap()
            .validate_shape()
            .unwrap();
        for (key, value) in [
            ("uid", serde_json::json!(0)),
            ("gid", serde_json::json!(0)),
            ("unit", serde_json::json!("node1")),
            ("policy_identity_sha256", serde_json::json!("c".repeat(64))),
            ("seccomp", serde_json::json!(0)),
            ("no_new_privileges", serde_json::json!(0)),
            ("mount_namespace", serde_json::json!("any")),
            ("workload_label", serde_json::json!("unconfined")),
        ] {
            let mut v = original.clone();
            v[key] = value;
            assert!(
                serde_json::from_value::<ProcessAdmission>(v)
                    .unwrap()
                    .validate_shape()
                    .is_err(),
                "{key}"
            );
        }
        let mut v = original.clone();
        v["roles"].as_array_mut().unwrap().pop();
        assert!(serde_json::from_value::<ProcessAdmission>(v)
            .unwrap()
            .validate_shape()
            .is_err());
        let mut v = original.clone();
        v["roles"][1]["label"] = original["supervisor_label"].clone();
        assert!(serde_json::from_value::<ProcessAdmission>(v)
            .unwrap()
            .validate_shape()
            .is_err());
        let mut v = original;
        v["unknown"] = serde_json::json!(true);
        assert!(serde_json::from_value::<ProcessAdmission>(v).is_err());
    }
    #[test]
    fn observation_pause_requires_explicit_work_and_cleanup_time() {
        ObservationPause {
            total_millis: 100,
            cleanup_reserve_millis: 20,
        }
        .validate()
        .unwrap();
        for (total, reserve) in [(0, 0), (100, 0), (100, 100), (100, 101), (60001, 1)] {
            assert!(ObservationPause {
                total_millis: total,
                cleanup_reserve_millis: reserve
            }
            .validate()
            .is_err());
        }
        assert!(serde_json::from_str::<ObservationPause>(r#"{"total_millis":100}"#).is_err());
    }
    #[test]
    fn pinned_input_rejects_alias_and_changed_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let path = root.join("input");
        std::fs::write(&path, b"original").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut pin = PinnedInput {
            path: path.clone(),
            sha256: hex::encode(Sha256::digest(b"original")),
            max_bytes: 32,
        };
        assert_eq!(pin.read().unwrap(), b"original");
        symlink(&path, root.join("alias")).unwrap();
        pin.path = root.join("alias");
        assert!(pin.read().is_err());
        pin.path = path.clone();
        std::fs::write(path, b"changed!").unwrap();
        assert!(pin.read().is_err());
    }
    #[test]
    fn pinned_input_rejects_group_write_and_hardlinks() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().canonicalize().unwrap().join("input");
        std::fs::write(&path, b"data").unwrap();
        let pin = PinnedInput {
            path: path.clone(),
            sha256: hex::encode(Sha256::digest(b"data")),
            max_bytes: 4,
        };
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o620)).unwrap();
        assert!(pin.read().is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::hard_link(&path, path.with_extension("link")).unwrap();
        assert!(pin.read().is_err());
    }
    #[test]
    fn listener_requires_canonical_numeric_loopback() {
        assert!(loopback("127.0.0.1:1234", "127.0.0.1:").is_ok());
        for bad in [
            "localhost:1234",
            "0.0.0.0:1234",
            "127.0.0.1:0",
            "127.0.0.1:0123",
            "127.0.0.1:65536",
        ] {
            assert!(loopback(bad, "127.0.0.1:").is_err());
        }
    }
}
