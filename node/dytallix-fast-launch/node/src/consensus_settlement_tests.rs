//! Local application qualification. These tests do not establish engine consensus.
use super::*;
use crate::crypto::{ActivePQC, PQC};
use crate::runtime::issuance_timing::{EpochObservation, TimingState, TIMING_STATE_KEY};
use crate::storage::state::Storage;
use crate::types::{tx::Tx, Msg, SignedTx};
use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use fips204::{
    ml_dsa_65,
    traits::{KeyGen, SerDes},
};
use rocksdb::{IteratorMode, WriteBatch, WriteOptions};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const CHAIN: &str = "consensus-qualification";
const INITIAL_DRT: u128 = 1_000_000;

struct Inputs {
    config: ConsensusConfig,
    genesis: Vec<u8>,
    owner: String,
    secret: Vec<u8>,
    public: Vec<u8>,
}

impl Inputs {
    fn new() -> Self {
        let (secret, public) = ActivePQC::keypair();
        let owner = crate::addr::initial_address(
            crate::addr::AddressNetwork::Development,
            CHAIN,
            crate::addr::OriginKeyAlgorithm::MlDsa65,
            &public,
        )
        .unwrap();
        let (validator, _) = ml_dsa_65::KG::try_keygen_with_rng(&mut rand_core::OsRng).unwrap();
        let genesis = serde_json::to_vec(&serde_json::json!({
            "chain_id": CHAIN,
            "accounts": [{"address": owner,
                "balances": {"udgt": "1000", "udrt": INITIAL_DRT.to_string()},
                "vesting": {"kind": "unlocked"}}],
            "staking": {"delegations": [{"delegator": owner, "amount_udgt": "100"}]},
            "reward_v2": {"version": 2, "activation_height": 1, "decimals": 6,
                "profile": "development", "max_validators": 4, "max_positions": 8,
                "validators": [{"address": "validator-one", "active": true, "jailed": false}],
                "positions": [{"owner": owner, "validator": "validator-one", "amount_udgt": "100"}]},
            "adaptive_issuance": {"version": 1, "profile": "development", "decimals": 6,
                "epoch_blocks": 2, "initial_epoch_budget_udrt": "1001", "max_recorded_epochs": 8,
                "controller": {"target_ppm": 500000, "shock_threshold_ppm": 100000,
                    "volatility_threshold_ppm": 1000000, "window_samples": 2,
                    "integral_min": -2000000, "integral_max": 2000000,
                    "soft": {"proportional": 0, "integral": 0, "derivative": 0},
                    "hard": {"proportional": 0, "integral": 0, "derivative": 0},
                    "base_udrt": 1001, "min_udrt": 1001, "max_udrt": 1001}}
        })).unwrap();
        let config = serde_json::from_value(serde_json::json!({
            "profile": "cometbft-local-qualification", "engine": "cometbft-v0.40.0",
            "chain_id": CHAIN, "app_state_sha256": hex::encode(Sha256::digest(&genesis)),
            "gas_price": 1, "max_tx_bytes": 65536, "max_block_bytes": 1048576, "max_txs": 64,
            "validators": [{"pubkey_type": "ml_dsa_65", "pubkey_base64": B64.encode(validator.into_bytes()),
                "power": 10, "reward_address": "validator-one"}]
        })).unwrap();
        Self {
            config,
            genesis,
            owner,
            secret,
            public,
        }
    }

    fn open(&self, path: &std::path::Path) -> ConsensusApplication {
        ConsensusApplication::open(path, self.config.clone(), self.genesis.clone()).unwrap()
    }

    fn initialized(&self, path: &std::path::Path) -> ConsensusApplication {
        let mut app = self.open(path);
        app.init_chain(CHAIN, 1, &self.genesis, &self.config.validators)
            .unwrap();
        app
    }

