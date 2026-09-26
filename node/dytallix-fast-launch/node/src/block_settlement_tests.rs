//! Current supply and versioned settlement checks remain in the default suite.
//! Historical timer economics require the explicit legacy diagnostic feature.
use super::*;
#[cfg(feature = "legacy-economic-fixtures")]
use crate::runtime::emission::EmissionEvent;
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
    #[cfg(feature = "legacy-economic-fixtures")]
    fn run(
        &mut self,
        height: u64,
        txs: &[Transaction],
        assets: &[String],
        empty: bool,
        staking: bool,
    ) -> Result<BlockOutcome> {
        commit_legacy_fixture_block(
            &mut self.state,
            &mut self.emission,
            &mut self.staking,
            &mut self.burn,
            staking,
            &BlockRequest {
                height,
                transactions: txs,
                assets,
                timestamp: height * 10,
                empty_blocks: empty,
            },
        )
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
#[cfg(feature = "legacy-economic-fixtures")]
fn value<T: serde::de::DeserializeOwned + Default>(f: &Fixture, key: &str) -> T {
    block_lifecycle::read(&f.state.storage, key).unwrap()
}

#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn block_commits_success_paid_failure_and_contiguous_receipts() {
    let mut f = Fixture::new();
    let input = [
        tx("reject", 9, 100),
        tx("ok", 0, 100),
        tx("paid-failure", 1, 1000),
        tx("after", 2, 200),
    ];
    let result = f.run(1, &input, &["asset-1".into()], false, true).unwrap();
    let block = result.block.unwrap();
    assert_eq!(
        block
            .txs
            .iter()
            .map(|t| t.hash.as_str())
            .collect::<Vec<_>>(),
        ["ok", "paid-failure", "after"]
    );
    assert_eq!(
        result
            .receipts
            .iter()
            .map(|r| r.success)
            .collect::<Vec<_>>(),
        [true, false, true]
    );
    for (i, r) in result.receipts.iter().enumerate() {
        assert_eq!(r.block_height, Some(1));
        assert_eq!(r.index, Some(i as u32));
    }
    assert_eq!(f.state.get_balance("alice", "udgt"), 700);
    assert_eq!(f.state.get_balance("bob", "udgt"), 300);
    assert_eq!(f.state.get_balance("alice", "udrt"), 850_000);
    assert_eq!(f.state.nonce_of("alice"), 3);
    assert_eq!(value::<u128>(&f, settlement::FEE_KEY), 150_000);
    assert_eq!(
        f.state.storage.get_receipt("reject").unwrap().block_height,
        None
    );
    assert_eq!(f.emission.circulating_supply, 100);
    assert_eq!(f.staking.pending_staking_emission, 25);
    let event: EmissionEvent =
        bincode::deserialize(&f.state.storage.db.get("emission:event:1").unwrap().unwrap())
            .unwrap();
    assert_eq!(event.timestamp, 10);
    assert_eq!(event.pools.values().sum::<u128>(), 100);
    assert_eq!(event.reward_index_after, Some(0));
    verify_recovery(&f.state.storage).unwrap();
}
#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn skipped_ticks_and_only_rejections_do_not_advance_emission() {
    let mut f = Fixture::new();
    let before = f.data();
    assert!(f.run(1, &[], &[], false, true).unwrap().block.is_none());
    assert_eq!(before, f.data());
    let rejected = tx("rejected", 10, 1);
    f.state.storage.put_pending_transaction(&rejected).unwrap();
    assert!(f
        .run(1, &[rejected], &[], false, true)
        .unwrap()
        .block
        .is_none());
    let receipt = f.state.storage.get_receipt("rejected").unwrap();
    assert!(!receipt.success);
    assert_eq!(receipt.block_height, None);
    assert_eq!(receipt.index, None);
    assert_eq!(receipt.gas_used, 0);
    assert_eq!(f.emission.circulating_supply, 0);
    assert_eq!(f.state.storage.height(), 0);
    assert_eq!(f.state.nonce_of("alice"), 0);
    assert!(f
        .state
        .storage
        .db
        .get(settlement::FEE_KEY)
        .unwrap()
        .is_none());
    verify_recovery(&f.state.storage).unwrap();
}

