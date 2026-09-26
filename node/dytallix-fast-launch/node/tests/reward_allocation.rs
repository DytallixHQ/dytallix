use dytallix_fast_node::runtime::reward_allocation::{
    allocate_reward_budget, IntervalRewardAllocation, RewardAllocationError, RewardLedger,
};
use std::collections::BTreeMap;

fn snapshot(weights: &[(&str, u128)]) -> BTreeMap<String, u128> {
    weights.iter().map(|(id, w)| (id.to_string(), *w)).collect()
}

#[test]
fn original_reward_amounts_are_exact_without_global_index_rounding() {
    let first = allocate_reward_budget(250_000, &snapshot(&[("alice", 600_000_000_000)])).unwrap();
    assert_eq!(first.entitlements["alice"], 250_000);
    assert_eq!(first.reserve, 0);
    let next = allocate_reward_budget(
        250_000,
        &snapshot(&[("alice", 600_000_000_000), ("bob", 400_000_000_000)]),
    )
    .unwrap();
    assert_eq!(next.entitlements["alice"], 150_000);
    assert_eq!(next.entitlements["bob"], 100_000);
    assert_eq!(
        first.entitlements["alice"] + next.entitlements["alice"],
        400_000
    );
    assert_eq!(next.reserve, 0);
}

#[test]
fn integer_remainders_are_separate_and_account_order_is_irrelevant() {
    let weights = snapshot(&[("c", 1), ("a", 1), ("b", 1), ("zero", 0)]);
    let result = allocate_reward_budget(10, &weights).unwrap();
    assert_eq!(
        result.entitlements,
        snapshot(&[("a", 3), ("b", 3), ("c", 3), ("zero", 0)])
    );
    assert_eq!(result.reserve, 1);
    let reversed = weights
        .iter()
        .rev()
        .map(|(id, w)| (id.clone(), *w))
        .collect();
    assert_eq!(allocate_reward_budget(10, &reversed).unwrap(), result);
    let tiny = allocate_reward_budget(1, &snapshot(&[("a", 2), ("b", 1)])).unwrap();
    assert_eq!(tiny.entitlements.values().sum::<u128>(), 0);
    assert_eq!(tiny.reserve, 1);
}

#[test]
fn extreme_products_and_output_boundaries_are_exact() {
    let sole = allocate_reward_budget(u128::MAX, &snapshot(&[("a", u128::MAX)])).unwrap();
    assert_eq!(sole.entitlements["a"], u128::MAX);
    assert_eq!(sole.reserve, 0);
    let split =
        allocate_reward_budget(u128::MAX, &snapshot(&[("a", u128::MAX - 1), ("b", 1)])).unwrap();
    assert_eq!(split.entitlements["a"], u128::MAX - 1);
    assert_eq!(split.entitlements["b"], 1);
    assert_eq!(split.reserve, 0);
    let odd = allocate_reward_budget(u128::MAX, &snapshot(&[("a", 1), ("b", 1)])).unwrap();
    assert_eq!(odd.entitlements["a"], u128::MAX / 2);
    assert_eq!(odd.entitlements["b"], u128::MAX / 2);
    assert_eq!(odd.reserve, 1);
    assert_eq!(
        allocate_reward_budget(0, &snapshot(&[("a", u128::MAX)]))
            .unwrap()
            .reserve,
        0
    );
}

#[test]
fn all_small_ratios_match_independent_direct_integer_products() {
    for budget in 0..=31u128 {
        for a in 0..=15u128 {
            for b in 0..=15u128 {
                if a + b == 0 {
                    continue;
                }
                let result =
                    allocate_reward_budget(budget, &snapshot(&[("a", a), ("b", b)])).unwrap();
                assert_eq!(result.entitlements["a"], budget * a / (a + b));
                assert_eq!(result.entitlements["b"], budget * b / (a + b));
                assert_eq!(
                    result.entitlements.values().sum::<u128>() + result.reserve,
                    budget
                );
            }
        }
    }
}