    fn signed(&self, nonce: u64, msg: Msg) -> SignedTx {
        SignedTx::sign(
            Tx {
                chain_id: CHAIN.into(),
                nonce,
                msgs: vec![msg],
                fee: 50000,
                memo: "local consensus fixture".into(),
            },
            &self.secret,
            &self.public,
        )
        .unwrap()
    }

    fn send(&self) -> SignedTx {
        self.signed(
            0,
            Msg::Send {
                from: self.owner.clone(),
                to: "recipient".into(),
                denom: "udgt".into(),
                amount: 10,
            },
        )
    }
}

fn block(height: u64, txs: Vec<Vec<u8>>) -> FinalizedBlockInput {
    FinalizedBlockInput {
        misbehavior: Vec::new(),
        height,
        time_seconds: i64::try_from(height * 10).unwrap(),
        time_nanos: 17,
        hash: format!("{height:064x}"),
        txs,
    }
}

fn signed_wire(envelope: SignedTx) -> Vec<u8> {
    serde_json::to_vec(&WireTransaction::Signed { envelope }).unwrap()
}

#[test]
fn omitted_governance_config_preserves_legacy_bytes_and_null_rejects() {
    let config = Inputs::new().config;
    let bytes = serde_json::to_vec(&config).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value.get("governance").is_none());
    assert_eq!(
        serde_json::to_vec(&serde_json::from_slice::<ConsensusConfig>(&bytes).unwrap()).unwrap(),
        bytes
    );
    value["governance"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<ConsensusConfig>(value).is_err());
}

#[test]
fn governance_v3_transport_requires_config_and_type_discriminator() {
    let config = Inputs::new().config;
    let typed = serde_json::to_vec(&serde_json::json!({
        "type": "ordinary_v3",
        "envelope_base64": ""
    }))
    .unwrap();
    assert!(wire(&config, &typed)
        .unwrap_err()
        .to_string()
        .contains("Governance profile is not configured"));
    let bypass = serde_json::to_vec(&serde_json::json!({
        "kind": "ordinary_v3",
        "envelope_base64": ""
    }))
    .unwrap();
    assert!(wire(&config, &bypass)
        .unwrap_err()
        .to_string()
        .contains("Ordinary transport requires type discriminator"));
}

fn observation_wire(parent_hash: String) -> Vec<u8> {
    serde_json::to_vec(&WireTransaction::EpochObservation {
        observation: EpochObservation {
            epoch: 0,
            utilization_ppm: 500000,
            volatility_ppm: 0,
            first_height: 1,
            last_height: 2,
            parent_hash,
        },
    })
    .unwrap()
}

fn data(app: &ConsensusApplication) -> BTreeMap<Vec<u8>, Vec<u8>> {
    app.storage
        .db
        .iterator(IteratorMode::Start)
        .map(|entry| {
            let (key, value) = entry.unwrap();
            (key.to_vec(), value.to_vec())
        })
        .collect()
}

fn timing(app: &ConsensusApplication) -> TimingState {
    TimingState::decode(&app.storage.db.get(TIMING_STATE_KEY).unwrap().unwrap()).unwrap()
}

fn balances(app: &ConsensusApplication, owner: &str) -> BTreeMap<String, u128> {
    app.storage
        .db
        .get(format!("acct:balances:{owner}"))
        .unwrap()
        .map(|bytes| bincode::deserialize(&bytes).unwrap())
        .unwrap_or_default()
}

fn write_sync(storage: &Storage, batch: WriteBatch) -> Result<()> {
    let mut options = WriteOptions::default();
    options.set_sync(true);
    storage.db.write_opt(batch, &options)?;
    Ok(())
}

fn commit_empty(app: &mut ConsensusApplication, height: u64) -> FinalizeResult {
    let result = app.finalize_block(block(height, vec![])).unwrap();
    app.commit().unwrap();
    result
}

