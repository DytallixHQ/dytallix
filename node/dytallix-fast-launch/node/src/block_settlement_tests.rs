//! Current supply and versioned settlement checks remain in the default suite.
//! Historical timer economics require the explicit legacy diagnostic feature.
use super::*;
use crate::runtime::emission::EmissionSchedule;
use std::sync::{Arc, Mutex};

struct Fixture {
    state: State,
    emission: EmissionEngine,
    staking: StakingModule,
    burn: FeeBurnEngine,
    dir: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        Self::funded(1_000_000, 0)
    }
    fn funded(alice_drt: u128, bob_drt: u128) -> Self {
        Self::with_stake(alice_drt, bob_drt, 0)
    }
    fn with_stake(alice_drt: u128, bob_drt: u128, stake: u128) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut storage = Storage::open(dir.path().join("node.db")).unwrap();
        let source = serde_json::json!({"chain_id":"test","accounts":[
            {"address":"alice","balances":{"udgt":"1000","udrt":alice_drt.to_string()}},
            {"address":"bob","balances":{"udrt":bob_drt.to_string()}}
        ], "staking":{"delegations":[{"delegator":"alice","amount_udgt":stake.to_string()}]}});
        crate::genesis::initialize(
            &mut storage,
            "test",
            Some(&serde_json::to_vec(&source).unwrap()),
        )
        .unwrap();
        let storage = Arc::new(storage);
        let mut state = State::new(storage.clone());
        state.get_account("alice");
        let mut emission = EmissionEngine::new(
            storage.clone(),
            Arc::new(Mutex::new(State::new(storage.clone()))),
        );
        emission.config.schedule = EmissionSchedule::Static { per_block: 100 };
        emission.config.initial_supply = alice_drt.checked_add(bob_drt).unwrap();
        let staking = StakingModule::new(storage);
        Self {
            state,
            emission,
            staking,
            burn: FeeBurnEngine::new(),
            dir,
        }
    }
    fn data(&self) -> Writes {
        self.state
            .storage
            .db
            .iterator(IteratorMode::Start)
            .map(|i| {
                let (k, v) = i.unwrap();
                (k.to_vec(), v.to_vec())
            })
            .collect()
    }
}
fn tx(hash: &str, nonce: u64, amount: u128) -> Transaction {
    Transaction::new(hash, "alice", "bob", amount, 50_000, nonce, None).with_gas(50_000, 1)
}


#[test]
fn asset_acknowledgement_keeps_later_arrivals_and_rejects_changed_prefix() {
    let mut pending = vec!["first".into(), "second".into()];
    let snapshot = pending.clone();
    pending.push("later".into());
    acknowledge_assets(&mut pending, &snapshot).unwrap();
    assert_eq!(pending, ["later"]);
    let before = pending.clone();
    assert!(acknowledge_assets(&mut pending, &snapshot).is_err());
    assert_eq!(before, pending);
}

#[test]
fn legacy_block_profiles_reject_all_selections() {
    for profile in [
        None,
        Some("development"),
        Some("mainnet"),
        Some("Development"),
    ] {
        for (governance, dev_endpoints) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            assert!(check_profile(profile, governance, dev_endpoints).is_err());
        }
    }
}








#[test]
fn supply_overlay_replaces_balances_and_checks_before_publication() {
    let f = Fixture::new();
    let before = f.data();
    let mut writes = Writes::new();
    writes.insert(
        b"acct:balances:alice".to_vec(),
        bincode::serialize(&BTreeMap::from([
            ("udgt".to_string(), 1000u128),
            ("udrt".to_string(), 950_000u128),
        ]))
        .unwrap(),
    );
    writes.insert(
        settlement::FEE_KEY.as_bytes().to_vec(),
        bincode::serialize(&50_000u128).unwrap(),
    );
    let planned = crate::supply::validate(&f.state.storage, &writes).unwrap();
    assert_eq!(
        (planned.liquid, planned.withheld_fees, planned.total),
        (950_000, 50_000, 1_000_000)
    );
    writes.insert(
        settlement::FEE_KEY.as_bytes().to_vec(),
        bincode::serialize(&49_999u128).unwrap(),
    );
    assert!(crate::supply::validate(&f.state.storage, &writes).is_err());
    assert_eq!(f.data(), before);
}

