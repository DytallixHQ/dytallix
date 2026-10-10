//! Operator check of a stopped node's database (E04 gap 15): the
//! application's startup checks, read only, with the first failure in full.
//! Stop the node first; the tool opens no write handle.
//!
//! `dytallix-state-check --config APPLICATION_CONFIG --genesis NATIVE_GENESIS --db APPDB`
//!
//! Prints one JSON object. The exit status is 0 when every check passes or
//! the database holds no consensus state, the failure class's status (10 to
//! 17) when a check fails, and 1 for an unclassified failure.
//!
//! After a halt, `--restart-target RELEASE_SHA512 --evidence SHA256
//! [--halted-block-hash HASH] --restart-output DIR` also writes the unsigned
//! restart authorization for the committed checkpoint (restart v1, E04 gap
//! 18): `restart-unsigned.json`, `restart-artifact.bin`, the exact bytes the
//! handover custodians sign, and `restart-request.json`, the signing request
//! `dytallix-root-sign sign-control` takes (`dytallix.control-request.v1`).
//!
//! `--restart-assemble REQUEST --restart-signatures DIR --restart-output DIR`
//! assembles the signed authorization from the request and the signature
//! files in DIR (every `*.json`): `restart-authorization.json`, the file the
//! service configuration pins. Only keys of the authority in force on this
//! node count.
use anyhow::{bail, ensure, Context, Result};
use dytallix_fast_node::consensus_settlement::{
    check_stopped, restart_inputs, restart_policy, ConsensusConfig, StoppedCheck,
};
use dytallix_fast_node::control_request::{self, Request, SignatureRecord};
use dytallix_fast_node::failure_class::{class_of, classify, describe, FailureClass};
use dytallix_fast_node::release_handover::restart;
use serde_json::{json, Value};
use sha2::{Digest, Sha256, Sha512};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAX_CONFIG: u64 = dytallix_fast_node::consensus_settlement::MAX_CONFIG_BYTES as u64;
const MAX_GENESIS: u64 = dytallix_fast_node::consensus_settlement::MAX_GENESIS_BYTES as u64;
const MAX_REQUEST: u64 = 256 * 1024;
const MAX_SIGNATURE: u64 = 128 * 1024;

/// The checks that need the owned root helper, which this tool cannot run.
const NOT_CHECKED: [&str; 2] = [
    "root_receipt_authorization",
    "emergency_upgrade_handover_replay",
];

fn read(path: &PathBuf, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .with_context(|| format!("Cannot open {}", path.display()))?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= limit, "{} exceeds {limit} bytes", path.display());
    Ok(bytes)
}

fn lower_hex(value: &str, bytes: usize, name: &str) -> Result<()> {
    ensure!(
        value.len() == bytes * 2
            && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "{name} must be {bytes} bytes of lowercase hexadecimal"
    );
    Ok(())
}
/// Create a file that must not exist yet.
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("Cannot create {}", path.display()))?
        .write_all(bytes)?;
    Ok(())
}

struct Restart {
    target: String,
    evidence: String,
    block: Option<String>,
    output: PathBuf,
}
fn write_restart(db: &Path, config: &ConsensusConfig, genesis: &[u8], r: &Restart) -> Result<Value> {
    lower_hex(&r.target, 64, "--restart-target")?;
    lower_hex(&r.evidence, 32, "--evidence")?;
    if let Some(block) = &r.block {
        lower_hex(block, 32, "--halted-block-hash")?;
    }
    let (payload, policy) = restart_inputs(db, config, genesis, &r.target, r.block.clone(), &r.evidence)?;
    let request = control_request::restart_request(&policy, &payload)?;
    let artifact = restart::artifact_bytes(&payload)?;
    let (sequence, height) = (payload.sequence, payload.halted_height);
    let unsigned = restart::Authorization {
        kind: restart::KIND.into(),
        payload,
        signatures: Vec::new(),
    };
    let (unsigned_path, artifact_path, request_path) = (
        r.output.join("restart-unsigned.json"),
        r.output.join("restart-artifact.bin"),
        r.output.join("restart-request.json"),
    );
    write_new(&unsigned_path, &serde_json::to_vec_pretty(&unsigned)?)?;
    write_new(&artifact_path, &artifact)?;
    write_new(&request_path, &serde_json::to_vec_pretty(&request)?)?;
    Ok(json!({"sequence":sequence,"halted_height":height,
        "artifact_sha512":hex::encode(Sha512::digest(&artifact)),
        "unsigned":unsigned_path,"artifact":artifact_path,"request":request_path}))
}

