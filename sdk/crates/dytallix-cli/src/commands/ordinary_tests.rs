use super::*;
use clap::Parser;
use dytallix_sdk::ordinary_v2::{CommittedContext, PublicOrdinaryConfig};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    OnceLock,
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dytallix-ordinary-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self(path)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn key65() -> &'static DytallixKeypair {
    static KEY: OnceLock<DytallixKeypair> = OnceLock::new();
    KEY.get_or_init(DytallixKeypair::generate)
}
fn secondary_key() -> &'static DytallixKeypair {
    static KEY: OnceLock<DytallixKeypair> = OnceLock::new();
    #[cfg(not(feature = "strict-local-mldsa65"))]
    let generate = DytallixKeypair::generate_mldsa87;
    #[cfg(feature = "strict-local-mldsa65")]
    let generate = DytallixKeypair::generate;
    KEY.get_or_init(generate)
}
fn fixture(key: &DytallixKeypair) -> ValidatedInputs {
    let vector: Value = serde_json::from_str(include_str!(
        "../../../../vendor/dytallix-protocol-types/tests/fixtures/ordinary_fee_v1_vectors.json"
    ))
    .unwrap();
    let mut profile: FeeProfile = serde_json::from_value(vector["profile"].clone()).unwrap();
    profile.activation_height = 1;
    let algorithm = if key.scheme() == KeyScheme::MlDsa65 {
        "mldsa65"
    } else {
        "mldsa87"
    };
    let origin = if key.scheme() == KeyScheme::MlDsa65 {
        OriginKeyAlgorithm::MlDsa65
    } else {
        OriginKeyAlgorithm::MlDsa87
    };
    let address = AccountAddress::from_origin_key(
        AddressNetwork::Development,
        "ordinary-cli-fixture",
        origin,
        key.public_key(),
    )
    .unwrap();
    let context = SigningContext {
        domain: RecoveryDomain {
            network: 3,
            chain_id: "ordinary-cli-fixture".into(),
            genesis_digest: [7; 32],
            account_id: *address.account_id(),
        },
        current_key: KeyIdentity {
            algorithm: algorithm.into(),
            public_key: key.public_key().to_vec(),
        },
        authorization_generation: 4,
        spending_nonce: 9,
        committed: CommittedContext {
            chain_id: "ordinary-cli-fixture".into(),
            genesis_digest: [7; 32],
            height: 0,
            app_hash: [8; 32],
        },
        profile_digest: ordinary::profile_digest(&profile).unwrap(),
        protected: false,
    };
    let account: AccountView = serde_json::from_value(json!({
        "version":1,"context":context.committed,"domain":{"network":3,"chain_id":context.domain.chain_id,
        "genesis_digest":bytes_to_hex(&context.domain.genesis_digest),"account_id":bytes_to_hex(&context.domain.account_id)},
        "account_id":bytes_to_hex(&context.domain.account_id),"address":address.encode(),"current_key":context.current_key,
        "authorization_generation":"4","spending_nonce":"9","protected":false,"profile_digest":bytes_to_hex(&context.profile_digest)
    })).unwrap();
    let view = ProfileView {
        version: 1,
        enabled: true,
        context: context.committed.clone(),
        config: Some(PublicOrdinaryConfig {
            version: 1,
            fee_profile: profile.clone(),
            max_state_bytes: 1_000_000,
            max_grants: 100,
            max_receipts: 100,
            max_retained_profiles: 10,
            max_transport_bytes: 200_000,
            queue_max_entries: 100,
            queue_max_wire_bytes: 1_000_000,
            queue_max_signature_work: 100,
        }),
    };
    ordinary::validate_views(&context, &view, &account).unwrap();
    ValidatedInputs {
        profile,
        view,
        account,
        context,
    }
}
fn save_context(dir: &Scratch, inputs: &ValidatedInputs) -> CapturedInputs {
    let paths = CapturedInputs {
        profile: dir.path("profile.json"),
        account: dir.path("account.json"),
        context: dir.path("context.json"),
    };
    write_json(&paths.profile, &inputs.view).unwrap();
    write_json(&paths.account, &inputs.account).unwrap();
    write_json(&paths.context, &inputs.context).unwrap();
    paths
}
fn save_key(dir: &Scratch, key: &DytallixKeypair) -> PathBuf {
    let path = dir.path("key.json");
    write_json(
        &path,
        &json!({"algorithm":if key.scheme()==KeyScheme::MlDsa65 {"mldsa65"} else {"mldsa87"},
        "public_key":key.public_key(),"private_key":key.private_key()}),
    )
    .unwrap();
    path
}