#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn charged_failure_alone_still_commits_a_block() {
    let mut f = Fixture::new();
    let out = f
        .run(1, &[tx("failed", 0, 1001)], &[], false, false)
        .unwrap();
    assert_eq!(out.block.unwrap().txs.len(), 1);
    assert!(!out.receipts[0].success);
    assert_eq!(f.state.nonce_of("alice"), 1);
}
#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn synchronous_write_failure_publishes_no_state_or_diagnostics() {
    let mut f = Fixture::new();
    let before = f.data();
    let accounts = format!("{:?}", f.state.accounts);
    let input = [tx("ok", 0, 100), tx("next", 1, 200)];
    let assets = vec!["asset".into()];
    let error = commit_legacy_fixture_with(
        &mut f.state,
        &mut f.emission,
        &mut f.staking,
        &mut f.burn,
        true,
        &BlockRequest {
            height: 1,
            transactions: &input,
            assets: &assets,
            timestamp: 10,
            empty_blocks: false,
        },
        |_, batch| {
            assert!(batch.len() > 10);
            bail!("Injected pre-write I/O failure")
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("Injected"));
    assert_eq!(f.data(), before);
    assert_eq!(format!("{:?}", f.state.accounts), accounts);
    assert_eq!(f.emission.circulating_supply, 0);
    assert_eq!(f.staking.pending_staking_emission, 0);
    assert!(f.burn.burn_events.is_empty());
    assert_eq!(assets, ["asset"]);
    assert!(f
        .run(1, &input, &assets, false, true)
        .unwrap()
        .block
        .is_some());
}
#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn later_planning_error_discards_earlier_transaction_effects() {
    let mut f = Fixture::new();
    f.state
        .storage
        .db
        .put("acct:balances:broken", b"invalid")
        .unwrap();
    let before = f.data();
    let mut second = tx("later", 1, 1);
    second.to = "broken".into();
    assert!(f
        .run(1, &[tx("first", 0, 100), second], &[], false, false)
        .is_err());
    assert_eq!(f.data(), before);
    assert_eq!(f.state.get_balance("alice", "udgt"), 1000);
    assert!(f.burn.burn_events.is_empty());
}
#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn lifecycle_error_discards_transactions_and_rejects_invalid_shares() {
    for invalid_split in [false, true] {
        let mut f = Fixture::with_stake(1_000_000, 0, 1);
        if invalid_split {
            f.emission.config.emission_breakdown.block_rewards = 255;
        } else {
            f.emission.config.schedule = EmissionSchedule::Static {
                per_block: u128::MAX,
            };
        }
        let before = f.data();
        assert!(f.run(1, &[tx("first", 0, 100)], &[], false, true).is_err());
        assert_eq!(before, f.data());
        assert_eq!(f.state.nonce_of("alice"), 0);
    }
}
#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn maximum_emission_allocation_conserves_full_width_amount() {
    let mut f = Fixture::funded(0, 0);
    f.emission.config.schedule = EmissionSchedule::Static {
        per_block: u128::MAX,
    };
    f.run(1, &[], &[], true, false).unwrap();
    let event = f.emission.get_event(1).unwrap();
    assert_eq!(event.pools.values().copied().sum::<u128>(), u128::MAX);
    let before = f.data();
    assert!(f.run(2, &[], &[], true, false).is_err());
    assert_eq!(before, f.data());
}
#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn staking_index_uses_durable_values_and_preserves_remainder() {
    let mut f = Fixture::with_stake(1_000_000, 0, 3);
    f.run(1, &[], &[], true, true).unwrap();
    assert_eq!(
        f.staking.reward_index,
        25 * crate::runtime::staking::REWARD_SCALE / 3
    );
    assert_eq!(
        f.staking.reward_index_residual,
        25 * crate::runtime::staking::REWARD_SCALE % 3
    );
    f.run(2, &[], &[], true, true).unwrap();
    assert_eq!(
        f.staking.reward_index,
        50 * crate::runtime::staking::REWARD_SCALE / 3
    );
    assert_eq!(
        f.staking.reward_index_residual,
        50 * crate::runtime::staking::REWARD_SCALE % 3
    );
    verify_recovery(&f.state.storage).unwrap();
}
#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn matching_retry_is_read_only_and_changed_retry_fails() {
    let mut f = Fixture::new();
    let input = [tx("one", 0, 100)];
    let first = f.run(1, &input, &[], false, true).unwrap().block.unwrap();
    let before = f.data();
    let burns = f.burn.burn_events.len();
    assert_eq!(
        f.run(1, &input, &[], false, true)
            .unwrap()
            .block
            .unwrap()
            .hash,
        first.hash
    );
    assert_eq!(before, f.data());
    assert_eq!(burns, f.burn.burn_events.len());
    let mut changed = input.clone();
    changed[0].amount = 101;
    assert!(f.run(1, &changed, &[], false, true).is_err());
    assert!(f.run(3, &[], &[], true, true).is_err());
    assert_eq!(before, f.data());
}
#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn reopen_verifies_all_state_and_does_not_charge_twice() {
    let mut f = Fixture::new();
    let input = [tx("one", 0, 100)];
    f.run(1, &input, &[], false, true).unwrap();
    let path = f.dir.path().join("node.db");
    let Fixture {
        state,
        emission,
        staking,
        burn,
        dir,
    } = f;
    drop((state, emission, staking, burn));
    let storage = Arc::new(Storage::open(path).unwrap());
    verify_recovery(&storage).unwrap();
    let state = State::new(storage.clone());
    let mut emission = EmissionEngine::new(
        storage.clone(),
        Arc::new(Mutex::new(State::new(storage.clone()))),
    );
    emission.config.schedule = EmissionSchedule::Static { per_block: 100 };
    emission.config.initial_supply = crate::supply::genesis_amount(&storage).unwrap();
    let staking = StakingModule::new(storage);
    let mut reopened = Fixture {
        state,
        emission,
        staking,
        burn: FeeBurnEngine::new(),
        dir,
    };
    let before = reopened.data();
    reopened.run(1, &input, &[], false, true).unwrap();
    assert_eq!(before, reopened.data());
    reopened
        .run(2, &[tx("two", 1, 100)], &[], false, true)
        .unwrap();
    assert_eq!(reopened.state.nonce_of("alice"), 2);
    verify_recovery(&reopened.state.storage).unwrap();
}
#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn head_state_receipt_and_orphan_corruption_require_recovery() {
    for kind in 0..7 {
        let mut f = Fixture::new();
        f.run(1, &[tx("one", 0, 100)], &[], false, false).unwrap();
        match kind {
            0 => f
                .state
                .storage
                .db
                .put("meta:height", 2u64.to_be_bytes())
                .unwrap(),
            1 => f
                .state
                .storage
                .db
                .put("emission:last_height", 2u64.to_be_bytes())
                .unwrap(),
            2 => f
                .state
                .storage
                .db
                .put("meta:best_hash", b"different")
                .unwrap(),
            3 => f
                .state
                .storage
                .db
                .put("acct:nonce:alice", bincode::serialize(&9u64).unwrap())
                .unwrap(),
            4 => f.state.storage.db.put("rcpt:one", b"{}").unwrap(),
            5 => f
                .state
                .storage
                .db
                .put("execution:v1:receipt:orphan", b"{}")
                .unwrap(),
            _ => f
                .state
                .storage
                .db
                .put("blk_num:0000000000000002", b"orphan")
                .unwrap(),
        }
        let before = f.data();
        assert!(verify_recovery(&f.state.storage).is_err(), "case {kind}");
        assert!(f.run(2, &[], &[], true, false).is_err());
        assert_eq!(before, f.data());
    }
}
#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn unmarked_partial_execution_requires_migration() {
    for key in [
        "emission:last_height",
        "meta:height",
        "execution:v1:receipt:orphan",
    ] {
        let mut f = Fixture::new();
        f.state.storage.db.put(key, 1u64.to_be_bytes()).unwrap();
        let before = f.data();
        assert!(f.run(1, &[], &[], true, false).is_err());
        assert_eq!(before, f.data());
    }
}
#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn block_hash_binds_assets_transaction_body_and_header() {
    let mut f = Fixture::new();
    let block = f
        .run(1, &[tx("one", 0, 100)], &["asset".into()], false, false)
        .unwrap()
        .block
        .unwrap();
    for field in 0..4 {
        let mut changed = block.clone();
        match field {
            0 => changed.header.asset_hashes.push("later".into()),
            1 => changed.txs[0].amount += 1,
            2 => changed.header.tx_count += 1,
            _ => changed.header.execution_state_digest = "other".into(),
        };
        assert_ne!(
            block.hash,
            Block::compute_hash(&changed.header, &changed.txs)
        );
    }
}
#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn identical_inputs_produce_identical_durable_block_state() {
    let mut a = Fixture::new();
    let mut b = Fixture::new();
    let input = [tx("one", 0, 100)];
    let x = a
        .run(1, &input, &["asset".into()], false, true)
        .unwrap()
        .block
        .unwrap();
    let y = b
        .run(1, &input, &["asset".into()], false, true)
        .unwrap()
        .block
        .unwrap();
    assert_eq!(x.hash, y.hash);
    assert_eq!(a.data(), b.data());
}
#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn pending_admission_cannot_overwrite_final_receipts() {
    let mut f = Fixture::new();
    let input = tx("one", 0, 100);
    f.state.storage.put_pending_transaction(&input).unwrap();
    f.run(1, std::slice::from_ref(&input), &[], false, false)
        .unwrap();
    let before = f.data();
    assert!(f
        .state
        .storage
        .put_pending_receipt(&TxReceipt::pending(&input))
        .is_err());
    assert!(f.state.storage.put_pending_transaction(&input).is_err());
    assert_eq!(before, f.data());
    verify_recovery(&f.state.storage).unwrap();
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
#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn legacy_fixture_asset_only_block_retains_header_assets() {
    let snapshot = vec!["first".into(), "second".into()];
    let mut f = Fixture::new();
    let block = f
        .run(1, &[], &snapshot, false, false)
        .unwrap()
        .block
        .unwrap();
    assert_eq!(block.header.asset_hashes, snapshot);
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
#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn legacy_fixture_changed_policy_fails_before_writes() {
    let mut f = Fixture::new();
    f.run(1, &[], &[], true, false).unwrap();
    let before = f.data();
    f.emission.config.schedule = EmissionSchedule::Static { per_block: 101 };
    assert!(verify_policy(&f.emission, false).is_err());
    assert!(f.run(2, &[], &[], true, false).is_err());
    assert_eq!(before, f.data());
}

