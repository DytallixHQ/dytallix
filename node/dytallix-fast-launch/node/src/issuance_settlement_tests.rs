//! Development adapter checks. These fixtures do not establish consensus finality.
use super::*;
use crate::runtime::emission::{EmissionEvent, EmissionSchedule};
use crate::runtime::issuance_timing::{EpochObservation, TimingState, TIMING_STATE_KEY};
use crate::runtime::reward_runtime::{RewardState, REWARD_STATE_KEY};
use std::sync::{Arc, Mutex};

const INITIAL_DRT: u128 = 1_000_000;

struct Fixture {
    state: State,
    emission: EmissionEngine,
    staking: StakingModule,
    burn: FeeBurnEngine,
    dir: tempfile::TempDir,
}

fn source(owner: &str) -> serde_json::Value {
    serde_json::json!({
        "chain_id": "test",
        "accounts": [{"address": owner, "balances": {"udgt": "1000", "udrt": INITIAL_DRT.to_string()},
            "vesting": {"kind": "unlocked"}}],
        "staking": {"delegations": [{"delegator": owner, "amount_udgt": "100"}]},
        "reward_v2": {"version": 2, "activation_height": 1, "decimals": 6, "profile": "development",
            "max_validators": 4, "max_positions": 8,
            "validators": [{"address": "fixture-validator", "active": true, "jailed": false}],
            "positions": [{"owner": owner, "validator": "fixture-validator", "amount_udgt": "100"}]},
        "adaptive_issuance": {
            "version": 1, "profile": "development", "decimals": 6, "epoch_blocks": 3,
            "initial_epoch_budget_udrt": "1001", "max_recorded_epochs": 16,
            "controller": {
                "target_ppm": 500000, "shock_threshold_ppm": 200000,
                "volatility_threshold_ppm": 300000, "window_samples": 2,
                "integral_min": -2000000, "integral_max": 2000000,
                "soft": {"proportional": 1000, "integral": 0, "derivative": 0},
                "hard": {"proportional": 2000, "integral": 0, "derivative": 0},
                "base_udrt": 1000, "min_udrt": 500, "max_udrt": 2000
            }
        }
    })
}

fn database(storage: &Storage) -> Writes {
    storage
        .db
        .iterator(IteratorMode::Start)
        .map(|entry| {
            let (key, value) = entry.unwrap();
            (key.to_vec(), value.to_vec())
        })
        .collect()
}

impl Fixture {
    fn new(owner: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut storage = Storage::open(dir.path().join("node.db")).unwrap();
        crate::genesis::initialize(
            &mut storage,
            "test",
            Some(&serde_json::to_vec(&source(owner)).unwrap()),
        )
        .unwrap();
        let storage = Arc::new(storage);
        let mut emission = EmissionEngine::new(
            storage.clone(),
            Arc::new(Mutex::new(State::new(storage.clone()))),
        );
        // This incompatible legacy schedule must never determine timed issuance.
        emission.config.schedule = EmissionSchedule::Static { per_block: 999_999 };
        emission.config.initial_supply = INITIAL_DRT;
        let mut state = State::new(storage.clone());
        state.get_account(owner);
        Self {
            state,
            emission,
            staking: StakingModule::new(storage),
            burn: FeeBurnEngine::new(),
            dir,
        }
    }

    fn run(&mut self, height: u64, observation: Option<&EpochObservation>) -> Result<BlockOutcome> {
        commit_issuance_development_block(
            &mut self.state,
            &mut self.emission,
            &mut self.staking,
            &mut self.burn,
            true,
            &request(height, &[], true),
            observation,
        )
    }

    fn first_epoch(&mut self) {
        for height in 1..=3 {
            self.run(height, None).unwrap();
        }
    }

    fn observation(&self) -> EpochObservation {
        EpochObservation {
            epoch: 0,
            utilization_ppm: 0,
            volatility_ppm: 0,
            first_height: 1,
            last_height: 3,
            parent_hash: self.state.storage.get_block_by_height(3).unwrap().hash,
        }
    }

