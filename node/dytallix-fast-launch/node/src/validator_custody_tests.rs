//! Funded custody tests for scheduled validator changes.
use super::*;
use crate::runtime::validator_lifecycle::ValidatorIdentity;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use fips204::{
    ml_dsa_65,
    traits::{KeyGen, SerDes},
};

fn fixture() -> (tempfile::TempDir, Arc<Storage>) {
    let dir = tempfile::tempdir().unwrap();
    let mut storage = Storage::open(dir.path().join("db")).unwrap();
    let genesis = serde_json::to_vec(&serde_json::json!({
        "chain_id":"custody-lifecycle",
        "accounts":[
            {"address":"alice","balances":{"udgt":"100","udrt":"1000"},"vesting":{"kind":"unlocked"}},
            {"address":"bob","balances":{"udgt":"100","udrt":"1000"},"vesting":{"kind":"unlocked"}}
        ],
        "staking":{"delegations":[
            {"delegator":"alice","amount_udgt":"40"},
            {"delegator":"bob","amount_udgt":"20"}
        ]},
        "reward_v2":{"version":2,"activation_height":1,"decimals":6,
            "profile":"development","max_validators":4,"max_positions":8,
            "validators":[{"address":"validator-a","active":true,"jailed":false},
                {"address":"validator-b","active":true,"jailed":false}],
            "positions":[{"owner":"alice","validator":"validator-a","amount_udgt":"40"},
                {"owner":"bob","validator":"validator-b","amount_udgt":"20"}]}
    })).unwrap();
    crate::genesis::initialize(&mut storage, "custody-lifecycle", Some(&genesis)).unwrap();
    let rewards = RewardState::decode(&storage.db.get(REWARD_STATE_KEY).unwrap().unwrap()).unwrap();
    let config = serde_json::from_value(serde_json::json!({
        "version":1,"profile":"cometbft-lifecycle-local-qualification","chain_id":"custody-lifecycle",
        "approved_operators":{"validator-a":"alice","validator-b":"bob"},
        "min_self_bond":"10","max_active":2,
        "evidence_max_age_blocks":10,"evidence_max_age_seconds":60,
        "processing_margin_blocks":2,"processing_margin_seconds":10
    })).unwrap();
    let identities = [("validator-a", "alice"), ("validator-b", "bob")]
        .into_iter()
        .map(|(id, owner)| {
            let (public, _) = ml_dsa_65::KG::try_keygen_with_rng(&mut rand_core::OsRng).unwrap();
            (
                id.into(),
                ValidatorIdentity {
                    owner: owner.into(),
                    pubkey_base64: B64.encode(public.into_bytes()),
                },
            )
        })
        .collect();
    let validators = LifecycleState::new(config, identities, &rewards).unwrap();
    storage
        .db
        .put(VALIDATOR_STATE_KEY, validators.encode().unwrap())
        .unwrap();
    crate::supply::validate_native(&storage, &BTreeMap::new()).unwrap();
    (dir, Arc::new(storage))
}

fn stage(storage: &Arc<Storage>, height: u64) -> Settlement {
    let mut rewards =
        RewardState::decode(&storage.db.get(REWARD_STATE_KEY).unwrap().unwrap()).unwrap();
    let mut validators =
        LifecycleState::decode(&storage.db.get(VALIDATOR_STATE_KEY).unwrap().unwrap()).unwrap();
    validators
        .advance(height, (height - 1) * 10, &mut rewards)
        .unwrap();
    rewards
        .stage_interval(height, &format!("parent-{height}"), 0)
        .unwrap();
    let mut writes = BTreeMap::from([
        (
            REWARD_STATE_KEY.as_bytes().to_vec(),
            rewards.encode().unwrap(),
        ),
        (
            VALIDATOR_STATE_KEY.as_bytes().to_vec(),
            validators.encode().unwrap(),
        ),
        (
            b"emission:last_height".to_vec(),
            height.to_be_bytes().to_vec(),
        ),
        (
            b"emission:circulating_supply".to_vec(),
            bincode::serialize(&0u128).unwrap(),
        ),
        (
            b"emission:pool:staking_rewards".to_vec(),
            bincode::serialize(&0u128).unwrap(),
        ),
    ]);
    let event = (
        height,
        height * 10,
        0u128,
        BTreeMap::<String, u128>::new(),
        None::<u128>,
        0u128,
    );
    writes.insert(
        format!("emission:event:{height}").into_bytes(),
        bincode::serialize(&event).unwrap(),
    );
    let mut staged = Settlement::new(storage.clone());
    staged.attach_reward_lifecycle(writes, height * 10).unwrap();
    staged
}

fn persist(staged: &Settlement) -> crate::supply::NativeSupply {
    let writes = staged.writes().unwrap();
    let supply = crate::supply::validate_native(&staged.storage, &writes).unwrap();
    let mut batch = WriteBatch::default();
    for (key, value) in writes {
        batch.put(key, value);
    }
    staged.storage.db.write(batch).unwrap();
    supply
}

