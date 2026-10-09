//! Root control preparation and assembly (E05; P01, 6 October 2026).
//!
//! `dytallix-control prepare OPERATION --config CONFIG --status STATUS --out REQUEST
//!     [--window-blocks N] [operation inputs]`
//! `dytallix-control assemble --config CONFIG --status STATUS --request REQUEST --out CONTROL SIGNATURE...`
//!
//! OPERATION is freeze, resume, upgrade-admit, upgrade-activate,
//! upgrade-cancel, handover-admit, handover-activate, handover-cancel or
//! kit-replacement. CONFIG is the chain's application configuration and
//! STATUS the node's `/status` view (`dytallix control status`), which also
//! gives the root keys in force. Each document input is given
//! as a file, whose SHA-256 is used, or as its digest:
//! `--incident` / `--incident-sha256` (freeze, resume), `--readiness` /
//! `--readiness-sha256` (resume), `--authorization` /
//! `--authorization-sha256` (admissions), `--evidence` / `--evidence-sha256`
//! (activations). A handover admission also takes `--target-release-sha512`
//! and `--transition receipt-index-v1|schema-preserving`; a receipt index
//! handover activation takes `--upgrade-activation`, the assembled upgrade
//! activation control. A kit replacement takes `--seat N`, `--old-kit DIR`
//! and `--new-kit DIR`: the directories of the leaving and the new kit's
//! public key records (`kit-N-upgrade.json` and so on, as `dytallix-root-sign
//! kit` writes them).
//!
//! The request is signed offline (`dytallix-root-sign sign-control`) by
//! three keys, and a kit replacement also by each of the new kit's three
//! keys; `assemble` writes the control to dry-run and submit with
//! `dytallix control check` and `dytallix control submit`. Outputs are never
//! overwritten.
use anyhow::{bail, ensure, Context, Result};
use dytallix_fast_node::consensus_settlement::ConsensusConfig;
use dytallix_fast_node::control_request::{
    assemble, prepare, sha256_hex, Operation, Request, SignatureRecord, Status, TransitionChoice,
};
use dytallix_fast_node::emergency_freeze::AuthorityKey;
use dytallix_fast_node::kit_replacement::Roles;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAX_INPUT: u64 = 16 * 1024 * 1024;

fn read(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .with_context(|| format!("Cannot open {}", path.display()))?
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_INPUT,
        "{} exceeds {MAX_INPUT} bytes",
        path.display()
    );
    Ok(bytes)
}

fn create(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| {
            format!(
                "Cannot create {} (outputs are never overwritten)",
                path.display()
            )
        })?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn config(path: &Path) -> Result<ConsensusConfig> {
    let config: ConsensusConfig =
        serde_json::from_slice(&read(path)?).context("The application configuration")?;
    config.validate()?;
    Ok(config)
}

/// Options as `--name value`; names may not repeat.
fn options(args: &[String]) -> Result<(BTreeMap<String, String>, Vec<String>)> {
    let mut named = BTreeMap::new();
    let mut positional = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if let Some(name) = arg.strip_prefix("--") {
            let value = args
                .next()
                .with_context(|| format!("--{name} needs a value"))?;
            ensure!(
                named.insert(name.to_owned(), value.clone()).is_none(),
                "--{name} is repeated"
            );
        } else {
            positional.push(arg.clone());
        }
    }
    Ok((named, positional))
}

/// A kit's public key records for one seat: `DIR/kit-SEAT-PURPOSE.json`.
fn kit(dir: &str, seat: &str) -> Result<Roles<AuthorityKey>> {
    let key = |purpose: &str| -> Result<AuthorityKey> {
        let path = Path::new(dir).join(format!("kit-{seat}-{purpose}.json"));
        serde_json::from_slice(&read(&path)?)
            .with_context(|| format!("{} is not a public key record", path.display()))
    };
    Ok(Roles {
        upgrade: key("upgrade")?,
        freeze: key("freeze")?,
        resume: key("resume")?,
    })
}

fn status(path: &str) -> Result<Status> {
    serde_json::from_slice(&read(Path::new(path))?).context("The status view")
}

/// A document's SHA-256: from `--NAME FILE` or `--NAME-sha256 HEX`.
fn digest(named: &mut BTreeMap<String, String>, name: &str) -> Result<String> {
    match (named.remove(name), named.remove(&format!("{name}-sha256"))) {
        (Some(file), None) => Ok(sha256_hex(&read(Path::new(&file))?)),
        (None, Some(hex)) => Ok(hex),
        _ => bail!("Give exactly one of --{name} FILE or --{name}-sha256 HEX"),
    }
}

