//! Local signed validator lifecycle integration; principal withdrawal remains disabled.
use super::*;
use crate::crypto::{ActivePQC, PQC};
use crate::runtime::validator_lifecycle::{proof_sign_bytes, LifecycleState, STATE_KEY};
use crate::types::{tx::Tx, Msg, SignedTx};
use fips204::{
    ml_dsa_65,
    traits::{KeyGen, SerDes, Signer},
};

const CHAIN: &str = "lifecycle-qualification";
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
            "profile":"cometbft-lifecycle-local-qualification","engine":"cometbft-v0.40.0","chain_id":CHAIN,
            "app_state_sha256":hex::encode(Sha256::digest(&genesis)),"gas_price":1,
            "max_tx_bytes":65536,"max_block_bytes":1048576,"max_txs":64,"validators":engine_validators,
            "lifecycle":{"version":1,"profile":"cometbft-lifecycle-local-qualification","chain_id":CHAIN,
                "approved_operators":operators,"min_self_bond":"10","max_active":max_active,
                "evidence_max_age_blocks":3,"evidence_max_age_seconds":3,
                "processing_margin_blocks":1,"processing_margin_seconds":1}})).unwrap();
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
    fn register_message(
        &self,
        key: &ConsensusKey,
        id: &str,
        nonce: u64,
        expiry: u64,
        amount: u128,
    ) -> Msg {
        let owner = &self.accounts[2].owner;
        Msg::ValidatorRegister {
            from: owner.clone(),
            validator: id.into(),
            consensus_pubkey: key.public.clone(),
            proof: key.proof("register", id, owner, nonce, expiry, amount),
            expires_at_height: expiry,
            amount_udgt: amount,
        }
    }
    fn register(&self) -> Vec<u8> {
        self.accounts[2].sign(
            0,
            vec![self.register_message(&self.keys[2], IDS[2], 0, 20, 100)],
        )
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
        misbehavior: Vec::new(),
        height,
        time_seconds: i64::try_from(height * 10).unwrap(),
        time_nanos: 0,
        hash: format!("{height:064x}"),
        txs,
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
fn custody(app: &ConsensusApplication, staked: u128, pending: u128, unbonding: u128) {
    let native = crate::supply::inspect_native(&app.storage).unwrap();
    assert_eq!(
        (
            native.dgt.staked,
            native.dgt.pending_bonded,
            native.dgt.unbonding
        ),
        (staked, pending, unbonding)
    );
    assert_eq!(native.dgt.issued, 3000);
    assert_eq!(
        native.dgt.liquid + native.dgt.staked + native.dgt.pending_bonded + native.dgt.unbonding,
        3000
    );
    assert_eq!(
        native.drt.liquid + native.drt.withheld_fees + native.drt.pools.values().sum::<u128>(),
        native.drt.total
    );
}

#[test]
fn signed_registration_matches_on_two_nodes_and_activates_rewards_at_h_plus_two() {
    let f = Fixture::new();
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let mut a = f.initialized(&a_dir.path().join("db"));
    let mut b = f.initialized(&b_dir.path().join("db"));
    let tx = f.register();
    for height in 1..=3 {
        let txs = if height == 1 {
            vec![tx.clone()]
        } else {
            vec![]
        };
        let before = data(&a);
        let result = a.finalize_block(input(height, txs.clone())).unwrap();
        successful(&result);
        assert_eq!(data(&a), before);
        assert_eq!(result, b.finalize_block(input(height, txs)).unwrap());
        assert_eq!(a.commit().unwrap(), b.commit().unwrap());
        assert_eq!(data(&a), data(&b));
        if height == 1 {
            assert_eq!(result.validator_updates.len(), 1);
            assert_eq!(result.validator_updates[0].pubkey_base64, f.keys[2].public);
            assert_eq!(result.validator_updates[0].power, 100);
            assert_eq!(power(&a, 1, &f.keys[2].public), 0);
            assert_eq!(power(&a, 2, &f.keys[2].public), 0);
            assert_eq!(power(&a, 3, &f.keys[2].public), 100);
        }
        custody(
            &a,
            if height < 3 { 200 } else { 300 },
            if height < 3 { 100 } else { 0 },
            0,
        );
        assert_eq!(
            rewards(&a)
                .unpaid
                .get(&f.accounts[2].owner)
                .copied()
                .unwrap_or(0),
            if height < 3 { 0 } else { 100 }
        );
    }
    assert_eq!(liquid(&a, &f.accounts[2].owner), 900);
}

#[test]
fn consecutive_bond_and_unbond_requests_preserve_each_activation_and_custody() {
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = f.initialized(&dir.path().join("db"));
    let owner = &f.accounts[2].owner;
    let bond = f.accounts[2].sign(
        0,
        vec![Msg::RewardBond {
            from: owner.clone(),
            validator: IDS[0].into(),
            amount_udgt: 100,
        }],
    );
    successful(&commit(&mut app, 1, vec![bond]));
    custody(&app, 200, 100, 0);
    let unbond = f.accounts[2].sign(
        1,
        vec![Msg::RewardBeginUnbond {
            from: owner.clone(),
            validator: IDS[0].into(),
            amount_udgt: 40,
        }],
    );
    successful(&commit(&mut app, 2, vec![unbond]));
    assert_eq!(power(&app, 2, &f.keys[0].public), 100);
    assert_eq!(power(&app, 3, &f.keys[0].public), 200);
    assert_eq!(power(&app, 4, &f.keys[0].public), 160);
    custody(&app, 200, 100, 0);
    commit(&mut app, 3, vec![]);
    custody(&app, 300, 0, 0);
    assert_eq!(rewards(&app).unpaid[owner], 100);
    commit(&mut app, 4, vec![]);
    custody(&app, 260, 0, 40);
    assert_eq!(rewards(&app).unpaid[owner], 169);
    assert_eq!(liquid(&app, owner), 900);
    let state = lifecycle(&app);
    let entry = state.unbonding.values().next().unwrap();
    assert_eq!(
        (
            entry.request_height,
            entry.effective_height,
            entry.last_exposure_height,
            entry.last_exposure_time_seconds
        ),
        (2, 4, Some(3), Some(30))
    );
}

#[test]
fn exit_preserves_exposure_history_and_withdrawal_stays_disabled_after_age_limits() {
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = f.initialized(&dir.path().join("db"));
    let owner = &f.accounts[0].owner;
    successful(&commit(
        &mut app,
        1,
        vec![f.accounts[0].sign(
            0,
            vec![Msg::ValidatorExit {
                from: owner.clone(),
                validator: IDS[0].into(),
            }],
        )],
    ));
    assert_eq!(power(&app, 2, &f.keys[0].public), 100);
    assert_eq!(power(&app, 3, &f.keys[0].public), 0);
    custody(&app, 200, 0, 0);
    for h in 2..=6 {
        commit(&mut app, h, vec![]);
    }
    custody(&app, 100, 0, 100);
    assert_eq!(rewards(&app).unpaid[owner], 300);
    let state = lifecycle(&app);
    let entry = state.unbonding.values().next().unwrap();
    assert_eq!(
        (entry.last_exposure_height, entry.last_exposure_time_seconds),
        (Some(2), Some(20))
    );
    assert!(!entry
        .maturity_satisfied(&state.config, 6, 25, true)
        .unwrap());
    assert!(!entry
        .maturity_satisfied(&state.config, 7, 24, true)
        .unwrap());
    assert!(!entry
        .maturity_satisfied(&state.config, 7, 25, false)
        .unwrap());
    assert!(entry
        .maturity_satisfied(&state.config, 7, 25, true)
        .unwrap());
    let result = commit(
        &mut app,
        7,
        vec![f.accounts[0].sign(
            1,
            vec![Msg::ValidatorWithdraw {
                from: owner.clone(),
                unbond_id: entry.id.clone(),
            }],
        )],
    );
    assert_ne!(result.tx_results[0].code, 0);
    assert!(result.tx_results[0].log.contains("withdraw"));
    custody(&app, 100, 0, 100);
    assert_eq!(liquid(&app, owner), 900);
    assert_eq!(lifecycle(&app).unbonding, state.unbonding);
}

#[test]
fn consecutive_key_rotations_remove_old_keys_at_exact_heights_across_restart() {
    let f = Fixture::new();
    let one = ConsensusKey::new();
    let two = ConsensusKey::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = f.initialized(&path);
    let first = commit(&mut app, 1, vec![f.rotate(&one, 0)]);
    successful(&first);
    let changes: BTreeMap<_, _> = first
        .validator_updates
        .iter()
        .map(|v| (v.pubkey_base64.clone(), v.power))
        .collect();
    assert_eq!(
        changes,
        BTreeMap::from([(f.keys[0].public.clone(), 0), (one.public.clone(), 100)])
    );
    drop(app);
    let mut app = f.open(&path);
    let second = commit(&mut app, 2, vec![f.rotate(&two, 1)]);
    successful(&second);
    let changes: BTreeMap<_, _> = second
        .validator_updates
        .iter()
        .map(|v| (v.pubkey_base64.clone(), v.power))
        .collect();
    assert_eq!(
        changes,
        BTreeMap::from([(one.public.clone(), 0), (two.public.clone(), 100)])
    );
    assert_eq!(power(&app, 2, &f.keys[0].public), 100);
    assert_eq!(power(&app, 3, &one.public), 100);
    assert_eq!(power(&app, 4, &two.public), 100);
    for h in 3..=4 {
        commit(&mut app, h, vec![]);
    }
    assert_eq!(power(&app, 4, &f.keys[0].public), 0);
    assert_eq!(power(&app, 4, &one.public), 0);
    assert_eq!(power(&app, 4, &two.public), 100);
    custody(&app, 200, 0, 0);
}

#[test]
fn registration_failure_and_lost_ack_recover_one_pending_bond_and_one_activation() {
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = f.initialized(&path);
    let before = data(&app);
    let request = input(1, vec![f.register()]);
    let result = app.finalize_block(request.clone()).unwrap();
    successful(&result);
    assert!(app
        .commit_with(|_, _| anyhow::bail!("injected lifecycle prewrite failure"))
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
            anyhow::bail!("injected lifecycle lost acknowledgement")
        })
        .is_err());
    let durable = data(&app);
    drop(app);
    let mut app = f.open(&path);
    assert_eq!(app.info().unwrap().height, 1);
    assert_eq!(app.finalize_block(request).unwrap(), result);
    app.commit().unwrap();
    assert_eq!(data(&app), durable);
    custody(&app, 200, 100, 0);
    commit(&mut app, 2, vec![]);
    commit(&mut app, 3, vec![]);
    custody(&app, 300, 0, 0);
    assert_eq!(liquid(&app, &f.accounts[2].owner), 900);
}

