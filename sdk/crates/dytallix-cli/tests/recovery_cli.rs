//! The CLI's recovery flow (E04 gap 17, T-c2), offline: prepare, each party
//! signs with its own key file, assemble, sponsor. Submission needs a node.
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use dytallix_core::keypair::DytallixKeypair;
use serde_json::{json, Value};

fn dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "dytallix-cli-recovery-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dytallix"))
        .args(["recovery"])
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}
fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
fn key_file(dir: &Path, name: &str, key: &DytallixKeypair) -> String {
    let path = dir.join(format!("{name}.key.json"));
    std::fs::write(
        &path,
        json!({"algorithm":"mldsa65","public_key":key.public_key(),"private_key":key.private_key()}).to_string(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    path.to_str().unwrap().to_owned()
}
fn identity(key: &DytallixKeypair) -> Value {
    json!({"algorithm":"mldsa65","public_key":key.public_key()})
}
fn view(id: u8, active: &DytallixKeypair, guardians: &[DytallixKeypair]) -> Value {
    let policy = if guardians.is_empty() {
        Value::Null
    } else {
        json!({"threshold":2,"guardians":guardians.iter().enumerate()
            .map(|(i, g)| json!({"key":identity(g),"control_group":format!("custodian-{i}")})).collect::<Vec<_>>()})
    };
    json!({
        "version":1,
        "context":{"chain_id":"recovery-test","genesis_digest":"01".repeat(32),"height":"20","app_hash":"02".repeat(32)},
        "domain":{"network":3,"chain_id":"recovery-test","genesis_digest":"01".repeat(32),"account_id":format!("{id:02x}").repeat(32)},
        "address":"address","status":"Normal","active_key":identity(active),
        "active_generation":"4","spending_nonce":"7","sponsor_nonce":"3",
        "policy":policy,"policy_version":if guardians.is_empty() {"0"} else {"1"},
        "recovery_sequence":"0","policy_change_sequence":"0",
        "pending_recovery":null,"pending_policy":null,
        "timing":{"timing_version":"1","recovery_delay":"2","finalization_window":"5","policy_delay":"2",
            "policy_window":"5","submission_lifetime":"30","algorithms":["mldsa65"]},
        "fee":{"profile_version":"1","profile_digest":"09".repeat(32),"denomination":"udrt","gas_price":"2",
            "minimum_gas":"10","max_transaction_gas":"1000000","max_fee_cap":"2000000"}
    })
}
fn write(dir: &Path, name: &str, value: &Value) -> String {
    let path = dir.join(name);
    std::fs::write(&path, value.to_string()).unwrap();
    path.to_str().unwrap().to_owned()
}

#[test]
fn guardians_sign_offline_and_a_sponsor_pays() {
    let home = dir("flow");
    let path = |name: &str| home.join(name).to_str().unwrap().to_owned();
    let active = DytallixKeypair::generate();
    let replacement = DytallixKeypair::generate();
    let sponsor = DytallixKeypair::generate();
    let mut guardians: Vec<DytallixKeypair> = (0..3).map(|_| DytallixKeypair::generate()).collect();
    guardians.sort_by(|a, b| a.public_key().cmp(b.public_key()));

    let target_view = write(&home, "target-view.json", &view(11, &active, &guardians));
    let sponsor_view = write(&home, "sponsor-view.json", &view(12, &sponsor, &[]));
    let request = write(
        &home,
        "request.json",
        &json!({"action":"start","replacement":identity(&replacement),"request_id":"05".repeat(32)}),
    );
    let prepared = run(
        &home,
        &[
            "prepare",
            "--view",
            &target_view,
            "--request",
            &request,
            "--expiry-height",
            "40",
            "--output",
            &path("op.json"),
        ],
    );
    assert!(prepared.status.success(), "{}", text(&prepared));
    let op: Value =
        serde_json::from_str(&std::fs::read_to_string(home.join("op.json")).unwrap()).unwrap();
    assert_eq!(op["requirements"]["guardian_threshold"], 2);

    // Each party signs with its own key.
    let sign = |key: &DytallixKeypair, name: &str, role: &str| {
        let file = key_file(&home, name, key);
        run(
            &home,
            &[
                "sign",
                "--operation",
                &path("op.json"),
                "--role",
                role,
                "--key-file",
                &file,
                "--output",
                &path(&format!("{name}.sig.json")),
            ],
        )
    };
    for (i, g) in guardians.iter().enumerate() {
        let signed = sign(g, &format!("guardian-{i}"), "operation");
        assert!(signed.status.success(), "{}", text(&signed));
    }
    assert!(sign(&replacement, "replacement", "possession")
        .status
        .success());
    let refused = sign(&active, "active", "operation");
    assert!(
        !refused.status.success() && text(&refused).contains("not named for the operation role"),
        "{}",
        text(&refused)
    );

    // One guardian is below the quorum; two meet it.
    let below = run(
        &home,
        &[
            "assemble",
            "--operation",
            &path("op.json"),
            "--signature",
            &path("guardian-0.sig.json"),
            "--signature",
            &path("replacement.sig.json"),
            "--output",
            &path("signed-below.json"),
        ],
    );
    assert!(!below.status.success(), "{}", text(&below));
    let assembled = run(
        &home,
        &[
            "assemble",
            "--operation",
            &path("op.json"),
            "--signature",
            &path("guardian-2.sig.json"),
            "--signature",
            &path("replacement.sig.json"),
            "--signature",
            &path("guardian-0.sig.json"),
            "--output",
            &path("signed.json"),
        ],
    );
    assert!(assembled.status.success(), "{}", text(&assembled));

    let sponsor_key = key_file(&home, "sponsor", &sponsor);
    let sponsored = run(
        &home,
        &[
            "sponsor",
            "--signed",
            &path("signed.json"),
            "--target-view",
            &target_view,
            "--sponsor-view",
            &sponsor_view,
            "--key-file",
            &sponsor_key,
            "--gas-limit",
            "100000",
            "--maximum-charge-udrt",
            "200000",
            "--expiry-height",
            "40",
            "--output",
            &path("tx.json"),
        ],
    );
    assert!(sponsored.status.success(), "{}", text(&sponsored));
    let tx: Value =
        serde_json::from_str(&std::fs::read_to_string(home.join("tx.json")).unwrap()).unwrap();
    use base64::{engine::general_purpose::STANDARD, Engine};
    let envelope = dytallix_sdk::recovery::decode_envelope(
        &STANDARD
            .decode(tx["envelope_base64"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(envelope.recovery.signatures.len(), 3);
    assert_eq!(
        tx["authorization_id"],
        dytallix_sdk::recovery::hex(&dytallix_sdk::recovery::authorization_id(&envelope).unwrap())
    );
    assert_eq!(envelope.sponsor.sponsor_nonce, 3);

    // The target never pays for itself, and outputs are never replaced.
    let own = run(
        &home,
        &[
            "sponsor",
            "--signed",
            &path("signed.json"),
            "--target-view",
            &target_view,
            "--sponsor-view",
            &target_view,
            "--key-file",
            &key_file(&home, "active-sponsor", &active),
            "--gas-limit",
            "100000",
            "--maximum-charge-udrt",
            "200000",
            "--expiry-height",
            "40",
            "--output",
            &path("tx-own.json"),
        ],
    );
    assert!(!own.status.success(), "{}", text(&own));
    let again = run(
        &home,
        &[
            "assemble",
            "--operation",
            &path("op.json"),
            "--signature",
            &path("guardian-2.sig.json"),
            "--signature",
            &path("replacement.sig.json"),
            "--signature",
            &path("guardian-0.sig.json"),
            "--output",
            &path("signed.json"),
        ],
    );
    assert!(!again.status.success());
    std::fs::remove_dir_all(&home).unwrap();
}