#[test]
fn supply_requires_complete_supported_records_and_exact_encoding() {
    let f = Fixture::new();
    let before = f.data();
    for (key, value) in [
        ("supply:drt_genesis", vec![0; 15]),
        ("supply:drt_burned", bincode::serialize(&0u128).unwrap()),
        ("genesis:monetary:v1", vec![1]),
        ("emission:pool:unknown", bincode::serialize(&0u128).unwrap()),
        (
            "emission:circulating_supply",
            bincode::serialize(&1u128).unwrap(),
        ),
        ("emission:last_height", 1u64.to_be_bytes().to_vec()),
        ("acct:balances:alice", vec![0; 3]),
    ] {
        let writes = Writes::from([(key.as_bytes().to_vec(), value)]);
        assert!(
            crate::supply::validate(&f.state.storage, &writes).is_err(),
            "{key}"
        );
    }
    assert_eq!(f.data(), before);
}

#[test]
fn supply_checks_full_width_custody_sums() {
    let f = Fixture::funded(u128::MAX, 0);
    for key in ["execution:v1:withheld_udrt", "emission:pool:block_rewards"] {
        let writes = Writes::from([(key.as_bytes().to_vec(), bincode::serialize(&1u128).unwrap())]);
        assert!(crate::supply::validate(&f.state.storage, &writes).is_err());
    }
    let writes = Writes::from([(
        b"acct:balances:bob".to_vec(),
        bincode::serialize(&BTreeMap::from([("udrt".to_string(), 1u128)])).unwrap(),
    )]);
    assert!(crate::supply::validate(&f.state.storage, &writes).is_err());
}


#[test]
fn dgt_overlay_checks_individual_stake_against_total_and_issuance() {
    use crate::runtime::staking::DelegatorRewardRecord;
    let f = Fixture::new();
    let before = f.data();
    let mut writes = Writes::from([
        (
            b"acct:balances:alice".to_vec(),
            bincode::serialize(&BTreeMap::from([
                ("udgt".to_string(), 600u128),
                ("udrt".to_string(), 1_000_000u128),
            ]))
            .unwrap(),
        ),
        (
            b"staking:total_stake".to_vec(),
            bincode::serialize(&400u128).unwrap(),
        ),
        (
            b"staking:delegator:alice".to_vec(),
            bincode::serialize(&DelegatorRewardRecord {
                stake_amount: 400,
                ..Default::default()
            })
            .unwrap(),
        ),
    ]);
    let supply = crate::supply::validate_native(&f.state.storage, &writes).unwrap();
    assert_eq!(
        (supply.dgt.liquid, supply.dgt.staked, supply.dgt.issued),
        (600, 400, 1000)
    );
    writes.insert(
        b"staking:total_stake".to_vec(),
        bincode::serialize(&399u128).unwrap(),
    );
    assert!(crate::supply::validate_native(&f.state.storage, &writes)
        .unwrap_err()
        .to_string()
        .contains("delegator records"));
    writes.insert(
        b"staking:total_stake".to_vec(),
        bincode::serialize(&400u128).unwrap(),
    );
    writes.insert(
        b"supply:dgt_minted".to_vec(),
        bincode::serialize(&999u128).unwrap(),
    );
    assert!(crate::supply::validate_native(&f.state.storage, &writes)
        .unwrap_err()
        .to_string()
        .contains("DGT conservation"));
    assert_eq!(f.data(), before);
}