#[test]
fn independent_databases_commit_identical_inputs_and_exact_block_issuance() {
    let inputs = Inputs::new();
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let mut a = inputs.initialized(&a_dir.path().join("db"));
    let mut b = inputs.initialized(&b_dir.path().join("db"));
    let genesis_root = a.storage.db.get(GENESIS_APP_HASH_KEY).unwrap().unwrap();
    assert_eq!(genesis_root, a.info().unwrap().app_hash.as_bytes());
    let signed_send = signed_wire(inputs.send());
    for height in 1..=2 {
        let before = data(&a);
        let info = a.info().unwrap();
        let txs = if height == 1 {
            vec![signed_send.clone()]
        } else {
            vec![]
        };
        let a_result = a.finalize_block(block(height, txs.clone())).unwrap();
        let b_result = b.finalize_block(block(height, txs)).unwrap();
        assert_eq!(a_result, b_result);
        assert_eq!(
            data(&a),
            before,
            "FinalizeBlock must not publish staged state"
        );
        assert_eq!(a.info().unwrap(), info);
        assert_eq!(a.commit().unwrap(), b.commit().unwrap());
        assert_eq!(data(&a), data(&b));
        assert_eq!(
            a.storage.db.get(GENESIS_APP_HASH_KEY).unwrap().unwrap(),
            genesis_root,
            "Committed blocks must preserve the genesis application commitment"
        );
        assert_eq!(
            timing(&a).total_issued.total().unwrap(),
            if height == 1 { 501 } else { 1001 }
        );
        assert_eq!(
            crate::supply::inspect_native(&a.storage).unwrap(),
            crate::supply::inspect_native(&b.storage).unwrap()
        );
    }
    let supply = crate::supply::inspect_native(&a.storage).unwrap();
    assert_eq!(supply.drt.pools["validator_rewards"], 400);
    assert_eq!(supply.drt.pools["staking_rewards"], 300);
    assert_eq!(supply.drt.pools["treasury"], 300);
    assert_eq!(supply.drt.pools["issuance_reserve"], 1);
    assert_eq!(supply.drt.total, INITIAL_DRT + 1001);
    assert_eq!(balances(&a, "recipient")["udgt"], 10);
}

#[test]
fn commit_failure_before_write_preserves_all_state_and_can_retry() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = inputs.initialized(&dir.path().join("db"));
    let before = data(&app);
    let info = app.info().unwrap();
    let result = app.finalize_block(block(1, vec![])).unwrap();
    assert!(app
        .commit_with(|_, _| anyhow::bail!("injected before write"))
        .is_err());
    assert_eq!(data(&app), before);
    assert_eq!(app.info().unwrap(), info);
    assert_eq!(app.finalize_block(block(1, vec![])).unwrap(), result);
    app.commit().unwrap();
    assert_eq!(timing(&app).total_issued.total().unwrap(), 501);
}

#[test]
fn lost_commit_acknowledgement_reopens_without_duplicate_issuance() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = inputs.initialized(&path);
    let result = app.finalize_block(block(1, vec![])).unwrap();
    assert!(app
        .commit_with(|storage, batch| {
            write_sync(storage, batch)?;
            anyhow::bail!("injected lost acknowledgement")
        })
        .is_err());
    let committed = data(&app);
    drop(app);
    let mut reopened = inputs.open(&path);
    assert_eq!(reopened.info().unwrap().height, 1);
    assert_eq!(reopened.info().unwrap().app_hash, result.app_hash);
    assert_eq!(reopened.finalize_block(block(1, vec![])).unwrap(), result);
    reopened.commit().unwrap();
    assert_eq!(data(&reopened), committed);
    assert_eq!(timing(&reopened).total_issued.total().unwrap(), 501);
    commit_empty(&mut reopened, 2);
    assert_eq!(timing(&reopened).total_issued.total().unwrap(), 1001);
}

#[test]
fn restart_before_commit_recomputes_same_result_without_publishing() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = inputs.initialized(&path);
    let before = data(&app);
    let input = block(1, vec![signed_wire(inputs.send())]);
    let result = app.finalize_block(input.clone()).unwrap();
    drop(app);
    let mut reopened = inputs.open(&path);
    assert_eq!(data(&reopened), before);
    assert_eq!(reopened.info().unwrap().height, 0);
    assert_eq!(reopened.finalize_block(input).unwrap(), result);
    reopened.commit().unwrap();
    assert_eq!(balances(&reopened, "recipient")["udgt"], 10);
}

