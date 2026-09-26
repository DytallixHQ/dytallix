//! Explicit local finalization fixtures. These tests do not prove consensus finality.
use dytallix_fast_node::{
    addr::{initial_address, AddressNetwork, OriginKeyAlgorithm},
    block_settlement::{self, BlockOutcome, BlockRequest},
    crypto::{ActivePQC, PQC},
    genesis,
    runtime::{
        emission::{EmissionBreakdown, EmissionConfig, EmissionEngine, EmissionSchedule},
        fee_burn::FeeBurnEngine,
        reward_runtime::{RewardState, REWARD_STATE_KEY},
        staking::StakingModule,
    },
    state::State,
    storage::{
        state::Storage,
        transaction_record::SignedEnvelope,
        tx::{Transaction, TxMessage},
    },
    supply,
    types::{tx::Tx, Msg, SignedTx},
};
use rocksdb::IteratorMode;
use std::sync::{Arc, Mutex};

const CHAIN: &str = "reward-block-local";
const VALIDATOR: &str = "fixture-validator";
const FEE: u128 = 50_000;
const INITIAL_DRT: u128 = 10_000_000;
struct Actor {
    address: String,
    secret: Vec<u8>,
    public: Vec<u8>,
}
impl Actor {
    fn new() -> Self {
        let (secret, public) = ActivePQC::keypair();
        let algorithm = OriginKeyAlgorithm::MlDsa65;
        let address =
            initial_address(AddressNetwork::Development, CHAIN, algorithm, &public).unwrap();
        Self {
            address,
            secret,
            public,
        }
    }
}
struct Fixture {
    state: State,
    emission: EmissionEngine,
    staking: StakingModule,
    burn: FeeBurnEngine,
    actors: Vec<Actor>,
    source: Vec<u8>,
    per_block: u128,
    dir: tempfile::TempDir,
}
impl Fixture {
    fn new(stakes: [u128; 2], amounts: [u128; 2], per_block: u128, locked: Option<bool>) -> Self {
        let actors = vec![Actor::new(), Actor::new()];
        let accounts: Vec<_> = actors.iter().enumerate().map(|(i, actor)| {
            let vesting = if i == 0 && locked.is_some() {
                serde_json::json!({"kind":"linear_after_cliff", "total_amount":amounts[i].to_string(),
                    "start_time":0,"cliff_duration":1000,"vesting_duration":2000,"allow_staking":locked.unwrap()})
            } else { serde_json::json!({"kind":"unlocked"}) };
            serde_json::json!({"address":actor.address,"balances":{"udgt":amounts[i].to_string(),"udrt":INITIAL_DRT.to_string()},"vesting":vesting})
        }).collect();
        let delegations: Vec<_> = actors.iter().enumerate().filter(|(i,_)| stakes[*i]>0).map(|(i,actor)|
            serde_json::json!({"delegator":actor.address,"amount_udgt":stakes[i].to_string()})).collect();
        let positions: Vec<_> = actors.iter().enumerate().filter(|(i,_)| stakes[*i]>0).map(|(i,actor)|
            serde_json::json!({"owner":actor.address,"validator":VALIDATOR,"amount_udgt":stakes[i].to_string()})).collect();
        let source = serde_json::to_vec(&serde_json::json!({"chain_id":CHAIN,"accounts":accounts,
            "staking":{"delegations":delegations},"reward_v2":{"version":2,"activation_height":1,
                "decimals":6,"profile":"development","max_validators":4,"max_positions":8,
                "validators":[{"address":VALIDATOR,"active":true,"jailed":false}],"positions":positions}})).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut storage = Storage::open(dir.path().join("node.db")).unwrap();
        genesis::initialize(&mut storage, CHAIN, Some(&source)).unwrap();
        Self::open(Arc::new(storage), actors, source, per_block, dir)
    }
    fn open(
        storage: Arc<Storage>,
        actors: Vec<Actor>,
        source: Vec<u8>,
        per_block: u128,
        dir: tempfile::TempDir,
    ) -> Self {
        // This 60/25/10/5 split is a synthetic accounting fixture, not approved production allocation.
        let config = EmissionConfig {
            schedule: EmissionSchedule::Static { per_block },
            initial_supply: INITIAL_DRT * 2,
            emission_breakdown: EmissionBreakdown {
                block_rewards: 60,
                staking_rewards: 25,
                ai_module_incentives: 10,
                bridge_operations: 5,
            },
        };
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
            actors,
            source,
            per_block,
            dir,
        }
    }
    fn restart(self) -> Self {
        let Self {
            state,
            emission,
            staking,
            burn,
            actors,
            source,
            per_block,
            dir,
        } = self;
        drop(state);
        drop(emission);
        drop(staking);
        drop(burn);
        let mut storage = Storage::open(dir.path().join("node.db")).unwrap();
        genesis::initialize(&mut storage, CHAIN, Some(&source)).unwrap();
        block_settlement::verify_recovery(&storage).unwrap();
        Self::open(Arc::new(storage), actors, source, per_block, dir)
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
    fn unpaid(&self, actor: usize) -> u128 {
        self.reward()
            .unpaid
            .get(&self.actors[actor].address)
            .copied()
            .unwrap_or(0)
    }
    fn snapshot(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.state
            .storage
            .db
            .iterator(IteratorMode::Start)
            .map(|item| {
                let (k, v) = item.unwrap();
                (k.to_vec(), v.to_vec())
            })
            .collect()
    }
    fn run(&mut self, height: u64, transactions: &[Transaction]) -> anyhow::Result<BlockOutcome> {
        block_settlement::commit_reward_development_block(
            &mut self.state,
            &mut self.emission,
            &mut self.staking,
            &mut self.burn,
            true,
            &BlockRequest {
                height,
                transactions,
                assets: &[],
                timestamp: height * 10,
                empty_blocks: true,
            },
        )
    }
    fn signed(&self, actor: usize, nonce: u64, messages: Vec<Msg>) -> Transaction {
        let signer = &self.actors[actor];
        let signed = SignedTx::sign(
            Tx {
                chain_id: CHAIN.into(),
                nonce,
                msgs: messages,
                fee: FEE,
                memo: "reward block fixture".into(),
            },
            &signer.secret,
            &signer.public,
        )
        .unwrap();
        // Deliberately small fixture conversion. Block settlement independently reconstructs
        // this complete representation from the original signed canonical envelope.
        let mut tx = Transaction::new(
            signed.tx_hash().unwrap(),
            &signer.address,
            &signer.address,
            0,
            FEE,
            nonce,
            Some(signed.signature.clone()),
        )
        .with_pqc(&signed.public_key, CHAIN, &signed.tx.memo)
        .with_gas(FEE as u64, 1);
        let mut converted = Vec::new();
        for msg in &signed.tx.msgs {
            converted.push(match msg {
                Msg::RewardBond {
                    from,
                    validator,
                    amount_udgt,
                } => TxMessage::RewardBond {
                    from: from.clone(),
                    validator: validator.clone(),
                    amount_udgt: *amount_udgt,
                },
                Msg::RewardBeginUnbond {
                    from,
                    validator,
                    amount_udgt,
                } => TxMessage::RewardBeginUnbond {
                    from: from.clone(),
                    validator: validator.clone(),
                    amount_udgt: *amount_udgt,
                },
                Msg::RewardClaim { from } => TxMessage::RewardClaim { from: from.clone() },
                Msg::Send {
                    from,
                    to,
                    denom,
                    amount,
                } => {
                    assert_eq!(denom, "udgt");
                    tx.amount = tx.amount.checked_add(*amount).unwrap();
                    if tx.to == signer.address {
                        tx.to = to.clone();
                        tx.denom = denom.clone();
                    }
                    TxMessage::Send {
                        from: from.clone(),
                        to: to.clone(),
                        denom: denom.clone(),
                        amount: *amount,
                    }
                }
                _ => panic!("unsupported fixture message"),
            });
        }
        tx.messages = Some(converted);
        let envelope: SignedEnvelope =
            serde_json::from_value(serde_json::to_value(&signed).unwrap()).unwrap();
        self.state
            .storage
            .put_pending_signed_transaction(&tx, Some(envelope))
            .unwrap();
        tx
    }
    fn bond(&self, actor: usize, nonce: u64, amount: u128, validator: &str) -> Transaction {
        self.signed(
            actor,
            nonce,
            vec![Msg::RewardBond {
                from: self.actors[actor].address.clone(),
                validator: validator.into(),
                amount_udgt: amount,
            }],
        )
    }
    fn claim(&self, actor: usize, nonce: u64) -> Transaction {
        self.signed(
            actor,
            nonce,
            vec![Msg::RewardClaim {
                from: self.actors[actor].address.clone(),
            }],
        )
    }
}

