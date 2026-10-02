//! Explicit service settings: the disposable development mode, or the
//! production mode of a production build (production activation v1, A5).
//! Paths and limits do not grant release authority.
use crate::processes::ProcessBounds;
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{Metadata, OpenOptions};
use std::io::Read;
use std::net::SocketAddr;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The development build's only service mode.
pub const DEVELOPMENT_MODE: &str = "disposable-loopback-native";
/// The production build's only service mode (production activation v1, A5).
pub const PRODUCTION_MODE: &str = "production-native";
/// The mode this build runs; neither build accepts the other's.
pub const MODE: &str = if cfg!(feature = "production") {
    PRODUCTION_MODE
} else {
    DEVELOPMENT_MODE
};
/// The engine reads a binding of at most 4 KiB.
pub const MAX_BINDING_BYTES: usize = 4096;
/// The longest catch-up budget: seven days.
pub const MAX_CATCH_UP_MILLIS: u64 = 7 * 24 * 60 * 60 * 1000;
/// Light block exports: one primary and its witnesses.
pub const MAX_LIGHT_BLOCK_EXPORTS: usize = 16;
/// The production engine's seed-backed peer identity file.
pub const PEER_SEED_FILE: &str = "pqc_peer_seed.bin";

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
    /// Lowers the adapter's compiled limits; requires `adapter_listen`.
    pub adapter_limits: Option<AdapterLimits>,
    /// The adapter's client channel listener (client channel v1, E04 gap
    /// 19); requires `adapter_listen`.
    pub adapter_channel: Option<AdapterChannel>,
    pub metrics: MetricsOutput,
    pub snapshots: Option<SnapshotOutput>,
    pub block_history: BlockHistory,
    /// After a halt, the pinned restart authorization (restart v1, E04 gap
    /// 18). The preflight selects its target release; the application runs
    /// the halted block on it.
    pub restart_authorization: Option<PinnedInput>,
    /// Production mode: the node's role (production activation v1, A5).
    pub role: Option<NodeRole>,
    /// Production mode: this host's published binding (A4), checked here
    /// against the pinned engine files and passed to the engine.
    pub binding: Option<PinnedInput>,
    /// Production mode: the operator's light block exports, when the engine
    /// configuration enables state sync (state sync v1).
    pub state_sync: Option<StateSyncInput>,
    /// Production mode: how long readiness waits for the engine to catch up
    /// (P01, 1 October 2026). Required; an E05 operating value.
    pub catch_up_millis: Option<u64>,
}

/// A production node's role (production activation v1, design F).
/// Validators hold a genesis validator key and run no listener but P2P;
/// endpoints run the HTTP adapter and its client channel; sentries run
/// neither. Sentries and endpoints carry a key outside the genesis set.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NodeRole {
    Validator,
    Sentry,
    Endpoint,
}
impl NodeRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Validator => "validator",
            Self::Sentry => "sentry",
            Self::Endpoint => "endpoint",
        }
    }
}

/// State sync from operator-supplied light blocks (state sync v1, rule 5):
/// the first export is the primary, the others its witnesses. The engine
/// verifies every light block from the configured trusted height.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateSyncInput {
    pub light_blocks: Vec<PathBuf>,
}
impl StateSyncInput {
    pub fn validate(&self, home: &Path) -> Result<()> {
        ensure!(
            !self.light_blocks.is_empty() && self.light_blocks.len() <= MAX_LIGHT_BLOCK_EXPORTS,
            "State sync needs one to {MAX_LIGHT_BLOCK_EXPORTS} light block exports"
        );
        let unique: BTreeSet<&PathBuf> = self.light_blocks.iter().collect();
        ensure!(
            unique.len() == self.light_blocks.len(),
            "Light block exports must differ"
        );
        for path in &self.light_blocks {
            ensure!(
                path.is_absolute() && std::fs::canonicalize(path)? == *path,
                "Light block export path alias"
            );
            ensure!(
                !path.starts_with(home),
                "Light block export inside the node home"
            );
            let metadata = std::fs::symlink_metadata(path)?;
            ensure!(
                metadata.is_dir()
                    && [0, unsafe { libc::geteuid() }].contains(&metadata.uid())
                    && metadata.mode() & 0o022 == 0,
                "Light block export must be a root or current-user directory without group or other write"
            );
        }
        Ok(())
    }
}

/// A host binding (production activation v1, A4), as the engine decodes it.
#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct ProductionBinding {
    schema: u16,
    role: String,
    chain_id: String,
    config_sha256: String,
    genesis_sha256: String,
    transport_sha256: String,
    peer_public_key_sha256: String,
    validator_public_key_sha256: String,
}

/// The pinned engine files a binding names.
pub struct EngineFiles<'a> {
    pub config: &'a [u8],
    pub genesis: &'a [u8],
    pub transport: &'a [u8],
    pub validator_public_key: &'a [u8],
}