#[test]
fn invalid_registration_authorization_proof_capacity_and_addresses_preserve_principal() {
    for case in 0..8 {
        let f = Fixture::with_capacity(if case == 6 { 2 } else { 8 });
        let dir = tempfile::tempdir().unwrap();
        let mut app = f.initialized(&dir.path().join("db"));
        let mut msg = f.register_message(
            if case == 4 { &f.keys[0] } else { &f.keys[2] },
            if case == 0 { "not-approved" } else { IDS[2] },
            if case == 7 { 1 } else { 0 },
            if case == 3 { 0 } else { 20 },
            if case == 5 { 9 } else { 100 },
        );
        if case == 2 {
            if let Msg::ValidatorRegister { proof, .. } = &mut msg {
                let mut bytes = B64.decode(&*proof).unwrap();
                bytes[0] ^= 1;
                *proof = B64.encode(bytes);
            }
        }
        let tx = if case == 1 {
            f.accounts[0].sign(0, vec![msg])
        } else {
            f.accounts[2].sign(0, vec![msg])
        };
        let result = commit(&mut app, 1, vec![tx]);
        assert_ne!(result.tx_results[0].code, 0, "case {case}");
        assert!(result.validator_updates.is_empty(), "case {case}");
        custody(&app, 200, 0, 0);
        assert_eq!(liquid(&app, &f.accounts[2].owner), 1000, "case {case}");
        assert!(lifecycle(&app).schedules.is_empty(), "case {case}");
    }
}

