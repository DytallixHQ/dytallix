//! Linux owned-process qualification of the native supervisor (feature
//! `qualification-fixtures`). Child ownership requires the process leader
//! thread, and libtest runs every test on a worker thread, so this runner has
//! no libtest harness: it runs each case on the main thread of a fresh copy of
//! itself. A case fails by panicking, by exiting otherwise, or by outliving
//! `CASE_LIMIT`. Arguments filter case names by substring (`--exact` for whole
//! names), as libtest's do; other flags are ignored.
//!
//! Run it with `scripts/run_native_process_tests.py`, which builds the fixture
//! and applies the security state each owned child must show.

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("native process qualification runs only on Linux; no cases run");
}

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    use dytallix_native_supervisor::processes::qualification::CASES;
    use std::process::{Command, ExitCode};
    use std::time::{Duration, Instant};
    const CASE_FLAG: &str = "--qualification-case";
    const CASE_LIMIT: Duration = Duration::from_secs(120);
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [flag, name] = args.as_slice() {
        if flag == CASE_FLAG {
            let Some((_, case)) = CASES.iter().find(|(case, _)| case == name) else {
                eprintln!("unknown case {name}");
                return ExitCode::FAILURE;
            };
            case();
            return ExitCode::SUCCESS;
        }
    }
    let exact = args.iter().any(|a| a == "--exact");
    let filters: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    let selected: Vec<&str> = CASES.iter().map(|(name, _)| *name)
        .filter(|name| filters.is_empty() || filters.iter().any(|f| if exact { name == f } else { name.contains(f.as_str()) }))
        .collect();
    let started = Instant::now();
    println!("\nrunning {} tests", selected.len());
    let mut failed = Vec::new();
    for name in &selected {
        println!("test processes::qualification::{name} ...");
        let case_started = Instant::now();
        let outcome = (|| -> std::io::Result<Option<std::process::ExitStatus>> {
            let mut child = Command::new(std::env::current_exe()?).args([CASE_FLAG, name]).spawn()?;
            loop {
                if let Some(status) = child.try_wait()? { return Ok(Some(status)); }
                if case_started.elapsed() > CASE_LIMIT { child.kill()?; child.wait()?; return Ok(None); }
                std::thread::sleep(Duration::from_millis(20));
            }
        })();
        let result = match outcome {
            Ok(Some(status)) if status.success() => "ok".to_owned(),
            Ok(Some(status)) => format!("FAILED ({status})"),
            Ok(None) => format!("FAILED (exceeded {}s)", CASE_LIMIT.as_secs()),
            Err(error) => format!("FAILED ({error})"),
        };
        if result != "ok" { failed.push(*name); }
        println!("test processes::qualification::{name} ... {result} ({:.2}s)", case_started.elapsed().as_secs_f64());
    }
    if !failed.is_empty() {
        println!("\nfailures:");
        for name in &failed { println!("    processes::qualification::{name}"); }
    }
    println!("\ntest result: {}. {} passed; {} failed; 0 ignored; 0 measured; {} filtered out; finished in {:.2}s\n",
        if failed.is_empty() { "ok" } else { "FAILED" }, selected.len() - failed.len(), failed.len(),
        CASES.len() - selected.len(), started.elapsed().as_secs_f64());
    if failed.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