    fn timing(&self) -> TimingState {
        TimingState::decode(
            &self
                .state
                .storage
                .db
                .get(TIMING_STATE_KEY)
                .unwrap()
                .unwrap(),
        )
        .unwrap()
    }

    fn reward(&self) -> RewardState {
        RewardState::decode(
            &self
                .state
                .storage
                .db
                .get(REWARD_STATE_KEY)
                .unwrap()
                .unwrap(),
        )
        .unwrap()
    }

    fn data(&self) -> Writes {
        database(&self.state.storage)
    }

    fn reopen(self) -> Self {
        let Self {
            state,
            emission,
            staking,
            burn,
            dir,
        } = self;
        let config = emission.config.clone();
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
        Self {
            state: State::new(storage.clone()),
            emission,
            staking: StakingModule::new(storage),
            burn: FeeBurnEngine::new(),
            dir,
        }
    }
}

fn request(height: u64, transactions: &[Transaction], empty_blocks: bool) -> BlockRequest<'_> {
    BlockRequest {
        height,
        transactions,
        assets: &[],
        timestamp: height * 10,
        empty_blocks,
    }
}

fn event_pools(f: &Fixture, height: u64) -> [u128; 4] {
    let event: EmissionEvent = bincode::deserialize(
        &f.state
            .storage
            .db
            .get(format!("emission:event:{height}"))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(event.pools.len(), 4);
    let pools = [
        event.pools["validator_rewards"],
        event.pools["staking_rewards"],
        event.pools["treasury"],
        event.pools["issuance_reserve"],
    ];
    assert_eq!(pools.iter().sum::<u128>(), event.total_emitted);
    pools
}

fn assert_custody(f: &Fixture, emitted: u128, staking_claimed: u128) {
    let native = crate::supply::inspect_native(&f.state.storage).unwrap();
    assert_eq!(native.drt.emitted, emitted);
    assert_eq!(native.drt.total, INITIAL_DRT + emitted);
    assert_eq!(
        native.drt.total,
        native.drt.liquid + native.drt.withheld_fees + native.drt.pools.values().sum::<u128>()
    );
    assert_eq!(
        native.dgt.issued,
        native.dgt.liquid + native.dgt.staked + native.dgt.unbonding
    );
    assert_eq!(f.reward().total_claimed, staking_claimed);
    verify_recovery(&f.state.storage).unwrap();
}

#[test]
fn epoch_zero_and_controller_epoch_issue_all_four_pools_exactly_once_per_block() {
    let mut f = Fixture::new("alice");
    assert_eq!(f.timing().last_height, 0);
    assert_eq!(f.timing().epoch_budget_udrt, 1001);
    let expected = [[134, 100, 100, 1], [133, 100, 100, 0], [133, 100, 100, 0]];
    let mut issued = 0;
    for (index, pools) in expected.into_iter().enumerate() {
        let height = index as u64 + 1;
        f.run(height, None).unwrap();
        assert_eq!(event_pools(&f, height), pools);
        issued += pools.iter().sum::<u128>();
        assert_eq!(f.timing().total_issued.total().unwrap(), issued);
        assert_custody(&f, issued, 0);
    }
    assert_eq!(issued, 1001);
    assert_eq!(f.reward().unpaid["alice"], 300);
    assert!(!f
        .data()
        .keys()
        .any(|key| key.starts_with(b"adaptive:v1:event:")));
    let observation = f.observation();
    // Error 500000 ppm with hard gain 2000 adds 1000 to the base 1000.
    let expected = [[267, 200, 200, 0], [267, 200, 200, 0], [266, 200, 200, 0]];
    for (index, pools) in expected.into_iter().enumerate() {
        let height = index as u64 + 4;
        f.run(
            height,
            if height == 4 {
                Some(&observation)
            } else {
                None
            },
        )
        .unwrap();
        assert_eq!(event_pools(&f, height), pools);
        issued += pools.iter().sum::<u128>();
        assert_eq!(f.timing().epoch_budget_udrt, 2000);
        assert_custody(&f, issued, 0);
    }
    assert_eq!(issued, 3001);
    assert_eq!(f.timing().active_epoch, 1);
    assert_eq!(f.reward().unpaid["alice"], 900);
    assert_eq!(
        f.data()
            .keys()
            .filter(|key| key.starts_with(b"adaptive:v1:event:"))
            .count(),
        1
    );
}

#[test]
fn missing_wrong_parent_range_epoch_and_nonboundary_observations_change_no_state() {
    let mut f = Fixture::new("alice");
    let early = EpochObservation {
        epoch: 0,
        utilization_ppm: 0,
        volatility_ppm: 0,
        first_height: 1,
        last_height: 3,
        parent_hash: "not-a-parent".into(),
    };
    let initial = f.data();
    assert!(f.run(1, Some(&early)).is_err());
    assert_eq!(f.data(), initial);
    f.first_epoch();
    let correct = f.observation();
    let before = f.data();
    assert!(f.run(4, None).is_err());
    assert_eq!(f.data(), before);
    for variant in 0..5 {
        let mut bad = correct.clone();
        match variant {
            0 => bad.parent_hash = "wrong-parent".into(),
            1 => bad.first_height = 2,
            2 => bad.last_height = 2,
            3 => bad.epoch = 1,
            4 => bad.utilization_ppm = 1_000_001,
            _ => unreachable!(),
        }
        assert!(f.run(4, Some(&bad)).is_err(), "variant {variant}");
        assert_eq!(f.data(), before, "variant {variant}");
        assert_eq!(f.emission.circulating_supply, 1001);
    }
    f.run(4, Some(&correct)).unwrap();
    let committed = f.data();
    assert!(f.run(5, Some(&correct)).is_err());
    assert_eq!(f.data(), committed);
    let mut altered_retry = correct.clone();
    altered_retry.utilization_ppm = 1;
    assert!(f.run(4, Some(&altered_retry)).is_err());
    assert_eq!(f.data(), committed);
}

#[test]
fn empty_boundary_tick_does_not_commit_observation_budget_or_supply() {
    let mut f = Fixture::new("alice");
    f.first_epoch();
    let observation = f.observation();
    let before = f.data();
    let result = commit_issuance_development_block(
        &mut f.state,
        &mut f.emission,
        &mut f.staking,
        &mut f.burn,
        true,
        &request(4, &[], false),
        Some(&observation),
    )
    .unwrap();
    assert!(result.block.is_none());
    assert_eq!(f.data(), before);
    assert_eq!(f.emission.circulating_supply, 1001);
    assert_eq!(f.timing().active_epoch, 0);
    assert_eq!(f.timing().last_height, 3);
    f.run(4, Some(&observation)).unwrap();
    assert_eq!(f.timing().active_epoch, 1);
}

#[test]
fn boundary_prewrite_failure_preserves_all_records_and_retry_commits_once() {
    let mut f = Fixture::new("alice");
    f.first_epoch();
    let observation = f.observation();
    let before = f.data();
    let accounts = format!("{:?}", f.state.accounts);
    let error = commit_issuance_with(
        &mut f.state,
        &mut f.emission,
        &mut f.staking,
        &mut f.burn,
        true,
        &request(4, &[], true),
        true,
        true,
        Some(&observation),
        |_, batch| {
            assert!(batch.len() > 10);
            bail!("Injected issuance pre-write failure")
        },
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("Injected issuance pre-write failure"));
    assert_eq!(f.data(), before);
    assert_eq!(format!("{:?}", f.state.accounts), accounts);
    assert_eq!(f.emission.circulating_supply, 1001);
    f = f.reopen();
    assert_eq!(f.data(), before);
    f.run(4, Some(&observation)).unwrap();
    assert_custody(&f, 1668, 0);
    assert_eq!(f.reward().unpaid["alice"], 500);
}

#[test]
fn boundary_lost_ack_reopen_retry_preserves_one_controller_event_and_issuance() {
    let mut f = Fixture::new("alice");
    f.first_epoch();
    let observation = f.observation();
    let error = commit_issuance_with(
        &mut f.state,
        &mut f.emission,
        &mut f.staking,
        &mut f.burn,
        true,
        &request(4, &[], true),
        true,
        true,
        Some(&observation),
        |storage, batch| {
            let mut options = WriteOptions::default();
            options.set_sync(true);
            storage.db.write_opt(batch, &options)?;
            bail!("Injected issuance lost acknowledgement")
        },
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("Injected issuance lost acknowledgement"));
    assert_eq!(f.emission.circulating_supply, 1001);
    let committed = f.data();
    f = f.reopen();
    assert_eq!(f.data(), committed);
    commit_issuance_with(
        &mut f.state,
        &mut f.emission,
        &mut f.staking,
        &mut f.burn,
        true,
        &request(4, &[], true),
        true,
        true,
        Some(&observation),
        |_, _| panic!("Committed issuance retry must not write"),
    )
    .unwrap();
    assert_eq!(f.data(), committed);
    assert_eq!(f.emission.circulating_supply, 1668);
    assert_custody(&f, 1668, 0);
    assert_eq!(
        f.data()
            .keys()
            .filter(|key| key.starts_with(b"adaptive:v1:event:"))
            .count(),
        1
    );
}

#[test]
fn timer_and_reward_only_adapters_reject_timing_without_mutation() {
    let mut f = Fixture::new("alice");
    let before = f.data();
    let accounts = format!("{:?}", f.state.accounts);
    assert!(commit_block(
        &mut f.state,
        &mut f.emission,
        &mut f.staking,
        &mut f.burn,
        true,
        &request(1, &[], true)
    )
    .is_err());
    assert_eq!(f.data(), before);
    assert_eq!(format!("{:?}", f.state.accounts), accounts);
    assert_eq!(f.emission.circulating_supply, 0);
    assert_eq!(f.staking.pending_staking_emission, 0);
    assert!(f.burn.burn_events.is_empty());
    assert!(commit_reward_development_block(
        &mut f.state,
        &mut f.emission,
        &mut f.staking,
        &mut f.burn,
        true,
        &request(1, &[], true)
    )
    .is_err());
    assert_eq!(f.data(), before);
    assert_eq!(format!("{:?}", f.state.accounts), accounts);
    assert_eq!(f.emission.circulating_supply, 0);
    assert_eq!(f.staking.pending_staking_emission, 0);
    assert!(f.burn.burn_events.is_empty());
    // Explicit adaptive finalization remains available after both rejections.
    let outcome = f.run(1, None).unwrap();
    assert!(outcome.block.is_some());
    assert_eq!(f.timing().last_height, 1);
}

#[test]
fn legacy_emission_mutators_reject_orphan_timing_and_custody_markers() {
    for key in [
        "adaptive:orphan",
        "issuance:orphan",
        "emission:pool:validator_rewards",
        "emission:pool:treasury",
        "emission:pool:issuance_reserve",
    ] {
        for amount in [0u128, 7] {
            let dir = tempfile::tempdir().unwrap();
            let storage = Arc::new(Storage::open(dir.path().join("db")).unwrap());
            storage
                .db
                .put(key, bincode::serialize(&amount).unwrap())
                .unwrap();
            let mut state = State::new(storage.clone());
            let mut emission = EmissionEngine::new(
                storage.clone(),
                Arc::new(Mutex::new(State::new(storage.clone()))),
            );
            let before = database(&storage);
            let config = bincode::serialize(&emission.config).unwrap();
            let errors = [
                emission.apply_until(1).unwrap_err(),
                emission.process_block_emission(1, &mut state).unwrap_err(),
                emission.claim("staking_rewards", 0, "alice").unwrap_err(),
                emission.update_config(emission.config.clone()).unwrap_err(),
            ];
            for error in errors {
                assert!(
                    error.contains("legacy emission mutation is disabled"),
                    "{key}: {error}"
                );
            }
            assert_eq!(database(&storage), before, "{key}, amount {amount}");
            assert_eq!(bincode::serialize(&emission.config).unwrap(), config);
            assert_eq!(emission.circulating_supply, 0);
            assert!(state.accounts.is_empty());
        }
    }
}

#[test]
fn signed_claim_at_epoch_boundary_lost_ack_reopen_retry_pays_and_charges_once() {
    use crate::crypto::{ActivePQC, PQC};
    use crate::storage::transaction_record::SignedEnvelope;
    use crate::types::{tx::Tx, Msg, SignedTx};
    let (secret, public) = ActivePQC::keypair();
    let algorithm = crate::addr::OriginKeyAlgorithm::MlDsa65;
    let owner = crate::addr::initial_address(
        crate::addr::AddressNetwork::Development,
        "test",
        algorithm,
        &public,
    )
    .unwrap();
    let mut f = Fixture::new(&owner);
    f.first_epoch();
    let observation = f.observation();
    let signed = SignedTx::sign(
        Tx {
            chain_id: "test".into(),
            nonce: 0,
            msgs: vec![Msg::RewardClaim {
                from: owner.clone(),
            }],
            fee: 50_000,
            memo: "issuance boundary claim".into(),
        },
        &secret,
        &public,
    )
    .unwrap();
    let transaction = crate::signed_transaction::normalize(&signed, 1).unwrap();
    let envelope: SignedEnvelope =
        serde_json::from_value(serde_json::to_value(&signed).unwrap()).unwrap();
    f.state
        .storage
        .put_pending_signed_transaction(&transaction, Some(envelope))
        .unwrap();
    let block_request = request(4, std::slice::from_ref(&transaction), true);
    let error = commit_issuance_with(
        &mut f.state,
        &mut f.emission,
        &mut f.staking,
        &mut f.burn,
        true,
        &block_request,
        true,
        true,
        Some(&observation),
        |storage, batch| {
            let mut options = WriteOptions::default();
            options.set_sync(true);
            storage.db.write_opt(batch, &options)?;
            bail!("Injected boundary claim lost acknowledgement")
        },
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("Injected boundary claim lost acknowledgement"));
    let committed = f.data();
    f = f.reopen();
    let outcome = commit_issuance_with(
        &mut f.state,
        &mut f.emission,
        &mut f.staking,
        &mut f.burn,
        true,
        &block_request,
        true,
        true,
        Some(&observation),
        |_, _| panic!("Committed boundary claim retry must not write"),
    )
    .unwrap();
    assert_eq!(f.data(), committed);
    assert_eq!(outcome.receipts.len(), 1);
    assert!(outcome.receipts[0].success);
    assert_eq!(outcome.receipts[0].block_height, Some(4));
    assert_eq!(outcome.receipts[0].fee, 50_000);
    assert_eq!(f.state.nonce_of(&owner), 1);
    assert_eq!(
        f.state.get_balance(&owner, "udrt"),
        INITIAL_DRT - 50_000 + 500
    );
    assert_eq!(f.reward().total_budget, 500);
    assert!(f.reward().unpaid.is_empty());
    assert_custody(&f, 1668, 500);
    let supply = crate::supply::inspect(&f.state.storage).unwrap();
    assert_eq!(supply.pools["staking_rewards"], 0);
    assert_eq!(supply.pools["validator_rewards"], 667);
    assert_eq!(supply.pools["treasury"], 500);
    assert_eq!(supply.pools["issuance_reserve"], 1);
    assert_eq!(supply.withheld_fees, 50_000);
    let stored = f
        .state
        .storage
        .get_transaction_record(&transaction.hash)
        .unwrap()
        .unwrap();
    assert!(stored.matches(&transaction).unwrap());
    assert!(stored.signed_envelope.is_some());
}