#[test]
fn failed_second_message_rolls_back_registration_and_pending_funding() {
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = f.initialized(&dir.path().join("db"));
    let tx = f.accounts[2].sign(
        0,
        vec![
            f.register_message(&f.keys[2], IDS[2], 0, 20, 100),
            Msg::RewardBond {
                from: f.accounts[2].owner.clone(),
                validator: IDS[0].into(),
                amount_udgt: 10000,
            },
        ],
    );
    let result = commit(&mut app, 1, vec![tx]);
    assert_ne!(result.tx_results[0].code, 0);
    assert!(result.validator_updates.is_empty());
    custody(&app, 200, 0, 0);
    assert_eq!(liquid(&app, &f.accounts[2].owner), 1000);
    assert!(lifecycle(&app).schedules.is_empty());
    let valid = f.accounts[2].sign(1, vec![f.register_message(&f.keys[2], IDS[2], 1, 20, 100)]);
    successful(&commit(&mut app, 2, vec![valid]));
    custody(&app, 200, 100, 0);
}

#[test]
fn corrupted_pending_lifecycle_fails_recovery_without_repair() {
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = f.initialized(&path);
    successful(&commit(&mut app, 1, vec![f.register()]));
    let mut state = lifecycle(&app);
    state.schedules.get_mut(&3).unwrap().additions[0].amount += 1;
    app.storage
        .db
        .put(STATE_KEY, bincode::serialize(&state).unwrap())
        .unwrap();
    let corrupted = data(&app);
    assert!(app.info().is_err());
    assert_eq!(data(&app), corrupted);
    drop(app);
    assert!(ConsensusApplication::open(&path, f.config.clone(), f.genesis.clone()).is_err());
}