#[test]
fn altered_pending_input_rejects_without_replacing_the_first_result() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = inputs.initialized(&dir.path().join("db"));
    let before = data(&app);
    let original = block(1, vec![]);
    let result = app.finalize_block(original.clone()).unwrap();
    for change in 0..4 {
        let mut changed = original.clone();
        match change {
            0 => changed.hash = "ab".repeat(32),
            1 => changed.time_seconds += 1,
            2 => changed.time_nanos += 1,
            _ => changed.txs.push(b"invalid".to_vec()),
        }
        assert!(app.finalize_block(changed).is_err());
        assert_eq!(data(&app), before);
    }
    assert_eq!(app.finalize_block(original).unwrap(), result);
    assert_eq!(app.commit().unwrap().app_hash, result.app_hash);
}

#[test]
fn epoch_boundary_requires_exact_prior_engine_hash_before_any_write() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = inputs.initialized(&dir.path().join("db"));
    commit_empty(&mut app, 1);
    commit_empty(&mut app, 2);
    let before = data(&app);
    assert!(app.finalize_block(block(3, vec![])).is_err());
    assert_eq!(data(&app), before);
    let incorrect = observation_wire("ab".repeat(32));
    assert!(app.finalize_block(block(3, vec![incorrect])).is_err());
    assert_eq!(data(&app), before);
    let valid = observation_wire(block(2, vec![]).hash);
    let result = app.finalize_block(block(3, vec![valid])).unwrap();
    assert_eq!(result.tx_results.len(), 1);
    assert_eq!(result.tx_results[0].code, 0);
    assert_eq!(data(&app), before);
    app.commit().unwrap();
    assert_eq!(timing(&app).active_epoch, 1);
    assert_eq!(timing(&app).total_issued.total().unwrap(), 1502);
    commit_empty(&mut app, 4);
    assert_eq!(timing(&app).total_issued.total().unwrap(), 2002);
}

#[test]
fn decided_transactions_return_ordered_results_without_pending_admission() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = inputs.initialized(&dir.path().join("db"));
    let good = inputs.send();
    let mut bad = good.clone();
    let mut signature = B64.decode(&bad.signature).unwrap();
    signature[0] ^= 1;
    bad.signature = B64.encode(signature);
    let good_wire = signed_wire(good);
    let before = data(&app);
    let result = app
        .finalize_block(block(
            1,
            vec![
                b"not-json".to_vec(),
                signed_wire(bad),
                good_wire.clone(),
                good_wire,
            ],
        ))
        .unwrap();
    assert_eq!(result.tx_results.len(), 4);
    assert_ne!(result.tx_results[0].code, 0);
    assert_ne!(result.tx_results[1].code, 0);
    assert_eq!(result.tx_results[2].code, 0);
    assert_ne!(result.tx_results[3].code, 0);
    assert_eq!(data(&app), before);
    app.commit().unwrap();
    assert_eq!(balances(&app, "recipient")["udgt"], 10);
    assert_eq!(balances(&app, &inputs.owner)["udgt"], 890);
    let nonce: u64 = bincode::deserialize(
        &app.storage
            .db
            .get(format!("acct:nonce:{}", inputs.owner))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(nonce, 1);
    assert_eq!(
        crate::supply::inspect_native(&app.storage)
            .unwrap()
            .drt
            .withheld_fees,
        50000
    );
}