struct Assemble {
    request: PathBuf,
    signatures: PathBuf,
    output: PathBuf,
}
/// The signed restart authorization, from the request and every signature
/// file (`*.json`) in the directory.
fn assemble_restart(db: &Path, config: &ConsensusConfig, genesis: &[u8], a: &Assemble) -> Result<Value> {
    let request: Request = serde_json::from_slice(&read(&a.request, MAX_REQUEST)?)
        .context("Invalid restart signing request")?;
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&a.signatures)
        .with_context(|| format!("Cannot read {}", a.signatures.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<_>>()?;
    paths.retain(|path| path.extension().is_some_and(|ext| ext == "json"));
    paths.sort();
    let signatures = paths
        .iter()
        .map(|path| {
            serde_json::from_slice::<SignatureRecord>(&read(path, MAX_SIGNATURE)?)
                .with_context(|| format!("{} is not a signature file", path.display()))
        })
        .collect::<Result<Vec<_>>>()?;
    let policy = restart_policy(db, config, genesis)?;
    let bytes = control_request::assemble_restart(&policy, &request, &signatures)?;
    let path = a.output.join("restart-authorization.json");
    write_new(&path, &bytes)?;
    Ok(json!({"sequence":request.envelope.sequence,"halted_height":request.envelope.not_before_height,
        "signatures":signatures.len(),"authorization":path,
        "sha256":hex::encode(Sha256::digest(&bytes)),"bytes":bytes.len()}))
}

fn run() -> Result<(StoppedCheck, Option<Value>)> {
    let mut args = std::env::args_os().skip(1);
    let (mut config, mut genesis, mut db) = (None, None, None);
    let (mut target, mut evidence, mut block, mut output) = (None, None, None, None);
    let (mut assemble, mut signatures) = (None, None);
    while let Some(flag) = args.next() {
        let value = args.next().context("Each argument requires a value")?;
        let slot: &mut Option<PathBuf> = match flag.to_str() {
            Some("--config") => &mut config,
            Some("--genesis") => &mut genesis,
            Some("--db") => &mut db,
            Some("--restart-output") => &mut output,
            Some("--restart-target") => &mut target,
            Some("--evidence") => &mut evidence,
            Some("--halted-block-hash") => &mut block,
            Some("--restart-assemble") => &mut assemble,
            Some("--restart-signatures") => &mut signatures,
            _ => bail!("usage: dytallix-state-check --config FILE --genesis FILE --db DIR \
                [--restart-target SHA512 --evidence SHA256 [--halted-block-hash HASH] --restart-output DIR \
                | --restart-assemble REQUEST --restart-signatures DIR --restart-output DIR]"),
        };
        ensure!(slot.replace(PathBuf::from(value)).is_none(), "Duplicate argument");
    }
    let (config, genesis) = classify(
        (|| {
            let bytes = read(&config.context("--config is required")?, MAX_CONFIG)?;
            let config: ConsensusConfig =
                serde_json::from_slice(&bytes).context("Invalid application configuration")?;
            Ok((config, read(&genesis.context("--genesis is required")?, MAX_GENESIS)?))
        })(),
        FailureClass::Configuration,
    )?;
    let db = db.context("--db is required")?;
    let check = check_stopped(&db, &config, &genesis)?;
    let text = |value: Option<PathBuf>| value.map(|v| v.to_string_lossy().into_owned());
    let restart = match (text(target), text(evidence), text(block), output, assemble, signatures) {
        (None, None, None, None, None, None) => None,
        (Some(target), Some(evidence), block, Some(output), None, None) => {
            Some(write_restart(&db, &config, &genesis, &Restart { target, evidence, block, output })?)
        }
        (None, None, None, Some(output), Some(request), Some(signatures)) => Some(assemble_restart(
            &db,
            &config,
            &genesis,
            &Assemble { request, signatures, output },
        )?),
        _ => bail!("--restart-target needs --evidence and --restart-output; \
            --restart-assemble needs --restart-signatures and --restart-output"),
    };
    Ok((check, restart))
}

fn report(outcome: Result<(StoppedCheck, Option<Value>)>) -> (Value, u8) {
    match outcome {
        Ok((StoppedCheck::Empty, _)) => (json!({"schema":1,"result":"empty"}), 0),
        Ok((StoppedCheck::Passed(info), restart)) => {
            let mut value = json!({"schema":1,"result":"pass","height":info.height,
                "app_hash":info.app_hash,"not_checked":NOT_CHECKED});
            if let Some(restart) = restart {
                value["restart"] = restart;
            }
            (value, 0)
        }
        Err(error) => {
            let class = class_of(&error);
            (
                json!({"schema":1,"result":"fail","class":class.map(FailureClass::name),
                    "error":describe(&error),"not_checked":NOT_CHECKED}),
                class.map_or(1, FailureClass::exit_status),
            )
        }
    }
}

fn main() -> std::process::ExitCode {
    let (report, status) = report(run());
    println!("{report}");
    std::process::ExitCode::from(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_reports_its_class_and_full_text() {
        let error = classify::<(StoppedCheck, Option<Value>)>(
            Err(anyhow::anyhow!("Account totals differ from the account records")),
            FailureClass::Supply,
        );
        let (value, status) = report(error.context("History check"));
        assert_eq!(status, 12);
        assert_eq!(value["class"], "supply");
        assert_eq!(
            value["error"],
            "History check: Account totals differ from the account records"
        );
        let (value, status) = report(Err(anyhow::anyhow!("usage")));
        assert_eq!((status, value["class"].clone()), (1, Value::Null));
    }
    #[test]
    fn restart_inputs_are_lowercase_hex_and_outputs_are_new_files() {
        lower_hex(&"ab".repeat(64), 64, "target").unwrap();
        for bad in ["AB".repeat(64), "ab".repeat(63), "zz".repeat(64)] {
            assert!(lower_hex(&bad, 64, "target").is_err());
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("restart-artifact.bin");
        write_new(&path, b"first").unwrap();
        assert!(write_new(&path, b"second").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
    }
}