#[test]
fn validator_pending_bond_debits_liquid_and_activates_at_h_plus_two() {
    let (_dir, storage) = fixture();
    let mut h1 = stage(&storage, 1);
    h1.reward_bond("alice", "validator-a", 10).unwrap();
    assert_eq!(
        h1.rewards.as_ref().unwrap().owner_bonded("alice").unwrap(),
        40
    );
    assert_eq!(h1.account("alice").unwrap().balance_of("udgt"), 50);
    assert_eq!(h1.pending_bond("alice").unwrap(), 10);
    let s1 = persist(&h1);
    assert_eq!(
        (
            s1.dgt.liquid,
            s1.dgt.staked,
            s1.dgt.pending_bonded,
            s1.dgt.unbonding
        ),
        (130, 60, 10, 0)
    );
    let h2 = stage(&storage, 2);
    assert_eq!(
        h2.rewards.as_ref().unwrap().owner_bonded("alice").unwrap(),
        40
    );
    assert_eq!(persist(&h2).dgt.pending_bonded, 10);
    let h3 = stage(&storage, 3);
    assert_eq!(
        h3.rewards.as_ref().unwrap().owner_bonded("alice").unwrap(),
        50
    );
    let s3 = persist(&h3);
    assert_eq!(
        (s3.dgt.liquid, s3.dgt.staked, s3.dgt.pending_bonded),
        (130, 70, 0)
    );
}

#[test]
fn validator_unbond_keeps_effective_principal_until_removal_height() {
    let (_dir, storage) = fixture();
    let mut h1 = stage(&storage, 1);
    h1.reward_begin_unbond("alice", "validator-a", 10).unwrap();
    assert_eq!(persist(&h1).dgt.unbonding, 0);
    assert_eq!(persist(&stage(&storage, 2)).dgt.staked, 60);
    let h3 = stage(&storage, 3);
    let s3 = persist(&h3);
    assert_eq!(
        (
            s3.dgt.liquid,
            s3.dgt.staked,
            s3.dgt.pending_bonded,
            s3.dgt.unbonding
        ),
        (140, 50, 0, 10)
    );
    let lifecycle = h3.validators.as_ref().unwrap();
    assert_eq!(
        lifecycle.unbonding_by_owner().unwrap(),
        BTreeMap::from([("alice".into(), 10)])
    );
}

#[test]
fn validator_failed_bond_preserves_fees_and_principal() {
    let (_dir, storage) = fixture();
    let mut h1 = stage(&storage, 1);
    h1.charge("alice", 7).unwrap();
    let before = h1.writes().unwrap();
    let error = h1
        .reward_bond("alice", "unknown-validator", 10)
        .unwrap_err();
    assert!(error.downcast_ref::<RuleViolation>().is_some());
    assert_eq!(h1.writes().unwrap(), before);
    assert_eq!(h1.account("alice").unwrap().balance_of("udgt"), 60);
    assert_eq!(h1.account("alice").unwrap().balance_of("udrt"), 993);
    assert_eq!(h1.account("alice").unwrap().nonce, 1);
    assert!(h1
        .reward_bond("alice", "validator-a", 61)
        .unwrap_err()
        .downcast_ref::<RuleViolation>()
        .is_some());
    assert_eq!(h1.writes().unwrap(), before);
    persist(&h1);
}

#[test]
fn validator_pending_bond_is_counted_once_and_cannot_be_spent_twice() {
    let (_dir, storage) = fixture();
    let mut h1 = stage(&storage, 1);
    h1.reward_bond("alice", "validator-a", 60).unwrap();
    assert!(h1.reward_bond("alice", "validator-a", 1).is_err());
    assert!(h1.transfer("alice", "bob", "udgt", 1).is_err());
    let mut writes = h1.writes().unwrap();
    let supply = crate::supply::validate_native(&storage, &writes).unwrap();
    assert_eq!(supply.dgt.pending_bonded, 60);
    assert_eq!(supply.dgt.issued, 200);
    let raw = writes.get_mut(b"acct:balances:alice".as_slice()).unwrap();
    let mut balances: BTreeMap<String, u128> = bincode::deserialize(raw).unwrap();
    balances.insert("udgt".into(), 1);
    *raw = bincode::serialize(&balances).unwrap();
    assert!(crate::supply::validate_native(&storage, &writes).is_err());
}

#[test]
fn validator_exit_retains_each_delegator_principal() {
    let (_dir, storage) = fixture();
    let mut h1 = stage(&storage, 1);
    h1.reward_bond("bob", "validator-a", 5).unwrap();
    persist(&h1);
    persist(&stage(&storage, 2));
    let mut h3 = stage(&storage, 3);
    h3.validator_exit("alice", 0, "validator-a").unwrap();
    persist(&h3);
    persist(&stage(&storage, 4));
    let h5 = stage(&storage, 5);
    assert_eq!(
        h5.validators
            .as_ref()
            .unwrap()
            .unbonding_by_owner()
            .unwrap(),
        BTreeMap::from([("alice".into(), 40), ("bob".into(), 5)])
    );
    let supply = persist(&h5);
    assert_eq!(
        (supply.dgt.liquid, supply.dgt.staked, supply.dgt.unbonding),
        (135, 20, 45)
    );
}

#[test]
fn validator_pending_principal_backs_only_staking_permitted_vesting() {
    let (_dir, storage) = fixture();
    let mut rewards =
        RewardState::decode(&storage.db.get(REWARD_STATE_KEY).unwrap().unwrap()).unwrap();
    rewards.locks.insert(
        "alice".into(),
        crate::runtime::reward_runtime::VestingLock {
            total_amount: 100,
            start_time: 100,
            cliff_duration: 10,
            vesting_duration: 20,
            permits_staking: true,
        },
    );
    storage
        .db
        .put(REWARD_STATE_KEY, rewards.encode().unwrap())
        .unwrap();
    let mut h1 = stage(&storage, 1);
    h1.reward_bond("alice", "validator-a", 10).unwrap();
    let error = h1.transfer("alice", "bob", "udgt", 1).unwrap_err();
    assert!(error.downcast_ref::<RuleViolation>().is_some());
    let supply = persist(&h1);
    assert_eq!(
        (
            supply.dgt.liquid,
            supply.dgt.staked,
            supply.dgt.pending_bonded
        ),
        (130, 60, 10)
    );
}