#[test]
fn signed_claim_commits_current_block_reward_pool_and_liquid_custody_together() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = inputs.initialized(&dir.path().join("db"));
    let claim = inputs.signed(
        0,
        Msg::RewardClaim {
            from: inputs.owner.clone(),
        },
    );
    let before = data(&app);
    let result = app
        .finalize_block(block(1, vec![signed_wire(claim)]))
        .unwrap();
    assert_eq!(result.tx_results[0].code, 0);
    assert_eq!(data(&app), before);
    app.commit().unwrap();
    let supply = crate::supply::inspect_native(&app.storage).unwrap();
    assert_eq!(supply.drt.pools["staking_rewards"], 0);
    assert_eq!(
        balances(&app, &inputs.owner)["udrt"],
        INITIAL_DRT + 150 - 50000
    );
    assert_eq!(supply.drt.total, INITIAL_DRT + 501);
    assert_eq!(
        supply.drt.liquid + supply.drt.withheld_fees + supply.drt.pools.values().sum::<u128>(),
        supply.drt.total
    );
}

#[test]
fn reopen_rejects_corrupt_durable_consensus_record() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = inputs.initialized(&path);
    commit_empty(&mut app, 1);
    let key = b"consensus:v1:block:0000000000000001";
    let original = app
        .storage
        .db
        .get(key)
        .unwrap()
        .expect("committed consensus record");
    let mut damaged = original.to_vec();
    damaged[0] ^= 1;
    app.storage.db.put(key, damaged).unwrap();
    drop(app);
    assert!(
        ConsensusApplication::open(&path, inputs.config.clone(), inputs.genesis.clone()).is_err()
    );
}

#[test]
fn recovery_requires_original_genesis_root_before_and_after_first_commit() {
    let inputs = Inputs::new();
    for height in [0, 1] {
        for remove in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("db");
            let mut app = inputs.initialized(&path);
            if height == 1 {
                commit_empty(&mut app, 1);
            }
            if remove {
                app.storage.db.delete(GENESIS_APP_HASH_KEY).unwrap();
            } else {
                app.storage
                    .db
                    .put(GENESIS_APP_HASH_KEY, "ab".repeat(32).as_bytes())
                    .unwrap();
            }
            let corrupted = data(&app);
            let error = app.info().unwrap_err().to_string();
            if remove {
                assert!(
                    error.contains("Genesis application commitment missing"),
                    "{error}"
                );
            } else if height == 0 {
                assert!(
                    error.contains("Initial consensus monetary state differs"),
                    "{error}"
                );
            } else {
                assert!(
                    error.contains("Initial consensus parent or application commitment differs"),
                    "{error}"
                );
            }
            assert_eq!(
                data(&app),
                corrupted,
                "Recovery must not repair a corrupt commitment"
            );
            drop(app);
            assert!(ConsensusApplication::open(
                &path,
                inputs.config.clone(),
                inputs.genesis.clone()
            )
            .is_err());
        }
    }
}

#[test]
fn recovery_binds_monetary_genesis_marker_to_original_source_at_every_height() {
    let inputs = Inputs::new();
    for height in [0, 1] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let mut app = inputs.initialized(&path);
        if height == 1 {
            commit_empty(&mut app, 1);
        }
        let key = b"genesis:monetary:v1";
        let mut marker = app.storage.db.get(key).unwrap().unwrap();
        marker[1] ^= 1;
        app.storage.db.put(key, marker).unwrap();
        let corrupted = data(&app);
        let error = app.info().unwrap_err().to_string();
        assert!(
            error.contains("Monetary genesis marker differs from original source"),
            "{error}"
        );
        assert_eq!(data(&app), corrupted);
        drop(app);
        assert!(
            ConsensusApplication::open(&path, inputs.config.clone(), inputs.genesis.clone())
                .is_err()
        );
    }
}

#[test]
fn production_classical_validators_and_changed_binding_cannot_change_existing_mode() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let app = inputs.initialized(&path);
    let before = data(&app);
    drop(app);
    for change in 0..3 {
        let mut config = inputs.config.clone();
        match change {
            0 => config.profile = "production".into(),
            1 => config.validators[0].pubkey_type = "ed25519".into(),
            _ => config.app_state_sha256 = "ab".repeat(32),
        }
        assert!(ConsensusApplication::open(&path, config, inputs.genesis.clone()).is_err());
        let reopened = inputs.open(&path);
        assert_eq!(data(&reopened), before);
    }
}