#[test]
fn dgt_supply_rejects_unknown_or_incomplete_record_formats() {
    let f = Fixture::new();
    let before = f.data();
    for (key, value) in [
        ("supply:dgt_minted", vec![0; 17]),
        ("supply:dgt_burned", bincode::serialize(&0u128).unwrap()),
        ("staking:total_stake", vec![0; 15]),
        (
            "staking:unbonding:alice",
            bincode::serialize(&0u128).unwrap(),
        ),
        ("staking:delegator:alice", vec![0; 49]),
        ("staking:delegator:", vec![0; 48]),
        ("staking:delegator:missing", vec![0; 48]),
    ] {
        let writes = Writes::from([(key.as_bytes().to_vec(), value)]);
        assert!(
            crate::supply::validate_native(&f.state.storage, &writes).is_err(),
            "{key}"
        );
    }
    assert_eq!(f.data(), before);
}

#[test]
fn dgt_supply_requires_issuance_and_stake_counters() {
    for key in ["supply:dgt_minted", "staking:total_stake"] {
        let f = Fixture::new();
        f.state.storage.db.delete(key).unwrap();
        let before = f.data();
        assert!(verify_recovery(&f.state.storage).is_err());
        assert_eq!(f.data(), before);
    }
}

#[test]
fn dgt_supply_checks_cap_and_all_aggregate_sums() {
    use crate::runtime::staking::DelegatorRewardRecord;
    let f = Fixture::new();
    let amount = |v: u128| bincode::serialize(&v).unwrap();
    let account = |v: u128| bincode::serialize(&BTreeMap::from([("udgt".to_string(), v)])).unwrap();
    let record = |v: u128| {
        bincode::serialize(&DelegatorRewardRecord {
            stake_amount: v,
            ..Default::default()
        })
        .unwrap()
    };
    let cases = [
        Writes::from([(
            b"supply:dgt_minted".to_vec(),
            amount(crate::state::DGT_MAX_SUPPLY + 1),
        )]),
        Writes::from([(b"acct:balances:bob".to_vec(), account(u128::MAX))]),
        Writes::from([
            (b"staking:delegator:alice".to_vec(), record(u128::MAX)),
            (b"staking:delegator:bob".to_vec(), record(1)),
        ]),
        Writes::from([
            (b"staking:delegator:alice".to_vec(), record(u128::MAX)),
            (b"staking:total_stake".to_vec(), amount(u128::MAX)),
        ]),
    ];
    let before = f.data();
    for writes in cases {
        assert!(crate::supply::validate_native(&f.state.storage, &writes).is_err());
    }
    assert_eq!(f.data(), before);
}

#[test]
fn dgt_ratio_distinguishes_issued_supply_from_cap_and_handles_zero() {
    use crate::supply::DgtSupply;
    let make = |issued, staked| DgtSupply {
        height: 0,
        issued,
        staked,
        pending_bonded: 0,
        penalty_reserve: 0,
        governance_escrow: None,
        unbonding: 0,
        liquid: issued - staked,
    };
    assert_eq!(make(0, 0).stake_basis_points().unwrap(), 0);
    assert_eq!(make(3, 1).stake_basis_points().unwrap(), 3333);
    assert_eq!(
        make(crate::state::DGT_MAX_SUPPLY, crate::state::DGT_MAX_SUPPLY)
            .stake_basis_points()
            .unwrap(),
        10000
    );
    assert!(make(crate::state::DGT_MAX_SUPPLY + 1, 0)
        .stake_basis_points()
        .is_err());
    let value = serde_json::to_value(make(1000, 400).response()).unwrap();
    assert_eq!(value["issued"], "1000");
    assert_eq!(value["liquid"], "600");
    assert_eq!(value["staked"], "400");
    assert_eq!(value["cap"], crate::state::DGT_MAX_SUPPLY.to_string());
}


