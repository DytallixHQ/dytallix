//! Retirement and isolation checks for direct legacy emission entry points.
//! These checks do not replace current committed reward or issuance tests.

use dytallix_fast_node::runtime::emission::{
    EmissionBreakdown, EmissionConfig, EmissionEngine, EmissionEvent, EmissionSchedule,
};
use dytallix_fast_node::state::{AccountState, State};
use dytallix_fast_node::storage::state::Storage;
use rocksdb::IteratorMode;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

struct Fixture {
    engine: EmissionEngine,
    supplied_state: State,
    _directory: TempDir,
}

fn config(amount: u128) -> EmissionConfig {
    EmissionConfig {
        schedule: EmissionSchedule::Static { per_block: amount },
        initial_supply: 0,
        emission_breakdown: EmissionBreakdown {
            block_rewards: 60,
            staking_rewards: 25,
            ai_module_incentives: 10,
            bridge_operations: 5,
        },
    }
}

impl Fixture {
    fn new(historical: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let storage = Arc::new(Storage::open(directory.path().join("db")).unwrap());
        let state = Arc::new(Mutex::new(State::new(storage.clone())));
        let mut supplied_state = State::new(storage.clone());
        supplied_state.accounts.insert(
            "supplied-state-owner".to_owned(),
            AccountState {
                balances: BTreeMap::from([("udrt".to_owned(), 91)]),
                nonce: 4,
            },
        );
        state.lock().unwrap().accounts.insert(
            "engine-state-owner".to_owned(),
            AccountState {
                balances: BTreeMap::from([("udrt".to_owned(), 73)]),
                nonce: 8,
            },
        );
        if historical {
            storage
                .db
                .put("emission:last_height", 7u64.to_be_bytes())
                .unwrap();
            storage
                .db
                .put(
                    "emission:pool:staking_rewards",
                    bincode::serialize(&50u128).unwrap(),
                )
                .unwrap();
            storage
                .db
                .put(
                    "emission:circulating_supply",
                    bincode::serialize(&500u128).unwrap(),
                )
                .unwrap();
            let event = EmissionEvent {
                height: 7,
                timestamp: 42,
                total_emitted: 100,
                pools: HashMap::from([("staking_rewards".to_owned(), 25)]),
                reward_index_after: Some(123),
                circulating_supply: 500,
            };
            storage
                .db
                .put("emission:event:7", bincode::serialize(&event).unwrap())
                .unwrap();
        }
        let engine = EmissionEngine::new_with_config(storage, state, config(100));
        Self {
            engine,
            supplied_state,
            _directory: directory,
        }
    }

    fn database(&self) -> BTreeMap<Vec<u8>, Vec<u8>> {
        self.engine
            .storage
            .db
            .iterator(IteratorMode::Start)
            .map(|entry| {
                let (key, value) = entry.unwrap();
                (key.to_vec(), value.to_vec())
            })
            .collect()
    }

    fn assert_all_mutators_reject_without_changes(&mut self) {
        let before = self.database();
        let original_config = bincode::serialize(&self.engine.config).unwrap();
        let original_supply = self.engine.circulating_supply;
        let original_accounts =
            serde_json::to_value(&self.engine.state.lock().unwrap().accounts).unwrap();
        let supplied_accounts = serde_json::to_value(&self.supplied_state.accounts).unwrap();
        let errors = [
            self.engine.apply_until(12).unwrap_err(),
            self.engine.apply_until(0).unwrap_err(),
            self.engine
                .claim("staking_rewards", 10, "recipient")
                .unwrap_err(),
            self.engine
                .claim("staking_rewards", 0, "recipient")
                .unwrap_err(),
            self.engine
                .process_block_emission(12, &mut self.supplied_state)
                .unwrap_err(),
            self.engine.update_config(config(999)).unwrap_err(),
        ];
        for error in errors {
            assert!(
                error.contains("legacy emission mutation is disabled"),
                "{error}"
            );
        }
        assert_eq!(self.database(), before);
        assert_eq!(
            bincode::serialize(&self.engine.config).unwrap(),
            original_config
        );
        assert_eq!(self.engine.circulating_supply, original_supply);
        assert_eq!(
            serde_json::to_value(&self.engine.state.lock().unwrap().accounts).unwrap(),
            original_accounts
        );
        assert_eq!(
            serde_json::to_value(&self.supplied_state.accounts).unwrap(),
            supplied_accounts
        );
    }
}

#[test]
fn ordinary_build_rejects_direct_mutation_on_empty_and_historical_databases() {
    for historical in [false, true] {
        Fixture::new(historical).assert_all_mutators_reject_without_changes();
    }
}

#[test]
fn modern_markers_block_every_mutator_even_in_diagnostic_builds() {
    for marker in [
        "adaptive:orphan",
        "issuance:orphan",
        "consensus:v1:config",
        "consensus:unknown:orphan",
        "recovery:orphan",
        "ordinary:v1:state",
        "rewards:v2",
        "rewards:v2:state",
        "rewards:v2:unknown:orphan",
        "emission:pool:validator_rewards",
        "emission:pool:treasury",
        "emission:pool:issuance_reserve",
    ] {
        for value in [
            Vec::new(),
            b"malformed-marker".to_vec(),
            bincode::serialize(&0u128).unwrap(),
        ] {
            let mut fixture = Fixture::new(true);
            fixture.engine.storage.db.put(marker, &value).unwrap();
            fixture.assert_all_mutators_reject_without_changes();
            assert_eq!(
                fixture.engine.storage.db.get(marker).unwrap().unwrap(),
                value
            );
        }
    }
}

#[test]
fn historical_reads_and_explicit_configuration_helpers_remain_available() {
    let fixture = Fixture::new(true);
    let before = fixture.database();
    assert_eq!(fixture.engine.last_accounted_height(), 7);
    assert_eq!(fixture.engine.circulating_supply, 500);
    assert_eq!(fixture.engine.pool_amount("staking_rewards"), 50);
    assert_eq!(fixture.engine.get_latest_staking_rewards(), 25);
    assert_eq!(fixture.engine.get_event(7).unwrap().timestamp, 42);
    assert_eq!(fixture.engine.get_emission_events(1).len(), 1);
    assert_eq!(fixture.engine.snapshot().pools["staking_rewards"], 50);
    assert!(fixture.engine.config.emission_breakdown.is_valid());
    let reloaded = EmissionEngine::new_with_config(
        fixture.engine.storage.clone(),
        fixture.engine.state.clone(),
        config(100),
    );
    assert_eq!(reloaded.last_accounted_height(), 7);
    assert_eq!(reloaded.circulating_supply, 500);
    assert_eq!(reloaded.get_event(7).unwrap().total_emitted, 100);
    assert_eq!(fixture.database(), before);
}
