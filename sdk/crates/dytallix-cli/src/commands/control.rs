//! `dytallix control` (E05; P01, 6 October 2026): the online half of root
//! control signing over the pinned chain (node/docs/mainnet/control-signing.md
//! in the node repository). `status` saves the node's status view that
//! `dytallix-control prepare` reads; `check` dry-runs an assembled control;
//! `submit` broadcasts it. The custodians sign offline in between.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use anyhow::{bail, ensure, Context, Result};
use clap::{Args, Subcommand};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::bytes_to_hex;
use super::consensus::ChainConfig;
use super::ordinary::print_json;

/// The control kinds the node admits as transactions.
const KINDS: [&str; 3] = [
    "dytallix-emergency-control-v2",
    "dytallix-upgrade-control-v2",
    "dytallix-release-handover-v2",
];
/// A control with three signatures is about 180 KB; the node's transaction
/// bound is lower than this.
const MAX_CONTROL_BYTES: u64 = 1_048_576;

#[derive(Debug, Clone, Args)]
pub struct ControlArgs {
    #[command(subcommand)]
    command: ControlCommand,
}

#[derive(Debug, Clone, Subcommand)]
enum ControlCommand {
    /// Save the pinned node's status view, to prepare a control from.
    Status {
        /// New file for the status view (never overwritten).
        #[arg(long)]
        out: PathBuf,
        /// A loopback URL or endpoint pin file instead of the pinned node.
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Dry-run an assembled control against the pinned node (CheckTx).
    Check {
        /// The control file `dytallix-control assemble` wrote.
        control: PathBuf,
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Submit an assembled control to the pinned node.
    Submit {
        control: PathBuf,
        #[arg(long)]
        endpoint: Option<String>,
    },
}

/// Reads a control and refuses one that is not a root control for the
/// pinned chain. The node checks everything else.
fn read_control(path: &PathBuf, chain_id: &str) -> Result<Vec<u8>> {
    let size = fs::metadata(path)
        .with_context(|| format!("cannot read {}", path.display()))?
        .len();
    ensure!(
        size <= MAX_CONTROL_BYTES,
        "{} is too large for a control",
        path.display()
    );
    let bytes = fs::read(path)?;
    let control: Value = serde_json::from_slice(&bytes).context("the control is not JSON")?;
    let kind = control["kind"].as_str().unwrap_or_default();
    ensure!(
        KINDS.contains(&kind),
        "{} is not a root control",
        path.display()
    );
    ensure!(
        control["payload"]["chain_id"].as_str() == Some(chain_id),
        "the control is for chain {}, not the pinned chain {chain_id}",
        control["payload"]["chain_id"]
    );
    Ok(bytes)
}

pub async fn run(args: ControlArgs) -> Result<()> {
    let pin = ChainConfig::load()?;
    match args.command {
        ControlCommand::Status { out, endpoint } => {
            let client = pin.client(endpoint.as_deref())?;
            let (height, view) = client.query_status().await?;
            if view["height"].as_u64() != Some(height) {
                bail!("the status view's height differs from the query height");
            }
            let mut text = serde_json::to_vec_pretty(&view)?;
            text.push(b'\n');
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&out)
                .with_context(|| format!("cannot create {} (never overwritten)", out.display()))?;
            file.write_all(&text)?;
            print_json(&json!({
                "status": "SAVED", "chain_id": pin.chain_id, "height": height,
                "app_hash": view["app_hash"], "path": out.display().to_string(),
                "frozen": view["emergency_control"]["frozen"],
            }))
        }
        ControlCommand::Check { control, endpoint } => {
            let bytes = read_control(&control, &pin.chain_id)?;
            let digest = bytes_to_hex(&Sha256::digest(&bytes));
            let response = pin
                .client(endpoint.as_deref())?
                .check_control(bytes)
                .await?;
            print_json(&json!({
                "status": if response.admitted() { "WOULD_BE_ADMITTED" } else { "REFUSED" },
                "control_sha256": digest, "code": response.code, "log": response.log,
            }))
        }
        ControlCommand::Submit { control, endpoint } => {
            let bytes = read_control(&control, &pin.chain_id)?;
            let digest = bytes_to_hex(&Sha256::digest(&bytes));
            let response = pin
                .client(endpoint.as_deref())?
                .submit_control_sync(bytes)
                .await?;
            print_json(&json!({
                "status": if response.admitted() { "SUBMITTED" } else { "REFUSED" },
                "control_sha256": digest, "code": response.code, "log": response.log,
                "engine_hash": response.engine_hash,
                "note": "Admission to the mempool only; check `dytallix control status` for the commit.",
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &std::path::Path, name: &str, value: Value) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        path
    }

    #[test]
    fn only_a_root_control_for_the_pinned_chain_is_sent() {
        let dir = std::env::temp_dir().join(format!("dytallix-cli-control-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let control = json!({"kind": "dytallix-emergency-control-v2",
            "payload": {"chain_id": "dytallix-mainnet-1"}, "signatures": []});
        let path = write(&dir, "control.json", control.clone());
        assert!(read_control(&path, "dytallix-mainnet-1").is_ok());
        assert!(read_control(&path, "dytallix-staging-1").is_err());
        let mut other = control.clone();
        other["kind"] = json!("ordinary-v2");
        let path = write(&dir, "other.json", other);
        assert!(read_control(&path, "dytallix-mainnet-1").is_err());
        let path = dir.join("text.json");
        fs::write(&path, b"not json").unwrap();
        assert!(read_control(&path, "dytallix-mainnet-1").is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
