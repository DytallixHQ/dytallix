//! Local signed recovery persistence checks with explicit synthetic parameters.
//! These checks do not qualify live clients, production fees, or ordinary signing.
use super::*;
use crate::crypto::{ActivePQC, PQC};
use crate::recovery_fees::{RecoveryAccount, RecoveryBook, STATE_KEY};
use crate::storage::state::Storage;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use dytallix_protocol_types::recovery::*;
use dytallix_protocol_types::recovery_sponsor::{
    self as sponsor, FeeProfile, SponsorAuthorization, SponsoredRecovery,
};
use dytallix_protocol_types::recovery_wire::{
    self, RecoveryOperation, RecoverySignature, SignatureRole, SignedRecovery,
};
use fips204::{
    ml_dsa_65,
    traits::{KeyGen, SerDes},
};
use rocksdb::{IteratorMode, WriteBatch, WriteOptions};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const CHAIN: &str = "recovery-consensus-fixture";
const INITIAL_DRT: u128 = 10_000_000;
const TARGET: [u8; 32] = [11; 32];
const SPONSOR: [u8; 32] = [12; 32];
const REQUEST: [u8; 32] = [13; 32];
const SECOND_TARGET: [u8; 32] = [14; 32];
const GAS_LIMIT: u64 = 1_000_000;

