//! Historical reads remain available. Diagnostic mutation cannot use current state.
use dytallix_fast_node::runtime::staking::{DelegatorRewardRecord, StakingModule};
use dytallix_fast_node::storage::state::Storage;
use rocksdb::IteratorMode;
use std::sync::Arc;

fn database(staking: &StakingModule) -> Vec<(Box<[u8]>, Box<[u8]>)> {
    staking
        .storage
        .db
        .iterator(IteratorMode::Start)
        .map(|entry| entry.unwrap())
        .collect()
}

#[test]
fn historical_staking_reads_preserve_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let storage = Arc::new(Storage::open(directory.path().join("db")).unwrap());
    storage
        .db
        .put("staking:total_stake", bincode::serialize(&100u128).unwrap())
        .unwrap();
    storage
        .db
        .put(
            "staking:reward_rate_bps",
            bincode::serialize(&123u64).unwrap(),
        )
        .unwrap();
    let record = DelegatorRewardRecord {
        stake_amount: 100,
        accrued_rewards: 37,
        last_reward_index: 0,
    };
    storage
        .db
        .put(
            "staking:delegator:owner",
            bincode::serialize(&record).unwrap(),
        )
        .unwrap();
    let staking = StakingModule::new(storage);
    let before = database(&staking);
    assert_eq!(staking.get_total_stake("owner"), 100);
    assert_eq!(staking.get_accrued_rewards("owner"), 37);
    assert_eq!(staking.get_reward_rate_bps(), 123);
    assert!(staking
        .ensure_legacy_fixture_mutation()
        .unwrap_err()
        .contains("retired"));
    assert_eq!(database(&staking), before);
}

#[test]
fn legacy_staking_rejects_every_current_namespace_without_changes() {
    for key in [
        "consensus:orphan",
        "recovery:orphan",
        "ordinary:v1:state",
        "rewards:v2",
        "rewards:v2:owner:orphan",
        "issuance:orphan",
        "adaptive:orphan",
        "emission:pool:validator_rewards",
        "emission:pool:treasury",
        "emission:pool:issuance_reserve",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let storage = Arc::new(Storage::open(directory.path().join("db")).unwrap());
        storage.db.put(key, b"malformed").unwrap();
        #[allow(unused_mut)]
        let mut staking = StakingModule::new(storage);
        let before = database(&staking);
        let memory = format!("{staking:?}");
        assert!(staking.ensure_legacy_fixture_mutation().is_err(), "{key}");
        assert_eq!(database(&staking), before, "{key}");
        assert_eq!(format!("{staking:?}"), memory, "{key}");
    }
}