fn run() -> Result<serde_json::Value> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args
        .first()
        .context("A command is required: prepare or assemble")?;
    match command.as_str() {
        "prepare" => {
            let operation = args.get(1).context("prepare needs an operation")?.clone();
            let (mut named, positional) = options(&args[2..])?;
            ensure!(
                positional.is_empty(),
                "Unexpected argument {:?}",
                positional
            );
            let config = config(Path::new(
                &named.remove("config").context("--config is required")?,
            ))?;
            let status = status(&named.remove("status").context("--status is required")?)?;
            let out = PathBuf::from(named.remove("out").context("--out is required")?);
            let blocks = named
                .remove("window-blocks")
                .map(|v| v.parse::<u64>().context("--window-blocks is a block count"))
                .transpose()?;
            let operation = match operation.as_str() {
                "freeze" => Operation::Freeze {
                    incident_sha256: digest(&mut named, "incident")?,
                },
                "resume" => Operation::Resume {
                    incident_sha256: digest(&mut named, "incident")?,
                    readiness_evidence_sha256: digest(&mut named, "readiness")?,
                },
                "upgrade-admit" => Operation::UpgradeAdmit {
                    authorization_sha256: digest(&mut named, "authorization")?,
                },
                "upgrade-activate" => Operation::UpgradeActivate {
                    evidence_sha256: digest(&mut named, "evidence")?,
                },
                "upgrade-cancel" => Operation::UpgradeCancel,
                "handover-admit" => Operation::HandoverAdmit {
                    target_release_sha512: named
                        .remove("target-release-sha512")
                        .context("--target-release-sha512 is required")?,
                    transition: match named
                        .remove("transition")
                        .context("--transition is required")?
                        .as_str()
                    {
                        "receipt-index-v1" => TransitionChoice::ReceiptIndexV1,
                        "schema-preserving" => TransitionChoice::SchemaPreserving,
                        other => bail!("Unknown transition {other}"),
                    },
                    authorization_sha256: digest(&mut named, "authorization")?,
                },
                "handover-activate" => Operation::HandoverActivate {
                    evidence_sha256: digest(&mut named, "evidence")?,
                    upgrade_activation_sha256: named
                        .remove("upgrade-activation")
                        .map(|file| read(Path::new(&file)).map(|b| sha256_hex(&b)))
                        .transpose()?,
                },
                "handover-cancel" => Operation::HandoverCancel,
                "kit-replacement" => {
                    let seat = named.remove("seat").context("--seat is required")?;
                    ensure!(
                        matches!(seat.as_str(), "1" | "2" | "3" | "4" | "5"),
                        "--seat is 1 to 5"
                    );
                    let old = kit(
                        &named.remove("old-kit").context("--old-kit is required")?,
                        &seat,
                    )?;
                    let new = kit(
                        &named.remove("new-kit").context("--new-kit is required")?,
                        &seat,
                    )?;
                    Operation::KitReplacement {
                        replaced: Roles {
                            upgrade: old.upgrade.key_id,
                            freeze: old.freeze.key_id,
                            resume: old.resume.key_id,
                        },
                        keys: new,
                    }
                }
                other => bail!("Unknown operation {other}"),
            };
            ensure!(
                named.is_empty(),
                "Unexpected options: {:?}",
                named.keys().collect::<Vec<_>>()
            );
            let request = prepare(&config, &status, &operation, blocks)?;
            let mut text = serde_json::to_vec_pretty(&request)?;
            text.push(b'\n');
            create(&out, &text)?;
            let signers = if matches!(operation, Operation::KitReplacement { .. }) {
                format!(
                    "{} of the {} current upgrade keys, and each of the 3 new keys",
                    request.authority.threshold,
                    request.authority.keys.len() - 3
                )
            } else {
                format!(
                    "{} of the {} {} keys",
                    request.authority.threshold,
                    request.authority.keys.len(),
                    request.authority.purpose
                )
            };
            Ok(json!({
                "status": "PREPARED", "operation": request.operation, "kind": request.kind,
                "chain_id": request.envelope.chain_id, "sequence": request.envelope.sequence,
                "anchor_height": request.anchor_height,
                "not_before_height": request.envelope.not_before_height,
                "not_after_height": request.envelope.not_after_height,
                "artifact_sha512": request.envelope.artifact_sha512,
                "signers": signers,
            }))
        }
        "assemble" => {
            let (mut named, positional) = options(&args[1..])?;
            let config = config(Path::new(
                &named.remove("config").context("--config is required")?,
            ))?;
            let status = status(&named.remove("status").context("--status is required")?)?;
            let request: Request = serde_json::from_slice(&read(Path::new(
                &named.remove("request").context("--request is required")?,
            ))?)
            .context("The control request")?;
            let out = PathBuf::from(named.remove("out").context("--out is required")?);
            ensure!(
                named.is_empty(),
                "Unexpected options: {:?}",
                named.keys().collect::<Vec<_>>()
            );
            ensure!(!positional.is_empty(), "Give the signature files");
            let signatures = positional
                .iter()
                .map(|path| {
                    serde_json::from_slice::<SignatureRecord>(&read(Path::new(path))?)
                        .with_context(|| format!("Signature {path}"))
                })
                .collect::<Result<Vec<_>>>()?;
            let control = assemble(&config, &status, &request, &signatures)?;
            create(&out, &control)?;
            Ok(json!({
                "status": "ASSEMBLED", "operation": request.operation, "kind": request.kind,
                "sequence": request.envelope.sequence, "signatures": signatures.len(),
                "bytes": control.len(), "control_sha256": sha256_hex(&control),
            }))
        }
        other => bail!("Unknown command {other}; use prepare or assemble"),
    }
}

fn main() {
    match run() {
        Ok(result) => println!("{}", serde_json::to_string_pretty(&result).expect("json")),
        Err(error) => {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
    }
}
