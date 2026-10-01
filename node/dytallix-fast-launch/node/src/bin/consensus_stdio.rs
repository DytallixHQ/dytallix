//! Private pipe protocol used only by the pinned local CometBFT bridge.
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use dytallix_fast_node::consensus_settlement::{
    BlockHistory, ConsensusApplication, ConsensusConfig, FinalizedBlockInput, SnapshotChunk,
    SnapshotOffer, ValidatorConfig,
};
use dytallix_fast_node::failure_class::{class_of, FailureClass};
use dytallix_fast_node::runtime::penalty_custody::EvidenceFact;
use dytallix_fast_node::runtime::validator_lifecycle::LifecycleConfig;
use dytallix_release_runtime::ownership::{self, Role};
use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicU8, Ordering};

const MAX_FRAME: u64 = 8 * 1024 * 1024;
fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("Missing string: {key}"))
}
fn unsigned(v: &Value, key: &str) -> Result<u64> {
    v.get(key)
        .and_then(Value::as_u64)
        .with_context(|| format!("Missing unsigned integer: {key}"))
}
fn signed(v: &Value, key: &str) -> Result<i64> {
    v.get(key)
        .and_then(Value::as_i64)
        .with_context(|| format!("Missing signed integer: {key}"))
}
#[derive(Debug, PartialEq, Eq)]
enum CheckTxKind {
    New,
    Recheck,
}
fn check_tx_kind(payload: &Value) -> Result<CheckTxKind> {
    match string(payload, "type")? {
        "new" => Ok(CheckTxKind::New),
        "recheck" => Ok(CheckTxKind::Recheck),
        _ => bail!("Unknown CheckTx type"),
    }
}
fn canonical_base64(value: &str) -> Result<Vec<u8>> {
    let bytes = STANDARD
        .decode(value)
        .context("Invalid base64 transport bytes")?;
    ensure!(
        STANDARD.encode(&bytes) == value,
        "Noncanonical base64 transport bytes"
    );
    Ok(bytes)
}
#[derive(Debug, PartialEq, Eq)]
enum QueryPath<'a> {
    Status,
    OrdinaryProfile,
    GovernanceProfile,
    OrdinaryReceipt(&'a str),
    OrdinaryAccount(&'a str),
    RecoveryAccount(&'a str),
    EmergencyReceipt(&'a str),
    StateProof(&'a str),
    AccountSummary(&'a str),
    Validators,
    Proposal(u64),
    Vote(u64, &'a str),
}
/// A canonical decimal proposal ID: no sign, no leading zero, at most u64.
fn proposal_id(raw: &str) -> Result<u64> {
    ensure!(
        !raw.is_empty()
            && raw.len() <= 20
            && raw.bytes().all(|b| b.is_ascii_digit())
            && (raw == "0" || !raw.starts_with('0')),
        "Proposal ID must be a canonical decimal"
    );
    Ok(raw.parse()?)
}
fn lowercase_hex64(raw: &str) -> bool {
    raw.len() == 64
        && raw
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn query_path(path: &str) -> Result<QueryPath<'_>> {
    match path {
        "" | "/status" | "/supply" => Ok(QueryPath::Status),
        "/ordinary/profile" => Ok(QueryPath::OrdinaryProfile),
        "/ordinary/profile_v3" => Ok(QueryPath::GovernanceProfile),
        "/staking/validators" => Ok(QueryPath::Validators),
        _ if path.starts_with("/account/") => {
            let address = path.strip_prefix("/account/").unwrap();
            // The chain decodes and checks the address; this bounds the input.
            ensure!(
                !address.is_empty()
                    && address.len() <= 128
                    && address
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()),
                "Account summary requires a lowercase account address"
            );
            Ok(QueryPath::AccountSummary(address))
        }
        _ if path.starts_with("/governance/proposal/") => Ok(QueryPath::Proposal(proposal_id(
            path.strip_prefix("/governance/proposal/").unwrap(),
        )?)),
        _ if path.starts_with("/governance/vote/") => {
            let rest = path.strip_prefix("/governance/vote/").unwrap();
            let (id, voter) = rest
                .split_once('/')
                .context("Vote query requires a proposal ID and an account ID")?;
            ensure!(
                lowercase_hex64(voter),
                "Vote query requires a lowercase 64-hex account ID"
            );
            Ok(QueryPath::Vote(proposal_id(id)?, voter))
        }
        _ if path.starts_with("/state/proof/") => {
            let key = path.strip_prefix("/state/proof/").unwrap();
            ensure!(
                !key.is_empty()
                    && key.len() <= 1_024
                    && key.len() % 2 == 0
                    && key
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "State proof query requires a lowercase hex key"
            );
            Ok(QueryPath::StateProof(key))
        }
        // E04 gap 17, T-c: what a client needs to build a recovery action.
        _ if path.starts_with("/recovery/account/") => {
            let id = path.strip_prefix("/recovery/account/").unwrap();
            ensure!(
                lowercase_hex64(id),
                "Recovery query requires a lowercase 64-hex account ID"
            );
            Ok(QueryPath::RecoveryAccount(id))
        }
        _ if path.starts_with("/emergency/receipt/") => {
            let id = path.strip_prefix("/emergency/receipt/").unwrap();
            ensure!(
                id.len() == 64
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "Emergency query requires a lowercase 64-hex digest"
            );
            Ok(QueryPath::EmergencyReceipt(id))
        }
        _ => {
            let (id, account) = if let Some(id) = path.strip_prefix("/ordinary/account/") {
                (id, true)
            } else {
                (
                    path.strip_prefix("/ordinary/receipt/")
                        .context("Unsupported query path")?,
                    false,
                )
            };
            ensure!(
                id.len() == 64
                    && id
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "Ordinary query requires a lowercase 64-hex ID"
            );
            Ok(if account {
                QueryPath::OrdinaryAccount(id)
            } else {
                QueryPath::OrdinaryReceipt(id)
            })
        }
    }
}
fn transactions(v: &Value) -> Result<Vec<Vec<u8>>> {
    v.get("txs")
        .and_then(Value::as_array)
        .context("Missing transactions")?
        .iter()
        .map(|tx| canonical_base64(tx.as_str().context("Transaction must be base64")?))
        .collect()
}
fn misbehavior(v: &Value) -> Result<Vec<EvidenceFact>> {
    match v.get("misbehavior") {
        None => Ok(Vec::new()),
        Some(value) => {
            let facts: Vec<EvidenceFact> = serde_json::from_value(value.clone())?;
            ensure!(facts.len() <= 64, "Too many evidence facts");
            Ok(facts)
        }
    }
}
fn block(v: &Value) -> Result<FinalizedBlockInput> {
    Ok(FinalizedBlockInput {
        height: unsigned(v, "height")?,
        time_seconds: signed(v, "time_seconds")?,
        time_nanos: i32::try_from(signed(v, "time_nanos")?)?,
        hash: string(v, "hash")?.into(),
        txs: transactions(v)?,
        misbehavior: misbehavior(v)?,
    })
}
fn check_evidence_limits(lifecycle: Option<&LifecycleConfig>, payload: &Value) -> Result<()> {
    let Some(lifecycle) = lifecycle else {
        return Ok(());
    };
    let blocks = u64::try_from(signed(payload, "evidence_max_age_blocks")?)
        .context("Engine evidence block age is negative")?;
    let seconds = u64::try_from(signed(payload, "evidence_max_age_seconds")?)
        .context("Engine evidence duration is negative")?;
    let nanos = i32::try_from(signed(payload, "evidence_max_age_nanos")?)
        .context("Engine evidence nanosecond remainder exceeds i32")?;
    ensure!(
        blocks == lifecycle.evidence_max_age_blocks
            && seconds == lifecycle.evidence_max_age_seconds
            && nanos == 0,
        "Engine evidence limits differ from lifecycle configuration"
    );
    Ok(())
}
fn handle(
    app: &mut ConsensusApplication,
    config: &ConsensusConfig,
    request: Value,
) -> Result<Value> {
    let method = string(&request, "method")?;
    let payload = request.get("payload").context("Missing payload")?;
    match method {
        "info" => {
            let info = app.info()?;
            let app_version = if config.penalty.is_some() {
                3
            } else if config.lifecycle.is_some() {
                2
            } else {
                1
            };
            Ok(
                json!({"height":info.height,"app_hash":info.app_hash,"app_version":app_version,
                "helper_admission_receipt":dytallix_fast_node::root_genesis::helper_admission_receipt()}),
            )
        }
        "init_chain" => {
            check_evidence_limits(config.lifecycle.as_ref(), payload)?;
            let mut validators = Vec::new();
            let engine_validators = payload
                .get("validators")
                .and_then(Value::as_array)
                .context("Missing validators")?;
            anyhow::ensure!(
                engine_validators.len() == config.validators.len(),
                "Engine genesis validator count differs from configuration"
            );
            for raw in engine_validators {
                let key = string(raw, "pubkey_base64")?;
                let expected = config
                    .validators
                    .iter()
                    .find(|v| v.pubkey_base64 == key)
                    .context("Unknown engine validator")?;
                validators.push(ValidatorConfig {
                    pubkey_type: string(raw, "pubkey_type")?.into(),
                    pubkey_base64: key.into(),
                    power: signed(raw, "power")?,
                    reward_address: expected.reward_address.clone(),
                });
            }
            let bytes = STANDARD.decode(string(payload, "app_state_bytes")?)?;
            let info = app.init_chain(
                string(payload, "chain_id")?,
                unsigned(payload, "initial_height")?,
                &bytes,
                &validators,
            )?;
            Ok(json!({"app_hash":info.app_hash}))
        }
        "check_tx" => {
            let kind = check_tx_kind(payload)?;
            let raw = canonical_base64(string(payload, "tx")?)?;
            let result = if kind == CheckTxKind::Recheck && config.ordinary.is_some() {
                app.recheck_ordinary_admission_result(&raw)?
            } else {
                app.check_tx_result(&raw)?
            };
            Ok(serde_json::to_value(result)?)
        }
        "prepare_proposal" => {
            let txs = app.prepare_proposal_with_evidence(
                unsigned(payload, "height")?,
                signed(payload, "time_seconds")?,
                i32::try_from(signed(payload, "time_nanos")?)?,
                transactions(payload)?,
                unsigned(payload, "max_tx_bytes")?,
                misbehavior(payload)?,
            )?;
            Ok(json!({"txs":txs.iter().map(|tx| STANDARD.encode(tx)).collect::<Vec<_>>()}))
        }
        "process_proposal" => Ok(json!({"accept":app.process_proposal(block(payload)?)?})),
        "finalize_block" => Ok(serde_json::to_value(app.finalize_block(block(payload)?)?)?),
        "commit" => {
            app.commit()?;
            Ok(json!({"retain_height": app.retain_height()?}))
        }
        // State sync v1, rule 4: restore a snapshot into an empty node.
        "offer_snapshot" => {
            let offer = app.offer_snapshot(
                unsigned(payload, "height")?,
                u32::try_from(unsigned(payload, "format")?)?,
                u32::try_from(unsigned(payload, "chunks")?)?,
                &canonical_base64(string(payload, "hash")?)?,
                &canonical_base64(string(payload, "metadata")?)?,
                string(payload, "app_hash")?,
            )?;
            let result = match offer {
                SnapshotOffer::Accept => "accept",
                SnapshotOffer::Reject => "reject",
                SnapshotOffer::RejectFormat => "reject_format",
                SnapshotOffer::Abort => "abort",
            };
            Ok(json!({"result": result}))
        }
        "apply_snapshot_chunk" => {
            let applied = app.apply_snapshot_chunk(
                u32::try_from(unsigned(payload, "index")?)?,
                &canonical_base64(string(payload, "chunk")?)?,
            )?;
            let result = match applied {
                SnapshotChunk::Accept => "accept",
                SnapshotChunk::Retry => "retry",
                SnapshotChunk::RejectSnapshot => "reject_snapshot",
                SnapshotChunk::Abort => "abort",
            };
            Ok(json!({"result": result}))
        }
        "query" => {
            ensure!(
                !payload
                    .get("prove")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                "Proof queries are not qualified"
            );
            let path = string(payload, "path")?;
            let path = query_path(path)?;
            let wanted = signed(payload, "height")?;
            use dytallix_fast_node::consensus_settlement::QueryRequest;
            let request = match path {
                QueryPath::Status => QueryRequest::Status,
                QueryPath::OrdinaryProfile => QueryRequest::OrdinaryProfile,
                QueryPath::GovernanceProfile => QueryRequest::GovernanceProfile,
                QueryPath::OrdinaryReceipt(id) => QueryRequest::OrdinaryReceipt(id),
                QueryPath::OrdinaryAccount(id) => QueryRequest::OrdinaryAccount(id),
                QueryPath::RecoveryAccount(id) => QueryRequest::RecoveryAccount(id),
                QueryPath::EmergencyReceipt(id) => QueryRequest::EmergencyReceipt(id),
                QueryPath::StateProof(key) => QueryRequest::StateProof(key),
                QueryPath::AccountSummary(address) => QueryRequest::AccountSummary(address),
                QueryPath::Validators => QueryRequest::Validators,
                QueryPath::Proposal(id) => QueryRequest::Proposal(id),
                QueryPath::Vote(id, voter) => QueryRequest::Vote(id, voter),
            };
            let (info, value) = app.query_at(request, wanted)?;
            Ok(
                json!({"code":0,"log":"","height":info.height,"value":STANDARD.encode(serde_json::to_vec(&value)?)}),
            )
        }
        _ => bail!("Unsupported application method"),
    }
}
// Keep partial input across cancellation polls. A frame is accepted only after
// its newline arrives. EOF with an incomplete frame remains a protocol error.
fn read_frame(
    input: &mut impl BufRead,
    check: impl Fn() -> Result<()>,
    wait: impl Fn() -> Result<()>,
) -> Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        check()?;
        match input.fill_buf() {
            Ok([]) => {
                ensure!(line.is_empty(), "Invalid pipe frame length");
                return Ok(None);
            }
            Ok(bytes) => {
                let end = bytes.iter().position(|v| *v == b'\n').map(|i| i + 1);
                let count = end.unwrap_or(bytes.len());
                ensure!(
                    count <= (MAX_FRAME as usize).saturating_sub(line.len()),
                    "Invalid pipe frame length"
                );
                line.extend_from_slice(&bytes[..count]);
                input.consume(count);
                if end.is_some() {
                    return Ok(Some(line));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                wait()?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
}

fn main() -> std::process::ExitCode {
    // Keep the inherited diagnostic socket free of the default blocking panic hook.
    // Panic propagation, unwinding and nonzero termination remain unchanged.
    std::panic::set_hook(Box::new(|_| {}));
    // run() completes owned cleanup before returning. Do not add Rust's generic
    // error-to-stderr fallback to the bounded helper diagnostic channel.
    let result = run();
    #[cfg(feature = "test-snapshot-verifier")]
    if let Err(error) = &result {
        // Only the test build reports why it stopped.
        eprintln!("{error:#}");
    }
    let fatal = FailureClass::from_exit_status(FATAL.load(Ordering::SeqCst).into());
    exit_code(fatal, result)
}

/// The exit status of the class of the last failed engine-stopping method,
/// or 0; a later successful commit clears it. The engine stops on such a
/// failure, so in service no call follows and the application exits with
/// its class however its input then ends (E04 gap 15). A test harness may
/// continue past it.
static FATAL: AtomicU8 = AtomicU8::new(0);

/// Methods whose failure stops the engine.
const ENGINE_FATAL: [&str; 6] = [
    "info",
    "init_chain",
    "prepare_proposal",
    "process_proposal",
    "finalize_block",
    "commit",
];

/// The exit status carries only the failure class (1 when unclassified):
/// the diagnostic channel gets no text.
fn exit_code(fatal: Option<FailureClass>, result: Result<()>) -> std::process::ExitCode {
    let class = fatal.or_else(|| result.as_ref().err().and_then(class_of));
    match (class, result) {
        (Some(class), _) => std::process::ExitCode::from(class.exit_status()),
        (None, Ok(())) => std::process::ExitCode::SUCCESS,
        (None, Err(_)) => std::process::ExitCode::FAILURE,
    }
}

fn run() -> Result<()> {
    ownership::install_cancellation()?;
    let admission = ownership::admit(Role::Application)?;
    let mut args = std::env::args().skip(1);
    let mut config_path = None;
    let mut genesis_path = None;
    let mut db_path = None;
    let mut root_path: Option<String> = None;
    #[cfg_attr(feature = "production", allow(unused_mut))]
    let mut development_root_path: Option<String> = None;
    #[cfg_attr(feature = "production", allow(unused_mut))]
    let mut emergency_verifier_path: Option<String> = None;
    #[cfg_attr(feature = "production", allow(unused_mut))]
    let mut candidate_path: Option<String> = None;
    let mut release_manifest_sha512 = None;
    let mut block_history = None;
    let mut snapshot_dir = None;
    let mut snapshot_interval = None;
    let mut snapshot_keep = None;
    let mut metrics_dir = None;
    let mut metrics_interval = None;
    let mut restart_path = None;
    while let Some(arg) = args.next() {
        let value = args.next().context("Each argument requires a value")?;
        let slot = match arg.as_str() {
            "--release-manifest-sha512" => &mut release_manifest_sha512,
            "--config" => &mut config_path,
            "--genesis" => &mut genesis_path,
            "--db" => &mut db_path,
            // The root genesis signed three of five (production activation
            // v1, A2): the only open path of a production build.
            "--root-config" => &mut root_path,
            // A production build has no development entry points
            // (production activation v1, A1).
            #[cfg(not(feature = "production"))]
            "--development-root-config" => &mut development_root_path,
            #[cfg(not(feature = "production"))]
            "--development-emergency-verifier-config" => &mut emergency_verifier_path,
            #[cfg(not(feature = "production"))]
            "--development-candidate-config" => &mut candidate_path,
            "--block-history" => &mut block_history,
            "--snapshot-dir" => &mut snapshot_dir,
            "--snapshot-interval" => &mut snapshot_interval,
            "--snapshot-keep" => &mut snapshot_keep,
            "--metrics-dir" => &mut metrics_dir,
            "--metrics-interval-seconds" => &mut metrics_interval,
            "--restart-authorization" => &mut restart_path,
            _ => bail!("Unsupported argument"),
        };
        ensure!(slot.replace(value).is_none(), "Duplicate argument");
    }
    let release_context = ownership::parse_context_sha512(
        &release_manifest_sha512.context("--release-manifest-sha512 is required")?,
    )?;
    admission.verify_context(&release_context)?;
    ownership::check_cancellation()?;
    let config_bytes = std::fs::read(config_path.context("--config is required")?)?;
    ensure!(
        config_bytes.len() <= dytallix_fast_node::consensus_settlement::MAX_CONFIG_BYTES,
        "Configuration exceeds limit"
    );
    let config: ConsensusConfig = serde_json::from_slice(&config_bytes)?;
    let genesis = std::fs::read(genesis_path.context("--genesis is required")?)?;
    ensure!(
        genesis.len() <= MAX_FRAME as usize,
        "Genesis exceeds local fixture limit"
    );
    let database = db_path.context("--db is required")?;
    // A local setting: the retained window (default) or every block record.
    let block_history = match block_history.as_deref() {
        None | Some("window") => BlockHistory::Window,
        Some("archive") => BlockHistory::Archive,
        Some(_) => bail!("--block-history must be window or archive"),
    };
    // Snapshots are written only when the operator sets all three values
    // (E05); there are no defaults.
    let snapshots = match (snapshot_dir, snapshot_interval, snapshot_keep) {
        (None, None, None) => None,
        (Some(dir), Some(interval), Some(keep)) => {
            Some(dytallix_fast_node::snapshot::SnapshotConfig {
                dir: dir.into(),
                interval: interval.parse().context("Invalid --snapshot-interval")?,
                keep: keep.parse().context("Invalid --snapshot-keep")?,
            })
        }
        _ => bail!("--snapshot-dir, --snapshot-interval and --snapshot-keep go together"),
    };
    ensure!(
        candidate_path.is_none()
            || (development_root_path.is_some() && emergency_verifier_path.is_some()),
        "Candidate verification requires root and emergency configuration"
    );
    // Restart v1 (E04 gap 18): the target release runs the halted block.
    ensure!(
        restart_path.is_none() || candidate_path.is_some(),
        "--restart-authorization requires --development-candidate-config"
    );
    let restart = restart_path
        .map(|path| -> Result<Vec<u8>> {
            let mut bytes = Vec::new();
            std::fs::File::open(path)?
                .take(262_145)
                .read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= 262_144, "Restart authorization exceeds limit");
            Ok(bytes)
        })
        .transpose()?;
    ensure!(
        root_path.is_none() || (development_root_path.is_none() && emergency_verifier_path.is_none()),
        "--root-config cannot be combined with development root configuration"
    );
    let mut app = match (root_path, development_root_path, emergency_verifier_path) {
        (Some(path), _, _) => {
            let root = dytallix_fast_node::root_genesis::RootGenesis::from_config(
                std::path::Path::new(&path),
            )?;
            ensure!(
                ownership::parse_context_sha512(&root.release_manifest_sha512)? == release_context,
                "Root authorization release differs from owner admission"
            );
            admission.check()?;
            ConsensusApplication::open_with_root(
                std::path::Path::new(&database),
                config.clone(),
                genesis,
                &config_bytes,
                root,
            )?
        }
        (None, Some(root_path), Some(verifier_path)) => {
            let bytes = std::fs::read(verifier_path)?;
            ensure!(
                bytes.len() <= 65_536,
                "Emergency verifier configuration exceeds limit"
            );
            let authorization =
                dytallix_fast_node::root_genesis::DevelopmentRootGenesis::from_development_config(
                    std::path::Path::new(&root_path),
                )?;
            ensure!(
                ownership::parse_context_sha512(&authorization.release_manifest_sha512)?
                    == release_context,
                "Root authorization release differs from owner admission"
            );
            admission.check()?;
            let candidate = if let Some(path) = candidate_path {
                let mut bytes = Vec::new();
                std::fs::File::open(path)?
                    .take(65_537)
                    .read_to_end(&mut bytes)?;
                ensure!(
                    bytes.len() <= 65536,
                    "Candidate configuration exceeds limit"
                );
                Some(serde_json::from_slice::<
                    dytallix_fast_node::runtime_candidate_v2::RuntimeCandidateInput,
                >(&bytes)?)
            } else {
                None
            };
            // The unchanged root consumer verifies the signed manifest digest
            // before opening state. Candidate input cannot authorize a release.
            ConsensusApplication::open_with_development_runtime_candidate_and_restart(
                std::path::Path::new(&database),
                config.clone(),
                genesis,
                &config_bytes,
                authorization,
                serde_json::from_slice(&bytes)?,
                candidate,
                restart,
            )?
        }
        (None, Some(path), None) => {
            let authorization =
                dytallix_fast_node::root_genesis::DevelopmentRootGenesis::from_development_config(
                    std::path::Path::new(&path),
                )?;
            ensure!(
                ownership::parse_context_sha512(&authorization.release_manifest_sha512)?
                    == release_context,
                "Root authorization release differs from owner admission"
            );
            admission.check()?;
            ConsensusApplication::open_with_development_root(
                std::path::Path::new(&database),
                config.clone(),
                genesis,
                &config_bytes,
                authorization,
            )?
        }
        (None, None, Some(_)) => {
            bail!("Emergency verifier requires development root configuration")
        }
        (None, None, None) => {
            ConsensusApplication::open(std::path::Path::new(&database), config.clone(), genesis)?
        }
    }
    .with_block_history(block_history);
    let snapshot_dir = snapshots.as_ref().map(|config| config.dir.clone());
    if let Some(config) = snapshots {
        app = app.with_snapshots(config)?;
    }
    // Metrics v1: a text file at the operator's interval; no defaults.
    match (metrics_dir, metrics_interval) {
        (None, None) => {}
        (Some(dir), Some(interval)) => {
            let dir = std::path::PathBuf::from(dir);
            let interval: u64 = interval.parse().context("Invalid --metrics-interval-seconds")?;
            ensure!(
                dir.is_absolute() && dir.is_dir() && (1..=3600).contains(&interval),
                "--metrics-dir must be an existing absolute directory and the interval 1 to 3600 seconds"
            );
            let metrics = std::sync::Arc::new(dytallix_fast_node::app_metrics::AppMetrics::new());
            app = app.with_metrics(metrics.clone());
            std::thread::spawn(move || loop {
                let snapshot = snapshot_dir
                    .as_deref()
                    .and_then(dytallix_fast_node::app_metrics::latest_snapshot);
                // A failed write leaves the previous file, which then shows as stale.
                let _ = metrics.write_file(&dir, snapshot);
                std::thread::sleep(std::time::Duration::from_secs(interval));
            });
        }
        _ => bail!("--metrics-dir and --metrics-interval-seconds go together"),
    }
    let stdin = std::io::stdin();
    let flags = unsafe { libc::fcntl(stdin.as_raw_fd(), libc::F_GETFL) };
    ensure!(
        flags >= 0
            && unsafe { libc::fcntl(stdin.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) }
                == 0,
        "Cannot make application input cancellable"
    );
    let mut input = std::io::BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    loop {
        let Some(line) = read_frame(
            &mut input,
            || {
                admission.check()?;
                ownership::check_cancellation()
            },
            || {
                let mut fd = libc::pollfd {
                    fd: stdin.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                let rc = unsafe { libc::poll(&mut fd, 1, 100) };
                if rc < 0 {
                    let error = std::io::Error::last_os_error();
                    if error.kind() != std::io::ErrorKind::Interrupted {
                        return Err(error.into());
                    }
                }
                ensure!(
                    fd.revents & (libc::POLLNVAL | libc::POLLERR) == 0,
                    "Application input polling failed"
                );
                Ok(())
            },
        )?
        else {
            return Ok(());
        };
        let response = match serde_json::from_slice::<Value>(&line)
            .map_err(anyhow::Error::from)
            .and_then(|request| {
                let method = request.get("method").and_then(Value::as_str).map(str::to_owned);
                let result = handle(&mut app, &config, request);
                let method = method.as_deref().unwrap_or_default();
                match &result {
                    Err(error) if ENGINE_FATAL.contains(&method) => FATAL.store(
                        class_of(error)
                            .unwrap_or(FailureClass::Execution)
                            .exit_status(),
                        Ordering::SeqCst,
                    ),
                    Ok(_) if method == "commit" => FATAL.store(0, Ordering::SeqCst),
                    _ => {}
                }
                result
            }) {
            Ok(result) => json!({"ok":true,"result":result}),
            Err(error) => json!({"ok":false,"error":error.to_string()}),
        };
        // The bridge refuses unknown response fields, so a test build cannot
        // serve a real engine.
        #[cfg(feature = "test-snapshot-verifier")]
        let response = {
            let mut response = response;
            response["test_build"] = json!("test-snapshot-verifier");
            response
        };
        serde_json::to_writer(&mut output, &response)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_status_carries_only_the_class() {
        let supply = dytallix_fast_node::failure_class::classify::<()>(
            Err(anyhow::anyhow!("totals differ")),
            FailureClass::Supply,
        );
        for (fatal, result, expected) in [
            (None, Ok(()), std::process::ExitCode::SUCCESS),
            (None, Err(anyhow::anyhow!("unclassified")), std::process::ExitCode::FAILURE),
            (None, supply, std::process::ExitCode::from(12)),
            // A failed block method decides the status however the input ends.
            (Some(FailureClass::Execution), Ok(()), std::process::ExitCode::from(15)),
            (
                Some(FailureClass::Storage),
                Err(anyhow::anyhow!("cancelled")),
                std::process::ExitCode::from(16),
            ),
        ] {
            assert_eq!(exit_code(fatal, result), expected);
        }
    }
    #[test]
    fn cancellable_frames_preserve_boundaries_and_reject_partial_eof() {
        let mut input = std::io::Cursor::new(b"one\ntwo\n");
        assert_eq!(
            read_frame(&mut input, || Ok(()), || Ok(())).unwrap(),
            Some(b"one\n".to_vec())
        );
        assert_eq!(
            read_frame(&mut input, || Ok(()), || Ok(())).unwrap(),
            Some(b"two\n".to_vec())
        );
        assert!(read_frame(&mut input, || Ok(()), || Ok(()))
            .unwrap()
            .is_none());
        assert!(read_frame(&mut std::io::Cursor::new(b"partial"), || Ok(()), || Ok(())).is_err());
        let mut oversized = vec![b'x'; MAX_FRAME as usize];
        oversized.push(b'\n');
        assert!(read_frame(&mut std::io::Cursor::new(oversized), || Ok(()), || Ok(())).is_err());
    }
    #[test]
    fn cancellable_frames_check_before_read_and_after_partial_input() {
        let calls = std::cell::Cell::new(0);
        let mut input = std::io::BufReader::with_capacity(2, std::io::Cursor::new(b"abcdef\n"));
        let error = read_frame(
            &mut input,
            || {
                let count = calls.get() + 1;
                calls.set(count);
                ensure!(count < 2, "cancelled test read");
                Ok(())
            },
            || Ok(()),
        )
        .unwrap_err();
        assert!(error.to_string().contains("cancelled test read"));
        assert_eq!(calls.get(), 2);
        assert!(read_frame(
            &mut std::io::Cursor::new(b"ok\n"),
            || bail!("cancelled before read"),
            || Ok(())
        )
        .is_err());
    }

    fn lifecycle() -> LifecycleConfig {
        LifecycleConfig {
            version: 1,
            profile: "cometbft-lifecycle-local-qualification".into(),
            chain_id: "evidence-limits-test".into(),
            approved_operators: std::collections::BTreeMap::from([(
                "validator".into(),
                "owner".into(),
            )]),
            min_self_bond: 10,
            max_active: 8,
            evidence_max_age_blocks: 3,
            evidence_max_age_seconds: 5,
            processing_margin_blocks: 1,
            processing_margin_seconds: 1,
        }
    }
    fn limits() -> Value {
        json!({"evidence_max_age_blocks":3,"evidence_max_age_seconds":5,"evidence_max_age_nanos":0})
    }
    #[test]
    fn evidence_wire_preserves_empty_compatibility_and_rejects_null() {
        assert!(misbehavior(&json!({})).unwrap().is_empty());
        assert!(misbehavior(&json!({"misbehavior":[]})).unwrap().is_empty());
        assert!(misbehavior(&json!({"misbehavior":null})).is_err());
    }
    #[test]
    fn evidence_wire_requires_bounded_complete_facts() {
        let fact = json!({"kind":"duplicate_vote","validator_address":"ab".repeat(20),
            "height":1,"time_seconds":12,"time_nanos":0,"power":10,"total_power":40});
        assert_eq!(
            misbehavior(&json!({"misbehavior":[fact.clone()]}))
                .unwrap()
                .len(),
            1
        );
        assert!(misbehavior(&json!({"misbehavior":vec![fact.clone();65]})).is_err());
        let mut unknown = fact.clone();
        unknown["proof"] = json!("not-an-engine-fact");
        assert!(misbehavior(&json!({"misbehavior":[unknown]})).is_err());
        let mut missing = fact;
        missing.as_object_mut().unwrap().remove("total_power");
        assert!(misbehavior(&json!({"misbehavior":[missing]})).is_err());
    }
    #[test]
    fn lifecycle_accepts_exact_engine_evidence_limits() {
        check_evidence_limits(Some(&lifecycle()), &limits()).unwrap();
    }
    #[test]
    fn lifecycle_rejects_changed_or_negative_engine_evidence_limits() {
        let config = lifecycle();
        for (field, values) in [
            (
                "evidence_max_age_blocks",
                vec![json!(2), json!(4), json!(-1)],
            ),
            (
                "evidence_max_age_seconds",
                vec![json!(4), json!(6), json!(-1)],
            ),
            (
                "evidence_max_age_nanos",
                vec![
                    json!(1),
                    json!(-1),
                    json!(1_000_000_000i64),
                    json!(i64::MAX),
                ],
            ),
        ] {
            for value in values {
                let mut payload = limits();
                payload[field] = value;
                assert!(
                    check_evidence_limits(Some(&config), &payload).is_err(),
                    "Accepted changed {field}"
                );
            }
        }
    }
    #[test]
    fn lifecycle_requires_all_evidence_fields_as_integers() {
        let config = lifecycle();
        for field in [
            "evidence_max_age_blocks",
            "evidence_max_age_seconds",
            "evidence_max_age_nanos",
        ] {
            let mut missing = limits();
            missing.as_object_mut().unwrap().remove(field);
            assert!(check_evidence_limits(Some(&config), &missing).is_err());
            for value in [Value::Null, json!("3"), json!(3.5)] {
                let mut invalid = limits();
                invalid[field] = value;
                assert!(check_evidence_limits(Some(&config), &invalid).is_err());
            }
        }
    }
    #[test]
    fn fixed_validator_profile_keeps_legacy_evidence_payload_compatibility() {
        check_evidence_limits(None, &json!({})).unwrap();
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;

    /// Every query path, by variant; a new variant fails to compile here
    /// until it is named, and the test below fails until it is inventoried.
    fn variant(path: &QueryPath<'_>) -> &'static str {
        match path {
            QueryPath::Status => "Status",
            QueryPath::OrdinaryProfile => "OrdinaryProfile",
            QueryPath::GovernanceProfile => "GovernanceProfile",
            QueryPath::OrdinaryReceipt(_) => "OrdinaryReceipt",
            QueryPath::OrdinaryAccount(_) => "OrdinaryAccount",
            QueryPath::RecoveryAccount(_) => "RecoveryAccount",
            QueryPath::EmergencyReceipt(_) => "EmergencyReceipt",
            QueryPath::StateProof(_) => "StateProof",
            QueryPath::AccountSummary(_) => "AccountSummary",
            QueryPath::Validators => "Validators",
            QueryPath::Proposal(_) => "Proposal",
            QueryPath::Vote(..) => "Vote",
        }
    }
    const VARIANTS: [&str; 12] = [
        "Status",
        "OrdinaryProfile",
        "GovernanceProfile",
        "OrdinaryReceipt",
        "OrdinaryAccount",
        "RecoveryAccount",
        "EmergencyReceipt",
        "StateProof",
        "AccountSummary",
        "Validators",
        "Proposal",
        "Vote",
    ];

    #[test]
    fn typed_read_paths_require_canonical_identifiers() {
        assert_eq!(
            query_path("/staking/validators").unwrap(),
            QueryPath::Validators
        );
        assert_eq!(
            query_path("/governance/proposal/12").unwrap(),
            QueryPath::Proposal(12)
        );
        let voter = "ab".repeat(32);
        assert_eq!(
            query_path(&format!("/governance/vote/3/{voter}")).unwrap(),
            QueryPath::Vote(3, &voter)
        );
        for bad in [
            "/governance/proposal/",
            "/governance/proposal/012",
            "/governance/proposal/+1",
            "/governance/proposal/18446744073709551616",
            "/governance/vote/3",
            "/governance/vote/03/{voter}",
            "/account/",
            "/account/DYTALLIX1ABC",
            "/account/ddytallix1abc/extra",
            "/staking/validators/",
        ] {
            assert!(query_path(&bad.replace("{voter}", &voter)).is_err(), "{bad}");
        }
        assert!(query_path(&format!("/governance/vote/3/{}", "AB".repeat(32))).is_err());
        assert!(query_path(&format!("/account/{}", "a".repeat(129))).is_err());
        let account = "cd".repeat(32);
        assert_eq!(
            query_path(&format!("/recovery/account/{account}")).unwrap(),
            QueryPath::RecoveryAccount(&account)
        );
        for bad in ["CD".repeat(32), "cd".repeat(31), format!("{account}/x"), String::new()] {
            assert!(query_path(&format!("/recovery/account/{bad}")).is_err(), "{bad}");
        }
    }

    /// The interface inventory (interfaces v1) lists exactly the query paths
    /// the application accepts, each under the variant it parses to.
    #[test]
    fn inventory_lists_exactly_the_accepted_query_paths() {
        let inventory: Value = serde_json::from_str(include_str!(
            "../../../../docs/architecture/interfaces-v1.json"
        ))
        .unwrap();
        let example = |path: &str| {
            path.replace("{account_id}", &"a".repeat(64))
                .replace("{transaction_id}", &"b".repeat(64))
                .replace("{receipt_sha256}", &"c".repeat(64))
                .replace("{key}", "6163637400")
                .replace("{address}", "ddytallix1abc")
                .replace("{proposal_id}", "7")
        };
        let mut listed = std::collections::BTreeSet::new();
        for entry in inventory["interfaces"].as_array().unwrap() {
            if entry["group"] != "abci_query" {
                continue;
            }
            let expected = entry["variant"].as_str().unwrap();
            assert!(listed.insert(expected), "{expected} listed twice");
            let aliases = entry["aliases"].as_array().cloned().unwrap_or_default();
            let paths = std::iter::once(entry["path"].clone()).chain(aliases);
            for path in paths {
                let path = example(path.as_str().unwrap());
                let parsed = query_path(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
                assert_eq!(variant(&parsed), expected, "{path}");
            }
        }
        assert_eq!(listed, VARIANTS.into_iter().collect());
    }
    #[test]
    fn check_tx_kind_requires_explicit_trusted_engine_classification() {
        assert_eq!(
            check_tx_kind(&json!({"type":"new"})).unwrap(),
            CheckTxKind::New
        );
        assert_eq!(
            check_tx_kind(&json!({"type":"recheck"})).unwrap(),
            CheckTxKind::Recheck
        );
        for invalid in [
            json!({}),
            json!({"type":null}),
            json!({"type":1}),
            json!({"type":"evict"}),
            json!({"type":"Recheck"}),
        ] {
            assert!(check_tx_kind(&invalid).is_err());
        }
    }
    #[test]
    fn transaction_transport_preserves_exact_bytes_and_rejects_noncanonical_base64() {
        let raw = b"canonical ordinary bytes are opaque to transport";
        let encoded = STANDARD.encode(raw);
        assert_eq!(canonical_base64(&encoded).unwrap(), raw);
        assert_eq!(
            transactions(&json!({"txs":[encoded]})).unwrap(),
            vec![raw.to_vec()]
        );
        for bad in ["AA", "AA=", "AB==", "AA==\n", "-w==", "!!!!"] {
            assert!(canonical_base64(bad).is_err(), "{bad}");
            assert!(transactions(&json!({"txs":[bad]})).is_err(), "{bad}");
        }
        assert!(transactions(&json!({"txs":[1]})).is_err());
        assert!(transactions(&json!({})).is_err());
    }
    #[test]
    fn ordinary_query_paths_require_exact_receipt_identity() {
        let id = "ab".repeat(32);
        let path = format!("/ordinary/receipt/{id}");
        assert_eq!(query_path(&path).unwrap(), QueryPath::OrdinaryReceipt(&id));
        let account_path = format!("/ordinary/account/{id}");
        assert_eq!(
            query_path(&account_path).unwrap(),
            QueryPath::OrdinaryAccount(&id)
        );
        for invalid in ["", "123", &"AB".repeat(32), &"gg".repeat(32)] {
            assert!(query_path(&format!("/ordinary/account/{invalid}")).is_err());
        }
        assert!(query_path(&format!("{account_path}/extra")).is_err());
        assert_eq!(
            query_path("/ordinary/profile").unwrap(),
            QueryPath::OrdinaryProfile
        );
        assert_eq!(
            query_path("/ordinary/profile_v3").unwrap(),
            QueryPath::GovernanceProfile
        );
        assert!(query_path("/ordinary/profile_v4").is_err());
        for path in ["", "/status", "/supply"] {
            assert_eq!(query_path(path).unwrap(), QueryPath::Status);
        }
        for bad in [
            "/ordinary/receipt/",
            "/ordinary/profile/",
            "/ordinary/receipt/123",
            "/ordinary/submit",
        ] {
            assert!(query_path(bad).is_err());
        }
        assert!(query_path(&format!("/ordinary/receipt/{}", "AB".repeat(32))).is_err());
        assert!(query_path(&format!("/ordinary/receipt/{}", "gg".repeat(32))).is_err());
        assert!(query_path(&format!("{path}/extra")).is_err());
        // State proofs take a lowercase, even-length hex key.
        assert_eq!(
            query_path("/state/proof/61636374").unwrap(),
            QueryPath::StateProof("61636374")
        );
        for bad in ["", "abc", "ABCD", "zz", &"a".repeat(1_026)] {
            assert!(query_path(&format!("/state/proof/{bad}")).is_err());
        }
    }
    #[test]
    fn emergency_receipt_query_requires_exact_digest() {
        let digest = "ab".repeat(32);
        let path = format!("/emergency/receipt/{digest}");
        assert_eq!(
            query_path(&path).unwrap(),
            QueryPath::EmergencyReceipt(&digest)
        );
        for value in [
            "".to_string(),
            "0".repeat(63),
            "AB".repeat(32),
            "gg".repeat(32),
            format!("{digest}/extra"),
            format!("{digest}?prove=true"),
        ] {
            assert!(query_path(&format!("/emergency/receipt/{value}")).is_err());
        }
    }
}
