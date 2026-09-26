use dytallix_fast_node::runtime::issuance_timing::{
    decode_observation, observation_key, plan_block, verify_overlay, ControllerInputs,
    EpochObservation, GainsInput, PoolAmounts, TimingGenesis, TimingState, TIMING_STATE_KEY,
};
use dytallix_storage::{adaptive::prepare_initialize, state::Storage};
use rocksdb::{IteratorMode, WriteBatch, WriteOptions};
use std::collections::BTreeMap;

fn config(n: u64, initial: u64) -> TimingGenesis {
    TimingGenesis {
        version: 1,
        profile: "development".into(),
        decimals: 6,
        epoch_blocks: n,
        initial_epoch_budget_udrt: initial,
        max_recorded_epochs: 8,
        controller: ControllerInputs {
            target_ppm: 500_000,
            shock_threshold_ppm: 100_000,
            volatility_threshold_ppm: 1_000_000,
            window_samples: 2,
            integral_min: -2_000_000,
            integral_max: 2_000_000,
            soft: GainsInput {
                proportional: 2_000,
                integral: 0,
                derivative: 0,
            },
            hard: GainsInput {
                proportional: 2_000,
                integral: 0,
                derivative: 0,
            },
            base_udrt: initial,
            min_udrt: 0,
            max_udrt: u64::MAX,
        },
    }
}
fn view(storage: &Storage) -> BTreeMap<Vec<u8>, Vec<u8>> {
    storage
        .db
        .iterator(IteratorMode::Start)
        .map(|entry| {
            let (k, v) = entry.unwrap();
            (k.to_vec(), v.to_vec())
        })
        .collect()
}
fn fixture(config: TimingGenesis) -> (tempfile::TempDir, Storage, TimingState) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("db")).unwrap();
    let state = TimingState::new(config, "timing-test".into(), "07".repeat(32)).unwrap();
    let mut marker = vec![1u8];
    marker.extend_from_slice(&[7u8; 32]);
    {
        let guard = storage.lock_execution().unwrap();
        let mut batch = WriteBatch::default();
        prepare_initialize(
            &storage,
            state.binding,
            state.config.controller_config().unwrap(),
        )
        .unwrap()
        .append_checked(&storage, &guard, &mut batch)
        .unwrap();
        batch.put(b"genesis:monetary:v1", marker);
        batch.put(b"meta:chain_id", b"timing-test");
        batch.put(TIMING_STATE_KEY, state.encode().unwrap());
        let mut options = WriteOptions::default();
        options.set_sync(true);
        storage.db.write_opt(batch, &options).unwrap();
    }
    verify_overlay(&view(&storage)).unwrap();
    (dir, storage, state)
}
fn observation(epoch: u64, n: u64, parent: &str) -> EpochObservation {
    EpochObservation {
        epoch,
        utilization_ppm: 0,
        volatility_ppm: 0,
        first_height: epoch * n + 1,
        last_height: (epoch + 1) * n,
        parent_hash: parent.into(),
    }
}
fn commit(
    storage: &Storage,
    state: &TimingState,
    height: u64,
    obs: Option<&EpochObservation>,
) -> (TimingState, PoolAmounts) {
    let guard = storage.lock_execution().unwrap();
    let plan = plan_block(storage, state, height, "parent", obs).unwrap();
    let mut batch = WriteBatch::default();
    if let Some(journal) = &plan.journal {
        journal.append_checked(storage, &guard, &mut batch).unwrap();
    }
    for (k, v) in &plan.writes {
        batch.put(k, v);
    }
    batch.put(b"emission:last_height", height.to_be_bytes());
    storage.db.write(batch).unwrap();
    assert_eq!(verify_overlay(&view(storage)).unwrap(), plan.next_state);
    (plan.next_state, plan.block_pools)
}
#[test]
fn every_pool_prefix_conserves_each_epoch_without_advance_issuance() {
    for n in [1, 2, 3, 10, u64::MAX] {
        for budget in [0, 1, 2, 9, 10, 1001, u64::MAX] {
            let pools = PoolAmounts::epoch_split(budget);
            assert_eq!(pools.total().unwrap(), u128::from(budget));
            assert!(pools.issuance_reserve <= 2);
            assert_eq!(pools.prefix(n, 0).unwrap(), PoolAmounts::default());
            assert_eq!(pools.prefix(n, n).unwrap(), pools);
            if n <= 10 {
                let mut prefix = PoolAmounts::default();
                for k in 0..n {
                    prefix = prefix.checked_add(&pools.block(n, k).unwrap()).unwrap();
                    assert_eq!(prefix, pools.prefix(n, k + 1).unwrap());
                }
                assert_eq!(prefix, pools);
            }
        }
    }
    let p = PoolAmounts::epoch_split(2000);
    assert_eq!(
        (0..3)
            .map(|k| p.block(3, k).unwrap().total().unwrap())
            .collect::<Vec<_>>(),
        vec![667, 667, 666]
    );
    assert_eq!(
        PoolAmounts::epoch_split(1001)
            .block(3, 0)
            .unwrap()
            .issuance_reserve,
        1
    );
}
#[test]
fn completed_epoch_observation_produces_next_command_and_persists_evidence() {
    let (_dir, storage, mut state) = fixture(config(3, 1001));
    for height in 1..=3 {
        let (next, _) = commit(&storage, &state, height, None);
        state = next;
    }
    assert_eq!(state.total_issued.total().unwrap(), 1001);
    let before = view(&storage);
    let obs = observation(0, 3, "parent");
    let preview = plan_block(&storage, &state, 4, "parent", Some(&obs)).unwrap();
    assert_eq!(view(&storage), before);
    assert_eq!(preview.next_state.epoch_budget_udrt, 2001);
    assert_eq!(
        decode_observation(&preview.writes[&observation_key(0)]).unwrap(),
        obs
    );
    let (next, pools) = commit(&storage, &state, 4, Some(&obs));
    state = next;
    assert_eq!(pools, PoolAmounts::epoch_split(2001).block(3, 0).unwrap());
    assert!(state.total_issued.total().unwrap() < 1001 + 2001);
    for height in 5..=6 {
        state = commit(&storage, &state, height, None).0;
    }
    assert_eq!(state.total_issued.total().unwrap(), 3002);
    assert_eq!(state.active_epoch, 1);
}
#[test]
fn n_one_and_zero_budget_require_one_observation_each_new_epoch() {
    let (_dir, storage, state) = fixture(config(1, 0));
    let (mut state, pools) = commit(&storage, &state, 1, None);
    assert_eq!(pools.total().unwrap(), 0);
    assert!(plan_block(&storage, &state, 2, "parent", None).is_err());
    state = commit(&storage, &state, 2, Some(&observation(0, 1, "parent"))).0;
    assert_eq!(state.epoch_budget_udrt, 1000);
    assert_eq!(state.total_issued.total().unwrap(), 1000);
}
#[test]
fn invalid_boundary_inputs_and_stale_state_leave_storage_unchanged() {
    let (_dir, storage, state) = fixture(config(2, 10));
    assert!(plan_block(
        &storage,
        &state,
        1,
        "parent",
        Some(&observation(0, 2, "parent"))
    )
    .is_err());
    let state = commit(&storage, &state, 1, None).0;
    let prior = state.clone();
    let state = commit(&storage, &state, 2, None).0;
    let before = view(&storage);
    assert!(plan_block(&storage, &prior, 2, "parent", None).is_err());
    assert!(plan_block(&storage, &state, 4, "parent", None).is_err());
    assert!(plan_block(&storage, &state, 3, "parent", None).is_err());
    for change in 0..5 {
        let mut obs = observation(0, 2, "parent");
        match change {
            0 => obs.epoch = 1,
            1 => obs.first_height = 0,
            2 => obs.last_height = 3,
            3 => obs.parent_hash = "other".into(),
            _ => obs.utilization_ppm = 1_000_001,
        }
        assert!(plan_block(&storage, &state, 3, "parent", Some(&obs)).is_err());
    }
    assert_eq!(view(&storage), before);
}
#[test]
fn canonical_state_and_history_reject_corruption_unknown_records_and_budget_rewrites() {
    let (_dir, storage, state) = fixture(config(1, 10));
    let state = commit(&storage, &state, 1, None).0;
    let state = commit(&storage, &state, 2, Some(&observation(0, 1, "parent"))).0;
    let bytes = state.encode().unwrap();
    assert_eq!(TimingState::decode(&bytes).unwrap(), state);
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(TimingState::decode(&trailing).is_err());
    assert!(TimingState::decode(&bytes[..bytes.len() - 1]).is_err());
    let mut values = view(&storage);
    values.insert(b"issuance:unexpected".to_vec(), vec![]);
    assert!(verify_overlay(&values).is_err());
    let mut values = view(&storage);
    values.remove(&observation_key(0));
    assert!(verify_overlay(&values).is_err());
    let mut values = view(&storage);
    let mut obs = observation(0, 1, "parent");
    obs.volatility_ppm = 1;
    values.insert(observation_key(0), bincode::serialize(&obs).unwrap());
    assert!(verify_overlay(&values).is_err());
    let mut values = view(&storage);
    let mut altered = state.clone();
    altered.total_issued.validator_rewards += 1;
    values.insert(
        TIMING_STATE_KEY.as_bytes().to_vec(),
        altered.encode().unwrap(),
    );
    assert!(verify_overlay(&values).is_err());
    let mut values = view(&storage);
    values.insert(b"meta:chain_id".to_vec(), b"other".to_vec());
    assert!(verify_overlay(&values).is_err());
}
#[test]
fn explicit_configuration_limits_and_arithmetic_reject_without_defaults() {
    for change in 0..6 {
        let mut c = config(1, 10);
        match change {
            0 => c.epoch_blocks = 0,
            1 => c.profile = "mainnet".into(),
            2 => c.decimals = 18,
            3 => c.max_recorded_epochs = 0,
            4 => c.max_recorded_epochs = 1_000_001,
            _ => c.controller.min_udrt = 11,
        }
        assert!(TimingState::new(c, "test".into(), "07".repeat(32)).is_err());
    }
    let maximum = PoolAmounts {
        validator_rewards: u128::MAX,
        ..Default::default()
    };
    assert!(maximum
        .checked_add(&PoolAmounts {
            validator_rewards: 1,
            ..Default::default()
        })
        .is_err());
    assert!(maximum.prefix(0, 0).is_err());
    assert!(maximum.prefix(2, 3).is_err());
    assert!(maximum.block(1, 1).is_err());
    let (_dir, storage, state) = fixture(config(u64::MAX, u64::MAX));
    let state = commit(&storage, &state, 1, None).0;
    assert!(plan_block(&storage, &state, 0, "parent", None).is_err());
    let mut raw = serde_json::to_value(config(1, 10)).unwrap();
    raw.as_object_mut()
        .unwrap()
        .remove("initial_epoch_budget_udrt");
    assert!(serde_json::from_value::<TimingGenesis>(raw).is_err());
}