fn reward_write_fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let mut storage = Storage::open(dir.path().join("node.db")).unwrap();
    let source = serde_json::json!({
        "chain_id": "test",
        "accounts": [{"address": "alice", "balances": {"udgt": "1000", "udrt": "1000000"}, "vesting": {"kind": "unlocked"}}],
        "staking": {"delegations": [{"delegator": "alice", "amount_udgt": "100"}]},
        "reward_v2": {"version": 2, "activation_height": 1, "decimals": 6, "profile": "development",
            "max_validators": 4, "max_positions": 8,
            "validators": [{"address": "fixture-validator", "active": true, "jailed": false}],
            "positions": [{"owner": "alice", "validator": "fixture-validator", "amount_udgt": "100"}]}
    });
    crate::genesis::initialize(
        &mut storage,
        "test",
        Some(&serde_json::to_vec(&source).unwrap()),
    )
    .unwrap();
    let storage = Arc::new(storage);
    let mut state = State::new(storage.clone());
    state.get_account("alice");
    let mut emission = EmissionEngine::new(
        storage.clone(),
        Arc::new(Mutex::new(State::new(storage.clone()))),
    );
    emission.config.schedule = EmissionSchedule::Static { per_block: 100 };
    emission.config.initial_supply = 1_000_000;
    Fixture {
        state,
        emission,
        staking: StakingModule::new(storage),
        burn: FeeBurnEngine::new(),
        dir,
    }
}

#[test]
fn reward_v2_prewrite_failure_preserves_pool_liability_and_interval_state() {
    use crate::runtime::reward_runtime::{RewardState, REWARD_STATE_KEY};
    let mut f = reward_write_fixture();
    let before = f.data();
    let accounts = format!("{:?}", f.state.accounts);
    let request = BlockRequest {
        height: 1,
        transactions: &[],
        assets: &[],
        timestamp: 10,
        empty_blocks: true,
    };
    let error = commit_reward_with(
        &mut f.state,
        &mut f.emission,
        &mut f.staking,
        &mut f.burn,
        true,
        &request,
        true,
        |_, batch| {
            assert!(batch.len() > 10);
            bail!("Injected reward pre-write failure")
        },
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("Injected reward pre-write failure"));
    assert_eq!(f.data(), before);
    assert_eq!(format!("{:?}", f.state.accounts), accounts);
    assert_eq!(f.emission.circulating_supply, 0);
    assert!(f.burn.burn_events.is_empty());
    let reward =
        RewardState::decode(&f.state.storage.db.get(REWARD_STATE_KEY).unwrap().unwrap()).unwrap();
    assert_eq!(reward.last_height, 0);
    assert_eq!(reward.total_budget, 0);
    assert!(reward.unpaid.is_empty());
    assert!(crate::supply::inspect_native(&f.state.storage)
        .unwrap()
        .drt
        .pools
        .values()
        .all(|amount| *amount == 0));
    commit_reward_development_block(
        &mut f.state,
        &mut f.emission,
        &mut f.staking,
        &mut f.burn,
        true,
        &request,
    )
    .unwrap();
    let reward =
        RewardState::decode(&f.state.storage.db.get(REWARD_STATE_KEY).unwrap().unwrap()).unwrap();
    assert_eq!(reward.unpaid["alice"], 25);
    assert_eq!(
        crate::supply::inspect(&f.state.storage).unwrap().pools["staking_rewards"],
        25
    );
    verify_recovery(&f.state.storage).unwrap();
}

