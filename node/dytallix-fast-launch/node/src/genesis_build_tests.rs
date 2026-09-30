use super::*;
use serde_json::Value;
use std::path::PathBuf;

/// The committed rehearsal: synthetic records resolved with the approved
/// values and labeled proposals (tools/mainnet-preparation).
fn rehearsal() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/mainnet-preparation/fixtures/genesis-rehearsal")
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
    assert_eq!(c.upgrade.as_ref().unwrap().max_signatures, 3);
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

#[test]
fn a_chain_id_naming_mainnet_is_refused_until_activation() {
    let error = refused(|v| v["chain_id"] = "dytallix-mainnet-1".into());
    assert!(error.contains("mainnet or production"), "{error}");
}

#[test]
fn only_rehearsals_exist() {
    let error = refused(|v| v["mode"] = "production".into());
    assert!(error.contains("rehearsal"), "{error}");
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
