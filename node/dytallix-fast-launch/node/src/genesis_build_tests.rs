use super::*;
use serde_json::Value;
use std::path::PathBuf;

/// The committed rehearsal this build writes: synthetic records resolved with
/// the approved values and labeled proposals (tools/mainnet-preparation). A
/// production build writes the production-profile rehearsal on the staging
/// chain (A7).
fn rehearsal() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/mainnet-preparation/fixtures")
        .join(if crate::build_profile::PRODUCTION {
            "genesis-production-rehearsal"
        } else {
            "genesis-rehearsal"
        })
}

fn inputs_bytes() -> Vec<u8> {
    std::fs::read(rehearsal().join("inputs.json")).unwrap()
}

fn built() -> Built {
    let bytes = inputs_bytes();
    build(&read_inputs(&bytes).unwrap(), &bytes).unwrap()
}

/// Build from the rehearsal inputs after `change`, and return the error text.
fn refused(change: impl FnOnce(&mut Value)) -> String {
    let mut value: Value = serde_json::from_slice(&inputs_bytes()).unwrap();
    change(&mut value);
    let bytes = serde_json::to_vec(&value).unwrap();
    match read_inputs(&bytes).and_then(|inputs| build(&inputs, &bytes)) {
        Ok(_) => panic!("the changed inputs were accepted"),
        Err(error) => format!("{error:#}"),
    }
}

#[test]
fn the_rehearsal_rebuilds_byte_for_byte() {
    let b = built();
    for (name, bytes) in [
        ("native-genesis.json", &b.native_genesis),
        ("application-config.json", &b.application_config),
        ("genesis.json", &b.engine_genesis),
        ("BUILD_MANIFEST.json", &b.manifest),
    ] {
        let committed = std::fs::read(rehearsal().join(name)).unwrap();
        assert!(
            &committed == bytes,
            "{name} differs from the committed rehearsal; rebuild it with dytallix-genesis-build"
        );
    }
}

#[cfg(not(feature = "production"))]
#[test]
fn the_rehearsal_starts_a_chain() {
    let b = built();
    let dir = tempfile::tempdir().unwrap();
    let app_hash = verify(&b, &dir.path().join("db")).unwrap();
    assert_eq!(app_hash.len(), 64);
    // The application binds the exact native genesis bytes.
    assert_eq!(
        b.config.app_state_sha256,
        hex::encode(Sha256::digest(&b.native_genesis))
    );
    assert!(!b.native_genesis.ends_with(b"\n"));
    let manifest: Value = serde_json::from_slice(&b.manifest).unwrap();
    assert_eq!(manifest["production"], false);
    assert_eq!(manifest["mode"], "rehearsal");
}