/// Check a host's binding against its pinned engine files before any child
/// starts: the engine's own start checks, so a host whose files or keys
/// differ from the published pin plan never runs. Only a validator's key is
/// in the genesis validator set.
pub fn check_binding(raw: &[u8], role: NodeRole, files: &EngineFiles) -> Result<()> {
    let binding: ProductionBinding =
        serde_json::from_slice(raw).context("Production binding is malformed")?;
    ensure!(
        serde_json::to_vec(&binding)?.as_slice() == raw.trim_ascii(),
        "Production binding must use canonical compact JSON"
    );
    ensure!(
        binding.schema == 1 && binding.role == role.as_str(),
        "Production binding schema or role differs from this node"
    );
    let digest = |bytes: &[u8]| hex::encode(Sha256::digest(bytes));
    let genesis: serde_json::Value = serde_json::from_slice(files.genesis)?;
    let transport: serde_json::Value = serde_json::from_slice(files.transport)?;
    ensure!(
        genesis.get("chain_id").and_then(|v| v.as_str()) == Some(binding.chain_id.as_str())
            && binding.config_sha256 == digest(files.config)
            && binding.genesis_sha256 == digest(files.genesis)
            && binding.transport_sha256 == digest(files.transport),
        "Engine files differ from the production binding"
    );
    let peer = STANDARD.decode(
        transport
            .get("local_public_key_base64")
            .and_then(|v| v.as_str())
            .context("Transport local public key missing")?,
    )?;
    ensure!(
        binding.peer_public_key_sha256 == digest(&peer),
        "Peer key differs from the production binding"
    );
    ensure!(
        binding.validator_public_key_sha256 == digest(files.validator_public_key),
        "Validator key differs from the production binding"
    );
    let validators = genesis
        .get("validators")
        .and_then(|v| v.as_array())
        .context("Engine genesis validators missing")?;
    let mut in_genesis = false;
    for validator in validators {
        let value = validator
            .get("pub_key")
            .and_then(|v| v.get("value"))
            .and_then(|v| v.as_str())
            .context("Genesis validator key missing")?;
        in_genesis |= STANDARD.decode(value)? == files.validator_public_key;
    }
    ensure!(
        in_genesis == (role == NodeRole::Validator),
        "Only a validator's key may be in the genesis validator set"
    );
    Ok(())
}

/// The node's metrics files (metrics v1): the engine writes
/// `dytallix-engine.prom` and the application `dytallix-app.prom` here.
/// Required (P01, 28 September 2026): the incident runbooks detect with them
/// (E04 gap 15). An operator agent reads the directory, so it may be group
/// readable; the values are E05 inputs.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricsOutput {
    pub directory: PathBuf,
    pub interval_seconds: u64,
}

/// Application state snapshots (state sync v1): the application writes them
/// every `interval_blocks` and keeps the latest `keep`; the bridge serves
/// them. Optional; the values are E05 inputs.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotOutput {
    pub directory: PathBuf,
    pub interval_blocks: u64,
    pub keep: u64,
}

/// The application's block records: the retained window, or every record.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BlockHistory {
    Window,
    Archive,
}
impl BlockHistory {
    pub fn arg(self) -> &'static str {
        match self {
            Self::Window => "window",
            Self::Archive => "archive",
        }
    }
}

/// An output directory outside the protected state tree.
fn output_directory(path: &Path, home: &Path) -> Result<Metadata> {
    ensure!(
        path.is_absolute() && std::fs::canonicalize(path)? == path,
        "Output directory path alias"
    );
    ensure!(
        path != home
            && !["config", "data", "abci", "appdb"]
                .iter()
                .any(|name| path.starts_with(home.join(name))),
        "Output directory inside protected node state"
    );
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o022 == 0,
        "Output directory must be a current-user directory without group or other write"
    );
    Ok(metadata)
}
impl MetricsOutput {
    pub fn validate(&self, home: &Path) -> Result<()> {
        output_directory(&self.directory, home)?;
        ensure!(
            (1..=3600).contains(&self.interval_seconds),
            "Metrics interval must be 1 to 3600 seconds"
        );
        Ok(())
    }
}
impl SnapshotOutput {
    pub fn validate(&self, home: &Path) -> Result<()> {
        let metadata = output_directory(&self.directory, home)?;
        ensure!(
            metadata.mode() & 0o7777 == 0o700,
            "Snapshot directory must be current-user mode 0700"
        );
        ensure!(
            self.interval_blocks > 0 && self.keep > 0,
            "Snapshot interval and count kept must be positive"
        );
        Ok(())
    }
}

