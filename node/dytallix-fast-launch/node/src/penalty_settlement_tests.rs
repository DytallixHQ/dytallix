//! Signed local penalty settlement tests. Evidence facts enter through the trusted application boundary.
//! These tests do not generate or validate conflicting consensus votes.
use super::*;
use crate::crypto::{ActivePQC, PQC};
use crate::runtime::penalty_custody::{EvidenceFact, PenaltyState, STATE_KEY as PENALTY_STATE_KEY};
use crate::runtime::validator_lifecycle::{
    consensus_address, proof_sign_bytes, LifecycleState, STATE_KEY,
};
use crate::types::{tx::Tx, Msg, SignedTx};
use fips204::{
    ml_dsa_65,
    traits::{KeyGen, SerDes, Signer},
};

const CHAIN: &str = "penalty-qualification";
const IDS: [&str; 3] = ["validator-a", "validator-b", "validator-c"];

struct Account {
    owner: String,
    secret: Vec<u8>,
    public: Vec<u8>,
}
impl Account {
    fn new() -> Self {
        let (secret, public) = ActivePQC::keypair();
        let owner = crate::addr::initial_address(
            crate::addr::AddressNetwork::Development,
            CHAIN,
            crate::addr::OriginKeyAlgorithm::MlDsa65,
            &public,
        )
        .unwrap();
        Self {
            owner,
            secret,
            public,
        }
    }
    fn sign(&self, nonce: u64, msgs: Vec<Msg>) -> Vec<u8> {
        let envelope = SignedTx::sign(
            Tx {
                chain_id: CHAIN.into(),
                nonce,
                msgs,
                fee: 200_000,
                memo: "lifecycle fixture".into(),
            },
            &self.secret,
            &self.public,
        )
        .unwrap();
        serde_json::to_vec(&WireTransaction::Signed { envelope }).unwrap()
    }
}
struct ConsensusKey {
    public: String,
    secret: ml_dsa_65::PrivateKey,
}
impl ConsensusKey {
    fn new() -> Self {
        let (public, secret) = ml_dsa_65::KG::try_keygen_with_rng(&mut rand_core::OsRng).unwrap();
        Self {
            public: B64.encode(public.into_bytes()),
            secret,
        }
    }
    fn proof(
        &self,
        operation: &str,
        id: &str,
        owner: &str,
        nonce: u64,
        expiry: u64,
        amount: u128,
    ) -> String {
        let bytes = proof_sign_bytes(
            CHAIN,
            operation,
            id,
            owner,
            &self.public,
            nonce,
            expiry,
            amount,
        )
        .unwrap();
        B64.encode(self.secret.try_sign(&bytes, &[]).unwrap())
    }
}
struct Fixture {
    accounts: [Account; 3],
    keys: [ConsensusKey; 3],
    config: ConsensusConfig,
    genesis: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        Self::with_capacity(8)
    }
    fn with_capacity(max_active: usize) -> Self {
        let accounts = [Account::new(), Account::new(), Account::new()];
        let keys = [
            ConsensusKey::new(),
            ConsensusKey::new(),
            ConsensusKey::new(),
        ];
        let allocation: Vec<_> = accounts
            .iter()
            .map(|a| {
                serde_json::json!({"address":a.owner,
            "balances":{"udgt":"1000","udrt":"10000000"},"vesting":{"kind":"unlocked"}})
            })
            .collect();
        let delegations: Vec<_> = accounts[..2]
            .iter()
            .map(|a| serde_json::json!({"delegator":a.owner,"amount_udgt":"100"}))
            .collect();
        let positions: Vec<_> = (0..2).map(|i|serde_json::json!({"owner":accounts[i].owner,"validator":IDS[i],"amount_udgt":"100"})).collect();
        let validators: Vec<_> = IDS[..2]
            .iter()
            .map(|id| serde_json::json!({"address":id,"active":true,"jailed":false}))
            .collect();
        let genesis = serde_json::to_vec(&serde_json::json!({"chain_id":CHAIN,"accounts":allocation,
            "staking":{"delegations":delegations},
            "reward_v2":{"version":2,"activation_height":1,"decimals":6,"profile":"development",
                "max_validators":8,"max_positions":32,"validators":validators,"positions":positions},
            "adaptive_issuance":{"version":1,"profile":"development","decimals":6,
                "epoch_blocks":100,"initial_epoch_budget_udrt":"100000","max_recorded_epochs":8,
                "controller":{"target_ppm":500000,"shock_threshold_ppm":100000,"volatility_threshold_ppm":1000000,
                    "window_samples":2,"integral_min":-2000000,"integral_max":2000000,
                    "soft":{"proportional":0,"integral":0,"derivative":0},
                    "hard":{"proportional":0,"integral":0,"derivative":0},
                    "base_udrt":100000,"min_udrt":100000,"max_udrt":100000}}})).unwrap();
        let operators: BTreeMap<_, _> = (0..3)
            .map(|i| (IDS[i], accounts[i].owner.as_str()))
            .collect();
        let engine_validators: Vec<_> = (0..2)
            .map(|i| {
                serde_json::json!({"pubkey_type":"ml_dsa_65",
            "pubkey_base64":keys[i].public,"power":100,"reward_address":IDS[i]})
            })
            .collect();
        let config = serde_json::from_value(serde_json::json!({
            "profile":"cometbft-penalty-local-qualification","engine":"cometbft-v0.40.0","chain_id":CHAIN,
            "app_state_sha256":hex::encode(Sha256::digest(&genesis)),"gas_price":1,
            "max_tx_bytes":65536,"max_block_bytes":1048576,"max_txs":64,"validators":engine_validators,
            "lifecycle":{"version":1,"profile":"cometbft-lifecycle-local-qualification","chain_id":CHAIN,
                "approved_operators":operators,"min_self_bond":"10","max_active":max_active,
                "evidence_max_age_blocks":3,"evidence_max_age_seconds":3,
                "processing_margin_blocks":1,"processing_margin_seconds":1},
            "penalty":{"version":1,"profile":"cometbft-penalty-local-qualification",
                "chain_id":CHAIN,"penalty_numerator":1,"penalty_denominator":20,"production_activation":false}})).unwrap();
        Self {
            accounts,
            keys,
            config,
            genesis,
        }
    }
    fn open(&self, path: &Path) -> ConsensusApplication {
        ConsensusApplication::open(path, self.config.clone(), self.genesis.clone()).unwrap()
    }
    fn initialized(&self, path: &Path) -> ConsensusApplication {
        let mut app = self.open(path);
        app.init_chain(CHAIN, 1, &self.genesis, &self.config.validators)
            .unwrap();
        app
    }
    fn rotate(&self, key: &ConsensusKey, nonce: u64) -> Vec<u8> {
        self.accounts[0].sign(
            nonce,
            vec![Msg::ValidatorRotateKey {
                from: self.accounts[0].owner.clone(),
                validator: IDS[0].into(),
                consensus_pubkey: key.public.clone(),
                proof: key.proof("rotate", IDS[0], &self.accounts[0].owner, nonce, 20, 0),
                expires_at_height: 20,
            }],
        )
    }
}
fn input(height: u64, txs: Vec<Vec<u8>>) -> FinalizedBlockInput {
    FinalizedBlockInput {
        height,
        time_seconds: i64::try_from(height * 10).unwrap(),
        time_nanos: 0,
        hash: format!("{height:064x}"),
        txs,
        misbehavior: Vec::new(),
    }
}
fn commit(app: &mut ConsensusApplication, height: u64, txs: Vec<Vec<u8>>) -> FinalizeResult {
    let result = app.finalize_block(input(height, txs)).unwrap();
    app.commit().unwrap();
    result
}
fn successful(result: &FinalizeResult) {
    assert!(
        result.tx_results.iter().all(|r| r.code == 0),
        "{:?}",
        result.tx_results
    );
}
fn lifecycle(app: &ConsensusApplication) -> LifecycleState {
    LifecycleState::decode(&app.storage.db.get(STATE_KEY).unwrap().unwrap()).unwrap()
}
fn rewards(app: &ConsensusApplication) -> RewardState {
    RewardState::decode(&app.storage.db.get(REWARD_STATE_KEY).unwrap().unwrap()).unwrap()
}
fn data(app: &ConsensusApplication) -> Writes {
    app.storage
        .db
        .iterator(IteratorMode::Start)
        .map(|entry| {
            let (k, v) = entry.unwrap();
            (k.to_vec(), v.to_vec())
        })
        .collect()
}
fn liquid(app: &ConsensusApplication, owner: &str) -> u128 {
    let balances: BTreeMap<String, u128> = bincode::deserialize(
        &app.storage
            .db
            .get(format!("acct:balances:{owner}"))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    balances.get("udgt").copied().unwrap_or(0)
}
fn power(app: &ConsensusApplication, height: u64, key: &str) -> i64 {
    lifecycle(app)
        .validator_set(height)
        .unwrap()
        .iter()
        .find(|v| v.pubkey_base64 == key)
        .map_or(0, |v| v.power)
}
fn penalties(app: &ConsensusApplication) -> PenaltyState {
    PenaltyState::decode(&app.storage.db.get(PENALTY_STATE_KEY).unwrap().unwrap()).unwrap()
}
fn custody(
    app: &ConsensusApplication,
    staked: u128,
    pending: u128,
    unbonding: u128,
    reserve: u128,
) {
    let native = crate::supply::inspect_native(&app.storage).unwrap();
    assert_eq!(
        (
            native.dgt.staked,
            native.dgt.pending_bonded,
            native.dgt.unbonding,
            native.dgt.penalty_reserve
        ),
        (staked, pending, unbonding, reserve)
    );
    assert_eq!(native.dgt.issued, 3000);
    assert_eq!(
        native.dgt.liquid
            + native.dgt.staked
            + native.dgt.pending_bonded
            + native.dgt.unbonding
            + native.dgt.penalty_reserve,
        3000
    );
    assert_eq!(
        native.drt.liquid + native.drt.withheld_fees + native.drt.pools.values().sum::<u128>(),
        native.drt.total
    );
}
fn evidence(key: &ConsensusKey, height: u64, power: i64, total_power: i64) -> EvidenceFact {
    EvidenceFact {
        kind: "duplicate_vote".into(),
        validator_address: consensus_address(&key.public).unwrap(),
        height,
        time_seconds: height * 10,
        time_nanos: 0,
        power,
        total_power,
    }
}
fn commit_evidence(
    app: &mut ConsensusApplication,
    height: u64,
    facts: Vec<EvidenceFact>,
    txs: Vec<Vec<u8>>,
) -> FinalizeResult {
    let mut block = input(height, txs);
    block.misbehavior = facts;
    let result = app.finalize_block(block).unwrap();
    app.commit().unwrap();
    result
}
fn exit(f: &Fixture, account: usize, nonce: u64) -> Vec<u8> {
    f.accounts[account].sign(
        nonce,
        vec![Msg::ValidatorExit {
            from: f.accounts[account].owner.clone(),
            validator: IDS[account].into(),
        }],
    )
}
fn withdrawal(f: &Fixture, account: usize, nonce: u64, id: &str) -> Vec<u8> {
    f.accounts[account].sign(
        nonce,
        vec![Msg::ValidatorWithdraw {
            from: f.accounts[account].owner.clone(),
            unbond_id: id.into(),
        }],
    )
}
fn entry_id(app: &ConsensusApplication, owner: &str) -> String {
    lifecycle(app)
        .unbonding
        .values()
        .find(|entry| entry.owner == owner)
        .unwrap()
        .id
        .clone()
}

#[test]
fn fault_keeps_effective_power_until_h_plus_two_then_withdraws_net_principal_once() {
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = f.initialized(&dir.path().join("db"));
    commit(&mut app, 1, vec![]);
    let fact = evidence(&f.keys[0], 1, 100, 200);
    let result = commit_evidence(&mut app, 2, vec![fact.clone(), fact.clone()], vec![]);
    assert_eq!(result.validator_updates.len(), 1);
    assert_eq!(result.validator_updates[0].pubkey_base64, f.keys[0].public);
    assert_eq!(result.validator_updates[0].power, 0);
    assert_eq!(power(&app, 2, &f.keys[0].public), 100);
    assert_eq!(power(&app, 3, &f.keys[0].public), 100);
    assert_eq!(power(&app, 4, &f.keys[0].public), 0);
    custody(&app, 200, 0, 0, 0);
    let admitted = penalties(&app).incidents;
    assert_eq!(admitted.len(), 1);
    let repeated = commit_evidence(&mut app, 3, vec![fact.clone(), fact], vec![]);
    assert!(repeated.validator_updates.is_empty());
    assert_eq!(penalties(&app).incidents, admitted);
    let unpaid = rewards(&app).unpaid[&f.accounts[0].owner];
    commit(&mut app, 4, vec![]);
    assert_eq!(rewards(&app).unpaid[&f.accounts[0].owner], unpaid);
    custody(&app, 100, 0, 95, 5);
    let id = entry_id(&app, &f.accounts[0].owner);
    for height in 5..=8 {
        commit(&mut app, height, vec![]);
    }
    successful(&commit(&mut app, 9, vec![withdrawal(&f, 0, 0, &id)]));
    assert_eq!(liquid(&app, &f.accounts[0].owner), 995);
    custody(&app, 100, 0, 0, 5);
    let result = commit(&mut app, 10, vec![withdrawal(&f, 0, 1, &id)]);
    assert_ne!(result.tx_results[0].code, 0);
    assert_eq!(liquid(&app, &f.accounts[0].owner), 995);
    assert_eq!(penalties(&app).released_total().unwrap(), 95);
    assert_eq!(penalties(&app).deducted_total().unwrap(), 5);
}

#[test]
fn withdrawal_uses_parent_height_and_strict_height_limit() {
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = f.initialized(&dir.path().join("db"));
    successful(&commit(&mut app, 1, vec![exit(&f, 0, 0)]));
    for h in 2..=6 {
        commit(&mut app, h, vec![]);
    }
    let id = entry_id(&app, &f.accounts[0].owner);
    // Last exposure H2 plus age3 plus margin1 equals parent H6.
    let result = commit(&mut app, 7, vec![withdrawal(&f, 0, 1, &id)]);
    assert_ne!(result.tx_results[0].code, 0);
    custody(&app, 100, 0, 100, 0);
    successful(&commit(&mut app, 8, vec![withdrawal(&f, 0, 2, &id)]));
    assert_eq!(liquid(&app, &f.accounts[0].owner), 1000);
    custody(&app, 100, 0, 0, 0);
}

#[test]
fn withdrawal_uses_parent_time_and_strict_time_limit() {
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = f.initialized(&dir.path().join("db"));
    successful(&commit(&mut app, 1, vec![exit(&f, 0, 0)]));
    commit(&mut app, 2, vec![]);
    for h in 3..=7 {
        let mut block = input(h, vec![]);
        block.time_seconds = 20 + i64::try_from(h - 2).unwrap().min(4);
        block.time_nanos = if h == 7 { 1 } else { 0 };
        app.finalize_block(block).unwrap();
        app.commit().unwrap();
    }
    let id = entry_id(&app, &f.accounts[0].owner);
    let mut request = input(8, vec![withdrawal(&f, 0, 1, &id)]);
    request.time_seconds = 25;
    let result = app.finalize_block(request).unwrap();
    app.commit().unwrap();
    assert_ne!(result.tx_results[0].code, 0);
    custody(&app, 100, 0, 100, 0);
    let mut request = input(9, vec![withdrawal(&f, 0, 2, &id)]);
    request.time_seconds = 26;
    successful(&app.finalize_block(request).unwrap());
    app.commit().unwrap();
    custody(&app, 100, 0, 0, 0);
}

#[test]
fn evidence_precedes_same_block_withdrawal_and_replay_does_not_charge_twice() {
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = f.initialized(&dir.path().join("db"));
    successful(&commit(&mut app, 1, vec![exit(&f, 0, 0)]));
    commit(&mut app, 2, vec![]);
    commit(&mut app, 3, vec![]);
    let id = entry_id(&app, &f.accounts[0].owner);
    let fact = evidence(&f.keys[0], 2, 100, 200);
    let result = commit_evidence(
        &mut app,
        4,
        vec![fact.clone(), fact.clone()],
        vec![withdrawal(&f, 0, 1, &id)],
    );
    assert_ne!(result.tx_results[0].code, 0);
    custody(&app, 100, 0, 100, 0);
    let admitted = penalties(&app).incidents;
    assert_eq!(admitted.len(), 1);
    let original = admitted.values().next().unwrap();
    assert_eq!(original.admitted_height, 4);
    assert_eq!(original.activation_height, 6);
    let repeated = commit_evidence(&mut app, 5, vec![fact.clone(), fact.clone()], vec![]);
    assert!(repeated.validator_updates.is_empty());
    assert_eq!(penalties(&app).incidents, admitted);
    custody(&app, 100, 0, 100, 0);
    assert_eq!(penalties(&app).deducted_total().unwrap(), 0);
    let activated = commit_evidence(&mut app, 6, vec![fact], vec![]);
    assert!(activated.validator_updates.is_empty());
    let state = penalties(&app);
    assert_eq!(state.incidents.len(), 1);
    let settled = state.incidents.values().next().unwrap();
    assert_eq!(settled.admitted_height, original.admitted_height);
    assert_eq!(settled.activation_height, original.activation_height);
    assert_eq!(settled.allocations, original.allocations);
    assert_eq!(settled.settled_height, Some(6));
    commit(&mut app, 7, vec![]);
    custody(&app, 100, 0, 95, 5);
    successful(&commit(&mut app, 8, vec![withdrawal(&f, 0, 2, &id)]));
    assert_eq!(liquid(&app, &f.accounts[0].owner), 995);
}

#[test]
fn distinct_later_fault_respects_first_fault_cap_and_barred_validator_rejects_new_operations() {
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = f.initialized(&dir.path().join("db"));
    commit(&mut app, 1, vec![]);
    commit_evidence(&mut app, 2, vec![evidence(&f.keys[0], 1, 100, 200)], vec![]);
    let result = commit_evidence(
        &mut app,
        3,
        vec![evidence(&f.keys[0], 2, 100, 200)],
        vec![
            f.accounts[2].sign(
                0,
                vec![Msg::RewardBond {
                    from: f.accounts[2].owner.clone(),
                    validator: IDS[0].into(),
                    amount_udgt: 100,
                }],
            ),
            f.rotate(&ConsensusKey::new(), 0),
        ],
    );
    assert!(result.tx_results.iter().all(|r| r.code != 0));
    assert_eq!(liquid(&app, &f.accounts[2].owner), 1000);
    commit(&mut app, 4, vec![]);
    custody(&app, 100, 0, 95, 5);
    assert_eq!(penalties(&app).deducted_total().unwrap(), 5);
    let key = ConsensusKey::new();
    let owner = &f.accounts[0].owner;
    let tx = f.accounts[0].sign(
        1,
        vec![Msg::ValidatorRegister {
            from: owner.clone(),
            validator: IDS[0].into(),
            consensus_pubkey: key.public.clone(),
            proof: key.proof("register", IDS[0], owner, 1, 20, 100),
            expires_at_height: 20,
            amount_udgt: 100,
        }],
    );
    assert_ne!(commit(&mut app, 5, vec![tx]).tx_results[0].code, 0);
    custody(&app, 100, 0, 95, 5);
}

#[test]
fn historical_old_key_maps_to_stable_validator_after_rotation_and_excludes_later_bonds() {
    let f = Fixture::new();
    let key = ConsensusKey::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = f.initialized(&dir.path().join("db"));
    successful(&commit(&mut app, 1, vec![f.rotate(&key, 0)]));
    let owner = &f.accounts[2].owner;
    successful(&commit(
        &mut app,
        2,
        vec![f.accounts[2].sign(
            0,
            vec![Msg::RewardBond {
                from: owner.clone(),
                validator: IDS[0].into(),
                amount_udgt: 100,
            }],
        )],
    ));
    commit(&mut app, 3, vec![]);
    let result = commit_evidence(&mut app, 4, vec![evidence(&f.keys[0], 1, 100, 200)], vec![]);
    assert!(result
        .validator_updates
        .iter()
        .any(|v| v.pubkey_base64 == key.public && v.power == 0));
    assert_eq!(power(&app, 5, &key.public), 200);
    assert_eq!(power(&app, 6, &key.public), 0);
    commit(&mut app, 5, vec![]);
    commit(&mut app, 6, vec![]);
    custody(&app, 100, 0, 195, 5);
    assert_eq!(penalties(&app).owner_deducted(owner).unwrap(), 0);
    assert_eq!(
        penalties(&app)
            .owner_deducted(&f.accounts[0].owner)
            .unwrap(),
        5
    );
}

#[test]
fn invalid_evidence_metadata_rejects_block_without_writes() {
    for case in 0..6 {
        let f = Fixture::new();
        let dir = tempfile::tempdir().unwrap();
        let mut app = f.initialized(&dir.path().join("db"));
        commit(&mut app, 1, vec![]);
        let mut fact = evidence(&f.keys[0], 1, 100, 200);
        match case {
            0 => fact.validator_address = "00".repeat(20),
            1 => fact.power += 1,
            2 => fact.total_power += 1,
            3 => fact.time_seconds += 1,
            4 => fact.kind = "light_client_attack".into(),
            5 => fact.time_nanos = 1,
            _ => {}
        }
        let mut block = input(2, vec![]);
        block.misbehavior = vec![fact];
        let before = data(&app);
        assert!(!app.process_proposal(block.clone()).unwrap(), "case {case}");
        assert_eq!(data(&app), before, "case {case}");
        assert!(app.finalize_block(block).is_err(), "case {case}");
        assert_eq!(data(&app), before, "case {case}");
    }
}

#[test]
fn evidence_expiry_needs_both_ages_and_preserves_equality_boundary() {
    for parent in [4, 5] {
        let f = Fixture::new();
        let dir = tempfile::tempdir().unwrap();
        let mut app = f.initialized(&dir.path().join("db"));
        for h in 1..=parent {
            commit(&mut app, h, vec![]);
        }
        let mut block = input(parent + 1, vec![]);
        block.misbehavior = vec![evidence(&f.keys[0], 1, 100, 200)];
        let before = data(&app);
        let result = app.finalize_block(block);
        if parent == 4 {
            assert!(result.is_ok());
            app.commit().unwrap();
        } else {
            assert!(result.is_err());
            assert_eq!(data(&app), before);
        }
    }
}

#[test]
fn penalty_write_failure_lost_ack_and_reopen_preserve_one_incident_and_deduction() {
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = f.initialized(&path);
    commit(&mut app, 1, vec![]);
    let mut request = input(2, vec![]);
    request.misbehavior = vec![evidence(&f.keys[0], 1, 100, 200)];
    let before = data(&app);
    let result = app.finalize_block(request.clone()).unwrap();
    assert!(app
        .commit_with(|_, _| anyhow::bail!("injected penalty prewrite failure"))
        .is_err());
    assert_eq!(data(&app), before);
    drop(app);
    let mut app = f.open(&path);
    assert_eq!(app.finalize_block(request.clone()).unwrap(), result);
    assert!(app
        .commit_with(|storage, batch| {
            let mut options = WriteOptions::default();
            options.set_sync(true);
            storage.db.write_opt(batch, &options)?;
            anyhow::bail!("injected penalty lost acknowledgement")
        })
        .is_err());
    let durable = data(&app);
    drop(app);
    let mut app = f.open(&path);
    assert_eq!(app.finalize_block(request).unwrap(), result);
    app.commit().unwrap();
    assert_eq!(data(&app), durable);
    commit(&mut app, 3, vec![]);
    commit(&mut app, 4, vec![]);
    custody(&app, 100, 0, 95, 5);
    assert_eq!(penalties(&app).deducted_total().unwrap(), 5);
}

#[test]
fn foreign_owner_and_failed_second_message_cannot_release_unbonding() {
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = f.initialized(&dir.path().join("db"));
    successful(&commit(&mut app, 1, vec![exit(&f, 0, 0)]));
    for h in 2..=7 {
        commit(&mut app, h, vec![]);
    }
    let id = entry_id(&app, &f.accounts[0].owner);
    assert_ne!(
        commit(&mut app, 8, vec![withdrawal(&f, 2, 0, &id)]).tx_results[0].code,
        0
    );
    let tx = f.accounts[0].sign(
        1,
        vec![
            Msg::ValidatorWithdraw {
                from: f.accounts[0].owner.clone(),
                unbond_id: id.clone(),
            },
            Msg::ValidatorWithdraw {
                from: f.accounts[0].owner.clone(),
                unbond_id: id.clone(),
            },
        ],
    );
    assert_ne!(commit(&mut app, 9, vec![tx]).tx_results[0].code, 0);
    custody(&app, 100, 0, 100, 0);
    assert_eq!(penalties(&app).released_total().unwrap(), 0);
    successful(&commit(&mut app, 10, vec![withdrawal(&f, 0, 2, &id)]));
    custody(&app, 100, 0, 0, 0);
}

#[test]
fn penalty_profile_rejects_existing_lifecycle_database_and_locked_genesis() {
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut old_config = serde_json::to_value(&f.config).unwrap();
    old_config["profile"] = serde_json::json!("cometbft-lifecycle-local-qualification");
    old_config.as_object_mut().unwrap().remove("penalty");
    let old_config: ConsensusConfig = serde_json::from_value(old_config).unwrap();
    let mut old = ConsensusApplication::open(&path, old_config.clone(), f.genesis.clone()).unwrap();
    old.init_chain(CHAIN, 1, &f.genesis, &old_config.validators)
        .unwrap();
    commit(&mut old, 1, vec![]);
    let before = data(&old);
    drop(old);
    assert!(ConsensusApplication::open(&path, f.config.clone(), f.genesis.clone()).is_err());
    let old = ConsensusApplication::open(&path, old_config, f.genesis.clone()).unwrap();
    assert_eq!(data(&old), before);
    let mut locked: serde_json::Value = serde_json::from_slice(&f.genesis).unwrap();
    locked["accounts"][0]["vesting"] = serde_json::json!({"kind":"linear_after_cliff","total_amount":"1000","start_time":0,"cliff_duration":10,"vesting_duration":100,"allow_staking":true});
    let genesis = serde_json::to_vec(&locked).unwrap();
    let mut config = f.config.clone();
    config.app_state_sha256 = hex::encode(Sha256::digest(&genesis));
    let locked_path = dir.path().join("locked");
    assert!(ConsensusApplication::open(&locked_path, config, genesis).is_err());
    assert!(!locked_path.exists());
}

fn sign_65(key: &ConsensusKey, nonce: u64, msgs: Vec<Msg>) -> Vec<u8> {
    let tx = Tx {
        chain_id: CHAIN.into(),
        nonce,
        msgs,
        fee: 200000,
        memo: "ordinary delegator".into(),
    };
    let bytes = crate::crypto::canonical_json(&tx).unwrap();
    let envelope = SignedTx {
        tx,
        public_key: key.public.clone(),
        signature: B64.encode(
            key.secret
                .try_sign(&crate::crypto::sha3_256(&bytes), &[])
                .unwrap(),
        ),
        algorithm: "mldsa65".into(),
        version: 1,
    };
    envelope.verify().unwrap();
    serde_json::to_vec(&WireTransaction::Signed { envelope }).unwrap()
}

#[test]
fn ordinary_ml_dsa_65_delegator_can_withdraw_own_principal_with_origin_authorization() {
    let mut f = Fixture::new();
    let key = ConsensusKey::new();
    let owner = crate::addr::initial_address(
        crate::addr::AddressNetwork::Development,
        CHAIN,
        crate::addr::OriginKeyAlgorithm::MlDsa65,
        &B64.decode(&key.public).unwrap(),
    )
    .unwrap();
    let mut source: serde_json::Value = serde_json::from_slice(&f.genesis).unwrap();
    source["accounts"][2]["address"] = serde_json::json!(owner);
    f.genesis = serde_json::to_vec(&source).unwrap();
    f.config.app_state_sha256 = hex::encode(Sha256::digest(&f.genesis));
    f.config
        .lifecycle
        .as_mut()
        .unwrap()
        .approved_operators
        .insert(IDS[2].into(), owner.clone());
    let dir = tempfile::tempdir().unwrap();
    let mut app = f.initialized(&dir.path().join("db"));
    successful(&commit(
        &mut app,
        1,
        vec![sign_65(
            &key,
            0,
            vec![Msg::RewardBond {
                from: owner.clone(),
                validator: IDS[0].into(),
                amount_udgt: 40,
            }],
        )],
    ));
    commit(&mut app, 2, vec![]);
    commit(&mut app, 3, vec![]);
    successful(&commit(
        &mut app,
        4,
        vec![sign_65(
            &key,
            1,
            vec![Msg::RewardBeginUnbond {
                from: owner.clone(),
                validator: IDS[0].into(),
                amount_udgt: 40,
            }],
        )],
    ));
    for h in 5..=10 {
        commit(&mut app, h, vec![]);
    }
    let id = entry_id(&app, &owner);
    assert_eq!(liquid(&app, &owner), 960);
    let foreign = sign_65(
        &key,
        2,
        vec![Msg::ValidatorWithdraw {
            from: f.accounts[0].owner.clone(),
            unbond_id: id.clone(),
        }],
    );
    assert_ne!(commit(&mut app, 11, vec![foreign]).tx_results[0].code, 0);
    successful(&commit(
        &mut app,
        12,
        vec![sign_65(
            &key,
            2,
            vec![Msg::ValidatorWithdraw {
                from: owner.clone(),
                unbond_id: id,
            }],
        )],
    ));
    assert_eq!(liquid(&app, &owner), 1000);
    custody(&app, 200, 0, 0, 0);
}

#[test]
fn partial_unbond_keeps_fifo_exposure_provenance_when_a_later_bond_is_not_exposed() {
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = f.initialized(&dir.path().join("db"));
    let owner = &f.accounts[0].owner;
    successful(&commit(
        &mut app,
        1,
        vec![f.accounts[0].sign(
            0,
            vec![Msg::RewardBond {
                from: owner.clone(),
                validator: IDS[0].into(),
                amount_udgt: 100,
            }],
        )],
    ));
    successful(&commit(
        &mut app,
        2,
        vec![f.accounts[0].sign(
            1,
            vec![Msg::RewardBeginUnbond {
                from: owner.clone(),
                validator: IDS[0].into(),
                amount_udgt: 40,
            }],
        )],
    ));
    commit(&mut app, 3, vec![]);
    commit_evidence(&mut app, 4, vec![evidence(&f.keys[0], 1, 100, 200)], vec![]);
    commit(&mut app, 5, vec![]);
    commit(&mut app, 6, vec![]);
    custody(&app, 100, 0, 195, 5);
    // The first 40 units came from the genesis lot. The new 100-unit lot was
    // not effective at the fault height and must retain its full principal.
    // The original 60-unit remainder retains the oldest tranche ID and takes
    // the five-unit owner penalty before the split 40-unit tranche.
    let state = lifecycle(&app);
    let partial = state
        .unbonding
        .values()
        .find(|entry| entry.amount == 40)
        .unwrap();
    let partial_id = partial.id.clone();
    for h in 7..=8 {
        commit(&mut app, h, vec![]);
    }
    successful(&commit(
        &mut app,
        9,
        vec![withdrawal(&f, 0, 2, &partial_id)],
    ));
    assert_eq!(liquid(&app, owner), 840);
    custody(&app, 100, 0, 155, 5);
    commit(&mut app, 10, vec![]);
    let remainder = lifecycle(&app)
        .unbonding
        .values()
        .find(|entry| entry.amount == 160)
        .unwrap()
        .id
        .clone();
    successful(&commit(
        &mut app,
        11,
        vec![withdrawal(&f, 0, 3, &remainder)],
    ));
    assert_eq!(liquid(&app, owner), 995);
    custody(&app, 100, 0, 0, 5);
}

#[test]
fn durable_withdrawal_receipt_survives_lost_ack_without_releasing_twice() {
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = f.initialized(&path);
    successful(&commit(&mut app, 1, vec![exit(&f, 0, 0)]));
    for h in 2..=7 {
        commit(&mut app, h, vec![]);
    }
    let id = entry_id(&app, &f.accounts[0].owner);
    let request = input(8, vec![withdrawal(&f, 0, 1, &id)]);
    let before = data(&app);
    let result = app.finalize_block(request.clone()).unwrap();
    successful(&result);
    assert_eq!(data(&app), before);
    assert!(app
        .commit_with(|_, _| anyhow::bail!("injected withdrawal prewrite failure"))
        .is_err());
    assert_eq!(data(&app), before);
    drop(app);
    let mut app = f.open(&path);
    assert_eq!(app.finalize_block(request.clone()).unwrap(), result);
    assert!(app
        .commit_with(|storage, batch| {
            let mut options = WriteOptions::default();
            options.set_sync(true);
            storage.db.write_opt(batch, &options)?;
            anyhow::bail!("injected withdrawal lost acknowledgement")
        })
        .is_err());
    let durable = data(&app);
    drop(app);
    let mut app = f.open(&path);
    assert_eq!(app.finalize_block(request).unwrap(), result);
    app.commit().unwrap();
    assert_eq!(data(&app), durable);
    assert_eq!(liquid(&app, &f.accounts[0].owner), 1000);
    assert_eq!(penalties(&app).released_total().unwrap(), 100);
    assert_ne!(
        commit(&mut app, 9, vec![withdrawal(&f, 0, 2, &id)]).tx_results[0].code,
        0
    );
    custody(&app, 100, 0, 0, 0);
}