#[test]
fn stored_unsigned_validator_operations_require_the_original_account_envelope() {
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("records")).unwrap();
    let messages = vec![
        f.register_message(&f.keys[2], IDS[2], 0, 20, 100),
        Msg::ValidatorRotateKey {
            from: f.accounts[2].owner.clone(),
            validator: IDS[2].into(),
            consensus_pubkey: f.keys[2].public.clone(),
            proof: f.keys[2].proof("rotate", IDS[2], &f.accounts[2].owner, 0, 20, 0),
            expires_at_height: 20,
        },
        Msg::ValidatorExit {
            from: f.accounts[2].owner.clone(),
            validator: IDS[2].into(),
        },
        Msg::ValidatorWithdraw {
            from: f.accounts[2].owner.clone(),
            unbond_id: "0".into(),
        },
    ];
    for message in messages {
        let bytes = f.accounts[2].sign(0, vec![message]);
        let wire: WireTransaction = serde_json::from_slice(&bytes).unwrap();
        let WireTransaction::Signed { envelope } = wire else {
            panic!("signed fixture")
        };
        let mut transaction = crate::signed_transaction::normalize(&envelope, 1).unwrap();
        transaction.public_key = None;
        transaction.signature = None;
        storage
            .put_pending_signed_transaction(&transaction, None)
            .unwrap();
        let stored = storage
            .get_transaction_record(&transaction.hash)
            .unwrap()
            .unwrap();
        let error = crate::signed_transaction::verify_record(&stored, Some(CHAIN))
            .unwrap_err()
            .to_string();
        assert!(error.contains("original signed envelope"), "{error}");
        assert!(
            crate::signed_transaction::verify_reward_mode_record(&stored, Some(CHAIN)).is_err()
        );
    }
}