fn admission_send(inputs: &Inputs, nonce: u64, amount: u128) -> Vec<u8> {
    signed_wire(inputs.signed(
        nonce,
        Msg::Send {
            from: inputs.owner.clone(),
            to: "recipient".into(),
            denom: "udgt".into(),
            amount,
        },
    ))
}

fn owner_nonce(app: &ConsensusApplication, owner: &str) -> u64 {
    app.storage
        .db
        .get(format!("acct:nonce:{owner}"))
        .unwrap()
        .map(|bytes| bincode::deserialize(&bytes).unwrap())
        .unwrap_or(0)
}

#[test]
fn admission_rejects_committed_success_and_accepted_failure_by_normalized_hash() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = inputs.initialized(&dir.path().join("db"));
    let candidates = vec![
        admission_send(&inputs, 0, 10),
        admission_send(&inputs, 1, 1000),
    ];
    let before = data(&app);
    for raw in &candidates {
        assert_eq!(app.check_tx(raw).code, 0);
        assert_eq!(app.check_tx(raw).code, 0);
    }
    assert_eq!(data(&app), before);
    let result = app.finalize_block(block(1, candidates.clone())).unwrap();
    assert_eq!(result.tx_results[0].code, 0);
    assert_eq!(result.tx_results[1].code, 3);
    assert!(result.tx_results.iter().all(|result| result.gas_used > 0));
    for raw in &candidates {
        assert_eq!(
            app.check_tx(raw).code,
            0,
            "Pending receipts are not durable"
        );
    }
    app.commit().unwrap();
    assert_eq!(owner_nonce(&app, &inputs.owner), 2);
    assert_eq!(balances(&app, "recipient")["udgt"], 10);
    assert_eq!(balances(&app, &inputs.owner)["udgt"], 890);
    assert_eq!(balances(&app, &inputs.owner)["udrt"], INITIAL_DRT - 100000);
    assert_eq!(
        crate::supply::inspect_native(&app.storage)
            .unwrap()
            .drt
            .withheld_fees,
        100000
    );
    let committed = data(&app);
    for raw in &candidates {
        let decoded: serde_json::Value = serde_json::from_slice(raw).unwrap();
        let pretty = serde_json::to_vec_pretty(&decoded).unwrap();
        assert_ne!(Sha256::digest(raw), Sha256::digest(&pretty));
        for representation in [raw.as_slice(), pretty.as_slice()] {
            for _ in 0..2 {
                let rejected = app.check_tx(representation);
                assert_ne!(rejected.code, 0);
                assert_eq!(rejected.gas_used, 0);
            }
        }
    }
    assert_eq!(app.check_tx(&admission_send(&inputs, 2, 1)).code, 0);
    assert_eq!(
        data(&app),
        committed,
        "Admission must not change fee, nonce or principal"
    );
}

#[test]
fn proposal_filters_committed_receipts_before_byte_and_count_budgets() {
    let mut inputs = Inputs::new();
    inputs.config.max_txs = 2;
    let dir = tempfile::tempdir().unwrap();
    let mut app = inputs.initialized(&dir.path().join("db"));
    let success = admission_send(&inputs, 0, 10);
    let failure = admission_send(&inputs, 1, 1000);
    let result = app
        .finalize_block(block(1, vec![success.clone(), failure.clone()]))
        .unwrap();
    assert_eq!(result.tx_results[0].code, 0);
    assert_eq!(result.tx_results[1].code, 3);
    app.commit().unwrap();
    let fresh = admission_send(&inputs, 2, 1);
    let before = data(&app);
    let selected = app
        .prepare_proposal(
            2,
            20,
            17,
            vec![success.clone(), failure.clone(), fresh.clone()],
            u64::try_from(fresh.len()).unwrap(),
        )
        .unwrap();
    assert_eq!(selected, vec![fresh.clone()]);
    assert_eq!(data(&app), before);
    commit_empty(&mut app, 2);
    let observation = observation_wire(block(2, vec![]).hash);
    let before = data(&app);
    let selected = app
        .prepare_proposal(
            3,
            30,
            17,
            vec![
                success,
                failure,
                fresh.clone(),
                fresh.clone(),
                observation.clone(),
            ],
            u64::try_from(observation.len() + fresh.len()).unwrap(),
        )
        .unwrap();
    assert_eq!(selected, vec![observation, fresh]);
    assert_eq!(data(&app), before);
    let result = app.finalize_block(block(3, selected)).unwrap();
    assert!(result.tx_results.iter().all(|result| result.code == 0));
    app.commit().unwrap();
    assert_eq!(owner_nonce(&app, &inputs.owner), 3);
    assert_eq!(balances(&app, "recipient")["udgt"], 11);
}