struct Key {
    secret: Vec<u8>,
    identity: KeyIdentity,
}
impl Key {
    fn new() -> Self {
        let (secret, public_key) = ActivePQC::keypair();
        Self {
            secret,
            identity: KeyIdentity {
                algorithm: "mldsa65".into(),
                public_key,
            },
        }
    }
    fn address(&self) -> String {
        crate::addr::initial_address(
            crate::addr::AddressNetwork::Development,
            CHAIN,
            crate::addr::OriginKeyAlgorithm::MlDsa65,
            &self.identity.public_key,
        )
        .unwrap()
    }
    fn sign(&self, operation: &RecoveryOperation, role: SignatureRole) -> RecoverySignature {
        let bytes = recovery_wire::signing_bytes(operation, role, &self.identity).unwrap();
        RecoverySignature {
            role,
            key: self.identity.clone(),
            signature: ActivePQC::sign(&self.secret, &bytes),
        }
    }
}
fn profile() -> FeeProfile {
    FeeProfile {
        version: 1,
        activation_height: 1,
        denomination: "udrt".into(),
        gas_price: 2,
        minimum_gas: 10,
        max_transaction_gas: GAS_LIMIT,
        max_block_gas: GAS_LIMIT * 4,
        max_block_recovery_bytes: 1_000_000,
        max_block_recovery_signatures: 30,
        max_fee_cap: 2_000_000,
        max_pending_accounts: 4,
        max_due_expiry_events_per_height: 4,
        mandatory_expiry_gas_budget: 100,
        expiry_event_gas_cost: 10,
        action_costs: [10; 9],
        wire_byte_cost: 0,
        read_byte_cost: 0,
        write_byte_cost: 1,
        signature_costs: BTreeMap::from([("mldsa65".into(), 1)]),
    }
}
struct Fixture {
    config: ConsensusConfig,
    genesis: Vec<u8>,
    active: Key,
    payer: Key,
    guardians: Vec<Key>,
    replacement: Key,
    secondary: Key,
}
impl Fixture {
    fn new() -> Self {
        Self::with_profile(profile())
    }
    fn with_profile(profile: FeeProfile) -> Self {
        let active = Key::new();
        let payer = Key::new();
        let mut guardians: Vec<_> = (0..3).map(|_| Key::new()).collect();
        guardians.sort_by(|a, b| a.identity.cmp(&b.identity));
        let replacement = Key::new();
        let secondary = Key::new();
        let (validator, _) = ml_dsa_65::KG::try_keygen_with_rng(&mut rand_core::OsRng).unwrap();
        let genesis = serde_json::to_vec(&serde_json::json!({
            "chain_id": CHAIN,
            "accounts": [
                {"address":active.address(),"balances":{"udgt":"1000","udrt":INITIAL_DRT.to_string()},"vesting":{"kind":"unlocked"}},
                {"address":payer.address(),"balances":{"udgt":"1000","udrt":INITIAL_DRT.to_string()},"vesting":{"kind":"unlocked"}},
                {"address":secondary.address(),"balances":{"udgt":"1000","udrt":INITIAL_DRT.to_string()},"vesting":{"kind":"unlocked"}}],
            "staking":{"delegations":[{"delegator":active.address(),"amount_udgt":"100"}]},
            "reward_v2":{"version":2,"activation_height":1,"decimals":6,"profile":"development","max_validators":4,"max_positions":8,
                "validators":[{"address":"validator-one","active":true,"jailed":false}],
                "positions":[{"owner":active.address(),"validator":"validator-one","amount_udgt":"100"}]},
            "adaptive_issuance":{"version":1,"profile":"development","decimals":6,"epoch_blocks":100,"initial_epoch_budget_udrt":"1000","max_recorded_epochs":8,
                "controller":{"target_ppm":500000,"shock_threshold_ppm":100000,"volatility_threshold_ppm":1000000,"window_samples":2,
                    "integral_min":-2000000,"integral_max":2000000,"soft":{"proportional":0,"integral":0,"derivative":0},
                    "hard":{"proportional":0,"integral":0,"derivative":0},"base_udrt":1000,"min_udrt":1000,"max_udrt":1000}}
        })).unwrap();
        let digest: [u8; 32] = Sha256::digest(&genesis).into();
        let recovery_config = RecoveryConfig {
            timing_version: 1,
            recovery_delay: 2,
            finalization_window: 2,
            policy_delay: 2,
            policy_window: 2,
            submission_lifetime: 50,
            algorithms: BTreeMap::from([("mldsa65".into(), 1952)]),
        };
        let accounts = [
            (TARGET, &active),
            (SPONSOR, &payer),
            (SECOND_TARGET, &secondary),
        ]
        .into_iter()
        .map(|(id, key)| RecoveryAccount {
            address: key.address(),
            sponsor_nonce: 0,
            recovery: RecoveryState::new(
                RecoveryDomain {
                    network: 3,
                    chain_id: CHAIN.into(),
                    genesis_digest: digest,
                    account_id: id,
                },
                recovery_config.clone(),
                key.identity.clone(),
                0,
            )
            .unwrap(),
        })
        .collect();
        let mut config: ConsensusConfig=serde_json::from_value(serde_json::json!({
            "profile":"cometbft-local-qualification","engine":"cometbft-v0.40.0","chain_id":CHAIN,"app_state_sha256":hex::encode(digest),
            "gas_price":1,"max_tx_bytes":65536,"max_block_bytes":1048576,"max_txs":64,
            "validators":[{"pubkey_type":"ml_dsa_65","pubkey_base64":B64.encode(validator.into_bytes()),"power":10,"reward_address":"validator-one"}]
        })).unwrap();
        config.recovery = Some(RecoveryBook::new(profile, accounts).unwrap());
        Self {
            config,
            genesis,
            active,
            payer,
            guardians,
            replacement,
            secondary,
        }
    }
    fn open(&self, path: &std::path::Path) -> ConsensusApplication {
        ConsensusApplication::open(path, self.config.clone(), self.genesis.clone()).unwrap()
    }
    fn initialized(&self, path: &std::path::Path) -> ConsensusApplication {
        let mut app = self.open(path);
        app.init_chain(CHAIN, 1, &self.genesis, &self.config.validators)
            .unwrap();
        app
    }
    fn state(&self, id: [u8; 32]) -> &RecoveryState {
        &self.config.recovery.as_ref().unwrap().accounts[&hex::encode(id)].recovery
    }
    fn operation(&self, id: [u8; 32], kind: ActionKind, expiry: u64) -> RecoveryOperation {
        RecoveryOperation {
            domain: self.state(id).domain.clone(),
            action: Action {
                submission_expiry: expiry,
                kind,
            },
        }
    }
    fn policy(&self) -> RecoveryPolicy {
        RecoveryPolicy {
            threshold: 2,
            guardians: self
                .guardians
                .iter()
                .enumerate()
                .map(|(i, k)| Guardian {
                    key: k.identity.clone(),
                    control_group: format!("fixture-custodian-{i}"),
                })
                .collect(),
        }
    }
    fn signed(
        &self,
        operation: RecoveryOperation,
        signers: &[&Key],
        proofs: &[&Key],
    ) -> SignedRecovery {
        let mut signatures: Vec<_> = signers
            .iter()
            .map(|key| key.sign(&operation, SignatureRole::Operation))
            .chain(
                proofs
                    .iter()
                    .map(|key| key.sign(&operation, SignatureRole::Possession)),
            )
            .collect();
        signatures.sort_by(|a, b| a.role.cmp(&b.role).then(a.key.cmp(&b.key)));
        SignedRecovery {
            operation,
            signatures,
        }
    }
    fn enroll(&self) -> SignedRecovery {
        let operation = self.operation(
            TARGET,
            ActionKind::Enroll {
                active: ActiveAuthorization {
                    generation: 0,
                    nonce: 0,
                },
                policy: self.policy(),
            },
            40,
        );
        self.signed(
            operation,
            &[&self.active],
            &self.guardians.iter().collect::<Vec<_>>(),
        )
    }
    fn start(&self) -> SignedRecovery {
        let operation = self.operation(
            TARGET,
            ActionKind::Start {
                recovery: RecoveryAuthorization {
                    policy_version: 1,
                    sequence: 0,
                },
                request_id: REQUEST,
                replacement: self.replacement.identity.clone(),
                timing_version: 1,
            },
            40,
        );
        self.signed(
            operation,
            &[&self.guardians[0], &self.guardians[1]],
            &[&self.replacement],
        )
    }
    fn finalize(&self) -> SignedRecovery {
        let operation = self.operation(
            TARGET,
            ActionKind::Finalize {
                recovery: RecoveryAuthorization {
                    policy_version: 1,
                    sequence: 1,
                },
                request_id: REQUEST,
            },
            40,
        );
        self.signed(operation, &[&self.replacement], &[])
    }
    fn sponsored(&self, recovery: SignedRecovery, nonce: u64, limit: u64) -> SponsoredRecovery {
        self.sponsored_by(recovery, SPONSOR, &self.payer, 0, nonce, limit)
    }
    fn sponsored_by(
        &self,
        recovery: SignedRecovery,
        id: [u8; 32],
        key: &Key,
        generation: u64,
        nonce: u64,
        limit: u64,
    ) -> SponsoredRecovery {
        let profile = &self.config.recovery.as_ref().unwrap().profile;
        let sponsor = SponsorAuthorization {
            domain: recovery.operation.domain.clone(),
            recovery_version: recovery_wire::VERSION,
            operation_id: sponsor::operation_id(&recovery.operation).unwrap(),
            signer_manifest_digest: sponsor::signer_manifest_digest(&recovery).unwrap(),
            sponsor_account_id: id,
            sponsor_generation: generation,
            sponsor_nonce: nonce,
            sponsor_key: key.identity.clone(),
            fee_profile_version: profile.version,
            fee_profile_digest: sponsor::profile_digest(profile).unwrap(),
            denomination: profile.denomination.clone(),
            maximum_charge: profile.max_fee_cap,
            gas_limit: limit,
            expiry_height: 40,
        };
        let signature = ActivePQC::sign(
            &key.secret,
            &sponsor::sponsor_signing_bytes(&sponsor).unwrap(),
        );
        SponsoredRecovery {
            recovery,
            sponsor,
            signature,
        }
    }
}
fn wire(envelope: &SponsoredRecovery) -> Vec<u8> {
    serde_json::to_vec(&WireTransaction::Recovery {
        envelope_base64: B64.encode(sponsor::encode(envelope).unwrap()),
    })
    .unwrap()
}
fn block(height: u64, txs: Vec<Vec<u8>>) -> FinalizedBlockInput {
    FinalizedBlockInput {
        height,
        time_seconds: height as i64 * 10,
        time_nanos: 0,
        hash: format!("{height:064x}"),
        txs,
        misbehavior: Vec::new(),
    }
}
fn data(app: &ConsensusApplication) -> BTreeMap<Vec<u8>, Vec<u8>> {
    app.storage
        .db
        .iterator(IteratorMode::Start)
        .map(|e| {
            let (k, v) = e.unwrap();
            (k.to_vec(), v.to_vec())
        })
        .collect()
}
fn book(app: &ConsensusApplication) -> RecoveryBook {
    RecoveryBook::decode(&app.storage.db.get(STATE_KEY).unwrap().unwrap()).unwrap()
}
fn balance(app: &ConsensusApplication, owner: &str) -> u128 {
    let balances: BTreeMap<String, u128> = bincode::deserialize(
        &app.storage
            .db
            .get(format!("acct:balances:{owner}"))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    balances["udrt"]
}
fn ordinary_nonce(app: &ConsensusApplication, owner: &str) -> u64 {
    app.storage
        .db
        .get(format!("acct:nonce:{owner}"))
        .unwrap()
        .map(|bytes| bincode::deserialize(&bytes).unwrap())
        .unwrap_or(0)
}
fn commit(app: &mut ConsensusApplication, height: u64, txs: Vec<Vec<u8>>) -> FinalizeResult {
    let result = app.finalize_block(block(height, txs)).unwrap();
    app.commit().unwrap();
    result
}
fn write_sync(storage: &Storage, batch: WriteBatch) -> anyhow::Result<()> {
    let mut options = WriteOptions::default();
    options.set_sync(true);
    storage.db.write_opt(batch, &options)?;
    Ok(())
}

#[test]
fn signed_recovery_commits_authority_fee_and_receipts_only_at_commit() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let before = data(&app);
    let payer_ordinary_nonce = ordinary_nonce(&app, &fixture.payer.address());
    assert_eq!(payer_ordinary_nonce, 0);
    let result = app
        .finalize_block(block(
            1,
            vec![wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT))],
        ))
        .unwrap();
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    assert_eq!(
        data(&app),
        before,
        "FinalizeBlock must not publish any staged recovery or fee record"
    );
    app.commit().unwrap();
    let actual = result.tx_results[0].gas_used as u128 * 2;
    assert!(actual > 0 && actual < 2_000_000);
    assert_eq!(
        balance(&app, &fixture.payer.address()),
        INITIAL_DRT - actual
    );
    assert_eq!(
        crate::supply::inspect_native(&app.storage)
            .unwrap()
            .drt
            .withheld_fees,
        actual
    );
    let stored = book(&app);
    assert_eq!(stored.accounts[&hex::encode(SPONSOR)].sponsor_nonce, 1);
    assert_eq!(
        stored.accounts[&hex::encode(SPONSOR)]
            .recovery
            .spending_nonce,
        0
    );
    assert_eq!(
        stored.accounts[&hex::encode(TARGET)]
            .recovery
            .policy_version,
        1
    );
    assert_eq!(stored.sponsor_receipts.len(), 1);
    assert_eq!(stored.operation_success.len(), 1);
    let receipt = stored.sponsor_receipts.values().next().unwrap();
    assert_eq!(receipt.block_height, 1);
    assert_eq!(receipt.block_index, 0);
    assert_eq!(receipt.settled_fee, actual);
    assert_eq!(receipt.reserved_cap, 2_000_000);
    assert_eq!(
        receipt.settled_fee + receipt.released_reserve,
        receipt.reserved_cap
    );
    assert_eq!(receipt.sponsor_counter_before, 0);
    assert_eq!(receipt.sponsor_counter_after, 1);
    assert_eq!(balance(&app, &fixture.active.address()), INITIAL_DRT);
    assert_eq!(
        ordinary_nonce(&app, &fixture.payer.address()),
        payer_ordinary_nonce
    );
    let result = commit(
        &mut app,
        2,
        vec![wire(&fixture.sponsored(fixture.start(), 1, GAS_LIMIT))],
    );
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    assert_eq!(
        book(&app).accounts[&hex::encode(TARGET)].recovery.status,
        RecoveryStatus::PendingRecovery
    );
    commit(&mut app, 3, vec![]);
    let result = commit(
        &mut app,
        4,
        vec![wire(&fixture.sponsored(fixture.finalize(), 2, GAS_LIMIT))],
    );
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    let stored = book(&app);
    let target = &stored.accounts[&hex::encode(TARGET)].recovery;
    assert_eq!(target.status, RecoveryStatus::Normal);
    assert_eq!(target.active_key, fixture.replacement.identity);
    assert_eq!(target.active_generation, 3);
    assert_eq!(target.spending_nonce, 1);
    assert_eq!(stored.accounts[&hex::encode(SPONSOR)].sponsor_nonce, 3);
    assert_eq!(stored.sponsor_receipts.len(), 3);
    assert!(stored.expiry_index.is_empty());
}

