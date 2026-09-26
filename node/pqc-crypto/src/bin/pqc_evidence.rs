//! Minimal PQC evidence helper
//! Requires `pqc-real`. This checks the PQCManager backend, not FIPS conformance.
//! Generates keypair, signs canonical tx bytes, verifies OK, then tamper verifies FAIL.
//! Outputs artifacts to a provided directory argument (default: ./pqc_artifacts).

#[cfg(feature = "pqc-real")]
use base64::Engine;
#[cfg(feature = "pqc-real")]
use dytallix_pqc::PQCManager;
#[cfg(feature = "pqc-real")]
use std::fs::{self, File};
#[cfg(feature = "pqc-real")]
use std::io::Write;
#[cfg(feature = "pqc-real")]
use std::path::PathBuf;

#[cfg(feature = "pqc-real")]
fn b64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

#[cfg(not(feature = "pqc-real"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    Err("pqc_evidence requires the pqc-real feature; no evidence was produced".into())
}

#[cfg(feature = "pqc-real")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    run_pqc_evidence()
}

#[cfg(feature = "pqc-real")]
fn run_pqc_evidence() -> Result<(), Box<dyn std::error::Error>> {
    use sha3::{Digest, Sha3_256};

    let out_dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "pqc_artifacts".to_string());
    let out = PathBuf::from(out_dir);
    fs::create_dir_all(&out)?;

    let manager = PQCManager::new()?;
    let pubkey = manager.get_signature_public_key();
    let algo = manager.get_signature_algorithm();
    let pub_hex = hex::encode(pubkey);
    let mut f = File::create(out.join("pubkey.hex"))?;
    f.write_all(pub_hex.as_bytes())?;

    let canonical = br#"{\"chain_id\":\"dytallix-testnet\",\"from\":\"demo1\",\"to\":\"demo2\",\"amount\":12345,\"nonce\":1}"#;

    let mut hasher = Sha3_256::new();
    hasher.update(canonical);
    let digest = hasher.finalize();

    let sig = manager.sign(&digest)?;
    let signed = serde_json::json!({
        "algorithm": format!("{:?}", algo),
        "pubkey_hex": pub_hex,
        "payload_b64": b64(&digest),
        "sig_b64": b64(&sig.data),
    });
    fs::write(
        out.join("signed_tx.json"),
        serde_json::to_string_pretty(&signed)?,
    )?;

    let ok = manager.verify(&digest, &sig, pubkey)?;
    let mut ok_log = File::create(out.join("verify_ok.log"))?;
    if ok {
        writeln!(ok_log, "VERIFY_OK")?;
    } else {
        writeln!(ok_log, "VERIFY_FAIL")?;
    }

    let mut tampered = digest.to_vec();
    if let Some(b) = tampered.get_mut(0) {
        *b ^= 0x01;
    }
    let fail = manager.verify(&tampered, &sig, pubkey)?;
    let mut fail_log = File::create(out.join("verify_fail_tamper.log"))?;
    if fail {
        writeln!(fail_log, "VERIFY_OK")?;
    } else {
        writeln!(fail_log, "VERIFY_FAIL")?;
    }

    let summary = serde_json::json!({
        "algorithm": format!("{:?}", algo),
        "pubkey_bytes": pubkey.len(),
        "signature_bytes": sig.data.len(),
        "verify_ok": ok,
        "verify_fail_expected": !fail,
    });
    println!("{}", serde_json::to_string(&summary)?);

    validate_results(ok, fail)
}

#[cfg(any(feature = "pqc-real", test))]
fn validate_results(
    valid_accepted: bool,
    changed_accepted: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if !valid_accepted {
        return Err("valid signature verification failed".into());
    }
    if changed_accepted {
        return Err("changed message was accepted".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_results;

    #[test]
    fn evidence_requires_both_expected_results() {
        assert!(validate_results(true, false).is_ok());
        assert!(validate_results(false, false).is_err());
        assert!(validate_results(true, true).is_err());
        assert!(validate_results(false, true).is_err());
    }
}