#[test]
fn admission_receipts_follow_durable_commit_across_failures_and_recovery() {
    let inputs = Inputs::new();
    for (amount, expected_code, transferred) in [(10, 0, 10), (1000, 3, 0)] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let mut app = inputs.initialized(&path);
        let raw = admission_send(&inputs, 0, amount);
        let input = block(1, vec![raw.clone()]);
        let before = data(&app);
        assert_eq!(app.check_tx(&raw).code, 0);
        let result = app.finalize_block(input.clone()).unwrap();
        assert_eq!(result.tx_results[0].code, expected_code);
        assert_eq!(app.check_tx(&raw).code, 0);
        assert!(app
            .commit_with(|_, _| anyhow::bail!("injected before write"))
            .is_err());
        assert_eq!(data(&app), before);
        assert_eq!(app.check_tx(&raw).code, 0);
        drop(app);
        let mut reopened = inputs.open(&path);
        assert_eq!(reopened.info().unwrap().height, 0);
        assert_eq!(data(&reopened), before);
        assert_eq!(reopened.check_tx(&raw).code, 0);
        assert_eq!(reopened.finalize_block(input.clone()).unwrap(), result);
        assert_eq!(reopened.check_tx(&raw).code, 0);
        assert!(reopened
            .commit_with(|storage, batch| {
                write_sync(storage, batch)?;
                anyhow::bail!("injected lost acknowledgement")
            })
            .is_err());
        assert_ne!(
            reopened.check_tx(&raw).code,
            0,
            "A durable receipt rejects even when acknowledgement is lost"
        );
        let committed = data(&reopened);
        drop(reopened);
        let mut recovered = inputs.open(&path);
        assert_eq!(recovered.info().unwrap().height, 1);
        assert_eq!(recovered.info().unwrap().app_hash, result.app_hash);
        assert_ne!(recovered.check_tx(&raw).code, 0);
        assert_eq!(recovered.finalize_block(input).unwrap(), result);
        recovered.commit().unwrap();
        assert_ne!(recovered.check_tx(&raw).code, 0);
        assert_eq!(
            data(&recovered),
            committed,
            "Recovery cannot charge or transfer twice"
        );
        assert_eq!(owner_nonce(&recovered, &inputs.owner), 1);
        assert_eq!(
            balances(&recovered, &inputs.owner)["udgt"],
            900 - transferred
        );
        assert_eq!(
            balances(&recovered, "recipient")
                .get("udgt")
                .copied()
                .unwrap_or(0),
            transferred
        );
        assert_eq!(
            balances(&recovered, &inputs.owner)["udrt"],
            INITIAL_DRT - 50000
        );
        assert_eq!(
            crate::supply::inspect_native(&recovered.storage)
                .unwrap()
                .drt
                .withheld_fees,
            50000
        );
        let selected = recovered
            .prepare_proposal(2, 20, 17, vec![raw], 1048576)
            .unwrap();
        assert!(selected.is_empty());
        assert_eq!(data(&recovered), committed);
    }
}