#[test]
fn undefined_zero_stake_and_overflow_return_errors_without_disposition() {
    assert_eq!(
        allocate_reward_budget(7, &BTreeMap::new()),
        Err(RewardAllocationError::NoEligibleStake)
    );
    assert_eq!(
        allocate_reward_budget(0, &snapshot(&[("a", 0)])),
        Err(RewardAllocationError::NoEligibleStake)
    );
    assert_eq!(
        allocate_reward_budget(1, &snapshot(&[("a", u128::MAX), ("b", 1)])),
        Err(RewardAllocationError::TotalStakeOverflow)
    );
}

#[test]
fn record_roundtrip_and_repeated_calculation_are_deterministic() {
    let weights = snapshot(&[("a", 1), ("b", 1), ("c", 1)]);
    let record =
        IntervalRewardAllocation::new("caller-interval".into(), 10, weights.clone()).unwrap();
    assert_eq!(record.interval_id(), "caller-interval");
    assert_eq!(record.budget(), 10);
    assert_eq!(record.snapshot(), &weights);
    assert_eq!(record.allocation().reserve, 1);
    let encoded = bincode::serialize(&record).unwrap();
    let decoded: IntervalRewardAllocation = bincode::deserialize(&encoded).unwrap();
    assert_eq!(decoded, record);
    assert_eq!(bincode::serialize(&decoded).unwrap(), encoded);
    assert_eq!(
        IntervalRewardAllocation::new("caller-interval".into(), 10, weights).unwrap(),
        record
    );
    assert!(IntervalRewardAllocation::new(String::new(), 1, snapshot(&[("a", 1)])).is_err());
}

#[test]
fn decoding_rejects_changed_allocation_inputs_digest_and_version() {
    let record = IntervalRewardAllocation::new(
        "interval-a".into(),
        10,
        snapshot(&[("a", 1), ("b", 1), ("c", 1)]),
    )
    .unwrap();
    let original = serde_json::to_value(&record).unwrap();
    for field in [
        "version",
        "interval_id",
        "budget",
        "snapshot",
        "allocation",
        "digest",
    ] {
        let mut changed = original.clone();
        match field {
            "version" => changed[field] = serde_json::json!(2),
            "interval_id" => changed[field] = serde_json::json!("interval-b"),
            "budget" => changed[field] = serde_json::json!(11),
            "snapshot" => changed[field]["a"] = serde_json::json!(2),
            "allocation" => changed[field]["reserve"] = serde_json::json!(0),
            "digest" => changed[field][0] = serde_json::json!(u16::from(record.digest()[0]) ^ 1),
            _ => unreachable!(),
        }
        assert!(
            serde_json::from_value::<IntervalRewardAllocation>(changed).is_err(),
            "{field}"
        );
    }
}

fn interval(id: &str, budget: u128, weights: &[(&str, u128)]) -> IntervalRewardAllocation {
    IntervalRewardAllocation::new(id.into(), budget, snapshot(weights)).unwrap()
}

#[test]
fn ledger_preserves_original_monetary_sequence_and_claims_once() {
    let mut ledger = RewardLedger::default();
    assert_eq!(ledger.accrued("alice"), 0);
    ledger
        .accept(interval("first", 250_000, &[("alice", 600_000_000_000)]))
        .unwrap();
    assert_eq!(ledger.accrued("alice"), 250_000);
    assert_eq!(ledger.accrued("bob"), 0);
    let split = [("alice", 600_000_000_000), ("bob", 400_000_000_000)];
    ledger.accept(interval("second", 250_000, &split)).unwrap();
    assert_eq!(ledger.accrued("alice"), 400_000);
    assert_eq!(ledger.accrued("bob"), 100_000);
    assert_eq!(ledger.claim("alice").unwrap(), 400_000);
    assert_eq!(ledger.claim("alice").unwrap(), 0);
    ledger.accept(interval("third", 250_000, &split)).unwrap();
    assert_eq!(ledger.accrued("alice"), 150_000);
    assert_eq!(ledger.accrued("bob"), 200_000);
    assert_eq!(ledger.claim("alice").unwrap(), 150_000);
    assert_eq!(ledger.claim("bob").unwrap(), 200_000);
    assert_eq!(ledger.total_budget(), 750_000);
    assert_eq!(ledger.total_paid(), 750_000);
    assert_eq!(ledger.total_unpaid(), 0);
    assert_eq!(ledger.reserve(), 0);
    ledger.verify().unwrap();
}

