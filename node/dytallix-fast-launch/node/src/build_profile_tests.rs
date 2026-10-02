use super::*;
use crate::consensus_settlement::ConsensusConfig;
use crate::runtime::issuance_timing::TimingGenesis;
use std::path::PathBuf;

/// The committed rehearsal configuration (development profiles).
pub(crate) fn rehearsal_config() -> ConsensusConfig {
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

/// The production builder's rehearsal on the staging chain (A7): the
/// production profiles and the three root controls at schema 2.
pub(crate) fn production_rehearsal_config() -> ConsensusConfig {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../tools/mainnet-preparation/fixtures/genesis-production-rehearsal/application-config.json",
    );
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn production_rehearsal_timing() -> TimingGenesis {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../tools/mainnet-preparation/fixtures/genesis-production-rehearsal/native-genesis.json",
    );
    let native: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    serde_json::from_value(native["adaptive_issuance"].clone()).unwrap()
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
        assert!(development_entry().is_ok());
        assert!(production_open(false).is_ok() && production_open(true).is_ok());
        assert!(rehearsal_config().validate().is_ok());
    }

    #[test]
    fn a_development_build_refuses_production_profiles() {
        let error = error(&production_rehearsal_config());
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
        assert!(production_rehearsal_timing().validate().is_err());
        // Production control policies are refused too (A4).
        let mut config = rehearsal_config();
        config.emergency.as_mut().unwrap().development_only = false;
        assert!(format!("{:#}", config.validate().unwrap_err()).contains("differs from this build"));
        let mut config = rehearsal_config();
        config
            .emergency
            .as_mut()
            .unwrap()
            .automatic_transition_policy =
            crate::emergency_freeze::AutomaticTransitionPolicy::ContinuePreviouslyApprovedRules;
        assert!(format!("{:#}", config.validate().unwrap_err()).contains("differs from this build"));
        let mut config = rehearsal_config();
        config.release_handover.as_mut().unwrap().development_only = false;
        assert!(format!("{:#}", config.validate().unwrap_err()).contains("differs from this build"));
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
        assert!(development_entry().is_err());
        assert!(production_open(false).is_err() && production_open(true).is_ok());
        assert!(production_rehearsal_config().validate().is_ok());
        assert!(production_rehearsal_timing().validate().is_ok());
    }

    #[test]
    fn a_production_build_refuses_development_profiles() {
        let error = error(&rehearsal_config());
        assert!(error.contains("refuses development"), "{error}");
        let mut config = production_rehearsal_config();
        config.penalty.as_mut().unwrap().production_activation = false;
        assert!(config.validate().is_err());
        assert!(rehearsal_timing().validate().is_err());
    }

    #[test]
    fn production_carries_the_launch_scope_and_the_root_controls() {
        let mut config = production_rehearsal_config();
        config.governance = None;
        assert!(error(&config).contains("recovery, ordinary and governance"));
        // P01, 1 October 2026: all three root controls, at schema 2.
        let cases: [(&str, fn(&mut ConsensusConfig)); 5] = [
            ("no emergency", |c| c.emergency = None),
            ("no upgrade", |c| c.upgrade = None),
            ("no handover", |c| c.release_handover = None),
            ("schema 1 upgrade", |c| {
                let Some(crate::upgrade::Policy::V2(v2)) = c.upgrade.clone() else {
                    unreachable!("the production rehearsal carries upgrade schema 2")
                };
                c.upgrade = Some(crate::upgrade::Policy::V1(crate::upgrade::v1::Policy {
                    schema: 1,
                    development_only: false,
                    chain_id: v2.chain_id,
                    genesis_sha256: v2.genesis_sha256,
                    source_release_sha512: v2.initial_release_sha512,
                    authority_epoch: v2.authority_epoch,
                    authority: v2.authority,
                    initial_sequence: v2.initial_sequence,
                    max_control_bytes: v2.max_control_bytes,
                    max_signatures: v2.max_signatures,
                    migration_bounds: crate::upgrade::v1::MigrationBounds {
                        max_receipts: v2.migration_bounds.max_receipts,
                        max_receipt_bytes: v2.migration_bounds.max_receipt_bytes,
                        max_write_bytes: v2.migration_bounds.max_write_bytes,
                    },
                }))
            }),
            ("emergency schema 1", |c| {
                c.emergency.as_mut().unwrap().v2 = None
            }),
        ];
        for (name, change) in cases {
            let mut config = production_rehearsal_config();
            change(&mut config);
            assert!(config.validate().is_err(), "accepted {name}");
        }
        // Development control policies are refused.
        let mut config = production_rehearsal_config();
        config.emergency = rehearsal_config().emergency;
        assert!(error(&config).contains("differs from this build"));
        let mut config = production_rehearsal_config();
        config.release_handover.as_mut().unwrap().development_only = true;
        assert!(error(&config).contains("differs from this build"));
    }

    #[test]
    fn a_production_build_opens_no_chain_without_the_threshold_root() {
        let config = production_rehearsal_config();
        let dir = tempfile::tempdir().unwrap();
        let error = ConsensusApplication::open(dir.path().join("db"), config, b"{}".to_vec())
            .err()
            .map(|e| format!("{e:#}"))
            .unwrap();
        assert!(error.contains("signed three of five"), "{error}");
    }
}