/// The adapter's tighten-only limits (E04 gap 13, P01 28 September 2026).
/// Each value becomes one adapter flag; the adapter refuses a value above its
/// compiled ceiling, so a looser setting stops startup.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterLimits {
    pub max_connections: Option<u64>,
    pub max_request_body_bytes: Option<u64>,
    pub max_response_body_bytes: Option<u64>,
    pub max_headers: Option<u64>,
    pub max_header_bytes: Option<u64>,
    pub deadline_ms: Option<u64>,
}
impl AdapterLimits {
    fn flags(&self) -> [(&'static str, Option<u64>); 6] {
        [
            ("--max-connections", self.max_connections),
            ("--max-request-body-bytes", self.max_request_body_bytes),
            ("--max-response-body-bytes", self.max_response_body_bytes),
            ("--max-headers", self.max_headers),
            ("--max-header-bytes", self.max_header_bytes),
            ("--deadline-ms", self.deadline_ms),
        ]
    }
    pub fn validate(&self) -> Result<()> {
        let set: Vec<u64> = self.flags().iter().filter_map(|(_, v)| *v).collect();
        ensure!(
            !set.is_empty() && set.iter().all(|v| *v > 0),
            "Adapter limits must set at least one positive value"
        );
        Ok(())
    }
    /// The adapter arguments for the values that are set.
    pub fn args(&self) -> Vec<std::ffi::OsString> {
        self.flags()
            .iter()
            .filter_map(|(flag, value)| value.map(|v| [(*flag).into(), v.to_string().into()]))
            .flatten()
            .collect()
    }
}

/// The adapter's client channel listener (E04 gap 19, C-b). The pin is the
/// file clients receive. Its network must be the chain ID, and before
/// readiness the supervisor completes a channel exchange with its key, so a
/// seed that does not match the published pin stops startup.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterChannel {
    /// A canonical numeric `IP:port`, neither unspecified nor multicast.
    pub listen: String,
    pub pin: PinnedInput,
    /// Lower the adapter's channel ceilings (64 and 4).
    pub max_connections: Option<u64>,
    pub max_connections_per_address: Option<u64>,
}
impl AdapterChannel {
    pub fn address(&self) -> Result<SocketAddr> {
        let address: SocketAddr = self
            .listen
            .parse()
            .context("Channel listener must be a numeric IP:port")?;
        ensure!(
            address.to_string() == self.listen
                && !address.ip().is_unspecified()
                && !address.ip().is_multicast()
                && address.port() > 0,
            "Channel listener must be a canonical explicit address"
        );
        Ok(address)
    }
    pub fn endpoint_pin(&self) -> Result<dytallix_client_channel::EndpointPin> {
        ensure!(
            self.pin.max_bytes <= dytallix_client_channel::MAX_PIN_BYTES,
            "Channel pin bound exceeds the pin limit"
        );
        dytallix_client_channel::EndpointPin::parse(&self.pin.read()?)
            .map_err(|_| anyhow::anyhow!("Channel pin is malformed"))
    }
    fn validate(&self) -> Result<()> {
        self.address()?;
        self.endpoint_pin()?;
        ensure!(
            [self.max_connections, self.max_connections_per_address]
                .iter()
                .all(|v| v.is_none_or(|v| v > 0)),
            "Channel limits must be positive"
        );
        Ok(())
    }
    /// The adapter arguments; the network is the candidate's chain ID.
    pub fn args(&self, chain_id: &str) -> Vec<std::ffi::OsString> {
        let mut args: Vec<std::ffi::OsString> = vec![
            "--channel-listen".into(),
            self.listen.clone().into(),
            "--channel-network".into(),
            chain_id.into(),
        ];
        for (flag, value) in [
            ("--max-channel-connections", self.max_connections),
            (
                "--max-channel-connections-per-address",
                self.max_connections_per_address,
            ),
        ] {
            if let Some(value) = value {
                args.push(flag.into());
                args.push(value.to_string().into());
            }
        }
        args
    }
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
    /// The engine's operator RPC socket (RPC controls v1): diagnostics for
    /// the node's owner only.
    pub fn operator_rpc_socket(&self) -> PathBuf {
        self.home.join("data/rpc-operator.sock")
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
    /// The production mode of a production build (production activation v1).
    pub fn production(&self) -> bool {
        self.mode == PRODUCTION_MODE
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            unsafe { libc::geteuid() } != 0,
            "The service must run as a non-root user"
        );
        self.validate_mode()?;
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
        // The consensus configuration carries the genesis recovery accounts
        // (E05-a), so it has the application's larger bound.
        ensure!(
            self.consensus_config.max_bytes <= dytallix_fast_node::consensus_settlement::MAX_CONFIG_BYTES,
            "Configuration bound exceeds protocol limit"
        );
        self.consensus_config.read()?;
        for input in [
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
        if let Some(limits) = &self.adapter_limits {
            ensure!(
                self.adapter_listen.is_some(),
                "Adapter limits require a configured adapter"
            );
            limits.validate()?;
        }
        if let Some(channel) = &self.adapter_channel {
            ensure!(
                self.adapter_listen.is_some(),
                "The channel listener requires a configured adapter"
            );
            channel.validate()?;
        }
        self.metrics.validate(&self.home)?;
        if let Some(restart) = &self.restart_authorization {
            ensure!(
                restart.max_bytes <= 262_144,
                "Restart authorization bound exceeds the application limit"
            );
            restart.read()?;
        }
        if let Some(snapshots) = &self.snapshots {
            snapshots.validate(&self.home)?;
            ensure!(
                snapshots.directory != self.metrics.directory,
                "Snapshot and metrics directories must differ"
            );
        }
        Ok(())
    }
    /// Each build runs only its own mode. Production settings appear only
    /// in production mode, and there they fix what each role runs (design F).
    fn validate_mode(&self) -> Result<()> {
        ensure!(
            self.schema == 1 && self.mode == MODE,
            "This build runs only the {MODE} service mode"
        );
        self.validate_settings()
    }
    fn validate_settings(&self) -> Result<()> {
        if !self.production() {
            ensure!(
                self.role.is_none()
                    && self.binding.is_none()
                    && self.state_sync.is_none()
                    && self.catch_up_millis.is_none(),
                "Roles, bindings, state sync and catch-up budgets are production mode settings"
            );
            return Ok(());
        }
        let role = self.role.context("Production mode requires a node role")?;
        let binding = self
            .binding
            .as_ref()
            .context("Production mode requires this host's binding")?;
        ensure!(
            binding.max_bytes <= MAX_BINDING_BYTES,
            "Binding bound exceeds the engine limit"
        );
        let catch_up = self
            .catch_up_millis
            .context("Production mode requires a catch-up budget")?;
        ensure!(
            (self.process.startup_millis..=MAX_CATCH_UP_MILLIS).contains(&catch_up),
            "Catch-up budget must be from the startup bound to seven days"
        );
        let serves = self.adapter_listen.is_some()
            || self.adapter_limits.is_some()
            || self.adapter_channel.is_some();
        match role {
            NodeRole::Endpoint => ensure!(
                self.adapter_listen.is_some() && self.adapter_channel.is_some(),
                "An endpoint runs the HTTP adapter and its client channel"
            ),
            NodeRole::Validator | NodeRole::Sentry => ensure!(
                !serves,
                "Validators and sentries run no adapter or channel listener"
            ),
        }
        Ok(())
    }
    fn validate_root_inputs(&self) -> Result<()> {
        let root: serde_json::Value = serde_json::from_slice(&self.root_config.read()?)?;
        // The development root names a signed request; the threshold root
        // (A2) its combined signatures file.
        let records = if self.production() {
            ["policy_path", "signatures_path"]
        } else {
            ["policy_path", "request_path"]
        };
        let expected: BTreeSet<PathBuf> = records
            .iter()
            .map(|key| {
                root.get(key)
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from)
                    .context("Root public input path missing")
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
            "Pin both exact root public inputs"
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
        // The production transport takes its peer identity from the seed
        // file and refuses a packed node key (A4).
        let peer_secret = if self.production() {
            PEER_SEED_FILE
        } else {
            "node_key.json"
        };
        if self.production() {
            ensure!(
                matches!(std::fs::symlink_metadata(self.home.join("config/node_key.json")),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound),
                "The production transport refuses a packed peer key"
            );
        }
        let expected: BTreeSet<PathBuf> = [
            "config.toml",
            "genesis.json",
            peer_secret,
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
        for name in [peer_secret, "priv_validator_key.json"] {
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
        let laddr = |section: &str| {
            cfg.get(section)
                .and_then(|v| v.get("laddr"))
                .and_then(toml::Value::as_str)
                .context("Engine listener absent")
        };
        // The engine serves RPC on its owner-only Unix sockets (rpc profile
        // dytallix-pqc-unix-v1); its configuration names a loopback address.
        loopback(laddr("rpc")?, "tcp://127.0.0.1:")?;
        if self.production() {
            // One IP per node (P01, 30 September 2026): the P2P listener is
            // the node's single address, and an endpoint's channel listener
            // shares it.
            let p2p = explicit_endpoint(
                laddr("p2p")?
                    .strip_prefix("tcp://")
                    .context("Engine P2P listener must be TCP")?,
            )?;
            if let Some(channel) = &self.adapter_channel {
                let channel = channel.address()?;
                ensure!(
                    channel.ip() == p2p.ip() && channel.port() != p2p.port(),
                    "The channel listener must use the node's P2P address on another port"
                );
            }
        } else {
            loopback(laddr("p2p")?, "tcp://127.0.0.1:")?;
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
        let flag = |section: &str, key: &str| {
            cfg.get(section)
                .and_then(|v| v.get(key))
                .and_then(toml::Value::as_bool)
        };
        for (section, key) in [
            ("p2p", "pex"),
            ("p2p", "seed_mode"),
            ("rpc", "unsafe"),
            ("instrumentation", "prometheus"),
        ] {
            ensure!(
                flag(section, key) == Some(false),
                "Unsupported engine behavior: {section}.{key}"
            );
        }
        // A production node may join by state sync from the operator's light
        // blocks (state sync v1); the development mode never does.
        let state_sync = flag("statesync", "enable").context("Engine state sync setting absent")?;
        match &self.state_sync {
            Some(input) => {
                ensure!(
                    state_sync,
                    "Light block exports require the engine's state sync"
                );
                input.validate(&self.home)?;
            }
            None => ensure!(
                !state_sync,
                "Engine state sync requires the operator's light block exports"
            ),
        }
        let genesis: serde_json::Value =
            serde_json::from_slice(&inputs[&self.home.join("config/genesis.json")])?;
        let chain = genesis
            .get("chain_id")
            .and_then(|v| v.as_str())
            .context("Engine chain ID required")?
            .to_lowercase();
        // Only a production build may name mainnet (production activation v1).
        ensure!(
            !chain.is_empty()
                && (self.production() || (!chain.contains("mainnet") && !chain.contains("production"))),
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
        if self.production() {
            let binding = self
                .binding
                .as_ref()
                .context("Production mode requires this host's binding")?;
            let file = |name: &str| inputs[&self.home.join("config").join(name)].as_slice();
            check_binding(
                &binding.read()?,
                self.role.context("Production mode requires a node role")?,
                &EngineFiles {
                    config: file("config.toml"),
                    genesis: file("genesis.json"),
                    transport: file("pqc_transport.json"),
                    validator_public_key: &bytes,
                },
            )?;
        }
        Ok(())
    }
}

/// A canonical `IP:port` the production engine accepts as its P2P listener:
/// a global unicast address (private ranges included, as Go defines it) and
/// a port from 1024.
fn explicit_endpoint(value: &str) -> Result<SocketAddr> {
    let address: SocketAddr = value
        .parse()
        .context("Engine P2P listener must be a numeric IP:port")?;
    let ip = address.ip();
    let link_local = match ip {
        std::net::IpAddr::V4(v4) => v4.is_link_local() || v4.is_broadcast(),
        std::net::IpAddr::V6(v6) => (v6.segments()[0] & 0xffc0) == 0xfe80,
    };
    ensure!(
        address.to_string() == value
            && !ip.is_loopback()
            && !ip.is_unspecified()
            && !ip.is_multicast()
            && !link_local
            && address.port() >= 1024,
        "Engine P2P listener must be the node's canonical explicit address"
    );
    Ok(address)
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
    fn adapter_limits_become_adapter_flags() {
        let limits: AdapterLimits =
            serde_json::from_value(serde_json::json!({"max_connections":8,"deadline_ms":2500}))
                .unwrap();
        limits.validate().unwrap();
        assert_eq!(
            limits.args(),
            ["--max-connections", "8", "--deadline-ms", "2500"]
                .map(std::ffi::OsString::from)
                .to_vec()
        );
        assert!(AdapterLimits::default().validate().is_err());
        let zero: AdapterLimits =
            serde_json::from_value(serde_json::json!({"max_headers":0})).unwrap();
        assert!(zero.validate().is_err());
        assert!(
            serde_json::from_value::<AdapterLimits>(serde_json::json!({"max_sockets":1})).is_err()
        );
    }
    #[test]
    fn adapter_channel_pins_its_endpoint_and_becomes_flags() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().canonicalize().unwrap().join("pin.json");
        let key = dytallix_client_channel::Identity::from_seed(&[1; 32])
            .public_key()
            .to_vec();
        let pin = dytallix_client_channel::EndpointPin::new("chain-a", "node.example:26670", &key)
            .unwrap()
            .to_json();
        std::fs::write(&path, &pin).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let channel = |listen: &str, extra: serde_json::Value| {
            let mut value = serde_json::json!({"listen": listen, "pin": {
                "path": path, "sha256": hex::encode(Sha256::digest(&pin)), "max_bytes": 8192}});
            for (key, field) in extra.as_object().unwrap() {
                value[key] = field.clone();
            }
            serde_json::from_value::<AdapterChannel>(value)
        };
        let configured = channel("203.0.113.9:26670", serde_json::json!({"max_connections": 16}))
            .unwrap();
        configured.validate().unwrap();
        assert_eq!(configured.endpoint_pin().unwrap().public_key, key);
        assert_eq!(
            configured.args("chain-a"),
            [
                "--channel-listen",
                "203.0.113.9:26670",
                "--channel-network",
                "chain-a",
                "--max-channel-connections",
                "16"
            ]
            .map(std::ffi::OsString::from)
            .to_vec()
        );
        for listen in ["0.0.0.0:26670", "[::]:26670", "203.0.113.9:0", "node:26670", "224.0.0.1:1"] {
            assert!(
                channel(listen, serde_json::json!({})).unwrap().validate().is_err(),
                "{listen}"
            );
        }
        let zero = channel("203.0.113.9:1", serde_json::json!({"max_connections_per_address": 0}));
        assert!(zero.unwrap().validate().is_err());
        let large = channel("203.0.113.9:1", serde_json::json!({"pin": {
            "path": path, "sha256": hex::encode(Sha256::digest(&pin)), "max_bytes": 8193}}));
        assert!(large.unwrap().validate().is_err());
        let changed = channel("203.0.113.9:1", serde_json::json!({"pin": {
            "path": path, "sha256": "00".repeat(32), "max_bytes": 8192}}));
        assert!(changed.unwrap().validate().is_err());
        assert!(channel("203.0.113.9:1", serde_json::json!({"key": "x"})).is_err());
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
    fn output_directories_stay_outside_protected_state() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let home = root.join("home");
        for dir in [
            "home/data/metrics",
            "home/appdb",
            "home/config",
            "metrics",
            "snapshots",
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let mode = |dir: &str, mode| {
            std::fs::set_permissions(root.join(dir), std::fs::Permissions::from_mode(mode)).unwrap()
        };
        mode("metrics", 0o750);
        let metrics = |directory: PathBuf, interval_seconds| MetricsOutput {
            directory,
            interval_seconds,
        };
        metrics(root.join("metrics"), 15).validate(&home).unwrap();
        for protected in [
            "home",
            "home/data",
            "home/data/metrics",
            "home/appdb",
            "home/config",
        ] {
            assert!(
                metrics(root.join(protected), 15).validate(&home).is_err(),
                "{protected}"
            );
        }
        for interval in [0, 3601] {
            assert!(metrics(root.join("metrics"), interval)
                .validate(&home)
                .is_err());
        }
        symlink(root.join("metrics"), root.join("alias")).unwrap();
        assert!(metrics(root.join("alias"), 15).validate(&home).is_err());
        mode("metrics", 0o770);
        assert!(metrics(root.join("metrics"), 15).validate(&home).is_err());

        mode("snapshots", 0o700);
        let snapshots = |interval_blocks, keep| SnapshotOutput {
            directory: root.join("snapshots"),
            interval_blocks,
            keep,
        };
        snapshots(100, 2).validate(&home).unwrap();
        assert!(snapshots(0, 2).validate(&home).is_err());
        assert!(snapshots(100, 0).validate(&home).is_err());
        mode("snapshots", 0o750);
        assert!(snapshots(100, 2).validate(&home).is_err());
    }
    #[test]
    fn block_history_and_metrics_have_no_defaults() {
        for (text, mode) in [
            ("window", BlockHistory::Window),
            ("archive", BlockHistory::Archive),
        ] {
            let parsed: BlockHistory = serde_json::from_value(serde_json::json!(text)).unwrap();
            assert_eq!((parsed, parsed.arg()), (mode, text));
        }
        assert!(serde_json::from_value::<BlockHistory>(serde_json::json!("pruned")).is_err());
        assert!(
            serde_json::from_value::<MetricsOutput>(serde_json::json!({"directory":"/m"})).is_err()
        );
        assert!(serde_json::from_value::<SnapshotOutput>(
            serde_json::json!({"directory":"/s","interval_blocks":1,"keep":1,"format":1})
        )
        .is_err());
    }
    fn service(mode: &str) -> NativeServiceConfig {
        let pin = |path: &str| serde_json::json!({"path":path,"sha256":"0".repeat(64),"max_bytes":1});
        serde_json::from_value(serde_json::json!({
            "schema":1,"mode":mode,"home":"/n","lock_directory":"/l",
            "consensus_config":pin("/c"),"application_genesis":pin("/g"),"root_config":pin("/r"),
            "root_public_inputs":[],"emergency_verifier_config":pin("/e"),
            "candidate_config":pin("/k"),"process_admission":pin("/a"),"engine_inputs":[],
            "validator_public_key_sha256":"0".repeat(64),
            "process":{"startup_millis":1000,"stop_millis":1,"kill_millis":1,"poll_millis":1,
                "max_argument_bytes":1,"max_environment_bytes":1,"max_probe_request_bytes":1,
                "max_probe_response_bytes":1},
            "environment":{},"monitor_interval_millis":1,"max_state_entries":1,
            "metrics":{"directory":"/m","interval_seconds":15},"block_history":"window"}))
        .unwrap()
    }
    #[test]
    fn each_build_runs_only_its_own_mode() {
        let other = if cfg!(feature = "production") { DEVELOPMENT_MODE } else { PRODUCTION_MODE };
        let error = service(other).validate_mode().unwrap_err().to_string();
        assert!(error.contains(&format!("runs only the {MODE} service mode")), "{error}");
        assert_eq!(service(PRODUCTION_MODE).production(), true);
        assert_eq!(service(DEVELOPMENT_MODE).production(), false);
    }
    #[test]
    fn production_settings_fix_what_each_role_runs() {
        let pin = PinnedInput { path: "/b".into(), sha256: "0".repeat(64), max_bytes: 4096 };
        let production = |role| {
            let mut config = service(PRODUCTION_MODE);
            config.role = Some(role);
            config.binding = Some(pin.clone());
            config.catch_up_millis = Some(3_600_000);
            config
        };
        for role in [NodeRole::Validator, NodeRole::Sentry] {
            production(role).validate_settings().unwrap();
            let mut serving = production(role);
            serving.adapter_listen = Some("127.0.0.1:8545".into());
            assert!(serving.validate_settings().is_err(), "{role:?} with an adapter");
        }
        let mut endpoint = production(NodeRole::Endpoint);
        assert!(endpoint.validate_settings().is_err(), "endpoint without its listeners");
        endpoint.adapter_listen = Some("127.0.0.1:8545".into());
        assert!(endpoint.validate_settings().is_err(), "endpoint without its channel");
        endpoint.adapter_channel = Some(serde_json::from_value(serde_json::json!({
            "listen":"203.0.113.9:26670","pin":{"path":"/p","sha256":"0".repeat(64),"max_bytes":1}})).unwrap());
        endpoint.validate_settings().unwrap();
        // Every production setting is required, and bounded.
        for change in [
            (|c: &mut NativeServiceConfig| c.role = None) as fn(&mut NativeServiceConfig),
            |c| c.binding = None,
            |c| c.catch_up_millis = None,
            |c| c.catch_up_millis = Some(999),
            |c| c.catch_up_millis = Some(MAX_CATCH_UP_MILLIS + 1),
            |c| c.binding.as_mut().unwrap().max_bytes = MAX_BINDING_BYTES + 1,
        ] {
            let mut config = production(NodeRole::Sentry);
            change(&mut config);
            assert!(config.validate_settings().is_err());
        }
        let mut config = production(NodeRole::Sentry);
        config.catch_up_millis = Some(MAX_CATCH_UP_MILLIS);
        config.validate_settings().unwrap();
        // The development mode has none of them.
        for change in [
            (|c: &mut NativeServiceConfig| c.role = Some(NodeRole::Validator)) as fn(&mut NativeServiceConfig),
            |c| c.catch_up_millis = Some(1000),
            |c| c.state_sync = Some(StateSyncInput { light_blocks: vec!["/lb".into()] }),
        ] {
            let mut config = service(DEVELOPMENT_MODE);
            change(&mut config);
            assert!(config.validate_settings().is_err());
        }
        assert!(serde_json::from_value::<NodeRole>(serde_json::json!("observer")).is_err());
    }
    /// An engine configuration, a genesis with one validator and a
    /// transport file.
    fn engine_files() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let genesis = serde_json::to_vec(&serde_json::json!({"chain_id":"dytallix-mainnet-1",
            "validators":[{"pub_key":{"type":"cometbft/PubKeyMlDsa65","value":STANDARD.encode([7u8; 1952])}}]})).unwrap();
        let transport = serde_json::to_vec(&serde_json::json!({"version":1,"profile":"dytallix-pqc-production-v1",
            "local_public_key_base64":STANDARD.encode([9u8; 1952])})).unwrap();
        (b"config".to_vec(), genesis, transport)
    }
    fn binding_for(role: &str, files: &(Vec<u8>, Vec<u8>, Vec<u8>), validator: &[u8]) -> serde_json::Value {
        let digest = |bytes: &[u8]| hex::encode(Sha256::digest(bytes));
        serde_json::json!({"schema":1,"role":role,"chain_id":"dytallix-mainnet-1",
            "config_sha256":digest(&files.0),"genesis_sha256":digest(&files.1),
            "transport_sha256":digest(&files.2),"peer_public_key_sha256":digest(&[9u8; 1952]),
            "validator_public_key_sha256":digest(validator)})
    }
    fn raw(binding: &serde_json::Value) -> Vec<u8> {
        let decoded: ProductionBinding = serde_json::from_value(binding.clone()).unwrap();
        serde_json::to_vec(&decoded).unwrap()
    }
    #[test]
    fn binding_matches_the_pinned_engine_files_and_role() {
        let genesis_key = [7u8; 1952];
        let other_key = [8u8; 1952];
        let files = engine_files();
        let check = |raw: &[u8], role, key: &[u8]| {
            check_binding(raw, role, &EngineFiles {
                config: &files.0, genesis: &files.1, transport: &files.2, validator_public_key: key })
        };
        let validator = raw(&binding_for("validator", &files, &genesis_key));
        check(&validator, NodeRole::Validator, &genesis_key).unwrap();
        // A trailing newline is allowed, as the engine allows it.
        check(&[validator.as_slice(), b"\n"].concat(), NodeRole::Validator, &genesis_key).unwrap();
        // A genesis key never runs as a sentry or endpoint; a key outside the
        // genesis set never runs as a validator.
        for role in [NodeRole::Sentry, NodeRole::Endpoint] {
            let binding = raw(&binding_for(role.as_str(), &files, &genesis_key));
            assert!(check(&binding, role, &genesis_key).is_err(), "{role:?}");
            let binding = raw(&binding_for(role.as_str(), &files, &other_key));
            check(&binding, role, &other_key).unwrap();
        }
        let binding = raw(&binding_for("validator", &files, &other_key));
        assert!(check(&binding, NodeRole::Validator, &other_key).is_err());
        // The binding is for this node's role.
        assert!(check(&validator, NodeRole::Sentry, &genesis_key).is_err());
        // Each changed field is refused.
        for (key, value) in [
            ("schema", serde_json::json!(2)),
            ("chain_id", serde_json::json!("another-chain")),
            ("config_sha256", serde_json::json!("00".repeat(32))),
            ("genesis_sha256", serde_json::json!("00".repeat(32))),
            ("transport_sha256", serde_json::json!("00".repeat(32))),
            ("peer_public_key_sha256", serde_json::json!("00".repeat(32))),
            ("validator_public_key_sha256", serde_json::json!("00".repeat(32))),
        ] {
            let mut changed = binding_for("validator", &files, &genesis_key);
            changed[key] = value;
            assert!(check(&raw(&changed), NodeRole::Validator, &genesis_key).is_err(), "{key}");
        }
        // Only canonical compact JSON in the engine's field order.
        let text = String::from_utf8(validator.clone()).unwrap();
        for changed in [
            text.replacen("\"schema\":1", "\"schema\": 1", 1),
            text.replacen("{\"schema\":1,\"role\":\"validator\"", "{\"role\":\"validator\",\"schema\":1", 1),
            text.replacen("{\"schema\":1", "{\"schema\":1,\"note\":\"x\"", 1),
        ] {
            assert!(check(changed.as_bytes(), NodeRole::Validator, &genesis_key).is_err(), "{changed}");
        }
    }
    #[test]
    fn state_sync_exports_are_distinct_protected_directories() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let home = root.join("home");
        for dir in ["home/config", "primary", "witness"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
            std::fs::set_permissions(root.join(dir), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let exports = |dirs: &[&str]| StateSyncInput {
            light_blocks: dirs.iter().map(|d| root.join(d)).collect(),
        };
        exports(&["primary", "witness"]).validate(&home).unwrap();
        assert!(exports(&[]).validate(&home).is_err());
        assert!(exports(&["primary", "primary"]).validate(&home).is_err());
        assert!(exports(&["home/config"]).validate(&home).is_err());
        assert!(exports(&["absent"]).validate(&home).is_err());
        symlink(root.join("primary"), root.join("alias")).unwrap();
        assert!(exports(&["alias"]).validate(&home).is_err());
        std::fs::set_permissions(root.join("witness"), std::fs::Permissions::from_mode(0o775)).unwrap();
        assert!(exports(&["witness"]).validate(&home).is_err());
        let many: Vec<String> = (0..=MAX_LIGHT_BLOCK_EXPORTS).map(|i| format!("e{i}")).collect();
        for dir in &many {
            std::fs::create_dir(root.join(dir)).unwrap();
            std::fs::set_permissions(root.join(dir), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let names: Vec<&str> = many.iter().map(String::as_str).collect();
        assert!(exports(&names).validate(&home).is_err());
        exports(&names[..MAX_LIGHT_BLOCK_EXPORTS]).validate(&home).unwrap();
    }
    #[test]
    fn production_p2p_listener_is_one_explicit_address() {
        for good in ["203.0.113.9:26656", "[2001:db8::9]:26656", "10.0.0.2:1024"] {
            explicit_endpoint(good).unwrap();
        }
        for bad in [
            "127.0.0.1:26656",
            "0.0.0.0:26656",
            "[::]:26656",
            "[::1]:26656",
            "224.0.0.1:26656",
            "169.254.1.1:26656",
            "255.255.255.255:26656",
            "[fe80::1]:26656",
            "203.0.113.9:1023",
            "203.0.113.9:026656",
            "node.example:26656",
        ] {
            assert!(explicit_endpoint(bad).is_err(), "{bad}");
        }
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