#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn standalone_transaction_executor_cannot_modify_block_database() {
    let mut f = Fixture::new();
    f.run(1, &[], &[], true, false).unwrap();
    let before = f.data();
    assert!(crate::execution::execute_transaction(
        &tx("one", 0, 1),
        &mut f.state,
        2,
        0,
        &GasSchedule::default(),
        None
    )
    .is_err());
    assert_eq!(before, f.data());
}

#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn lost_commit_acknowledgement_recovers_once_and_refreshes_caches() {
    let mut f = Fixture::new();
    let input = [tx("one", 0, 100)];
    let request = BlockRequest {
        height: 1,
        transactions: &input,
        assets: &[],
        timestamp: 10,
        empty_blocks: false,
    };
    let error = commit_legacy_fixture_with(
        &mut f.state,
        &mut f.emission,
        &mut f.staking,
        &mut f.burn,
        true,
        &request,
        |storage, batch| {
            let mut options = WriteOptions::default();
            options.set_sync(true);
            storage.db.write_opt(batch, &options)?;
            bail!("Injected lost commit acknowledgement")
        },
    );
    assert!(error.is_err());
    assert_eq!(f.emission.circulating_supply, 0);
    assert_eq!(f.state.get_balance("alice", "udgt"), 1000);
    verify_recovery(&f.state.storage).unwrap();
    let before = f.data();
    f.run(1, &input, &[], false, true).unwrap();
    assert_eq!(before, f.data());
    assert_eq!(f.state.get_balance("alice", "udgt"), 900);
    assert_eq!(f.state.nonce_of("alice"), 1);
    assert_eq!(f.emission.circulating_supply, 100);
    assert_eq!(f.staking.pending_staking_emission, 25);
}
#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn in_block_balance_change_finalizes_admitted_rejection_without_a_fee() {
    use crate::storage::tx::TxMessage;
    let mut f = Fixture::funded(1_000_000, 100_000);
    let register = Transaction {
        messages: Some(vec![TxMessage::DmsRegister {
            from: "alice".into(),
            beneficiary: "bob".into(),
            period: 1,
        }]),
        ..tx("register", 0, 0)
    };
    f.run(1, &[register], &[], false, false).unwrap();
    let admitted = tx("admitted", 1, 1);
    f.state.storage.put_pending_transaction(&admitted).unwrap();
    let claim = Transaction {
        from: "bob".into(),
        messages: Some(vec![TxMessage::DmsClaim {
            from: "bob".into(),
            owner: "alice".into(),
        }]),
        ..tx("claim", 0, 0)
    };
    let result = f.run(2, &[claim, admitted], &[], false, false).unwrap();
    assert_eq!(result.block.unwrap().txs.len(), 1);
    let receipt = f.state.storage.get_receipt("admitted").unwrap();
    assert_eq!(receipt.status, crate::storage::receipts::TxStatus::Failed);
    assert_eq!(receipt.block_height, None);
    assert_eq!(receipt.index, None);
    assert_eq!(receipt.gas_used, 0);
    assert_eq!(value::<u128>(&f, settlement::FEE_KEY), 100_000);
    assert_eq!(f.state.nonce_of("alice"), 1);
    verify_recovery(&f.state.storage).unwrap();
    let path = f.dir.path().join("node.db");
    let Fixture {
        state,
        emission,
        staking,
        burn,
        dir,
    } = f;
    drop((state, emission, staking, burn));
    let reopened = Storage::open(path).unwrap();
    verify_recovery(&reopened).unwrap();
    assert_eq!(
        reopened.get_receipt("admitted").unwrap().status,
        crate::storage::receipts::TxStatus::Failed
    );
    drop((reopened, dir));
}
#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn rejected_receipt_write_failure_keeps_pending_and_state_unchanged() {
    let mut f = Fixture::new();
    let input = [tx("rejected", 8, 1)];
    f.state.storage.put_pending_transaction(&input[0]).unwrap();
    let before = f.data();
    assert!(commit_legacy_fixture_with(
        &mut f.state,
        &mut f.emission,
        &mut f.staking,
        &mut f.burn,
        false,
        &BlockRequest {
            height: 1,
            transactions: &input,
            assets: &[],
            timestamp: 10,
            empty_blocks: false
        },
        |_, _| bail!("Injected receipt write failure")
    )
    .is_err());
    assert_eq!(before, f.data());
    assert_eq!(
        f.state.storage.get_receipt("rejected").unwrap().status,
        crate::storage::receipts::TxStatus::Pending
    );
}