#[test]
fn command_arguments_require_explicit_context_key_endpoint_and_cap() {
    assert!(crate::Cli::try_parse_from([
        "dytallix",
        "ordinary",
        "query-profile",
        "--output",
        "out.json"
    ])
    .is_err());
    assert!(crate::Cli::try_parse_from(["dytallix", "ordinary", "prepare"]).is_err());
    let common = [
        "dytallix",
        "ordinary",
        "sign",
        "--profile",
        "p",
        "--account",
        "a",
        "--context",
        "c",
        "--body",
        "b",
        "--output",
        "s",
    ];
    assert!(crate::Cli::try_parse_from(common).is_err());
    let mut args = common.to_vec();
    args.extend(["--wallet", "one", "--key-file", "two"]);
    assert!(crate::Cli::try_parse_from(args).is_err());
    let mut args = common.to_vec();
    args.extend(["--key-file", "two"]);
    assert!(crate::Cli::try_parse_from(args).is_ok());
    for value in ["", "+1", "-1", "01", "1.0", " 1", "1e6"] {
        assert!(decimal_u128(value).is_err());
    }
    assert_eq!(decimal_u128(&u128::MAX.to_string()).unwrap(), u128::MAX);
    assert!(decimal_u64("18446744073709551616").is_err());
    assert!(canonical_id(&"A".repeat(64)).is_err());
    assert_eq!(canonical_id(&"ab".repeat(32)).unwrap(), [0xab; 32]);
}

#[tokio::test]
async fn prepare_sign_inspect_and_transport_are_offline_for_selected_keys() {
    for key in [key65(), secondary_key()] {
        let dir = Scratch::new();
        let fixture = fixture(key);
        let inputs = save_context(&dir, &fixture);
        let actions = dir.path("actions.json");
        write_json(
            &actions,
            &vec![Action::Data {
                data: "local client fixture".into(),
            }],
        )
        .unwrap();
        let body = dir.path("body.json");
        let signed = dir.path("signed.json");
        let transport = dir.path("transport.json");
        run(OrdinaryArgs {
            command: OrdinaryCommand::Prepare {
                inputs: inputs.clone(),
                actions,
                memo: String::new(),
                expiry_height: 10,
                gas_limit: 1000,
                maximum_fee_udrt: 3000,
                output: body.clone(),
            },
        })
        .await
        .unwrap();
        run(OrdinaryArgs {
            command: OrdinaryCommand::Sign {
                inputs: inputs.clone(),
                body: body.clone(),
                wallet: None,
                key_file: Some(save_key(&dir, key)),
                output: signed.clone(),
            },
        })
        .await
        .unwrap();
        run(OrdinaryArgs {
            command: OrdinaryCommand::Inspect {
                inputs: inputs.clone(),
                body: None,
                signed: Some(signed.clone()),
            },
        })
        .await
        .unwrap();
        run(OrdinaryArgs {
            command: OrdinaryCommand::Transport {
                inputs: inputs.clone(),
                signed: signed.clone(),
                output: transport.clone(),
            },
        })
        .await
        .unwrap();
        let signed: SignedOrdinary = read_json(&signed, false).unwrap();
        assert_eq!(
            ordinary::decode_transport(
                &read_bounded(&transport, false).unwrap(),
                &fixture.profile.limits,
                200_000
            )
            .unwrap(),
            signed
        );
        let summary = describe(&signed.body, &fixture.profile, "signed_offline").unwrap();
        assert_eq!(summary["maximum_fee_udrt"], "3000");
        assert_eq!(summary["required_cap_udrt"], "2000");
        assert_eq!(summary["minimum_charge_udrt"], "20");
        assert!(summary["actual_charge_udrt"].is_null());
        assert_eq!(summary["committed"], false);
        let public = serde_json::to_string(&summary).unwrap();
        assert!(!public.contains("private_key"));
        assert!(!public.contains(&serde_json::to_string(key.private_key()).unwrap()));
    }
}

#[test]
fn key_import_rejects_mismatch_alias_and_private_schema_without_echo() {
    let dir = Scratch::new();
    let mut value = json!({"algorithm":"mldsa65","public_key":key65().public_key(),"private_key":secondary_key().private_key()});
    let path = dir.path("bad.json");
    write_json(&path, &value).unwrap();
    let error = load_signing_key(None, Some(&path))
        .err()
        .unwrap()
        .to_string();
    assert_eq!(
        error,
        "private key file does not contain a valid matching key pair"
    );
    value["algorithm"] = json!("ML-DSA-65");
    assert!(exact_scheme(value["algorithm"].as_str().unwrap()).is_err());
    let raw = dir.path("malformed.json");
    write_new(
        &raw,
        br#"{"algorithm":"mldsa65","public_key":[],"private_key":"DO_NOT_ECHO_THIS_SECRET"}"#,
    )
    .unwrap();
    let error = read_json::<PrivateKeyInput>(&raw, true)
        .err()
        .unwrap()
        .to_string();
    assert!(!error.contains("DO_NOT_ECHO_THIS_SECRET"));
    assert!(serde_json::from_str::<PrivateKeyInput>(
        r#"{"algorithm":"mldsa65","public_key":[],"private_key":[],"unknown":true}"#
    )
    .is_err());
}

