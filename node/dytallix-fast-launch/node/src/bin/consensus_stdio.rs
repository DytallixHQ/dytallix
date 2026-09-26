//! Private pipe protocol used only by the pinned local CometBFT bridge.
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use dytallix_fast_node::consensus_settlement::{
    ConsensusApplication, ConsensusConfig, FinalizedBlockInput, ValidatorConfig,
};
use dytallix_fast_node::runtime::penalty_custody::EvidenceFact;
use dytallix_fast_node::runtime::validator_lifecycle::LifecycleConfig;
use dytallix_release_runtime::ownership::{self, Role};
use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};
use std::os::fd::AsRawFd;

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
    OrdinaryReceipt(&'a str),
    OrdinaryAccount(&'a str),
    EmergencyReceipt(&'a str),
}
fn query_path(path: &str) -> Result<QueryPath<'_>> {
    match path {
        "" | "/status" | "/supply" => Ok(QueryPath::Status),
        "/ordinary/profile" => Ok(QueryPath::OrdinaryProfile),
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
            Ok(json!({}))
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
                QueryPath::OrdinaryReceipt(id) => QueryRequest::OrdinaryReceipt(id),
                QueryPath::OrdinaryAccount(id) => QueryRequest::OrdinaryAccount(id),
                QueryPath::EmergencyReceipt(id) => QueryRequest::EmergencyReceipt(id),
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
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(_) => std::process::ExitCode::FAILURE,
    }
}

fn run() -> Result<()> {
    ownership::install_cancellation()?;
    let admission = ownership::admit(Role::Application)?;
    let mut args = std::env::args().skip(1);
    let mut config_path = None;
    let mut genesis_path = None;
    let mut db_path = None;
    let mut development_root_path = None;
    let mut emergency_verifier_path = None;
    let mut candidate_path = None;
    let mut release_manifest_sha512 = None;
    while let Some(arg) = args.next() {
        let value = args.next().context("Each argument requires a value")?;
        let slot = match arg.as_str() {
            "--release-manifest-sha512" => &mut release_manifest_sha512,
            "--config" => &mut config_path,
            "--genesis" => &mut genesis_path,
            "--db" => &mut db_path,
            "--development-root-config" => &mut development_root_path,
            "--development-emergency-verifier-config" => &mut emergency_verifier_path,
            "--development-candidate-config" => &mut candidate_path,
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
    ensure!(config_bytes.len() <= 65536, "Configuration exceeds limit");
    let config: ConsensusConfig = serde_json::from_slice(&config_bytes)?;
    let genesis = std::fs::read(genesis_path.context("--genesis is required")?)?;
    ensure!(
        genesis.len() <= MAX_FRAME as usize,
        "Genesis exceeds local fixture limit"
    );
    let database = db_path.context("--db is required")?;
    ensure!(
        candidate_path.is_none()
            || (development_root_path.is_some() && emergency_verifier_path.is_some()),
        "Candidate verification requires root and emergency configuration"
    );
    let mut app = match (development_root_path, emergency_verifier_path) {
        (Some(root_path), Some(verifier_path)) => {
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
            ConsensusApplication::open_with_development_runtime_candidate(
                std::path::Path::new(&database),
                config.clone(),
                genesis,
                &config_bytes,
                authorization,
                serde_json::from_slice(&bytes)?,
                candidate,
            )?
        }
        (Some(path), None) => {
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
        (None, Some(_)) => bail!("Emergency verifier requires development root configuration"),
        (None, None) => {
            ConsensusApplication::open(std::path::Path::new(&database), config.clone(), genesis)?
        }
    };
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
        let response = match serde_json::from_slice(&line)
            .map_err(anyhow::Error::from)
            .and_then(|request| handle(&mut app, &config, request))
        {
            Ok(result) => json!({"ok":true,"result":result}),
            Err(error) => json!({"ok":false,"error":error.to_string()}),
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
