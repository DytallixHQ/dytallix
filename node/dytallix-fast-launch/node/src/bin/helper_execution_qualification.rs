//! Local qualification sink. The external caller must bound this process's
//! output and elapsed time. Helper acceptance itself does not write to a sink.
use anyhow::{ensure, Context, Result};
use dytallix_fast_node::root_genesis::{DevelopmentRootGenesis, HelperQualificationStatus};
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

#[cfg(target_os = "linux")]
#[path = "helper_guard_cases/mod.rs"]
mod guard_cases;

fn run() -> Result<u8> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!((args.len()==4 || args.len()==6) && args[0]=="--root-config" && args[2]=="--max-output-bytes","Usage: helper-execution-qualification --root-config /absolute/config.json --max-output-bytes N [--case observed|missing-owner|wrong-role|no-go|cancel|owner-death]");
    let bound: usize = args[3]
        .to_str()
        .context("Output bound must be UTF-8")?
        .parse()?;
    ensure!(bound > 0, "Output bound must be positive");
    let case = if args.len() == 6 {
        ensure!(args[4] == "--case", "Expected --case");
        args[5].to_str().context("Case must be UTF-8")?
    } else {
        "observed"
    };
    dytallix_release_runtime::ownership::install_cancellation()?;
    let root = DevelopmentRootGenesis::from_development_config(Path::new(&args[1]))?;
    if case != "observed" {
        #[cfg(target_os = "linux")]
        return guard_cases::run(&root, case, bound);
        #[cfg(not(target_os = "linux"))]
        anyhow::bail!("Helper guard qualification requires Linux");
    }
    let result = root.execute_helper_for_qualification()?;
    let (status, exit_code) = match result.status {
        HelperQualificationStatus::HelperVerified => ("HELPER_VERIFIED", 0),
        HelperQualificationStatus::HelperRejected => ("HELPER_REJECTED", 2),
    };
    let evidence: serde_json::Value = serde_json::from_slice(result.evidence.bytes())?;
    let mut output = serde_json::to_vec(
        &serde_json::json!({"scope":"LOCAL_HELPER_QUALIFICATION","status":status,"evidence":evidence,"genesis_or_control_accepted":false,"production_qualified":false}),
    )?;
    ensure!(
        output
            .len()
            .checked_add(1)
            .is_some_and(|size| size <= bound),
        "Qualification output exceeds explicit sink bound"
    );
    output.push(b'\n');
    // This optional runner owns its sink. No acceptance or consensus path calls it.
    std::io::stdout().lock().write_all(&output)?;
    Ok(exit_code)
}
fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            let _ = writeln!(
                std::io::stderr().lock(),
                "Helper qualification failed: {error:#}"
            );
            ExitCode::from(1)
        }
    }
}