#[test]
fn recovery_commit_retry_and_lost_acknowledgement_do_not_bill_twice() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = fixture.initialized(&path);
    let input = block(
        1,
        vec![wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT))],
    );
    let before = data(&app);
    let result = app.finalize_block(input.clone()).unwrap();
    assert_eq!(result.tx_results[0].code, 0);
    assert!(app
        .commit_with(|_, _| anyhow::bail!("injected prewrite error"))
        .is_err());
    assert_eq!(data(&app), before);
    assert_eq!(app.finalize_block(input.clone()).unwrap(), result);
    assert!(app
        .commit_with(|storage, batch| {
            write_sync(storage, batch)?;
            anyhow::bail!("injected lost acknowledgement")
        })
        .is_err());
    let durable = data(&app);
    drop(app);
    let mut reopened = fixture.open(&path);
    assert_eq!(reopened.info().unwrap().height, 1);
    assert_eq!(reopened.finalize_block(input).unwrap(), result);
    reopened.commit().unwrap();
    assert_eq!(data(&reopened), durable);
    assert_eq!(
        book(&reopened).accounts[&hex::encode(SPONSOR)].sponsor_nonce,
        1
    );
    assert_eq!(book(&reopened).sponsor_receipts.len(), 1);
}

#[test]
fn restart_before_commit_discards_recovery_fee_and_recomputes_same_result() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = fixture.initialized(&path);
    let before = data(&app);
    let input = block(
        1,
        vec![wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT))],
    );
    let result = app.finalize_block(input.clone()).unwrap();
    drop(app);
    let mut reopened = fixture.open(&path);
    assert_eq!(data(&reopened), before);
    assert_eq!(reopened.info().unwrap().height, 0);
    assert_eq!(reopened.finalize_block(input).unwrap(), result);
    reopened.commit().unwrap();
    assert_eq!(book(&reopened).sponsor_receipts.len(), 1);
}

#[test]
fn recovery_replay_and_expired_sponsorship_have_no_fee_or_nonce_use() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let enroll = fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT);
    commit(&mut app, 1, vec![wire(&enroll)]);
    let before_book = book(&app);
    let before_balance = balance(&app, &fixture.payer.address());
    let fresh = fixture.sponsored(fixture.enroll(), 1, GAS_LIMIT);
    let mut expired = fixture.sponsored(fixture.start(), 1, GAS_LIMIT);
    expired.sponsor.expiry_height = 2;
    expired.signature = ActivePQC::sign(
        &fixture.payer.secret,
        &sponsor::sponsor_signing_bytes(&expired.sponsor).unwrap(),
    );
    let result = commit(
        &mut app,
        2,
        vec![wire(&enroll), wire(&fresh), wire(&expired)],
    );
    assert!(result
        .tx_results
        .iter()
        .all(|r| r.code != 0 && r.gas_used > 0));
    let after = book(&app);
    assert_eq!(after.accounts[&hex::encode(SPONSOR)].sponsor_nonce, 1);
    assert_eq!(after.sponsor_receipts, before_book.sponsor_receipts);
    assert_eq!(after.operation_success, before_book.operation_success);
    assert_eq!(balance(&app, &fixture.payer.address()), before_balance);
}

#[test]
fn accepted_recovery_out_of_gas_charges_limit_and_leaves_target_unchanged() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let result = commit(
        &mut app,
        1,
        vec![wire(&fixture.sponsored(fixture.enroll(), 0, 100))],
    );
    assert_eq!(result.tx_results[0].code, 3, "{:?}", result.tx_results);
    assert_eq!(result.tx_results[0].gas_used, 100);
    let stored = book(&app);
    let target = &stored.accounts[&hex::encode(TARGET)].recovery;
    assert_eq!(target.policy_version, 0);
    assert!(target.policy.is_none());
    assert_eq!(target.active_generation, 0);
    assert_eq!(target.spending_nonce, 0);
    assert_eq!(stored.accounts[&hex::encode(SPONSOR)].sponsor_nonce, 1);
    assert_eq!(stored.sponsor_receipts.len(), 1);
    assert!(stored.operation_success.is_empty());
    assert_eq!(balance(&app, &fixture.payer.address()), INITIAL_DRT - 200);
    assert_eq!(
        crate::supply::inspect_native(&app.storage)
            .unwrap()
            .drt
            .withheld_fees,
        200
    );
    let retry = commit(
        &mut app,
        2,
        vec![wire(&fixture.sponsored(fixture.enroll(), 1, GAS_LIMIT))],
    );
    assert_eq!(retry.tx_results[0].code, 0);
    assert_eq!(book(&app).operation_success.len(), 1);
}

#[test]
fn block_start_expiry_survives_rejected_finalize_without_a_sponsor_charge() {
    let mut costs = profile();
    costs.max_block_recovery_signatures = 5;
    let fixture = Fixture::with_profile(costs);
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    commit(
        &mut app,
        1,
        vec![wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT))],
    );
    commit(
        &mut app,
        2,
        vec![wire(&fixture.sponsored(fixture.start(), 1, GAS_LIMIT))],
    );
    for height in 3..6 {
        commit(&mut app, height, vec![]);
    }
    let before = data(&app);
    let payer_balance = balance(&app, &fixture.payer.address());
    let result = app
        .finalize_block(block(
            6,
            vec![wire(&fixture.sponsored(fixture.finalize(), 2, GAS_LIMIT))],
        ))
        .unwrap();
    assert_ne!(result.tx_results[0].code, 0);
    assert!(result.tx_results[0].gas_used > 0);
    assert_eq!(data(&app), before);
    app.commit().unwrap();
    let stored = book(&app);
    assert_eq!(
        stored.accounts[&hex::encode(TARGET)].recovery.status,
        RecoveryStatus::RecoveryLocked
    );
    assert!(stored.expiry_index.is_empty());
    assert_eq!(stored.accounts[&hex::encode(SPONSOR)].sponsor_nonce, 2);
    assert_eq!(balance(&app, &fixture.payer.address()), payer_balance);
}

