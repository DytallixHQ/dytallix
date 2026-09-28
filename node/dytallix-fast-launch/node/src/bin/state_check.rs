//! Operator check of a stopped node's database (E04 gap 15): the
//! application's startup checks, read only, with the first failure in full.
//! Stop the node first; the tool opens no write handle.
//!
//! `dytallix-state-check --config APPLICATION_CONFIG --genesis NATIVE_GENESIS --db APPDB`
//!
//! Prints one JSON object. The exit status is 0 when every check passes or
//! the database holds no consensus state, the failure class's status (10 to
//! 17) when a check fails, and 1 for an unclassified failure.
use anyhow::{bail, ensure, Context, Result};
use dytallix_fast_node::consensus_settlement::{check_stopped, ConsensusConfig, StoppedCheck};
use dytallix_fast_node::failure_class::{class_of, classify, describe, FailureClass};
use serde_json::{json, Value};
use std::io::Read;
use std::path::PathBuf;

const MAX_CONFIG: u64 = 65_536;
const MAX_GENESIS: u64 = 8 * 1024 * 1024;

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

fn run() -> Result<StoppedCheck> {
    let mut args = std::env::args_os().skip(1);
    let (mut config, mut genesis, mut db) = (None, None, None);
    while let Some(flag) = args.next() {
        let slot = match flag.to_str() {
            Some("--config") => &mut config,
            Some("--genesis") => &mut genesis,
            Some("--db") => &mut db,
            _ => bail!("usage: dytallix-state-check --config FILE --genesis FILE --db DIR"),
        };
        let value = args.next().context("Each argument requires a value")?;
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
    check_stopped(&db.context("--db is required")?, &config, &genesis)
}

fn report(outcome: Result<StoppedCheck>) -> (Value, u8) {
    match outcome {
        Ok(StoppedCheck::Empty) => (json!({"schema":1,"result":"empty"}), 0),
        Ok(StoppedCheck::Passed(info)) => (
            json!({"schema":1,"result":"pass","height":info.height,
                "app_hash":info.app_hash,"not_checked":NOT_CHECKED}),
            0,
        ),
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
        let error = classify::<StoppedCheck>(
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
}