#[test]
fn reward_v2_lost_ack_retry_keeps_one_funded_entitlement() {
    use crate::runtime::reward_runtime::{RewardState, REWARD_STATE_KEY};
    let mut f = reward_write_fixture();
    let request = BlockRequest {
        height: 1,
        transactions: &[],
        assets: &[],
        timestamp: 10,
        empty_blocks: true,
    };
    let error = commit_reward_with(
        &mut f.state,
        &mut f.emission,
        &mut f.staking,
        &mut f.burn,
        true,
        &request,
        true,
        |storage, batch| {
            let mut options = WriteOptions::default();
            options.set_sync(true);
            storage.db.write_opt(batch, &options)?;
            bail!("Injected reward lost acknowledgement")
        },
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("Injected reward lost acknowledgement"));
    assert_eq!(f.emission.circulating_supply, 0);
    verify_recovery(&f.state.storage).unwrap();
    let before = f.data();
    commit_reward_with(
        &mut f.state,
        &mut f.emission,
        &mut f.staking,
        &mut f.burn,
        true,
        &request,
        true,
        |_, _| panic!("Matching committed reward retry must not write"),
    )
    .unwrap();
    assert_eq!(f.data(), before);
    assert_eq!(f.emission.circulating_supply, 100);
    let reward =
        RewardState::decode(&f.state.storage.db.get(REWARD_STATE_KEY).unwrap().unwrap()).unwrap();
    assert_eq!(reward.last_height, 1);
    assert_eq!(reward.total_budget, 25);
    assert_eq!(reward.total_claimed, 0);
    assert_eq!(reward.unpaid["alice"], 25);
    let supply = crate::supply::inspect_native(&f.state.storage).unwrap();
    assert_eq!(supply.drt.pools["staking_rewards"], 25);
    assert_eq!(supply.drt.emitted, 100);
    assert_eq!(supply.drt.total, 1_000_100);
    assert_eq!(
        supply.drt.liquid + supply.drt.withheld_fees + supply.drt.pools.values().sum::<u128>(),
        supply.drt.total
    );
    assert_eq!(
        supply.dgt.issued,
        supply.dgt.liquid + supply.dgt.staked + supply.dgt.unbonding
    );
}

mod reward_claim_write_recovery {
    use super::*;
    use crate::crypto::{ActivePQC, PQC};
    use crate::runtime::reward_runtime::{RewardState, REWARD_STATE_KEY};
    use crate::storage::receipts::TxStatus;
    use crate::storage::transaction_record::SignedEnvelope;
    use crate::types::{tx::Tx, Msg, SignedTx};

    const INITIAL_DRT: u128 = 1_000_000;
    const CLAIM_FEE: u128 = 50_000;