#[test]
fn ledger_roundtrip_preserves_reserve_and_claim_partitioning() {
    let mut immediate = RewardLedger::default();
    let mut deferred = RewardLedger::default();
    for id in ["one", "two", "three"] {
        let record = interval(id, 10, &[("a", 1), ("b", 1), ("c", 1)]);
        immediate.accept(record.clone()).unwrap();
        deferred.accept(record).unwrap();
        for account in ["c", "a", "b"] {
            assert_eq!(immediate.claim(account).unwrap(), 3);
        }
        immediate = bincode::deserialize(&bincode::serialize(&immediate).unwrap()).unwrap();
    }
    let restored: RewardLedger =
        bincode::deserialize(&bincode::serialize(&deferred).unwrap()).unwrap();
    assert_eq!(restored, deferred);
    for account in ["a", "b", "c"] {
        assert_eq!(deferred.claim(account).unwrap(), 9);
    }
    assert_eq!(immediate, deferred);
    assert_eq!(deferred.total_budget(), 30);
    assert_eq!(deferred.total_paid(), 27);
    assert_eq!(deferred.total_unpaid(), 0);
    assert_eq!(deferred.reserve(), 3);
    assert_eq!(deferred.claim("missing").unwrap(), 0);
}

#[test]
fn duplicate_interval_and_accumulation_overflow_leave_ledger_unchanged() {
    let mut ledger = RewardLedger::default();
    let record = interval("same", u128::MAX, &[("a", u128::MAX)]);
    ledger.accept(record.clone()).unwrap();
    let before = bincode::serialize(&ledger).unwrap();
    assert_eq!(
        ledger.accept(record),
        Err(RewardAllocationError::DuplicateInterval)
    );
    assert_eq!(bincode::serialize(&ledger).unwrap(), before);
    assert_eq!(
        ledger.accept(interval("overflow", 1, &[("a", 1)])),
        Err(RewardAllocationError::ArithmeticOverflow)
    );
    assert_eq!(bincode::serialize(&ledger).unwrap(), before);
    let restored: RewardLedger = bincode::deserialize(&before).unwrap();
    assert_eq!(restored.accrued("a"), u128::MAX);
    assert_eq!(ledger.claim("a").unwrap(), u128::MAX);
    assert_eq!(ledger.total_paid(), u128::MAX);
    ledger.verify().unwrap();
}

#[test]
fn ledger_decode_rejects_inconsistent_account_records_and_counters() {
    let mut ledger = RewardLedger::default();
    ledger
        .accept(interval("one", 10, &[("a", 1), ("b", 1), ("c", 1)]))
        .unwrap();
    ledger.claim("a").unwrap();
    let original = serde_json::to_value(&ledger).unwrap();
    for field in [
        "records",
        "unpaid",
        "paid",
        "total_budget",
        "total_paid",
        "reserve",
    ] {
        let mut changed = original.clone();
        match field {
            "records" => changed[field] = serde_json::json!({}),
            "unpaid" => changed[field]["b"] = serde_json::json!(4),
            "paid" => changed[field]["a"] = serde_json::json!(4),
            "total_budget" => changed[field] = serde_json::json!(11),
            "total_paid" => changed[field] = serde_json::json!(4),
            "reserve" => changed[field] = serde_json::json!(2),
            _ => unreachable!(),
        }
        assert!(
            serde_json::from_value::<RewardLedger>(changed).is_err(),
            "{field}"
        );
    }
}

#[test]
fn duplicate_snapshot_accounts_and_unknown_record_fields_fail_decode() {
    let record = interval("one", 10, &[("a", 1), ("b", 1)]);
    let encoded = serde_json::to_string(&record).unwrap();
    let repeated = encoded.replace("\"snapshot\":{", "\"snapshot\":{\"a\":1,");
    assert_ne!(repeated, encoded);
    assert!(serde_json::from_str::<IntervalRewardAllocation>(&repeated).is_err());
    let mut unknown = serde_json::to_value(&record).unwrap();
    unknown["authority"] = serde_json::json!("unapproved");
    assert!(serde_json::from_value::<IntervalRewardAllocation>(unknown).is_err());
}