#[test]
fn independent_recovery_databases_follow_identical_canonical_block_order() {
    let fixture = Fixture::new();
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let mut a = fixture.initialized(&a_dir.path().join("db"));
    let mut b = fixture.initialized(&b_dir.path().join("db"));
    // Enrolling the target changes the generation needed when it sponsors the next operation.
    let payer_operation = fixture.operation(
        SPONSOR,
        ActionKind::Enroll {
            active: ActiveAuthorization {
                generation: 0,
                nonce: 0,
            },
            policy: fixture.policy(),
        },
        40,
    );
    let payer_signed = fixture.signed(
        payer_operation,
        &[&fixture.payer],
        &fixture.guardians.iter().collect::<Vec<_>>(),
    );
    let payer_enroll = fixture.sponsored_by(payer_signed, TARGET, &fixture.active, 1, 0, GAS_LIMIT);
    let input = vec![
        wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT)),
        wire(&payer_enroll),
    ];
    let first = commit(&mut a, 1, input.clone());
    let second = commit(&mut b, 1, input);
    assert_eq!(first, second);
    assert!(
        first.tx_results.iter().all(|r| r.code == 0),
        "{:?}",
        first.tx_results
    );
    assert_eq!(data(&a), data(&b));
    let stored = book(&a);
    assert_eq!(stored.accounts[&hex::encode(TARGET)].sponsor_nonce, 1);
    assert_eq!(stored.accounts[&hex::encode(SPONSOR)].sponsor_nonce, 1);
    assert_eq!(stored.sponsor_receipts.len(), 2);
}

#[test]
fn recovery_namespace_corruption_prevents_consensus_reopen() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = fixture.initialized(&path);
    commit(
        &mut app,
        1,
        vec![wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT))],
    );
    let valid_next = wire(&fixture.sponsored(fixture.start(), 1, GAS_LIMIT));
    assert_eq!(app.check_tx(&valid_next).code, 0);
    let mut stored = book(&app);
    stored
        .accounts
        .get_mut(&hex::encode(TARGET))
        .unwrap()
        .recovery
        .active_generation += 1;
    stored.validate().unwrap(); // Structurally valid state still requires the committed root.
    let mut batch = WriteBatch::default();
    batch.put(STATE_KEY, serde_json::to_vec(&stored).unwrap());
    write_sync(&app.storage, batch).unwrap();
    let corrupted = data(&app);
    assert_ne!(app.check_tx(&valid_next).code, 0);
    assert_eq!(data(&app), corrupted);
    drop(app);
    assert!(
        ConsensusApplication::open(&path, fixture.config.clone(), fixture.genesis.clone()).is_err()
    );
}

#[test]
fn recovery_signature_capacity_rejects_whole_block_without_partial_commit() {
    let mut costs = profile();
    costs.max_block_recovery_signatures = 5;
    let fixture = Fixture::with_profile(costs);
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let before = data(&app);
    let envelope = wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT));
    assert!(app
        .finalize_block(block(1, vec![envelope.clone(), envelope.clone()]))
        .is_err());
    assert_eq!(data(&app), before);
    assert_eq!(app.info().unwrap().height, 0);
    let result = commit(&mut app, 1, vec![envelope]);
    assert_eq!(result.tx_results[0].code, 0);
    assert_eq!(book(&app).sponsor_receipts.len(), 1);
}

#[test]
fn proposal_reserves_one_sponsorship_per_nonce_across_distinct_targets() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let first = wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT));
    let operation = fixture.operation(
        SECOND_TARGET,
        ActionKind::Enroll {
            active: ActiveAuthorization {
                generation: 0,
                nonce: 0,
            },
            policy: fixture.policy(),
        },
        40,
    );
    let recovery = fixture.signed(
        operation,
        &[&fixture.secondary],
        &fixture.guardians.iter().collect::<Vec<_>>(),
    );
    let second = wire(&fixture.sponsored(recovery, 0, GAS_LIMIT));
    assert_eq!(app.check_tx(&first).code, 0);
    assert_eq!(app.check_tx(&second).code, 0);
    let before = data(&app);
    let selected = app
        .prepare_proposal(
            1,
            10,
            0,
            vec![first.clone(), second, first.clone()],
            1_048_576,
        )
        .unwrap();
    assert_eq!(selected, vec![first]);
    assert_eq!(data(&app), before);
    let result = commit(&mut app, 1, selected);
    assert_eq!(result.tx_results[0].code, 0);
    let stored = book(&app);
    assert_eq!(stored.accounts[&hex::encode(SPONSOR)].sponsor_nonce, 1);
    assert_eq!(stored.sponsor_receipts.len(), 1);
    assert!(stored.accounts[&hex::encode(SECOND_TARGET)]
        .recovery
        .policy
        .is_none());
}

#[test]
fn check_tx_rejects_committed_sponsorship_before_and_after_restart() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = fixture.initialized(&path);
    let raw = wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT));
    let before = data(&app);
    let admitted = app.check_tx(&raw);
    assert_eq!(admitted.code, 0);
    assert!(admitted.gas_used > 0);
    assert_eq!(app.check_tx(&raw), admitted);
    assert_eq!(data(&app), before);
    commit(&mut app, 1, vec![raw.clone()]);
    let durable = data(&app);
    let rejected = app.check_tx(&raw);
    assert_ne!(rejected.code, 0);
    assert!(rejected.gas_used > 0);
    assert_eq!(data(&app), durable);
    drop(app);
    let reopened = fixture.open(&path);
    assert_eq!(reopened.check_tx(&raw), rejected);
    assert_eq!(data(&reopened), durable);
    assert_eq!(
        book(&reopened).accounts[&hex::encode(SPONSOR)].sponsor_nonce,
        1
    );
}

#[test]
fn recovery_profile_rejects_legacy_ordinary_signing_without_fee_or_nonce_changes() {
    use crate::types::{tx::Tx, Msg, SignedTx};
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let envelope = SignedTx::sign(
        Tx {
            chain_id: CHAIN.into(),
            nonce: 0,
            msgs: vec![Msg::Send {
                from: fixture.active.address(),
                to: fixture.payer.address(),
                denom: "udrt".into(),
                amount: 10,
            }],
            fee: 50_000,
            memo: "legacy envelope boundary".into(),
        },
        &fixture.active.secret,
        &fixture.active.identity.public_key,
    )
    .unwrap();
    let raw = serde_json::to_vec(&WireTransaction::Signed { envelope }).unwrap();
    let before = data(&app);
    assert_ne!(app.check_tx(&raw).code, 0);
    assert!(app
        .prepare_proposal(1, 10, 0, vec![raw.clone()], 1_048_576)
        .unwrap()
        .is_empty());
    assert_eq!(data(&app), before);
    let result = commit(&mut app, 1, vec![raw]);
    assert_ne!(result.tx_results[0].code, 0);
    assert_eq!(balance(&app, &fixture.active.address()), INITIAL_DRT);
    assert_eq!(balance(&app, &fixture.payer.address()), INITIAL_DRT);
    assert_eq!(ordinary_nonce(&app, &fixture.active.address()), 0);
    assert_eq!(
        crate::supply::inspect_native(&app.storage)
            .unwrap()
            .drt
            .withheld_fees,
        0
    );
    let stored = book(&app);
    assert_eq!(
        stored.accounts[&hex::encode(TARGET)]
            .recovery
            .spending_nonce,
        0
    );
    assert!(stored.sponsor_receipts.is_empty());
    assert!(stored.operation_success.is_empty());
}