#[test]
fn bounded_files_are_exclusive_private_and_reject_symlinks() {
    let dir = Scratch::new();
    let path = dir.path("artifact.json");
    write_new(&path, b"first").unwrap();
    assert!(write_new(&path, b"replacement").is_err());
    assert_eq!(read_bounded(&path, true).unwrap(), b"first");
    let huge = dir.path("oversize");
    fs::write(&huge, vec![0; MAX_FILE_BYTES + 1]).unwrap();
    assert!(read_bounded(&huge, false).is_err());
    assert!(read_bounded(&dir.0, false).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::{symlink, PermissionsExt};
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let link = dir.path("link");
        symlink(&path, &link).unwrap();
        assert!(read_bounded(&link, false).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_bounded(&path, true).is_err());
    }
}

fn genesis_fixture() -> (ValidatedInputs, GenesisIdentity, Vec<u8>) {
    let mut fixture = fixture(key65());
    let address = fixture.account.address.clone();
    let bytes=serde_json::to_vec(&json!({"chain_id":fixture.context.domain.chain_id,"accounts":[{"address":address,"balances":{"udrt":"1"}}]})).unwrap();
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    fixture.context.domain.genesis_digest = digest;
    fixture.context.committed.genesis_digest = digest;
    fixture.view.context = fixture.context.committed.clone();
    fixture.account.context = fixture.context.committed.clone();
    fixture.account.domain.genesis_digest = digest;
    let origin = fixture.context.current_key.clone();
    fixture.context.current_key = KeyIdentity {
        algorithm: secondary_key().scheme().algorithm_id().into(),
        public_key: secondary_key().public_key().to_vec(),
    };
    fixture.account.current_key = fixture.context.current_key.clone();
    ordinary::validate_views(&fixture.context, &fixture.view, &fixture.account).unwrap();
    let manifest = GenesisIdentity {
        domain: fixture.context.domain.clone(),
        address,
        origin_key: origin,
        current_key: fixture.context.current_key.clone(),
        fee_profile_digest: fixture.context.profile_digest,
    };
    (fixture, manifest, bytes)
}

#[test]
fn genesis_checks_exact_bytes_and_separate_origin_and_current_authority() {
    let (mut fixture, manifest, bytes) = genesis_fixture();
    check_genesis(&fixture, &manifest, &bytes).unwrap();
    let mut changed = bytes.clone();
    changed.push(b'\n');
    assert!(check_genesis(&fixture, &manifest, &changed).is_err());
    let mut wrong_origin = manifest;
    wrong_origin.origin_key = wrong_origin.current_key.clone();
    assert!(check_genesis(&fixture, &wrong_origin, &bytes).is_err());
    fixture.context.committed.height = 1;
    assert!(check_genesis(&fixture, &wrong_origin, &bytes).is_err());
}

#[test]
fn protected_genesis_identity_is_checked_without_enabling_signing() {
    let (mut fixture, manifest, bytes) = genesis_fixture();
    fixture.context.protected = true;
    fixture.account.protected = true;
    let dir = Scratch::new();
    let inputs = save_context(&dir, &fixture);
    let identity = inputs.load_identity().unwrap();
    check_genesis(&identity, &manifest, &bytes).unwrap();
    assert!(inputs.load().is_err());
    assert!(ordinary::prepare(
        &fixture.profile,
        &fixture.context,
        vec![Action::DmsPing],
        String::new(),
        10,
        1000,
        3000
    )
    .is_err());
}

#[tokio::test]
async fn signing_rejects_stale_context_and_wrong_current_key_without_output() {
    let dir = Scratch::new();
    let fixture = fixture(key65());
    let inputs = save_context(&dir, &fixture);
    let prepared = ordinary::prepare(
        &fixture.profile,
        &fixture.context,
        vec![Action::DmsPing],
        String::new(),
        10,
        1000,
        3000,
    )
    .unwrap();
    let body = dir.path("body.json");
    write_json(&body, prepared.body()).unwrap();
    let output = dir.path("signed.json");
    assert!(run(OrdinaryArgs {
        command: OrdinaryCommand::Sign {
            inputs: inputs.clone(),
            body: body.clone(),
            wallet: None,
            key_file: Some(save_key(&dir, secondary_key())),
            output: output.clone()
        }
    })
    .await
    .is_err());
    assert!(!output.exists());
    let mut stale = fixture.context.clone();
    stale.spending_nonce += 1;
    fs::write(&inputs.context, serde_json::to_vec(&stale).unwrap()).unwrap();
    assert!(inputs.load().is_err());
    assert!(!output.exists());
}

#[cfg(feature = "strict-local-mldsa65")]
#[test]
fn strict_profile_rejects_unsupported_algorithm_selection() {
    assert_eq!(exact_scheme("mldsa65").unwrap(), KeyScheme::MlDsa65);
    for algorithm in [
        "mldsa87",
        "mldsa44",
        "SlhDsa",
        "legacy-sphincsplus-shake-192s-simple",
    ] {
        assert!(exact_scheme(algorithm).is_err());
    }
}