#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn drt_supply_tracks_transfers_fees_and_emission_without_double_counting() {
    let mut f = Fixture::new();
    let before = crate::supply::inspect(&f.state.storage).unwrap();
    assert_eq!(
        (before.genesis, before.total, before.liquid),
        (1_000_000, 1_000_000, 1_000_000)
    );
    let send = Transaction {
        denom: "udrt".into(),
        ..tx("drt-send", 0, 200_000)
    };
    let fail = tx("drt-fee-failed-send", 1, 1001);
    f.run(1, &[send, fail], &[], false, true).unwrap();
    let supply = crate::supply::inspect(&f.state.storage).unwrap();
    assert_eq!(
        (supply.height, supply.emitted, supply.total),
        (1, 100, 1_000_100)
    );
    assert_eq!((supply.liquid, supply.withheld_fees), (900_000, 100_000));
    assert_eq!(supply.pools.values().sum::<u128>(), 100);
    // Pending rewards represent claims on the pool, not additional minted units.
    assert_eq!(
        f.staking.pending_staking_emission,
        supply.pools["staking_rewards"]
    );
    let response = serde_json::to_value(supply.response()).unwrap();
    assert_eq!(response["burned"], "0");
    assert_eq!(response["total"], "1000100");
    assert_eq!(f.state.get_balance("bob", "udrt"), 200_000);
    let durable = f.data();
    // A repeated read and block retry neither mint nor charge again.
    f.run(
        1,
        &[
            Transaction {
                denom: "udrt".into(),
                ..tx("drt-send", 0, 200_000)
            },
            tx("drt-fee-failed-send", 1, 1001),
        ],
        &[],
        false,
        true,
    )
    .unwrap();
    assert_eq!(crate::supply::inspect(&f.state.storage).unwrap(), supply);
    assert_eq!(f.data(), durable);
}