#[test]
fn derived_values_follow_their_rules() {
    let b = built();
    let c = &b.config;
    let ordinary = c.ordinary.as_ref().unwrap();
    let fees = &ordinary.fee_profile;
    let governance = c.governance.as_ref().unwrap();
    let recovery = &c.recovery.as_ref().unwrap().profile;
    // A basic transfer pays the minimum-gas floor: 1 DRT (P01, 30 September 2026).
    assert_eq!(
        u128::from(fees.minimum_gas) * u128::from(fees.gas_price),
        1_000_000
    );
    assert_eq!(c.gas_price, fees.gas_price);
    assert_eq!(
        fees.max_fee_cap,
        u128::from(fees.max_transaction_gas)
            * u128::from(governance.parameter_bounds.gas_price.max)
    );
    assert_eq!(
        recovery.max_fee_cap,
        u128::from(recovery.max_transaction_gas) * u128::from(recovery.gas_price)
    );
    assert_eq!(
        ordinary.max_transport_bytes,
        u64::from(fees.limits.max_wire_bytes).div_ceil(3) * 4 + 43
    );
    assert_eq!(governance.fee_profile.base, *fees);
    // Power is the bonded stake; validator-1 also holds a delegation.
    let power: BTreeMap<_, _> = c
        .validators
        .iter()
        .map(|v| (v.reward_address.as_str(), v.power))
        .collect();
    assert_eq!(power["validator-1"], 1_100_000_000_000);
    assert_eq!(power["validator-2"], 100_000_000_000);
    // Root controls fit one transaction and carry their threshold.
    let emergency = c.emergency.as_ref().unwrap();
    assert_eq!(emergency.max_control_bytes, c.max_tx_bytes);
    assert_eq!(
        emergency.v2.as_ref().unwrap().genesis_sha256,
        c.app_state_sha256
    );
    // Upgrade and handover schema 2 in both builds (A3, A7); the upgrade
    // custodians hold the handover authority (P01, 30 September 2026).
    let upgrade = c.upgrade.as_ref().unwrap().as_v2().unwrap();
    assert_eq!((upgrade.max_signatures, upgrade.min_notice_blocks), (3, 120_960));
    let handover = c.release_handover.as_ref().unwrap();
    assert_eq!(handover.schema, 2);
    assert_eq!(
        (&handover.authority, handover.authority_epoch),
        (&upgrade.authority, upgrade.authority_epoch)
    );
    assert_eq!(handover.v2.as_ref().unwrap().min_notice_blocks, 120_960);
    // Each build writes its own names and control flags.
    let production = crate::build_profile::PRODUCTION;
    assert_eq!(c.profile, crate::build_profile::PENALTY_PROFILE);
    assert_eq!(c.lifecycle.as_ref().unwrap().profile, crate::build_profile::LIFECYCLE_PROFILE);
    assert_eq!(c.penalty.as_ref().unwrap().production_activation, production);
    assert_eq!((emergency.development_only, handover.development_only), (!production, !production));
    // The engine's evidence limits equal the lifecycle's, in whole seconds.
    let engine: Value = serde_json::from_slice(&b.engine_genesis).unwrap();
    let lifecycle = c.lifecycle.as_ref().unwrap();
    assert_eq!(
        engine["consensus_params"]["evidence"]["max_age_num_blocks"],
        lifecycle.evidence_max_age_blocks.to_string()
    );
    assert_eq!(
        engine["consensus_params"]["evidence"]["max_age_duration"],
        (lifecycle.evidence_max_age_seconds * 1_000_000_000).to_string()
    );
    let native: Value = serde_json::from_slice(&b.native_genesis).unwrap();
    assert_eq!(native["reward_v2"]["max_validators"], 32);
    for section in ["reward_v2", "adaptive_issuance"] {
        assert_eq!(native[section]["profile"], crate::build_profile::MONETARY_PROFILE);
    }
    let manifest: Value = serde_json::from_slice(&b.manifest).unwrap();
    assert_eq!((&manifest["mode"], &manifest["production"]), (&Value::from(MODE), &Value::from(production)));
    assert_eq!(engine["app_state"], native);
}

#[test]
fn the_dgt_total_is_the_whole_supply() {
    let error = refused(|v| v["accounts"][4]["udgt"] = "199599999999999".into());
    assert!(error.contains("exactly 1,000,000,000 DGT"), "{error}");
}

#[test]
fn account_drt_equals_the_approved_bootstrap() {
    let error = refused(|v| v["drt_bootstrap_total_udrt"] = "999999999999".into());
    assert!(error.contains("bootstrap"), "{error}");
}

#[cfg(not(feature = "production"))]
#[test]
fn a_development_build_refuses_a_chain_id_naming_mainnet() {
    let error = refused(|v| v["chain_id"] = "dytallix-mainnet-1".into());
    assert!(error.contains("mainnet or production"), "{error}");
}

#[cfg(feature = "production")]
#[test]
fn a_production_build_may_name_mainnet() {
    let mut value: Value = serde_json::from_slice(&inputs_bytes()).unwrap();
    value["chain_id"] = "dytallix-mainnet-1".into();
    let bytes = serde_json::to_vec(&value).unwrap();
    let b = build(&read_inputs(&bytes).unwrap(), &bytes).unwrap();
    assert_eq!(b.config.chain_id, "dytallix-mainnet-1");
}

#[test]
fn each_build_writes_only_its_own_mode() {
    let other = if crate::build_profile::PRODUCTION {
        MODE_REHEARSAL
    } else {
        MODE_PRODUCTION
    };
    let error = refused(|v| v["mode"] = other.into());
    assert!(error.contains(&format!("only the {MODE} mode")), "{error}");
}