#[test]
fn ordinary_v2_paid_consensus_remains_disabled_after_signing_implementation() {
    use dytallix_protocol_types::ordinary::{
        self, Action, Denomination, OrdinaryTransaction, SignedOrdinary,
    };
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let current = book(&app);
    let target = &current.accounts[&hex::encode(TARGET)].recovery;
    let vectors: serde_json::Value = serde_json::from_str(include_str!(
        "../../../crates/protocol-types/tests/fixtures/ordinary_v2_vectors.json"
    ))
    .unwrap();
    let limits = serde_json::from_value(vectors["limits"].clone()).unwrap();
    let body = OrdinaryTransaction {
        domain: target.domain.clone(),
        authorization_generation: target.active_generation,
        spending_nonce: target.spending_nonce,
        key: fixture.active.identity.clone(),
        expiry_height: 20,
        ordinary_fee_contract_version: 1,
        fee_profile_version: 1,
        fee_profile_digest: [9; 32],
        fee_denomination: Denomination::Udrt,
        maximum_fee: 50_000,
        gas_limit: GAS_LIMIT,
        memo: "synthetic ordinary boundary".into(),
        actions: vec![Action::Send {
            recipient: SPONSOR,
            denomination: Denomination::Udrt,
            amount: 10,
        }],
    };
    let signature = ActivePQC::sign(
        &fixture.active.secret,
        &ordinary::signing_bytes(&body, &limits).unwrap(),
    );
    let signed = SignedOrdinary { body, signature };
    dytallix_runtime_crypto::ordinary::verify_signed(&signed, &limits).unwrap();
    let raw = crate::ordinary_transport::encode_transport(&signed, &limits, 100_000).unwrap();
    assert_eq!(
        crate::ordinary_transport::decode_transport(&raw, &limits, 100_000).unwrap(),
        signed
    );
    let before = data(&app);
    assert_ne!(app.check_tx(&raw).code, 0);
    assert!(app
        .prepare_proposal(1, 10, 0, vec![raw.clone()], 1_048_576)
        .unwrap()
        .is_empty());
    assert_eq!(data(&app), before);
    let result = commit(&mut app, 1, vec![raw]);
    assert_ne!(result.tx_results[0].code, 0);
    assert_eq!(balance(&app, &fixture.active.address()), INITIAL_DRT);
    assert_eq!(balance(&app, &fixture.payer.address()), INITIAL_DRT);
    assert_eq!(ordinary_nonce(&app, &fixture.active.address()), 0);
    assert_eq!(
        crate::supply::inspect_native(&app.storage)
            .unwrap()
            .drt
            .withheld_fees,
        0
    );
    let stored = book(&app);
    assert_eq!(
        stored.accounts[&hex::encode(TARGET)]
            .recovery
            .spending_nonce,
        0
    );
    assert!(stored.sponsor_receipts.is_empty());
    assert!(stored.operation_success.is_empty());
}

#[test]
fn future_recovery_activation_keeps_empty_blocks_available_before_fee_execution() {
    let mut costs = profile();
    costs.activation_height = 3;
    let fixture = Fixture::with_profile(costs);
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let raw = wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT));
    let before = data(&app);
    assert_ne!(app.check_tx(&raw).code, 0);
    assert_eq!(data(&app), before);
    for height in 1..=2 {
        let result = commit(&mut app, height, vec![]);
        assert!(result.tx_results.is_empty());
        let stored = book(&app);
        assert_eq!(stored.last_height, height);
        assert!(stored.sponsor_receipts.is_empty());
        assert_eq!(stored.accounts[&hex::encode(SPONSOR)].sponsor_nonce, 0);
        assert_eq!(balance(&app, &fixture.payer.address()), INITIAL_DRT);
        assert_eq!(
            crate::supply::inspect_native(&app.storage)
                .unwrap()
                .drt
                .withheld_fees,
            0
        );
    }
    let before = data(&app);
    let admitted = app.check_tx(&raw);
    assert_eq!(admitted.code, 0);
    assert!(admitted.gas_used > 0);
    assert_eq!(data(&app), before);
    let result = commit(&mut app, 3, vec![raw]);
    assert_eq!(result.tx_results[0].code, 0);
    assert_eq!(result.tx_results[0].gas_used, admitted.gas_used);
    let stored = book(&app);
    assert_eq!(stored.accounts[&hex::encode(SPONSOR)].sponsor_nonce, 1);
    assert_eq!(stored.sponsor_receipts.len(), 1);
    assert!(balance(&app, &fixture.payer.address()) < INITIAL_DRT);
}

#[test]
fn discarded_invalid_proposal_candidates_still_consume_signature_capacity() {
    let mut costs = profile();
    costs.max_block_recovery_signatures = 5;
    let fixture = Fixture::with_profile(costs);
    let dir = tempfile::tempdir().unwrap();
    let app = fixture.initialized(&dir.path().join("db"));
    let mut first = fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT);
    first.signature[0] ^= 1;
    let operation = fixture.operation(
        SECOND_TARGET,
        ActionKind::Enroll {
            active: ActiveAuthorization {
                generation: 0,
                nonce: 0,
            },
            policy: fixture.policy(),
        },
        40,
    );
    let recovery = fixture.signed(
        operation,
        &[&fixture.secondary],
        &fixture.guardians.iter().collect::<Vec<_>>(),
    );
    let mut second = fixture.sponsored(recovery, 0, GAS_LIMIT);
    second.signature[0] ^= 1;
    assert_ne!(first.sponsor.operation_id, second.sponsor.operation_id);
    let first = wire(&first);
    let second = wire(&second);
    let before = data(&app);
    // One candidate fits the work budget, fails authentication, and is omitted.
    for raw in [&first, &second] {
        assert!(app
            .prepare_proposal(1, 10, 0, vec![raw.clone()], 1_048_576)
            .unwrap()
            .is_empty());
        assert_eq!(data(&app), before);
    }
    // The omitted candidate must not reset the shared verification budget.
    assert!(app
        .prepare_proposal(1, 10, 0, vec![first, second], 1_048_576)
        .is_err());
    assert_eq!(data(&app), before);
    assert_eq!(book(&app).accounts[&hex::encode(SPONSOR)].sponsor_nonce, 0);
    assert!(book(&app).sponsor_receipts.is_empty());
    assert_eq!(balance(&app, &fixture.payer.address()), INITIAL_DRT);
}

fn resign_sponsor(value: &mut SponsoredRecovery, key: &Key) {
    value.signature = ActivePQC::sign(
        &key.secret,
        &sponsor::sponsor_signing_bytes(&value.sponsor).unwrap(),
    );
}
fn assert_rejection_without_charge(
    app: &mut ConsensusApplication,
    height: u64,
    envelope: SponsoredRecovery,
) {
    let before = book(app);
    let expected = before.begin_block(height).unwrap().book;
    let balances: Vec<_> = before
        .accounts
        .values()
        .map(|a| {
            (
                a.address.clone(),
                balance(app, &a.address),
                ordinary_nonce(app, &a.address),
            )
        })
        .collect();
    let fees = crate::supply::inspect_native(&app.storage)
        .unwrap()
        .drt
        .withheld_fees;
    let durable = data(app);
    let result = app
        .finalize_block(block(height, vec![wire(&envelope)]))
        .unwrap();
    assert_eq!(result.tx_results[0].code, 1, "{:?}", result.tx_results);
    assert_eq!(data(app), durable);
    app.commit().unwrap();
    assert_eq!(
        book(app),
        expected,
        "Rejected request must preserve all authority, sponsor counters and receipt history"
    );
    for (address, liquid, nonce) in balances {
        assert_eq!(balance(app, &address), liquid);
        assert_eq!(ordinary_nonce(app, &address), nonce);
    }
    assert_eq!(
        crate::supply::inspect_native(&app.storage)
            .unwrap()
            .drt
            .withheld_fees,
        fees
    );
}

