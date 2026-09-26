//! The mock backend must not activate sponsored recovery.
use super::*;
use crate::consensus_settlement::{ConsensusConfig, ValidatorConfig};
use base64::{engine::general_purpose::STANDARD, Engine};
use dytallix_protocol_types::recovery::{RecoveryConfig, RecoveryDomain};

fn book() -> RecoveryBook {
    let profile = FeeProfile {
        version: 1,
        activation_height: 1,
        denomination: "udrt".into(),
        gas_price: 1,
        minimum_gas: 10,
        max_transaction_gas: 100_000,
        max_block_gas: 200_000,
        max_block_recovery_bytes: 262_144,
        max_block_recovery_signatures: 16,
        max_fee_cap: 100_000,
        max_pending_accounts: 2,
        max_due_expiry_events_per_height: 2,
        mandatory_expiry_gas_budget: 20,
        expiry_event_gas_cost: 10,
        action_costs: [10; 9],
        wire_byte_cost: 1,
        read_byte_cost: 1,
        write_byte_cost: 1,
        signature_costs: BTreeMap::from([("mldsa65".into(), 10)]),
    };
    let account = RecoveryAccount {
        address: "local-owner".into(),
        recovery: RecoveryState::new(
            RecoveryDomain {
                network: 3,
                chain_id: "backend-test".into(),
                genesis_digest: [3; 32],
                account_id: [1; 32],
            },
            RecoveryConfig {
                timing_version: 1,
                recovery_delay: 2,
                finalization_window: 3,
                policy_delay: 2,
                policy_window: 3,
                submission_lifetime: 100,
                algorithms: BTreeMap::from([("mldsa65".into(), 1952)]),
            },
            KeyIdentity {
                algorithm: "mldsa65".into(),
                public_key: vec![1; 1952],
            },
            0,
        )
        .unwrap(),
        sponsor_nonce: 0,
    };
    RecoveryBook::new(profile, vec![account]).unwrap()
}

#[test]
fn missing_real_backend_rejects_block_activation_without_state_change() {
    let book = book();
    book.validate().unwrap();
    let before = book.encode().unwrap();
    let error = book.begin_block(1).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Recovery profile requires the FIPS 204 backend"
    );
    assert_eq!(book.encode().unwrap(), before);
    assert_eq!(book.last_height, 0);
    assert!(book.sponsor_receipts.is_empty());
}

#[test]
fn missing_real_backend_rejects_consensus_recovery_configuration() {
    let mut config = ConsensusConfig {
        profile: "cometbft-local-qualification".into(),
        engine: "cometbft-v0.40.0".into(),
        chain_id: "backend-test".into(),
        app_state_sha256: hex::encode([3; 32]),
        gas_price: 1,
        max_tx_bytes: 262_144,
        max_block_bytes: 1_048_576,
        max_txs: 100,
        validators: vec![ValidatorConfig {
            pubkey_type: "ml_dsa_65".into(),
            pubkey_base64: STANDARD.encode([2; 1952]),
            power: 1,
            reward_address: "local-validator".into(),
        }],
        lifecycle: None,
        penalty: None,
        recovery: None,
        ordinary: None,
        governance: None,
        emergency: None,
        upgrade: None,
        release_handover: None,
    };
    config.validate().unwrap();
    config.recovery = Some(book());
    let before = config.clone();
    assert_eq!(
        config.validate().unwrap_err().to_string(),
        "Recovery requires the real FIPS backend"
    );
    assert_eq!(config, before);
}