#[test]
fn exact_parent_snapshot_rewards_and_claims_preserve_supply() {
    let mut f = Fixture::new(
        [600_000_000_000, 0],
        [600_000_000_000, 400_000_000_000],
        1_000_000,
        None,
    );
    let bond = f.bond(1, 0, 400_000_000_000, VALIDATOR);
    assert!(f.run(1, &[bond]).unwrap().receipts[0].success);
    assert_eq!((f.unpaid(0), f.unpaid(1)), (250_000, 0));
    f.run(2, &[]).unwrap();
    assert_eq!((f.unpaid(0), f.unpaid(1)), (400_000, 100_000));
    let claims = [f.claim(0, 0), f.claim(1, 1)];
    let outcome = f.run(3, &claims).unwrap();
    assert!(outcome.receipts.iter().all(|r| r.success));
    // Block3 first allocates150000/100000 from its parent snapshot.
    assert_eq!(f.reward().total_claimed, 750_000);
    assert_eq!((f.unpaid(0), f.unpaid(1)), (0, 0));
    assert_eq!(
        f.state.get_balance(&f.actors[0].address, "udrt"),
        INITIAL_DRT - FEE + 550_000
    );
    assert_eq!(
        f.state.get_balance(&f.actors[1].address, "udrt"),
        INITIAL_DRT - 2 * FEE + 200_000
    );
    let supply = supply::inspect_native(&f.state.storage).unwrap();
    assert_eq!(supply.drt.pools["staking_rewards"], 0);
    assert_eq!(supply.drt.emitted, 3_000_000);
    assert_eq!(supply.dgt.staked, 1_000_000_000_000);
    assert_eq!(
        supply.drt.liquid + supply.drt.withheld_fees + supply.drt.pools.values().sum::<u128>(),
        supply.drt.total
    );
}

