use super::*;
use std::os::unix::fs::PermissionsExt;

fn config(directory: &std::path::Path, helper: PathBuf) -> EmergencyVerifierConfig {
    let scratch = directory.join("scratch");
    std::fs::create_dir(&scratch).unwrap();
    std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700)).unwrap();
    EmergencyVerifierConfig {
        helper_sha256: hex::encode(Sha256::digest(std::fs::read(&helper).unwrap())),
        helper_path: helper,
        helper_scratch_path: scratch,
        helper_execution: None,
        max_helper_bytes: 32 * 1024 * 1024,
        max_request_bytes: 65536,
        timeout_ms: 5000,
    }
}
fn fake(directory: &std::path::Path, body: &str) -> PathBuf {
    let path = directory.join("fault-only-helper");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn attempt(verifier: &EmergencyVerifier) -> Result<bool> {
    verifier.verify(
        "development-chain",
        &[1; 64],
        1,
        10,
        10,
        10,
        b"development-artifact",
        &vec![1; 29792],
    )
}

#[test]
fn emergency_verifier_requires_explicit_local_configuration() {
    let directory = tempfile::tempdir().unwrap();
    let mut c = config(directory.path(), fake(directory.path(), "exit 1"));
    c.helper_scratch_path = PathBuf::from("relative");
    assert!(EmergencyVerifier::new(c.clone()).is_err());
    c.helper_scratch_path = directory.path().join("scratch");
    c.helper_sha256 = "AA".repeat(32);
    assert!(EmergencyVerifier::new(c.clone()).is_err());
    c.helper_sha256 = "aa".repeat(32);
    c.timeout_ms = 0;
    assert!(EmergencyVerifier::new(c).is_err());
}
#[test]
fn emergency_verifier_preserves_infrastructure_failures() {
    let cases = [
        ("exit-one", "/bin/cat >/dev/null\nexit 1"),
        ("unclassified-exit-two", "/bin/cat >/dev/null\nexit 2"),
        (
            "fake-diagnostic-exit-one",
            "/bin/cat >/dev/null\necho 'root verification rejected' >&2\nexit 1",
        ),
        ("malformed-response", "/bin/cat >/dev/null\necho '{}'"),
        ("deadline", "exec /bin/sleep 2"),
    ];
    for (name, body) in cases {
        let directory = tempfile::tempdir().unwrap();
        let mut c = config(directory.path(), fake(directory.path(), body));
        if name == "deadline" {
            c.timeout_ms = 10;
        }
        let v = EmergencyVerifier::new(c).unwrap();
        let err = attempt(&v).expect_err(name);
        assert!(
            is_infrastructure_error(&err.context("outer proposal context")),
            "{name}"
        );
        assert_eq!(
            std::fs::read_dir(&v.config.helper_scratch_path)
                .unwrap()
                .count(),
            0,
            "{name}"
        );
    }
}
#[test]
fn emergency_verifier_hash_and_scratch_failures_are_infrastructure() {
    let directory = tempfile::tempdir().unwrap();
    let c = config(directory.path(), fake(directory.path(), "exit 1"));
    let mut bad = c.clone();
    bad.helper_sha256 = "aa".repeat(32);
    let err = attempt(&EmergencyVerifier::new(bad).unwrap()).unwrap_err();
    assert!(is_infrastructure_error(&err));
    std::fs::set_permissions(
        &c.helper_scratch_path,
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    assert!(is_infrastructure_error(
        &attempt(&EmergencyVerifier::new(c).unwrap()).unwrap_err()
    ));
}
#[test]
fn emergency_verifier_refusal_requires_exact_exit_protocol() {
    let directory = tempfile::tempdir().unwrap();
    let c = config(
        directory.path(),
        fake(
            directory.path(),
            "/bin/cat >/dev/null\necho 'root verification rejected' >&2\nexit 2",
        ),
    );
    let v = EmergencyVerifier::new(c).unwrap();
    assert!(!attempt(&v).unwrap());
    assert!(!v
        .verify(
            "development-chain",
            &[1; 64],
            1,
            10,
            10,
            10,
            b"artifact",
            &[1; 2]
        )
        .unwrap());
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct Fixture {
    PublicKey: String,
    Request: FixtureRequest,
}
#[derive(Deserialize)]
#[allow(non_snake_case)]
struct FixtureRequest {
    Envelope: serde_json::Value,
    Signature: String,
    Artifact: String,
}

#[test]
#[ignore = "Requires explicitly built SLH-DSA helper and disposable public signature fixture"]
fn emergency_verifier_actual_slh_dsa_signature_and_domain_binding() {
    let directory = tempfile::tempdir().unwrap();
    let helper =
        PathBuf::from(std::env::var("DYT_EMERGENCY_VERIFIER").expect("explicit built helper"));
    let f: Fixture = serde_json::from_slice(
        &std::fs::read(
            std::env::var("DYT_EMERGENCY_PUBLIC_FIXTURE").expect("explicit public fixture"),
        )
        .unwrap(),
    )
    .unwrap();
    let v = EmergencyVerifier::new(config(directory.path(), helper)).unwrap();
    let public = B64.decode(f.PublicKey).unwrap();
    let sig = B64.decode(f.Request.Signature).unwrap();
    let artifact = B64.decode(f.Request.Artifact).unwrap();
    let e = f.Request.Envelope;
    let chain = e["ChainID"].as_str().unwrap();
    let sequence = e["Sequence"].as_u64().unwrap();
    let height = e["NotBeforeHeight"].as_u64().unwrap();
    assert!(v
        .verify(chain, &public, sequence, height, height, height, &artifact, &sig)
        .unwrap());
    let mut changed_sig = sig.clone();
    changed_sig[100] ^= 1;
    assert!(!v
        .verify(
            chain,
            &public,
            sequence,
            height,
            height,
            height,
            &artifact,
            &changed_sig
        )
        .unwrap());
    let mut changed_key = public.clone();
    changed_key[40] ^= 1;
    assert!(!v
        .verify(
            chain,
            &changed_key,
            sequence,
            height,
            height,
            height,
            &artifact,
            &sig
        )
        .unwrap());
    let mut changed_artifact = artifact.clone();
    changed_artifact.push(0);
    assert!(!v
        .verify(
            chain,
            &public,
            sequence,
            height,
            height,
            height,
            &changed_artifact,
            &sig
        )
        .unwrap());
    assert!(!v
        .verify(
            chain,
            &public,
            sequence + 1,
            height,
            height,
            height,
            &artifact,
            &sig
        )
        .unwrap());
    assert!(!v
        .verify(
            "another-development-chain",
            &public,
            sequence,
            height,
            height,
            height,
            &artifact,
            &sig
        )
        .unwrap());
    assert!(!v
        .verify(
            chain,
            &public,
            sequence,
            height + 1,
            height,
            height,
            &artifact,
            &sig
        )
        .unwrap());
    assert!(!v
        .verify(
            chain,
            &public,
            sequence,
            height,
            height,
            height + 1,
            &artifact,
            &sig
        )
        .unwrap());
    let mut low = v.config.clone();
    low.max_request_bytes = 100;
    assert!(is_infrastructure_error(
        &EmergencyVerifier::new(low)
            .unwrap()
            .verify(chain, &public, sequence, height, height, height, &artifact, &sig)
            .unwrap_err()
    ));
    assert_eq!(
        std::fs::read_dir(&v.config.helper_scratch_path)
            .unwrap()
            .count(),
        0
    );
}

fn upgrade_attempt(verifier: &EmergencyVerifier) -> Result<bool> {
    crate::upgrade::Verifier::verify(
        verifier,
        "development-chain",
        &[1; 64],
        1,
        10,
        10,
        10,
        b"development-artifact",
        &vec![1; 29792],
    )
}

// This helper only tests response transport and binding. It does not verify a signature.
fn response_fixture(action: &str) -> crate::root_genesis::Response {
    let artifact = b"development-artifact";
    // The wire contract uses struct order, not map order.
    let request = serde_json::to_vec(&Request {
        Envelope: Envelope {
            Version: 1,
            Profile: PROFILE,
            ChainID: "development-chain",
            Action: if action == "upgrade" {
                "upgrade"
            } else {
                "emergency"
            },
            Sequence: 1,
            NotBeforeHeight: 10,
            NotAfterHeight: 10,
            ArtifactDigest: Sha512::digest(artifact).to_vec(),
        },
        Signature: B64.encode(vec![1; 29792]),
        Artifact: B64.encode(artifact),
    })
    .unwrap();
    crate::root_genesis::Response {
        status: "VERIFIED".into(),
        request_sha256: hex::encode(Sha256::digest(request)),
        artifact_sha512: hex::encode(Sha512::digest(artifact)),
        chain_id: "development-chain".into(),
        action: action.into(),
        sequence: 1,
        production_qualified: false,
    }
}

#[test]
fn upgrade_verifier_requires_exact_action_and_response_binding() {
    for field in [
        "none",
        "status",
        "request",
        "artifact",
        "chain",
        "action",
        "sequence",
        "qualified",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut response = response_fixture("upgrade");
        match field {
            "none" => {}
            "status" => response.status = "ACCEPTED".into(),
            "request" => response.request_sha256 = "00".repeat(32),
            "artifact" => response.artifact_sha512 = "00".repeat(64),
            "chain" => response.chain_id = "other-chain".into(),
            "action" => response.action = "emergency".into(),
            "sequence" => response.sequence = 2,
            "qualified" => response.production_qualified = true,
            _ => unreachable!(),
        }
        let raw = serde_json::to_string(&response).unwrap();
        let helper = fake(
            directory.path(),
            &format!("/bin/cat >/dev/null\nprintf '%s' '{raw}'"),
        );
        let v = EmergencyVerifier::new(config(directory.path(), helper)).unwrap();
        if field == "none" {
            assert!(upgrade_attempt(&v).unwrap());
            // An upgrade response cannot satisfy the emergency trait.
            assert!(is_infrastructure_error(&attempt(&v).unwrap_err()));
        } else {
            assert!(
                is_infrastructure_error(&upgrade_attempt(&v).expect_err(field)),
                "{field}"
            );
        }
    }
}

#[test]
fn emergency_verifier_rejects_upgrade_response_even_with_emergency_request_hash() {
    let directory = tempfile::tempdir().unwrap();
    let mut response = response_fixture("emergency");
    response.action = "upgrade".into();
    let raw = serde_json::to_string(&response).unwrap();
    let helper = fake(
        directory.path(),
        &format!("/bin/cat >/dev/null\nprintf '%s' '{raw}'"),
    );
    let v = EmergencyVerifier::new(config(directory.path(), helper)).unwrap();
    assert!(is_infrastructure_error(&attempt(&v).unwrap_err()));
}

#[test]
fn upgrade_verifier_preserves_refusal_and_infrastructure_distinction() {
    for (body, deterministic) in [
        (
            "/bin/cat >/dev/null\necho 'root verification rejected' >&2\nexit 2",
            true,
        ),
        ("/bin/cat >/dev/null\nexit 2", false),
        (
            "/bin/cat >/dev/null\necho 'root verification rejected' >&2\nexit 1",
            false,
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let v =
            EmergencyVerifier::new(config(directory.path(), fake(directory.path(), body))).unwrap();
        if deterministic {
            assert!(!upgrade_attempt(&v).unwrap());
        } else {
            assert!(is_infrastructure_error(&upgrade_attempt(&v).unwrap_err()));
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let v =
        EmergencyVerifier::new(config(directory.path(), fake(directory.path(), "exit 1"))).unwrap();
    assert!(!crate::upgrade::Verifier::verify(
        &v,
        "development-chain",
        &[1; 64],
        1,
        10,
        10,
        10,
        b"artifact",
        &[0; 2]
    )
    .unwrap());
}

#[test]
#[ignore = "Requires explicit SLH helper and separate disposable emergency and upgrade fixtures"]
fn upgrade_verifier_actual_slh_action_and_artifact_domain_separation() {
    let directory = tempfile::tempdir().unwrap();
    let helper =
        PathBuf::from(std::env::var("DYT_EMERGENCY_VERIFIER").expect("explicit built helper"));
    let v = EmergencyVerifier::new(config(directory.path(), helper)).unwrap();
    for (path_env, expected_action) in [
        ("DYT_UPGRADE_PUBLIC_FIXTURE", "upgrade"),
        ("DYT_EMERGENCY_V2_PUBLIC_FIXTURE", "emergency"),
    ] {
        let f: Fixture = serde_json::from_slice(
            &std::fs::read(std::env::var(path_env).expect("explicit public fixture")).unwrap(),
        )
        .unwrap();
        let key = B64.decode(f.PublicKey).unwrap();
        let sig = B64.decode(f.Request.Signature).unwrap();
        let artifact = B64.decode(f.Request.Artifact).unwrap();
        let e = f.Request.Envelope;
        assert_eq!(e["Action"].as_str().unwrap(), expected_action);
        let chain = e["ChainID"].as_str().unwrap();
        let sequence = e["Sequence"].as_u64().unwrap();
        let before = e["NotBeforeHeight"].as_u64().unwrap();
        let after = e["NotAfterHeight"].as_u64().unwrap();
        let emergency = ControlVerifier::verify(
            &v, chain, &key, sequence, before, before, after, &artifact, &sig,
        )
        .unwrap();
        let upgrade = crate::upgrade::Verifier::verify(
            &v, chain, &key, sequence, before, before, after, &artifact, &sig,
        )
        .unwrap();
        assert_eq!(emergency, expected_action == "emergency");
        assert_eq!(upgrade, expected_action == "upgrade");
        let mut altered = artifact.clone();
        altered[0] ^= 1;
        assert!(!crate::upgrade::Verifier::verify(
            &v, chain, &key, sequence, before, before, after, &altered, &sig
        )
        .unwrap());
    }
}
