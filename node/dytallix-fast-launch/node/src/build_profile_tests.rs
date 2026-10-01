use super::*;
use crate::consensus_settlement::ConsensusConfig;
use crate::runtime::issuance_timing::TimingGenesis;
use std::path::PathBuf;

/// The committed rehearsal configuration (development profiles).
fn rehearsal_config() -> ConsensusConfig {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/mainnet-preparation/fixtures/genesis-rehearsal/application-config.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn rehearsal_timing() -> TimingGenesis {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/mainnet-preparation/fixtures/genesis-rehearsal/native-genesis.json");
    let native: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    serde_json::from_value(native["adaptive_issuance"].clone()).unwrap()
}

/// The rehearsal configuration renamed to the production profiles, without the
/// root controls (step A4), with the validator-proof digest recomputed.
fn production_named(mut config: ConsensusConfig) -> ConsensusConfig {
    config.profile = "cometbft-production-v1".into();
    let lifecycle = config.lifecycle.as_mut().unwrap();
    lifecycle.profile = "cometbft-lifecycle-production-v1".into();
    let digest = {
        // The digest binds the lifecycle role; compute it as the node does,
        // without its build check on the profile name.
        let mut bytes = b"DYTALLIX/ORDINARY-VALIDATOR-PROOF-PROFILE\0".to_vec();
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&1952u32.to_be_bytes());
        bytes.extend_from_slice(&3309u32.to_be_bytes());
        bytes.extend_from_slice(b"PURE-ML-DSA/EMPTY-CONTEXT\0");
        let mut role = lifecycle.clone();
        role.approved_operators.clear();
        role.min_self_bond = 0;
        role.max_active = 0;
        let json = serde_json::to_vec(&role).unwrap();
        bytes.extend_from_slice(&(json.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&json);
        let digest: [u8; 32] = sha3::Digest::finalize(sha3::Digest::chain_update(
            sha3::Sha3_256::default(),
            &bytes,
        ))
        .into();
        digest
    };
    let penalty = config.penalty.as_mut().unwrap();
    penalty.profile = "cometbft-production-v1".into();
    penalty.production_activation = true;
    let ordinary = config.ordinary.as_mut().unwrap();
    ordinary.fee_profile.validator_proof_profile_digest = digest;
    config.governance.as_mut().unwrap().fee_profile.base = ordinary.fee_profile.clone();
    config.emergency = None;
    config.upgrade = None;
    config.release_handover = None;
    config
}

fn error(config: &ConsensusConfig) -> String {
    format!("{:#}", config.validate().unwrap_err())
}

#[test]
fn chain_ids_naming_mainnet_follow_the_build() {
    assert!(chain_id_allowed("dytallix-rehearsal-1"));
    assert_eq!(chain_id_allowed("dytallix-mainnet-1"), PRODUCTION);
    assert_eq!(chain_id_allowed("Production-Net"), PRODUCTION);
}

#[cfg(not(feature = "production"))]
mod development {
    use super::*;

    #[test]
    fn a_development_build_runs_only_development_profiles() {
        assert_eq!(
            consensus_profiles().collect::<Vec<_>>(),
            [
                "cometbft-local-qualification",
                "cometbft-lifecycle-local-qualification",
                "cometbft-penalty-local-qualification"
            ]
        );
        assert!(development_entry().is_ok() && production_open().is_ok());
        assert!(rehearsal_config().validate().is_ok());
    }

    #[test]
    fn a_development_build_refuses_production_profiles() {
        let error = error(&production_named(rehearsal_config()));
        assert!(
            error.contains("Production or unsupported engine profile is disabled"),
            "{error}"
        );
        let mut config = rehearsal_config();
        config.lifecycle.as_mut().unwrap().profile = "cometbft-lifecycle-production-v1".into();
        assert!(config.validate().is_err());
        let mut config = rehearsal_config();
        config.penalty.as_mut().unwrap().production_activation = true;
        let error = format!("{:#}", config.validate().unwrap_err());
        assert!(error.contains("production builds only"), "{error}");
        let mut timing = rehearsal_timing();
        timing.profile = "production".into();
        assert!(timing.validate().is_err());
    }

    #[test]
    fn a_development_build_refuses_a_mainnet_chain_id() {
        let text = serde_json::to_string(&rehearsal_config())
            .unwrap()
            .replace("dytallix-rehearsal-1", "dytallix-mainnet-1");
        let config: ConsensusConfig = serde_json::from_str(&text).unwrap();
        let error = error(&config);
        assert!(error.contains("naming mainnet or production"), "{error}");
    }
}

#[cfg(feature = "production")]
mod production {
    use super::*;
    use crate::consensus_settlement::ConsensusApplication;

    #[test]
    fn a_production_build_runs_only_production_profiles() {
        assert_eq!(
            consensus_profiles().collect::<Vec<_>>(),
            ["cometbft-production-v1"]
        );
        assert!(development_entry().is_err() && production_open().is_err());
        assert!(production_named(rehearsal_config()).validate().is_ok());
        let mut timing = rehearsal_timing();
        timing.profile = "production".into();
        assert!(timing.validate().is_ok());
    }

    #[test]
    fn a_production_build_refuses_development_profiles() {
        let error = error(&rehearsal_config());
        assert!(error.contains("refuses development"), "{error}");
        let mut config = production_named(rehearsal_config());
        config.penalty.as_mut().unwrap().production_activation = false;
        assert!(config.validate().is_err());
        assert!(rehearsal_timing().validate().is_err());
    }

    #[test]
    fn production_carries_the_launch_scope_and_waits_for_root_controls() {
        let mut config = production_named(rehearsal_config());
        config.governance = None;
        assert!(error(&config).contains("recovery, ordinary and governance"));
        let mut config = production_named(rehearsal_config());
        config.emergency = rehearsal_config().emergency;
        assert!(error(&config).contains("step A4"));
    }

    #[test]
    fn a_production_build_opens_no_chain_before_the_root_path() {
        let config = production_named(rehearsal_config());
        let dir = tempfile::tempdir().unwrap();
        let error = ConsensusApplication::open(dir.path().join("db"), config, b"{}".to_vec())
            .err()
            .map(|e| format!("{e:#}"))
            .unwrap();
        assert!(error.contains("root-signed genesis"), "{error}");
    }
}