#[test]
fn the_root_controls_take_the_handover_authority_from_the_upgrade() {
    // The inputs carry no separate handover authority.
    let error = refused(|v| v["root"]["handover"]["authority"] = serde_json::json!({"keys": [], "threshold": 3}));
    assert!(error.contains("unknown field"), "{error}");
    // A production genesis carries every root control (P01, 1 October 2026).
    if crate::build_profile::PRODUCTION {
        let error = refused(|v| v["root"] = Value::Null);
        assert!(error.contains("needs the root controls"), "{error}");
    }
}

#[cfg(feature = "production")]
#[test]
fn a_production_genesis_has_no_unsigned_start() {
    let dir = tempfile::tempdir().unwrap();
    let error = verify(&built(), &dir.path().join("db")).unwrap_err();
    assert!(format!("{error:#}").contains("no development entry points"), "{error:#}");
}

#[test]
fn unknown_fields_are_refused() {
    let error = refused(|v| v["ordinary"]["private_key"] = "not a real secret".into());
    assert!(error.contains("unknown field"), "{error}");
}

#[test]
fn amounts_are_canonical_decimal_strings() {
    let error = refused(|v| v["accounts"][0]["udrt"] = "0996000000000".into());
    assert!(error.contains("canonical decimal"), "{error}");
}

#[test]
fn a_self_bond_below_the_minimum_is_refused() {
    let error = refused(|v| v["validators"][0]["self_bond_udgt"] = "99999999999".into());
    assert!(error.contains("below min_self_bond"), "{error}");
}

#[test]
fn vesting_covers_the_whole_balance() {
    let error = refused(|v| v["accounts"][1]["vesting"]["total_amount"] = "1".into());
    assert!(error.contains("whole DGT balance"), "{error}");
}

#[test]
fn two_accounts_cannot_share_an_origin_key() {
    let error = refused(|v| {
        let key = v["accounts"][0]["origin_public_key_base64"].clone();
        v["accounts"][1]["origin_public_key_base64"] = key;
    });
    assert!(error.contains("share an origin key"), "{error}");
}

#[test]
fn delegations_name_a_genesis_validator() {
    let error = refused(|v| v["delegations"][0]["validator_id"] = "validator-9".into());
    assert!(error.contains("unknown validator"), "{error}");
}

#[test]
fn genesis_time_is_explicit_whole_seconds() {
    for bad in [
        "2027-01-01T00:00:00.5Z",
        "2027-01-01 00:00:00Z",
        "2027-13-01T00:00:00Z",
    ] {
        let error = refused(|v| v["genesis_time"] = bad.into());
        assert!(error.contains("genesis_time"), "{bad}: {error}");
    }
}

#[test]
fn the_node_refuses_a_genesis_outside_its_governance_bounds() {
    // E05-a rule 4: the genesis self-bond lies within its governed bounds.
    let error = refused(|v| {
        v["governance"]["bounds"]["min_self_bond"]["min"] = "200000000000".into();
        v["governance"]["bounds"]["min_self_bond"]["max"] = "300000000000".into();
    });
    assert!(error.to_lowercase().contains("bound"), "{error}");
}

#[test]
fn the_reference_send_fee_bound_holds_at_genesis() {
    // The rehearsal's approved profile prices the reference basic Send at
    // 1 DRT (P01, 30 September 2026); a bound that excludes it is refused.
    for (end, value) in [("max", "999999"), ("min", "1000001")] {
        let error = refused(|v| v["governance"]["bounds"]["reference_send_fee_udrt"][end] = value.into());
        assert!(error.contains("governance bounds"), "{end}: {error}");
    }
    let error = refused(|v| {
        v["governance"]["bounds"].as_object_mut().unwrap().remove("reference_send_fee_udrt");
    });
    assert!(error.contains("reference_send_fee_udrt"), "{error}");
}

#[test]
fn each_genesis_fits_the_bound_of_its_reader() {
    genesis_sizes(&vec![b' '; MAX_GENESIS_BYTES], &vec![b' '; MAX_ENGINE_GENESIS_BYTES]).unwrap();
    assert!(genesis_sizes(&vec![b' '; MAX_GENESIS_BYTES + 1], b"{}").is_err());
    assert!(genesis_sizes(b"{}", &vec![b' '; MAX_ENGINE_GENESIS_BYTES + 1]).is_err());
}