#[test]
fn exit_affects_next_snapshot_and_preserves_unbonding_custody() {
    let mut f = Fixture::new([100, 0], [100, 100], 1_000_000, None);
    let exit = f.signed(
        0,
        0,
        vec![Msg::RewardBeginUnbond {
            from: f.actors[0].address.clone(),
            validator: VALIDATOR.into(),
            amount_udgt: 100,
        }],
    );
    assert!(f.run(1, &[exit]).unwrap().receipts[0].success);
    assert_eq!(f.unpaid(0), 250_000);
    assert_eq!(f.reward().unbonding[&f.actors[0].address], 100);
    assert_eq!(f.state.get_balance(&f.actors[0].address, "udgt"), 0);
    let bond = f.bond(1, 0, 100, VALIDATOR);
    f.run(2, &[bond]).unwrap();
    assert_eq!(f.reward().inactive_reserve, 250_000);
    assert_eq!(f.unpaid(1), 0);
    f.run(3, &[]).unwrap();
    assert_eq!(f.unpaid(1), 250_000);
    assert_eq!(f.reward().inactive_reserve, 250_000);
    let supply = supply::inspect_native(&f.state.storage).unwrap();
    assert_eq!((supply.dgt.staked, supply.dgt.unbonding), (100, 100));
}

#[test]
fn integer_rounding_reserve_has_no_claim_recipient() {
    let mut f = Fixture::new([1, 2], [1, 2], 4, None);
    f.run(1, &[]).unwrap();
    assert_eq!((f.unpaid(0), f.unpaid(1)), (0, 0));
    assert_eq!(f.reward().rounding_reserve, 1);
    let claims = [f.claim(0, 0), f.claim(1, 0)];
    let outcome = f.run(2, &claims).unwrap();
    assert!(outcome.receipts.iter().all(|r| r.success));
    assert_eq!(f.reward().total_claimed, 0);
    assert_eq!(f.reward().rounding_reserve, 2);
    assert_eq!(
        supply::inspect(&f.state.storage).unwrap().pools["staking_rewards"],
        2
    );
    assert_eq!(
        f.state.get_balance(&f.actors[0].address, "udrt"),
        INITIAL_DRT - FEE
    );
}

#[test]
fn restart_and_identical_retry_do_not_allocate_or_charge_twice() {
    let mut f = Fixture::new([100, 0], [100, 100], 1_000_000, None);
    let bond = f.bond(1, 0, 100, VALIDATOR);
    f.run(1, std::slice::from_ref(&bond)).unwrap();
    let before = f.snapshot();
    f = f.restart();
    f.run(1, std::slice::from_ref(&bond)).unwrap();
    assert_eq!(f.snapshot(), before);
    assert!(f.run(1, &[]).is_err());
    assert_eq!(f.snapshot(), before);
    assert_eq!(f.reward().total_budget, 250_000);
    assert_eq!(f.state.nonce_of(&f.actors[1].address), 1);
}

