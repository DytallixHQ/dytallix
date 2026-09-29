//! Creates a node's client channel key and its endpoint pin (E04 gap 19,
//! C-b; `docs/architecture/client-channel-v1.md`).
//!
//! ```text
//! dytallix-channel-key generate --seed-file HOME/config/client_channel_seed.bin
//! dytallix-channel-key pin --seed-file FILE --network CHAIN_ID --address HOST:PORT --output FILE
//! ```
//!
//! `generate` writes a new owner-only 32-byte seed and never replaces a
//! file. `pin` writes the public pin that clients and the supervisor read.
//! Neither prints or copies the seed.

use dytallix_client_channel::{EndpointPin, Identity};
use dytallix_pqc_http_adapter::channel::read_seed;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: dytallix-channel-key generate --seed-file FILE\n       dytallix-channel-key pin --seed-file FILE --network CHAIN_ID --address HOST:PORT --output FILE";

fn create(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn run(args: Vec<String>) -> Result<serde_json::Value, String> {
    let mut args = args.into_iter();
    let command = args.next().ok_or(USAGE)?;
    let mut flags = std::collections::BTreeMap::new();
    while let Some(flag) = args.next() {
        let known = ["--seed-file", "--network", "--address", "--output"];
        let value = args.next().ok_or(USAGE)?;
        if !known.contains(&flag.as_str()) || flags.insert(flag, value).is_some() {
            return Err(USAGE.into());
        }
    }
    let mut take = |flag: &str| flags.remove(flag).ok_or_else(|| USAGE.to_owned());
    let seed_file = PathBuf::from(take("--seed-file")?);
    match command.as_str() {
        "generate" => {
            if !flags.is_empty() {
                return Err(USAGE.into());
            }
            let seed = Identity::generate_seed().map_err(|e| e.to_string())?;
            create(&seed_file, seed.as_ref(), 0o600)?;
            let identity = Identity::from_seed(&*read_seed(&seed_file)?);
            Ok(serde_json::json!({
                "seed_file": seed_file.display().to_string(),
                "public_key_sha256": dytallix_client_channel::fingerprint(identity.public_key()),
            }))
        }
        "pin" => {
            let (network, address, output) =
                (take("--network")?, take("--address")?, take("--output")?);
            if !flags.is_empty() {
                return Err(USAGE.into());
            }
            let identity = Identity::from_seed(&*read_seed(&seed_file)?);
            let pin = EndpointPin::new(&network, &address, identity.public_key())
                .map_err(|_| "the network is the chain ID and the address is HOST:PORT")?;
            create(Path::new(&output), &pin.to_json(), 0o644)?;
            Ok(serde_json::json!({
                "pin": output,
                "network": pin.network,
                "address": pin.address,
                "public_key_sha256": pin.fingerprint(),
            }))
        }
        _ => Err(USAGE.into()),
    }
}

fn main() {
    match run(std::env::args().skip(1).collect()) {
        Ok(report) => println!("{report}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn generate_then_pin_and_never_replace() {
        let dir = std::env::temp_dir().join(format!("dyt-ch-key-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let seed = dir.join("seed.bin");
        let seed = seed.to_str().unwrap();
        let pin_path = dir.join("pin.json");
        let pin_path = pin_path.to_str().unwrap();

        let generated = run(args(&["generate", "--seed-file", seed])).unwrap();
        let written = std::fs::read(seed).unwrap();
        assert_eq!(written.len(), 32);
        assert!(!generated.to_string().contains(&format!("{written:?}")));
        // An existing seed is never replaced.
        assert!(run(args(&["generate", "--seed-file", seed])).is_err());
        assert_eq!(std::fs::read(seed).unwrap(), written);

        let report = run(args(&[
            "pin",
            "--seed-file",
            seed,
            "--network",
            "chain-1",
            "--address",
            "node.example:26670",
            "--output",
            pin_path,
        ]))
        .unwrap();
        let pin = EndpointPin::parse(&std::fs::read(pin_path).unwrap()).unwrap();
        assert_eq!(pin.network, "chain-1");
        assert_eq!(
            pin.public_key,
            Identity::from_seed(&written.try_into().unwrap())
                .public_key()
                .to_vec()
        );
        assert_eq!(report["public_key_sha256"], generated["public_key_sha256"]);
        assert!(run(args(&[
            "pin",
            "--seed-file",
            seed,
            "--network",
            "chain-1",
            "--address",
            "node.example:26670",
            "--output",
            pin_path,
        ]))
        .is_err());

        for bad in [
            args(&[
                "pin",
                "--seed-file",
                seed,
                "--network",
                "chain-1",
                "--address",
                "no-port",
                "--output",
                "x",
            ]),
            args(&["generate", "--seed-file", seed, "--network", "chain-1"]),
            args(&["generate", "--seed-file"]),
            args(&["rotate", "--seed-file", seed]),
            args(&["generate", "--seed-file", seed, "--seed-file", seed]),
        ] {
            assert!(run(bad).is_err());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