#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn drt_supply_uses_storage_instead_of_emission_cache_or_config() {
    let mut f = Fixture::new();
    f.run(1, &[], &[], true, false).unwrap();
    f.emission.circulating_supply = u128::MAX;
    f.emission.config.initial_supply = u128::MAX;
    let info = f.emission.get_supply_info().unwrap();
    assert_eq!(
        (
            info.initial_supply,
            info.circulating_supply,
            info.total_supply
        ),
        (1_000_000, 100, 1_000_100)
    );
}

#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn mismatched_initial_supply_stops_before_commit() {
    let mut f = Fixture::new();
    f.emission.config.initial_supply = 0;
    let before = f.data();
    assert!(verify_policy(&f.emission, false)
        .unwrap_err()
        .to_string()
        .contains("funded genesis"));
    assert!(f.run(1, &[tx("first", 0, 1)], &[], false, false).is_err());
    assert_eq!(f.data(), before);
    assert_eq!(f.state.nonce_of("alice"), 0);
}

#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn genesis_plus_emission_overflow_stops_before_commit() {
    let mut f = Fixture::funded(u128::MAX, 0);
    let before = f.data();
    assert!(f.run(1, &[], &[], true, false).is_err());
    assert_eq!(f.data(), before);
    let response =
        serde_json::to_value(crate::supply::inspect(&f.state.storage).unwrap().response()).unwrap();
    assert_eq!(response["total"], u128::MAX.to_string());
}

