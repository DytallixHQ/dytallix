use super::*;
use clap::Parser;
use dytallix_core::keypair::DytallixKeypair;
use dytallix_sdk::ordinary_v2::{
    self as ordinary, CommittedContext, KeyIdentity, OriginKeyAlgorithm, PublicOrdinaryConfig,
    RecoveryDomain,
};
use dytallix_sdk::ordinary_v3::{CLASS_PARAMETER_CHANGE, CLASS_VALIDATOR_REGISTRY};
use std::fs;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    OnceLock,
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dytallix-governance-cli-{}-{}",
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
fn key() -> &'static DytallixKeypair {
    static KEY: OnceLock<DytallixKeypair> = OnceLock::new();
    KEY.get_or_init(DytallixKeypair::generate)
}

struct Fixture {
    inputs: Validated,
    account: AccountView,
    address: String,
}
/// The shared v3 vector profile (activation 30) at committed height 40.
fn fixture() -> Fixture {
    let vectors: Value = serde_json::from_str(include_str!(
        "../../../../vendor/dytallix-protocol-types/tests/fixtures/ordinary_v3_vectors.json"
    ))
    .unwrap();
    let mut profile: FeeProfileV3 =
        serde_json::from_value(vectors["fee_profile"]["input"].clone()).unwrap();
    // A fee change carries every cost; the vectors' 128-byte bound is too small.
    profile.max_governance_action_bytes = 1_024;
    let address = AccountAddress::from_origin_key(
        AddressNetwork::Development,
        "governance-cli-fixture",
        OriginKeyAlgorithm::MlDsa65,
        key().public_key(),
    )
    .unwrap();
    let committed = CommittedContext {
        chain_id: "governance-cli-fixture".into(),
        genesis_digest: [7; 32],
        height: 40,
        app_hash: [8; 32],
    };
    let context = SigningContext {
        domain: RecoveryDomain {
            network: 3,
            chain_id: committed.chain_id.clone(),
            genesis_digest: committed.genesis_digest,
            account_id: *address.account_id(),
        },
        current_key: KeyIdentity {
            algorithm: "mldsa65".into(),
            public_key: key().public_key().to_vec(),
        },
        authorization_generation: 1,
        spending_nonce: 6,
        committed: committed.clone(),
        profile_digest: ordinary::profile_digest(&profile.base).unwrap(),
        protected: false,
    };
    let account: AccountView = serde_json::from_value(json!({
        "version":1,"context":committed,"domain":{"network":3,"chain_id":context.domain.chain_id,
        "genesis_digest":bytes_to_hex(&context.domain.genesis_digest),"account_id":bytes_to_hex(&context.domain.account_id)},
        "account_id":bytes_to_hex(&context.domain.account_id),"address":address.encode(),"current_key":context.current_key,
        "authorization_generation":"1","spending_nonce":"6","protected":false,"profile_digest":bytes_to_hex(&context.profile_digest)
    })).unwrap();
    let view = ProfileView {
        version: 1,
        enabled: true,
        context: committed.clone(),
        config: Some(PublicOrdinaryConfig {
            version: 1,
            fee_profile: profile.base.clone(),
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
    let governance_view = GovernanceProfileView {
        version: 1,
        enabled: true,
        context: committed,
        fee_profile: Some(profile.clone()),
        next_proposal_id: 3,
    };
    governance::validate_views(&context, &view, &governance_view, &account).unwrap();
    Fixture {
        inputs: Validated {
            profile,
            view,
            governance: governance_view,
            context,
        },
        account,
        address: address.encode(),
    }
}
fn save(dir: &Scratch, f: &Fixture) -> GovernanceInputs {
    let paths = GovernanceInputs {
        ordinary: CapturedInputs {
            profile: dir.path("profile.json"),
            account: dir.path("account.json"),
            context: dir.path("context.json"),
        },
        governance_profile: dir.path("governance.json"),
    };
    write_json(&paths.ordinary.profile, &f.inputs.view).unwrap();
    write_json(&paths.ordinary.account, &f.account).unwrap();
    write_json(&paths.ordinary.context, &f.inputs.context).unwrap();
    write_json(&paths.governance_profile, &f.inputs.governance).unwrap();
    paths
}
fn save_key(dir: &Scratch) -> PathBuf {
    let path = dir.path("key.json");
    write_json(
        &path,
        &json!({"algorithm":"mldsa65","public_key":key().public_key(),"private_key":key().private_key()}),
    )
    .unwrap();
    path
}
fn body(dir: &Scratch, name: &str) -> BodyArgs {
    BodyArgs {
        memo: String::new(),
        expiry_height: 100,
        gas_limit: 100,
        maximum_fee_udrt: 200,
        output: dir.path(name),
    }
}
fn change() -> ChangeArgs {
    ChangeArgs {
        max_active: None,
        min_self_bond_udgt: None,
        fees: None,
        registry_add: None,
        registry_remove: None,
    }
}
/// Prepare, sign and inspect one action offline; return the signed body.
async fn signed(
    dir: &Scratch,
    inputs: &GovernanceInputs,
    command: GovernanceCommand,
) -> SignedOrdinary {
    let body_path = match &command {
        GovernanceCommand::Propose { body, .. }
        | GovernanceCommand::Deposit { body, .. }
        | GovernanceCommand::Vote { body, .. } => body.output.clone(),
        _ => unreachable!(),
    };
    run(GovernanceArgs { command }).await.unwrap();
    let signed_path = body_path.with_extension("signed.json");
    run(GovernanceArgs {
        command: GovernanceCommand::Sign {
            inputs: inputs.clone(),
            body: body_path,
            wallet: None,
            key_file: Some(dir.path("key.json")),
            output: signed_path.clone(),
        },
    })
    .await
    .unwrap();
    run(GovernanceArgs {
        command: GovernanceCommand::Inspect {
            inputs: inputs.clone(),
            body: None,
            signed: Some(signed_path.clone()),
        },
    })
    .await
    .unwrap();
    read_json(&signed_path, false).unwrap()
}

#[test]
fn propose_requires_exactly_one_change_and_an_owner_only_for_registry_add() {
    let common = [
        "dytallix",
        "governance",
        "propose",
        "--profile",
        "p",
        "--account",
        "a",
        "--context",
        "c",
        "--governance-profile",
        "g",
        "--expiry-height",
        "100",
        "--gas-limit",
        "100",
        "--maximum-fee-udrt",
        "200",
        "--output",
        "o",
    ];
    let parse = |extra: &[&str]| {
        let mut args = common.to_vec();
        args.extend(extra);
        crate::Cli::try_parse_from(args)
    };
    assert!(parse(&[]).is_err());
    assert!(parse(&["--max-active", "16"]).is_ok());
    assert!(parse(&["--max-active", "016"]).is_err());
    assert!(parse(&["--max-active", "16", "--registry-remove", "v1"]).is_err());
    assert!(parse(&["--registry-add", "v1"]).is_err());
    assert!(parse(&["--registry-add", "v1", "--owner", "x"]).is_ok());
    let vote = |choice: &str| {
        crate::Cli::try_parse_from([
            "dytallix",
            "governance",
            "vote",
            "--profile",
            "p",
            "--account",
            "a",
            "--context",
            "c",
            "--governance-profile",
            "g",
            "--proposal-id",
            "3",
            "--choice",
            choice,
            "--expiry-height",
            "100",
            "--gas-limit",
            "100",
            "--maximum-fee-udrt",
            "200",
            "--output",
            "o",
        ])
    };
    for choice in ["yes", "no", "no-with-veto", "abstain"] {
        assert!(vote(choice).is_ok(), "{choice}");
    }
    assert!(vote("veto").is_err());
    #[cfg(feature = "legacy-network")]
    assert!(crate::Cli::try_parse_from(["dytallix", "governance", "legacy", "proposals"]).is_ok());
}

#[tokio::test]
async fn every_governance_action_prepares_signs_and_inspects_offline() {
    let dir = Scratch::new();
    let f = fixture();
    let inputs = save(&dir, &f);
    save_key(&dir);
    let fees = dir.path("fees.json");
    let base = &f.inputs.profile.base;
    let values = FeeValues {
        gas_price: base.gas_price,
        transaction_overhead: base.transaction_overhead,
        receipt_metadata_cost: base.receipt_metadata_cost,
        wire_byte_cost: base.wire_byte_cost,
        read_byte_cost: base.read_byte_cost,
        write_byte_cost: base.write_byte_cost,
        action_costs: base.action_costs,
        signature_costs: base.signature_costs.clone(),
        validator_proof_costs: base.validator_proof_costs.clone(),
        governance_action_costs: f.inputs.profile.governance_action_costs,
        account_creation_fee_udrt: base.account_creation_fee_udrt,
    };
    write_json(&fees, &values).unwrap();
    let registry = RegistryChange::Add {
        validator_id: "v1".into(),
        owner: f.address.clone(),
    };
    let cases = [
        (
            ChangeArgs {
                max_active: Some(16),
                ..change()
            },
            None,
            ParameterChange::MaxActive(16).action_data(),
            CLASS_PARAMETER_CHANGE,
        ),
        (
            ChangeArgs {
                fees: Some(fees),
                ..change()
            },
            None,
            ParameterChange::Fees(values).action_data(),
            CLASS_PARAMETER_CHANGE,
        ),
        (
            ChangeArgs {
                registry_add: Some("v1".into()),
                ..change()
            },
            Some(f.address.clone()),
            registry.action_data(),
            CLASS_VALIDATOR_REGISTRY,
        ),
    ];
    for (index, (change, owner, data, class)) in cases.into_iter().enumerate() {
        let command = GovernanceCommand::Propose {
            inputs: inputs.clone(),
            change,
            owner,
            body: body(&dir, &format!("proposal-{index}.json")),
        };
        let signed = signed(&dir, &inputs, command).await;
        let [Action::GovernanceProposal {
            proposal_id,
            action_class,
            action_data,
            ..
        }] = signed.body.actions.as_slice()
        else {
            panic!("expected one proposal")
        };
        // The captured view's next ID, never a caller's guess.
        assert_eq!((*proposal_id, *action_class), (3, class));
        assert_eq!(action_data, &data);
    }
    let bad_owner = GovernanceCommand::Propose {
        inputs: inputs.clone(),
        change: ChangeArgs {
            registry_add: Some("v1".into()),
            ..change()
        },
        owner: Some("dyt1notanaddress".into()),
        body: body(&dir, "bad-owner.json"),
    };
    assert!(run(GovernanceArgs { command: bad_owner }).await.is_err());
    let stray_owner = GovernanceCommand::Propose {
        inputs: inputs.clone(),
        change: ChangeArgs {
            max_active: Some(16),
            ..change()
        },
        owner: Some(f.address.clone()),
        body: body(&dir, "stray-owner.json"),
    };
    let error = run(GovernanceArgs {
        command: stray_owner,
    })
    .await
    .unwrap_err();
    assert!(
        error.to_string().contains("--owner applies only"),
        "{error}"
    );
    let deposit = GovernanceCommand::Deposit {
        inputs: inputs.clone(),
        proposal_id: 2,
        amount_udgt: 250,
        body: body(&dir, "deposit.json"),
    };
    let signed_deposit = signed(&dir, &inputs, deposit).await;
    assert_eq!(
        signed_deposit.body.actions,
        vec![governance::deposit(2, 250)]
    );
    let vote = GovernanceCommand::Vote {
        inputs: inputs.clone(),
        proposal_id: 2,
        choice: Choice::NoWithVeto,
        body: body(&dir, "vote.json"),
    };
    let signed_vote = signed(&dir, &inputs, vote).await;
    assert_eq!(
        signed_vote.body.actions,
        vec![governance::vote(2, VoteChoice::NoWithVeto)]
    );
    let summary = describe(&signed_vote.body, &f.inputs.profile, "signed_offline").unwrap();
    assert_eq!(summary["committed"], false);
    assert_eq!(summary["rule_failures_are_charged"], true);
}

#[tokio::test]
async fn refresh_accepts_a_later_head_with_unchanged_authority_and_profile() {
    let f = fixture();
    let dir = Scratch::new();
    let inputs = save(&dir, &f);
    save_key(&dir);
    let command = GovernanceCommand::Propose {
        inputs: inputs.clone(),
        change: ChangeArgs {
            max_active: Some(16),
            ..change()
        },
        owner: None,
        body: body(&dir, "proposal.json"),
    };
    let signed = signed(&dir, &inputs, command).await;
    let later = CommittedContext {
        height: 45,
        app_hash: [9; 32],
        ..f.inputs.context.committed.clone()
    };
    let views = |context: &CommittedContext| {
        let mut profile = f.inputs.view.clone();
        profile.context = context.clone();
        let mut governance_view = f.inputs.governance.clone();
        governance_view.context = context.clone();
        let mut account = f.account.clone();
        account.context = context.clone();
        (profile, governance_view, account)
    };
    let (p, g, a) = views(&later);
    refresh(&f.inputs, &signed, &p, &g, &a).unwrap();
    // The expiry is checked at the refreshed height.
    let (p, g, a) = views(&CommittedContext {
        height: 99,
        ..later.clone()
    });
    assert!(refresh(&f.inputs, &signed, &p, &g, &a).is_err());
    let (p, g, a) = views(&CommittedContext {
        height: 39,
        ..later.clone()
    });
    assert!(refresh(&f.inputs, &signed, &p, &g, &a).is_err());
    let (p, g, a) = views(&CommittedContext {
        chain_id: "other".into(),
        ..later.clone()
    });
    assert!(refresh(&f.inputs, &signed, &p, &g, &a).is_err());
    let (p, mut g, a) = views(&later);
    g.next_proposal_id = 4;
    let error = refresh(&f.inputs, &signed, &p, &g, &a).unwrap_err();
    assert!(error.to_string().contains("is taken"), "{error}");
    let (p, g, mut a) = views(&later);
    a.spending_nonce += 1;
    assert!(refresh(&f.inputs, &signed, &p, &g, &a).is_err());
    let (p, mut g, a) = views(&later);
    g.fee_profile.as_mut().unwrap().governance_action_costs[0] += 1;
    assert!(refresh(&f.inputs, &signed, &p, &g, &a).is_err());
    let (mut p, g, a) = views(&later);
    p.context.height += 1;
    let error = refresh(&f.inputs, &signed, &p, &g, &a).unwrap_err();
    assert!(error.to_string().contains("advanced"), "{error}");
}