#[test]
fn unsupported_validator_and_failed_message_roll_back_bond_effects() {
    let mut f = Fixture::new([0, 0], [100, 100], 1_000_000, None);
    let tx = f.signed(
        0,
        0,
        vec![
            Msg::RewardBond {
                from: f.actors[0].address.clone(),
                validator: VALIDATOR.into(),
                amount_udgt: 20,
            },
            Msg::RewardBond {
                from: f.actors[0].address.clone(),
                validator: "unsupported".into(),
                amount_udgt: 1,
            },
        ],
    );
    assert!(!f.run(1, &[tx]).unwrap().receipts[0].success);
    assert!(f.reward().positions.is_empty());
    assert_eq!(f.state.get_balance(&f.actors[0].address, "udgt"), 100);
    assert_eq!(
        f.state.get_balance(&f.actors[0].address, "udrt"),
        INITIAL_DRT - FEE
    );
    assert_eq!(f.state.nonce_of(&f.actors[0].address), 1);
    assert_eq!(f.reward().inactive_reserve, 250_000);
    supply::inspect_native(&f.state.storage).unwrap();
}

#[test]
fn explicit_vesting_controls_bonds_and_blocks_locked_transfers() {
    for permits_staking in [false, true] {
        let mut f = Fixture::new([0, 0], [100, 100], 1_000_000, Some(permits_staking));
        let bond = f.bond(0, 0, 1, VALIDATOR);
        let transfer = f.signed(
            0,
            1,
            vec![Msg::Send {
                from: f.actors[0].address.clone(),
                to: f.actors[1].address.clone(),
                denom: "udgt".into(),
                amount: 1,
            }],
        );
        let outcome = f.run(1, &[bond, transfer]).unwrap();
        assert_eq!(
            outcome
                .receipts
                .iter()
                .map(|r| r.success)
                .collect::<Vec<_>>(),
            vec![permits_staking, false]
        );
        assert_eq!(f.state.get_balance(&f.actors[1].address, "udgt"), 100);
        assert_eq!(
            f.state.get_balance(&f.actors[0].address, "udgt"),
            if permits_staking { 99 } else { 100 }
        );
        assert_eq!(f.reward().total_budget, 250_000);
        supply::inspect_native(&f.state.storage).unwrap();
    }
}

#[test]
fn reward_activation_rejects_timer_unsigned_and_legacy_paths_without_block_writes() {
    let mut f = Fixture::new([100, 0], [100, 100], 1_000_000, None);
    let before = f.snapshot();
    assert!(block_settlement::commit_block(
        &mut f.state,
        &mut f.emission,
        &mut f.staking,
        &mut f.burn,
        true,
        &BlockRequest {
            height: 1,
            transactions: &[],
            assets: &[],
            timestamp: 10,
            empty_blocks: true
        }
    )
    .is_err());
    assert_eq!(f.snapshot(), before);
    let tx = Transaction::base(
        "unsigned-reward-mode",
        &f.actors[0].address,
        &f.actors[1].address,
        1,
        FEE,
        0,
    )
    .with_gas(FEE as u64, 1);
    f.state.storage.put_pending_transaction(&tx).unwrap();
    let pending = f.snapshot();
    assert!(f.run(1, &[tx]).is_err());
    assert_eq!(f.snapshot(), pending);
    let dir = tempfile::tempdir().unwrap();
    let mut legacy = Storage::open(dir.path().join("legacy.db")).unwrap();
    let mut source: serde_json::Value = serde_json::from_slice(&f.source).unwrap();
    source.as_object_mut().unwrap().remove("reward_v2");
    // Explicit unlocked vesting has no legacy lock state to migrate.
    genesis::initialize(
        &mut legacy,
        CHAIN,
        Some(&serde_json::to_vec(&source).unwrap()),
    )
    .unwrap();
    let before: Vec<_> = legacy
        .db
        .iterator(IteratorMode::Start)
        .map(Result::unwrap)
        .collect();
    assert!(genesis::initialize(&mut legacy, CHAIN, Some(&f.source)).is_err());
    let after: Vec<_> = legacy
        .db
        .iterator(IteratorMode::Start)
        .map(Result::unwrap)
        .collect();
    assert_eq!(before, after);
}