#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn percentage_schedule_uses_funded_genesis_supply() {
    let mut f = Fixture::funded(1_000_000_000_000, 0);
    f.emission.config.schedule = EmissionSchedule::Percentage {
        annual_inflation_rate: 500,
    };
    f.run(1, &[], &[], true, false).unwrap();
    let supply = crate::supply::inspect(&f.state.storage).unwrap();
    assert_eq!(supply.emitted, 50_000_000_000 / 5_256_000);
    assert_eq!(supply.total, supply.genesis + supply.emitted);
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

#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn dgt_custody_counts_funded_stake_once_and_preserves_it_through_blocks() {
    let mut f = Fixture::with_stake(1_000_000, 0, 400);
    let initial = crate::supply::inspect_native(&f.state.storage).unwrap();
    assert_eq!(
        (initial.dgt.issued, initial.dgt.liquid, initial.dgt.staked),
        (1000, 600, 400)
    );
    assert_eq!(initial.dgt.stake_basis_points().unwrap(), 4000);
    let input = [tx("dgt-send", 0, 100), tx("dgt-failed", 1, 1000)];
    f.run(1, &input, &[], false, true).unwrap();
    let supply = crate::supply::inspect_native(&f.state.storage).unwrap();
    assert_eq!(
        (supply.dgt.issued, supply.dgt.liquid, supply.dgt.staked),
        (1000, 600, 400)
    );
    assert_eq!(supply.dgt.height, 1);
    assert_eq!(supply.drt.withheld_fees, 100_000);
    assert_eq!(f.state.get_balance("alice", "udgt"), 500);
    assert_eq!(f.state.get_balance("bob", "udgt"), 100);
    let before = f.data();
    f.run(1, &input, &[], false, true).unwrap();
    assert_eq!(
        crate::supply::inspect_native(&f.state.storage).unwrap(),
        supply
    );
    assert_eq!(f.data(), before);
    let path = f.dir.path().join("node.db");
    let Fixture {
        state,
        emission,
        staking,
        burn,
        dir,
    } = f;
    drop((state, emission, staking, burn));
    let reopened = Storage::open(path).unwrap();
    assert_eq!(crate::supply::inspect_native(&reopened).unwrap(), supply);
    drop((reopened, dir));
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

#[cfg(feature = "legacy-economic-fixtures")]
#[test]
fn committed_transaction_record_is_required_and_matches_block_body() {
    let mut f = Fixture::new();
    let transaction = tx("indexed", 0, 100);
    f.run(1, std::slice::from_ref(&transaction), &[], false, false)
        .unwrap();
    let record = f
        .state
        .storage
        .get_transaction_record("indexed")
        .unwrap()
        .unwrap();
    assert!(record.matches(&transaction).unwrap());
    verify_recovery(&f.state.storage).unwrap();
    f.state.storage.db.delete("tx:indexed").unwrap();
    let before = f.data();
    assert!(verify_recovery(&f.state.storage)
        .unwrap_err()
        .to_string()
        .contains("transaction record"));
    assert_eq!(before, f.data());
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

#[cfg(any(feature = "pqc-fips204", feature = "pqc-real"))]
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
        #[cfg(feature = "pqc-fips204")]
        let algorithm = crate::addr::OriginKeyAlgorithm::MlDsa65;
        #[cfg(all(feature = "pqc-real", not(feature = "pqc-fips204")))]
        let algorithm = crate::addr::OriginKeyAlgorithm::LegacyDilithium5;
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