#[test]
fn protected_and_target_equal_sponsors_reject_without_fee_or_authority_changes() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let equal = fixture.sponsored_by(fixture.enroll(), TARGET, &fixture.active, 0, 0, GAS_LIMIT);
    assert_rejection_without_charge(&mut app, 1, equal);
    let operation = fixture.operation(
        SPONSOR,
        ActionKind::Enroll {
            active: ActiveAuthorization {
                generation: 0,
                nonce: 0,
            },
            policy: fixture.policy(),
        },
        40,
    );
    let recovery = fixture.signed(
        operation,
        &[&fixture.payer],
        &fixture.guardians.iter().collect::<Vec<_>>(),
    );
    let enroll = fixture.sponsored_by(recovery, TARGET, &fixture.active, 0, 0, GAS_LIMIT);
    assert_eq!(
        commit(&mut app, 2, vec![wire(&enroll)]).tx_results[0].code,
        0
    );
    let operation = fixture.operation(
        SPONSOR,
        ActionKind::Start {
            recovery: RecoveryAuthorization {
                policy_version: 1,
                sequence: 0,
            },
            request_id: REQUEST,
            replacement: fixture.replacement.identity.clone(),
            timing_version: 1,
        },
        40,
    );
    let recovery = fixture.signed(
        operation,
        &[&fixture.guardians[0], &fixture.guardians[1]],
        &[&fixture.replacement],
    );
    let start = fixture.sponsored_by(recovery, TARGET, &fixture.active, 0, 1, GAS_LIMIT);
    assert_eq!(
        commit(&mut app, 3, vec![wire(&start)]).tx_results[0].code,
        0
    );
    assert_eq!(
        book(&app).accounts[&hex::encode(SPONSOR)].recovery.status,
        RecoveryStatus::PendingRecovery
    );
    let protected =
        fixture.sponsored_by(fixture.enroll(), SPONSOR, &fixture.payer, 2, 0, GAS_LIMIT);
    assert_rejection_without_charge(&mut app, 4, protected);
    // Cancellation preserves protection. The sponsor remains unable to pay.
    let operation = fixture.operation(
        SPONSOR,
        ActionKind::Cancel {
            recovery: RecoveryAuthorization {
                policy_version: 1,
                sequence: 1,
            },
            request_id: REQUEST,
        },
        40,
    );
    let recovery = fixture.signed(
        operation,
        &[&fixture.guardians[0], &fixture.guardians[1]],
        &[],
    );
    let cancel = fixture.sponsored_by(recovery, TARGET, &fixture.active, 0, 2, GAS_LIMIT);
    assert_eq!(
        commit(&mut app, 5, vec![wire(&cancel)]).tx_results[0].code,
        0
    );
    assert_eq!(
        book(&app).accounts[&hex::encode(SPONSOR)].recovery.status,
        RecoveryStatus::RecoveryLocked
    );
    let locked = fixture.sponsored_by(fixture.enroll(), SPONSOR, &fixture.payer, 3, 0, GAS_LIMIT);
    assert_rejection_without_charge(&mut app, 6, locked);
}

#[test]
fn signed_recovery_rejection_matrix_preserves_balances_counters_and_target_state() {
    let mut costs = profile();
    costs.max_fee_cap = INITIAL_DRT * 2;
    let fixture = Fixture::with_profile(costs);
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let mut valid = fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT);
    valid.sponsor.maximum_charge = 2_000_000;
    resign_sponsor(&mut valid, &fixture.payer);
    assert_eq!(app.check_tx(&wire(&valid)).code, 0);
    for case in 0..11 {
        let mut candidate = valid.clone();
        match case {
            0 => candidate.recovery.signatures[0].signature[0] ^= 1,
            1 => candidate.signature[0] ^= 1,
            2 => {
                candidate.sponsor.sponsor_generation = 1;
                resign_sponsor(&mut candidate, &fixture.payer);
            }
            3 => {
                candidate.sponsor.sponsor_nonce = 1;
                resign_sponsor(&mut candidate, &fixture.payer);
            }
            4 => {
                candidate.sponsor.expiry_height = 5;
                resign_sponsor(&mut candidate, &fixture.payer);
            }
            5 => {
                candidate.sponsor.fee_profile_version = 2;
                resign_sponsor(&mut candidate, &fixture.payer);
            }
            6 => {
                candidate.sponsor.fee_profile_digest[0] ^= 1;
                resign_sponsor(&mut candidate, &fixture.payer);
            }
            7 => {
                candidate.sponsor.gas_limit = 5;
                resign_sponsor(&mut candidate, &fixture.payer);
            }
            8 => {
                candidate.sponsor.maximum_charge = 1;
                resign_sponsor(&mut candidate, &fixture.payer);
            }
            9 => {
                candidate.sponsor.maximum_charge = INITIAL_DRT + 1;
                resign_sponsor(&mut candidate, &fixture.payer);
            }
            10 => {
                let operation = fixture.operation(
                    TARGET,
                    ActionKind::Enroll {
                        active: ActiveAuthorization {
                            generation: 1,
                            nonce: 0,
                        },
                        policy: fixture.policy(),
                    },
                    40,
                );
                let recovery = fixture.signed(
                    operation,
                    &[&fixture.active],
                    &fixture.guardians.iter().collect::<Vec<_>>(),
                );
                candidate = fixture.sponsored(recovery, 0, GAS_LIMIT);
                candidate.sponsor.maximum_charge = 2_000_000;
                resign_sponsor(&mut candidate, &fixture.payer);
            }
            _ => unreachable!(),
        }
        assert_rejection_without_charge(&mut app, case + 1, candidate);
    }
    let result = commit(&mut app, 12, vec![wire(&valid)]);
    assert_eq!(
        result.tx_results[0].code, 0,
        "Valid request remains executable after every rejected case"
    );
    assert_eq!(book(&app).accounts[&hex::encode(SPONSOR)].sponsor_nonce, 1);
}

fn commit_action_restart_and_retry(
    fixture: &Fixture,
    path: &std::path::Path,
    mut app: ConsensusApplication,
    height: u64,
    envelope: SponsoredRecovery,
) -> ConsensusApplication {
    let before = book(&app);
    let payer_before = balance(&app, &fixture.payer.address());
    let fees_before = crate::supply::inspect_native(&app.storage)
        .unwrap()
        .drt
        .withheld_fees;
    let input = block(height, vec![wire(&envelope)]);
    let durable = data(&app);
    let result = app.finalize_block(input.clone()).unwrap();
    assert_eq!(
        result.tx_results[0].code, 0,
        "{:?}: {:?}",
        envelope.recovery.operation.action.kind, result.tx_results
    );
    assert_eq!(data(&app), durable);
    app.commit().unwrap();
    let stored = book(&app);
    let id = hex::encode(sponsor::authorization_id(&envelope.sponsor).unwrap());
    let receipt = &stored.sponsor_receipts[&id];
    assert!(receipt.success && receipt.settled_fee > 0);
    assert_eq!(
        receipt.settled_fee,
        result.tx_results[0].gas_used as u128 * 2
    );
    assert_eq!(
        payer_before - balance(&app, &fixture.payer.address()),
        receipt.settled_fee
    );
    assert_eq!(
        crate::supply::inspect_native(&app.storage)
            .unwrap()
            .drt
            .withheld_fees
            - fees_before,
        receipt.settled_fee
    );
    assert_eq!(
        receipt.settled_fee + receipt.released_reserve,
        receipt.reserved_cap
    );
    assert_eq!(
        stored.sponsor_receipts.len(),
        before.sponsor_receipts.len() + 1
    );
    assert_eq!(
        stored.accounts[&hex::encode(SPONSOR)].sponsor_nonce,
        before.accounts[&hex::encode(SPONSOR)].sponsor_nonce + 1
    );
    let committed = data(&app);
    drop(app);
    let mut reopened = fixture.open(path);
    assert_eq!(data(&reopened), committed);
    assert_eq!(reopened.finalize_block(input).unwrap(), result);
    reopened.commit().unwrap();
    assert_eq!(
        data(&reopened),
        committed,
        "Restarted exact retry must retain the original fee and receipt"
    );
    reopened
}