    fn setup() -> (Fixture, Transaction) {
        let (secret, public) = ActivePQC::keypair();
        let algorithm = crate::addr::OriginKeyAlgorithm::MlDsa65;
        let owner = crate::addr::initial_address(
            crate::addr::AddressNetwork::Development,
            "test",
            algorithm,
            &public,
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut storage = Storage::open(dir.path().join("node.db")).unwrap();
        let source = serde_json::json!({
            "chain_id": "test", "accounts": [{"address": owner,
                "balances": {"udgt": "1000", "udrt": INITIAL_DRT.to_string()}, "vesting": {"kind": "unlocked"}}],
            "staking": {"delegations": [{"delegator": owner, "amount_udgt": "100"}]},
            "reward_v2": {"version": 2, "activation_height": 1, "decimals": 6, "profile": "development",
                "max_validators": 4, "max_positions": 8,
                "validators": [{"address": "fixture-validator", "active": true, "jailed": false}],
                "positions": [{"owner": owner, "validator": "fixture-validator", "amount_udgt": "100"}]}
        });
        crate::genesis::initialize(
            &mut storage,
            "test",
            Some(&serde_json::to_vec(&source).unwrap()),
        )
        .unwrap();
        let storage = Arc::new(storage);
        let mut emission = EmissionEngine::new(
            storage.clone(),
            Arc::new(Mutex::new(State::new(storage.clone()))),
        );
        // Synthetic 60/25/10/5 allocation: each 100-unit interval funds 25 reward units.
        emission.config.schedule = EmissionSchedule::Static { per_block: 100 };
        emission.config.initial_supply = INITIAL_DRT;
        let mut fixture = Fixture {
            state: State::new(storage.clone()),
            emission,
            staking: StakingModule::new(storage),
            burn: FeeBurnEngine::new(),
            dir,
        };
        commit_reward_development_block(
            &mut fixture.state,
            &mut fixture.emission,
            &mut fixture.staking,
            &mut fixture.burn,
            true,
            &BlockRequest {
                height: 1,
                transactions: &[],
                assets: &[],
                timestamp: 10,
                empty_blocks: true,
            },
        )
        .unwrap();
        let signed = SignedTx::sign(
            Tx {
                chain_id: "test".into(),
                nonce: 0,
                msgs: vec![Msg::RewardClaim {
                    from: owner.clone(),
                }],
                fee: CLAIM_FEE,
                memo: "claim recovery fixture".into(),
            },
            &secret,
            &public,
        )
        .unwrap();
        let transaction = crate::signed_transaction::normalize(&signed, 1).unwrap();
        let envelope: SignedEnvelope =
            serde_json::from_value(serde_json::to_value(&signed).unwrap()).unwrap();
        fixture
            .state
            .storage
            .put_pending_signed_transaction(&transaction, Some(envelope))
            .unwrap();
        fixture.state.get_account(&owner);
        assert_eq!(reward(&fixture).unpaid[&owner], 25);
        assert_eq!(
            crate::supply::inspect(&fixture.state.storage)
                .unwrap()
                .pools["staking_rewards"],
            25
        );
        (fixture, transaction)
    }

    fn reward(fixture: &Fixture) -> RewardState {
        RewardState::decode(
            &fixture
                .state
                .storage
                .db
                .get(REWARD_STATE_KEY)
                .unwrap()
                .unwrap(),
        )
        .unwrap()
    }

    fn reopen(fixture: Fixture) -> Fixture {
        let Fixture {
            state,
            emission,
            staking,
            burn,
            dir,
        } = fixture;
        let config = emission.config.clone();
        // Every Arc<Storage>, including the emission state cache, must close first.
        drop(state);
        drop(emission);
        drop(staking);
        drop(burn);
        let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
        verify_recovery(&storage).unwrap();
        let emission = EmissionEngine::new_with_config(
            storage.clone(),
            Arc::new(Mutex::new(State::new(storage.clone()))),
            config,
        );
        Fixture {
            state: State::new(storage.clone()),
            emission,
            staking: StakingModule::new(storage),
            burn: FeeBurnEngine::new(),
            dir,
        }
    }

    fn assert_committed_claim(fixture: &mut Fixture, transaction: &Transaction) {
        let reward = reward(fixture);
        // The claim includes its 25-unit parent-state allocation for block 2.
        assert_eq!(reward.last_height, 2);
        assert_eq!(reward.total_budget, 50);
        assert_eq!(reward.total_claimed, 50);
        assert!(reward.unpaid.is_empty());
        assert_eq!((reward.rounding_reserve, reward.inactive_reserve), (0, 0));
        assert_eq!(
            fixture.state.get_balance(&transaction.from, "udrt"),
            INITIAL_DRT - CLAIM_FEE + 50
        );
        assert_eq!(fixture.state.nonce_of(&transaction.from), 1);
        let receipt = fixture
            .state
            .storage
            .get_receipt(&transaction.hash)
            .unwrap();
        assert_eq!(receipt.status, TxStatus::Success);
        assert!(receipt.success);
        assert_eq!(receipt.block_height, Some(2));
        assert_eq!(receipt.index, Some(0));
        assert_eq!(receipt.nonce, 0);
        assert_eq!(receipt.fee, CLAIM_FEE);
        assert_eq!(receipt.gas_limit, CLAIM_FEE as u64);
        assert_eq!(receipt.gas_price, 1);
        assert!(receipt.gas_used > 0 && receipt.gas_used <= receipt.gas_limit);
        let record = fixture
            .state
            .storage
            .get_transaction_record(&transaction.hash)
            .unwrap()
            .unwrap();
        assert!(record.matches(transaction).unwrap());
        assert!(record.signed_envelope.is_some());
        let supply = crate::supply::inspect_native(&fixture.state.storage).unwrap();
        assert_eq!(supply.drt.genesis, INITIAL_DRT);
        assert_eq!(supply.drt.emitted, 200);
        assert_eq!(supply.drt.total, INITIAL_DRT + 200);
        assert_eq!(supply.drt.pools["staking_rewards"], 0);
        assert_eq!(supply.drt.pools.values().sum::<u128>(), 150);
        assert_eq!(supply.drt.withheld_fees, CLAIM_FEE);
        assert_eq!(supply.drt.liquid, INITIAL_DRT - CLAIM_FEE + 50);
        assert_eq!(
            supply.drt.liquid + supply.drt.withheld_fees + supply.drt.pools.values().sum::<u128>(),
            supply.drt.total
        );
        assert_eq!(
            supply.dgt.issued,
            supply.dgt.liquid + supply.dgt.staked + supply.dgt.unbonding
        );
        verify_recovery(&fixture.state.storage).unwrap();
    }

    #[test]
    fn signed_claim_prewrite_failure_preserves_liability_pool_liquid_nonce_and_receipt() {
        let (mut fixture, transaction) = setup();
        let request = BlockRequest {
            height: 2,
            transactions: std::slice::from_ref(&transaction),
            assets: &[],
            timestamp: 20,
            empty_blocks: true,
        };
        let before = fixture.data();
        let supply_before = crate::supply::inspect_native(&fixture.state.storage).unwrap();
        let reward_before = reward(&fixture);
        let accounts_before = format!("{:?}", fixture.state.accounts);
        let error = commit_reward_with(
            &mut fixture.state,
            &mut fixture.emission,
            &mut fixture.staking,
            &mut fixture.burn,
            true,
            &request,
            true,
            |_, batch| {
                assert!(batch.len() > 10);
                bail!("Injected signed claim pre-write failure")
            },
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("Injected signed claim pre-write failure"));
        assert_eq!(fixture.data(), before);
        assert_eq!(
            crate::supply::inspect_native(&fixture.state.storage).unwrap(),
            supply_before
        );
        assert_eq!(reward(&fixture), reward_before);
        assert_eq!(format!("{:?}", fixture.state.accounts), accounts_before);
        assert_eq!(
            fixture.state.get_balance(&transaction.from, "udrt"),
            INITIAL_DRT
        );
        assert_eq!(fixture.state.nonce_of(&transaction.from), 0);
        let receipt = fixture
            .state
            .storage
            .get_receipt(&transaction.hash)
            .unwrap();
        assert_eq!(receipt.status, TxStatus::Pending);
        assert_eq!(receipt.block_height, None);
        assert_eq!(fixture.emission.circulating_supply, 100);
        assert_eq!(reward(&fixture).unpaid[&transaction.from], 25);
        fixture = reopen(fixture);
        assert_eq!(fixture.data(), before);
        commit_reward_development_block(
            &mut fixture.state,
            &mut fixture.emission,
            &mut fixture.staking,
            &mut fixture.burn,
            true,
            &request,
        )
        .unwrap();
        assert_committed_claim(&mut fixture, &transaction);
    }

    #[test]
    fn signed_claim_lost_ack_reopen_retry_pays_and_charges_exactly_once() {
        let (mut fixture, transaction) = setup();
        let request = BlockRequest {
            height: 2,
            transactions: std::slice::from_ref(&transaction),
            assets: &[],
            timestamp: 20,
            empty_blocks: true,
        };
        let error = commit_reward_with(
            &mut fixture.state,
            &mut fixture.emission,
            &mut fixture.staking,
            &mut fixture.burn,
            true,
            &request,
            true,
            |storage, batch| {
                let mut options = WriteOptions::default();
                options.set_sync(true);
                storage.db.write_opt(batch, &options)?;
                bail!("Injected signed claim lost acknowledgement")
            },
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("Injected signed claim lost acknowledgement"));
        // Failure prevented cache publication even though the atomic write committed.
        assert_eq!(fixture.emission.circulating_supply, 100);
        let committed = fixture.data();
        let receipt_before = fixture
            .state
            .storage
            .db
            .get(format!("rcpt:{}", transaction.hash))
            .unwrap()
            .unwrap();
        fixture = reopen(fixture);
        assert_eq!(fixture.data(), committed);
        assert_committed_claim(&mut fixture, &transaction);
        let outcome = commit_reward_with(
            &mut fixture.state,
            &mut fixture.emission,
            &mut fixture.staking,
            &mut fixture.burn,
            true,
            &request,
            true,
            |_, _| panic!("Committed signed claim retry must not write"),
        )
        .unwrap();
        assert_eq!(outcome.receipts.len(), 1);
        assert_eq!(outcome.receipts[0].tx_hash, transaction.hash);
        assert!(outcome.receipts[0].success);
        assert_eq!(fixture.data(), committed);
        assert_eq!(
            fixture
                .state
                .storage
                .db
                .get(format!("rcpt:{}", transaction.hash))
                .unwrap()
                .unwrap(),
            receipt_before
        );
        assert_committed_claim(&mut fixture, &transaction);
        assert_eq!(fixture.emission.circulating_supply, 200);
    }
}

#[test]
fn retired_timer_rejects_empty_funded_and_staked_requests_without_mutation() {
    for (empty, staking_enabled) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut f = Fixture::with_stake(1_000_000, 0, 3);
        let input = [tx("retired", 0, 100)];
        let assets = vec!["retired-asset".into()];
        for transactions in [&[][..], &input[..]] {
            let before = f.data();
            let accounts = format!("{:?}", f.state.accounts);
            let issuance = f.emission.circulating_supply;
            let index = f.staking.reward_index;
            let residual = f.staking.reward_index_residual;
            let pending = f.staking.pending_staking_emission;
            let burn_count = f.burn.burn_events.len();
            assert!(commit_block(
                &mut f.state,
                &mut f.emission,
                &mut f.staking,
                &mut f.burn,
                staking_enabled,
                &BlockRequest {
                    height: 1,
                    transactions,
                    assets: &assets,
                    timestamp: 10,
                    empty_blocks: empty
                }
            )
            .is_err());
            assert_eq!(f.data(), before);
            assert_eq!(format!("{:?}", f.state.accounts), accounts);
            assert_eq!(f.emission.circulating_supply, issuance);
            assert_eq!(f.staking.reward_index, index);
            assert_eq!(f.staking.reward_index_residual, residual);
            assert_eq!(f.staking.pending_staking_emission, pending);
            assert_eq!(f.burn.burn_events.len(), burn_count);
            assert_eq!(assets, ["retired-asset"]);
        }
    }
}

#[test]
fn retired_timer_never_invokes_the_storage_writer() {
    let mut f = Fixture::new();
    let input = [tx("retired-writer", 0, 100)];
    let before = f.data();
    let accounts = format!("{:?}", f.state.accounts);
    assert!(commit_with(
        &mut f.state,
        &mut f.emission,
        &mut f.staking,
        &mut f.burn,
        true,
        &BlockRequest {
            height: 1,
            transactions: &input,
            assets: &[],
            timestamp: 10,
            empty_blocks: true
        },
        |_, _| panic!("Retired timer must reject before the write callback")
    )
    .is_err());
    assert_eq!(f.data(), before);
    assert_eq!(format!("{:?}", f.state.accounts), accounts);
    assert_eq!(f.emission.circulating_supply, 0);
    assert_eq!(f.staking.pending_staking_emission, 0);
    assert!(f.burn.burn_events.is_empty());
}
