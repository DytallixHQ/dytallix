//! Deterministic genesis builder (E05-d), rehearsal only.
//!
//! `dytallix-genesis-build --inputs GENESIS_INPUTS.json --out DIR [--verify]`
//!
//! Writes `native-genesis.json`, `application-config.json`, `genesis.json`
//! (the engine genesis) and `BUILD_MANIFEST.json` into DIR, which must not
//! exist. The same inputs always give the same bytes. `--verify` also starts
//! a chain from the build on a temporary database and prints its genesis
//! application hash. The output is never a production genesis.
use anyhow::{bail, ensure, Context, Result};
use dytallix_fast_node::genesis_build::{build, read_inputs, verify};
use serde_json::json;
use std::io::Read;
use std::path::PathBuf;

const MAX_INPUTS: u64 = 8 * 1024 * 1024;

fn run() -> Result<serde_json::Value> {
    let mut inputs = None;
    let mut out = None;
    let mut check = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--inputs" if inputs.is_none() => {
                inputs = Some(PathBuf::from(args.next().context("--inputs needs a path")?))
            }
            "--out" if out.is_none() => {
                out = Some(PathBuf::from(args.next().context("--out needs a path")?))
            }
            "--verify" if !check => check = true,
            other => bail!("Unexpected or repeated argument {other}"),
        }
    }
    let inputs = inputs.context("--inputs is required")?;
    let out = out.context("--out is required")?;
    let mut bytes = Vec::new();
    std::fs::File::open(&inputs)
        .with_context(|| format!("Cannot open {}", inputs.display()))?
        .take(MAX_INPUTS + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_INPUTS,
        "Inputs exceed {MAX_INPUTS} bytes"
    );
    let built = build(&read_inputs(&bytes)?, &bytes)?;
    let app_hash = if check {
        let db =
            std::env::temp_dir().join(format!("dytallix-genesis-verify-{}", uuid::Uuid::new_v4()));
        let result = verify(&built, &db);
        let _ = std::fs::remove_dir_all(&db);
        Some(result?)
    } else {
        None
    };
    ensure!(!out.exists(), "{} already exists", out.display());
    std::fs::create_dir_all(&out)?;
    for (name, file) in [
        ("native-genesis.json", &built.native_genesis),
        ("application-config.json", &built.application_config),
        ("genesis.json", &built.engine_genesis),
        ("BUILD_MANIFEST.json", &built.manifest),
    ] {
        std::fs::write(out.join(name), file)?;
    }
    let manifest: serde_json::Value = serde_json::from_slice(&built.manifest)?;
    Ok(json!({
        "status": "BUILT",
        "production": false,
        "build_digest": manifest["build_digest"],
        "verified_app_hash": app_hash,
    }))
}

fn main() {
    match run() {
        Ok(result) => println!("{}", serde_json::to_string_pretty(&result).expect("json")),
        Err(error) => {
            eprintln!("{error:#}");
            std::process::exit(2);
        }
    }
}