#[test]
fn all_nine_recovery_actions_commit_real_signatures_fees_and_restart_retries() {
    let mut fixture = Fixture::new();
    fixture.config.max_tx_bytes = 131_072;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = fixture.initialized(&path);
    let mut seen = std::collections::BTreeSet::new();
    for (height, tag) in [
        (1, 1),
        (2, 2),
        (3, 3),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 9),
        (8, 7),
        (10, 8),
        (11, 3),
        (13, 4),
    ] {
        while app.info().unwrap().height + 1 < height {
            let next = app.info().unwrap().height + 1;
            commit(&mut app, next, vec![]);
        }
        let stored = book(&app);
        let target = &stored.accounts[&hex::encode(TARGET)].recovery;
        let active = ActiveAuthorization {
            generation: target.active_generation,
            nonce: target.spending_nonce,
        };
        let recovery = RecoveryAuthorization {
            policy_version: target.policy_version,
            sequence: target.recovery_sequence,
        };
        let authorization = PolicyAuthorization {
            policy_version: target.policy_version,
            sequence: target.policy_change_sequence,
        };
        let active_key = if target.active_key == fixture.active.identity {
            &fixture.active
        } else {
            &fixture.replacement
        };
        let guardian_signers = vec![&fixture.guardians[0], &fixture.guardians[1]];
        let (kind, signers, proofs): (ActionKind, Vec<&Key>, Vec<&Key>) = match tag {
            1 => (
                ActionKind::Enroll {
                    active,
                    policy: fixture.policy(),
                },
                vec![active_key],
                fixture.guardians.iter().collect(),
            ),
            2 => (
                ActionKind::Rotate {
                    active,
                    replacement: fixture.replacement.identity.clone(),
                },
                vec![active_key],
                vec![&fixture.replacement],
            ),
            3 => (
                ActionKind::Start {
                    recovery,
                    request_id: REQUEST,
                    replacement: fixture.active.identity.clone(),
                    timing_version: 1,
                },
                guardian_signers,
                vec![&fixture.active],
            ),
            4 => (
                ActionKind::Finalize {
                    recovery,
                    request_id: REQUEST,
                },
                vec![&fixture.active],
                vec![],
            ),
            5 => (
                ActionKind::Cancel {
                    recovery,
                    request_id: REQUEST,
                },
                guardian_signers,
                vec![],
            ),
            6 => (
                ActionKind::Resume {
                    recovery,
                    active_key: target.active_key.clone(),
                },
                guardian_signers,
                vec![],
            ),
            7 => {
                let mut signers = guardian_signers;
                signers.push(active_key);
                (
                    ActionKind::StagePolicy {
                        active,
                        authorization,
                        update_id: [height as u8; 32],
                        policy: fixture.policy(),
                        timing_version: 1,
                    },
                    signers,
                    fixture.guardians.iter().collect(),
                )
            }
            8 => {
                let mut signers = guardian_signers;
                signers.push(active_key);
                (
                    ActionKind::ActivatePolicy {
                        active,
                        authorization,
                        update_id: target.pending_policy.as_ref().unwrap().update_id,
                    },
                    signers,
                    vec![],
                )
            }
            9 => (
                ActionKind::CancelPolicy {
                    authorization,
                    update_id: target.pending_policy.as_ref().unwrap().update_id,
                },
                guardian_signers,
                vec![],
            ),
            _ => unreachable!(),
        };
        let operation = fixture.operation(TARGET, kind, 40);
        let signed = fixture.signed(operation, &signers, &proofs);
        let envelope = fixture.sponsored(
            signed,
            stored.accounts[&hex::encode(SPONSOR)].sponsor_nonce,
            GAS_LIMIT,
        );
        app = commit_action_restart_and_retry(&fixture, &path, app, height, envelope);
        seen.insert(tag);
    }
    assert_eq!(seen, (1..=9).collect());
    let stored = book(&app);
    let target = &stored.accounts[&hex::encode(TARGET)].recovery;
    assert_eq!(stored.sponsor_receipts.len(), 11);
    assert_eq!(target.status, RecoveryStatus::Normal);
    assert_eq!(target.active_key, fixture.active.identity);
    assert_eq!(target.policy_version, 2);
    assert!(stored.expiry_index.is_empty());
}

#[test]
fn signed_future_expiry_capacity_accepts_exact_boundary_and_releases_cancelled_slot() {
    let mut costs = profile();
    costs.max_due_expiry_events_per_height = 1;
    costs.mandatory_expiry_gas_budget = 10;
    let fixture = Fixture::with_profile(costs);
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let operation = fixture.operation(
        SECOND_TARGET,
        ActionKind::Enroll {
            active: ActiveAuthorization {
                generation: 0,
                nonce: 0,
            },
            policy: fixture.policy(),
        },
        40,
    );
    let second_enroll = fixture.signed(
        operation,
        &[&fixture.secondary],
        &fixture.guardians.iter().collect::<Vec<_>>(),
    );
    let result = commit(
        &mut app,
        1,
        vec![
            wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT)),
            wire(&fixture.sponsored(second_enroll, 1, GAS_LIMIT)),
        ],
    );
    assert!(result.tx_results.iter().all(|r| r.code == 0));
    let second_start = fixture.signed(
        fixture.operation(
            SECOND_TARGET,
            ActionKind::Start {
                recovery: RecoveryAuthorization {
                    policy_version: 1,
                    sequence: 0,
                },
                request_id: [20; 32],
                replacement: fixture.replacement.identity.clone(),
                timing_version: 1,
            },
            40,
        ),
        &[&fixture.guardians[0], &fixture.guardians[1]],
        &[&fixture.replacement],
    );
    let before_balance = balance(&app, &fixture.payer.address());
    let result = commit(
        &mut app,
        2,
        vec![
            wire(&fixture.sponsored(fixture.start(), 2, GAS_LIMIT)),
            wire(&fixture.sponsored(second_start.clone(), 3, GAS_LIMIT)),
        ],
    );
    assert_eq!(result.tx_results[0].code, 0);
    assert_eq!(result.tx_results[1].code, 1);
    assert_eq!(
        before_balance - balance(&app, &fixture.payer.address()),
        result.tx_results[0].gas_used as u128 * 2
    );
    let stored = book(&app);
    assert_eq!(stored.sponsor_receipts.len(), 3);
    assert_eq!(stored.accounts[&hex::encode(SPONSOR)].sponsor_nonce, 3);
    assert_eq!(stored.expiry_index[&6].len(), 1);
    assert!(stored.accounts[&hex::encode(SECOND_TARGET)]
        .recovery
        .pending_recovery
        .is_none());
    let cancel = fixture.signed(
        fixture.operation(
            TARGET,
            ActionKind::Cancel {
                recovery: RecoveryAuthorization {
                    policy_version: 1,
                    sequence: 1,
                },
                request_id: REQUEST,
            },
            40,
        ),
        &[&fixture.guardians[0], &fixture.guardians[1]],
        &[],
    );
    let result = commit(
        &mut app,
        3,
        vec![
            wire(&fixture.sponsored(cancel, 3, GAS_LIMIT)),
            wire(&fixture.sponsored(second_start, 4, GAS_LIMIT)),
        ],
    );
    assert!(result.tx_results.iter().all(|r| r.code == 0));
    let stored = book(&app);
    assert!(!stored.expiry_index.contains_key(&6));
    assert_eq!(stored.expiry_index[&7].len(), 1);
    let paid = balance(&app, &fixture.payer.address());
    for height in 4..=7 {
        commit(&mut app, height, vec![]);
    }
    assert_eq!(balance(&app, &fixture.payer.address()), paid);
    let stored = book(&app);
    assert!(stored.expiry_index.is_empty());
    assert_eq!(
        stored.accounts[&hex::encode(SECOND_TARGET)].recovery.status,
        RecoveryStatus::RecoveryLocked
    );
}

#[test]
fn legacy_consensus_state_never_converts_and_unsupported_recovery_book_never_rewrites() {
    let mut fixture = Fixture::new();
    let recovery = fixture.config.recovery.take().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy");
    let mut legacy = fixture.initialized(&path);
    commit(&mut legacy, 1, vec![]);
    let original = data(&legacy);
    assert!(legacy.storage.db.get(STATE_KEY).unwrap().is_none());
    drop(legacy);
    fixture.config.recovery = Some(recovery);
    assert!(
        ConsensusApplication::open(&path, fixture.config.clone(), fixture.genesis.clone()).is_err()
    );
    let raw = Storage::open(path.clone()).unwrap();
    let after: BTreeMap<Vec<u8>, Vec<u8>> = raw
        .db
        .iterator(IteratorMode::Start)
        .map(|entry| {
            let (k, v) = entry.unwrap();
            (k.to_vec(), v.to_vec())
        })
        .collect();
    assert_eq!(after, original);
    assert!(raw.db.get(STATE_KEY).unwrap().is_none());
    drop(raw);
    let fresh_path = dir.path().join("fresh");
    let fresh = fixture.initialized(&fresh_path);
    let mut stored = book(&fresh);
    assert_eq!(stored.version, 1);
    stored.version = 2;
    let bytes = serde_json::to_vec(&stored).unwrap();
    assert!(RecoveryBook::decode(&bytes).is_err());
    let mut batch = WriteBatch::default();
    batch.put(STATE_KEY, &bytes);
    write_sync(&fresh.storage, batch).unwrap();
    let unchanged = data(&fresh);
    drop(fresh);
    assert!(ConsensusApplication::open(
        &fresh_path,
        fixture.config.clone(),
        fixture.genesis.clone()
    )
    .is_err());
    let raw = Storage::open(fresh_path).unwrap();
    let after: BTreeMap<Vec<u8>, Vec<u8>> = raw
        .db
        .iterator(IteratorMode::Start)
        .map(|entry| {
            let (k, v) = entry.unwrap();
            (k.to_vec(), v.to_vec())
        })
        .collect();
    assert_eq!(after, unchanged);
    assert_eq!(raw.db.get(STATE_KEY).unwrap().unwrap(), bytes);
}

#[test]
fn aggregate_recovery_gas_bytes_and_signatures_bound_all_consensus_entry_points() {
    for bound in ["gas", "bytes", "signatures"] {
        let mut costs = profile();
        let limit = if bound == "gas" {
            costs.max_transaction_gas = 50;
            costs.max_block_gas = 50;
            costs.action_costs = [40; 9];
            costs.write_byte_cost = 0;
            50
        } else {
            GAS_LIMIT
        };
        if bound == "signatures" {
            costs.max_block_recovery_signatures = 5;
        }
        let mut fixture = Fixture::with_profile(costs);
        if bound == "bytes" {
            let length = sponsor::encode(&fixture.sponsored(fixture.enroll(), 0, limit))
                .unwrap()
                .len() as u64;
            fixture
                .config
                .recovery
                .as_mut()
                .unwrap()
                .profile
                .max_block_recovery_bytes = length;
        }
        let first = fixture.sponsored(fixture.enroll(), 0, limit);
        let operation = fixture.operation(
            SPONSOR,
            ActionKind::Enroll {
                active: ActiveAuthorization {
                    generation: 0,
                    nonce: 0,
                },
                policy: fixture.policy(),
            },
            40,
        );
        let recovery = fixture.signed(
            operation,
            &[&fixture.payer],
            &fixture.guardians.iter().collect::<Vec<_>>(),
        );
        let second = fixture.sponsored_by(recovery, SECOND_TARGET, &fixture.secondary, 0, 0, limit);
        assert_eq!(
            sponsor::encode(&first).unwrap().len(),
            sponsor::encode(&second).unwrap().len()
        );
        let first = wire(&first);
        let second = wire(&second);
        let dir = tempfile::tempdir().unwrap();
        let mut app = fixture.initialized(&dir.path().join("db"));
        let before = data(&app);
        for raw in [&first, &second] {
            assert_eq!(
                app.prepare_proposal(1, 10, 0, vec![raw.clone()], 1_048_576)
                    .unwrap(),
                vec![raw.clone()],
                "One request must fit {bound}"
            );
            assert!(app.process_proposal(block(1, vec![raw.clone()])).unwrap());
            assert_eq!(data(&app), before);
        }
        let both = vec![first.clone(), second];
        assert!(
            app.prepare_proposal(1, 10, 0, both.clone(), 1_048_576)
                .is_err(),
            "Proposal preparation must enforce aggregate {bound}"
        );
        assert!(
            !app.process_proposal(block(1, both.clone())).unwrap(),
            "Proposal validation must enforce aggregate {bound}"
        );
        assert!(
            app.finalize_block(block(1, both)).is_err(),
            "Block preparation must enforce aggregate {bound}"
        );
        assert_eq!(data(&app), before);
        assert_eq!(app.info().unwrap().height, 0);
        let result = commit(&mut app, 1, vec![first]);
        assert_eq!(result.tx_results[0].code, 0);
        let stored = book(&app);
        assert_eq!(stored.sponsor_receipts.len(), 1);
        assert_eq!(stored.accounts[&hex::encode(SPONSOR)].sponsor_nonce, 1);
    }
}

#[test]
fn signed_maximum_sponsor_nonce_request_rejects_without_blocking_valid_request() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let exhausted_request = fixture.sponsored(fixture.enroll(), u64::MAX, GAS_LIMIT);
    let valid = fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT);
    let before = balance(&app, &fixture.payer.address());
    let result = commit(&mut app, 1, vec![wire(&exhausted_request), wire(&valid)]);
    assert_eq!(result.tx_results[0].code, 1);
    assert!(result.tx_results[0].gas_used > 0);
    assert_eq!(result.tx_results[1].code, 0);
    let charged = result.tx_results[1].gas_used as u128 * 2;
    assert_eq!(before - balance(&app, &fixture.payer.address()), charged);
    assert_eq!(
        crate::supply::inspect_native(&app.storage)
            .unwrap()
            .drt
            .withheld_fees,
        charged
    );
    let stored = book(&app);
    assert_eq!(stored.accounts[&hex::encode(SPONSOR)].sponsor_nonce, 1);
    assert_eq!(stored.sponsor_receipts.len(), 1);
    assert_eq!(stored.operation_success.len(), 1);
    assert!(!stored.sponsor_receipts.contains_key(&hex::encode(
        sponsor::authorization_id(&exhausted_request.sponsor).unwrap()
    )));
}
