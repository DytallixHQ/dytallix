//! Combined ordinary/recovery consensus with real signatures and synthetic profiles.
//! These local checks do not qualify production prices, custody, or deployment.
use super::*;
use crate::crypto::{ActivePQC, PQC};
use crate::ordinary_authority::DiscretionaryGrant;
use crate::ordinary_state::OrdinaryConfig;
use crate::recovery_fees::{RecoveryAccount, RecoveryBook};
use crate::storage::state::Storage;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use dytallix_protocol_types::address::{AccountAddress, AddressNetwork};
use dytallix_protocol_types::ordinary::{
    self, Action as OrdinaryAction, Denomination, Limits, OrdinaryTransaction, SignedOrdinary,
};
use dytallix_protocol_types::ordinary_fees::{self, FeeProfile as OrdinaryFeeProfile};
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

const CHAIN: &str = "ordinary-consensus-fixture";
const INITIAL_DRT: u128 = 10_000_000;
const REQUEST: [u8; 32] = [13; 32];
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
    fn id(&self) -> [u8; 32] {
        *AccountAddress::decode(AddressNetwork::Development, &self.address())
            .unwrap()
            .account_id()
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
        max_block_recovery_signatures: 100,
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
            (active.id(), &active),
            (payer.id(), &payer),
            (secondary.id(), &secondary),
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
            "profile":"cometbft-lifecycle-local-qualification","engine":"cometbft-v0.40.0","chain_id":CHAIN,"app_state_sha256":hex::encode(digest),
            "gas_price":1,"max_tx_bytes":65536,"max_block_bytes":1048576,"max_txs":64,
            "validators":[{"pubkey_type":"ml_dsa_65","pubkey_base64":B64.encode(validator.into_bytes()),"power":100,"reward_address":"validator-one"}],
            "lifecycle":{"version":1,"profile":"cometbft-lifecycle-local-qualification","chain_id":CHAIN,
                "approved_operators":{"validator-one":active.address()},"min_self_bond":"10","max_active":4,
                "evidence_max_age_blocks":3,"evidence_max_age_seconds":3,
                "processing_margin_blocks":1,"processing_margin_seconds":1}
        })).unwrap();
        let mut book = RecoveryBook::new(profile, accounts).unwrap();
        book.origins = [&active, &payer, &secondary]
            .into_iter()
            .map(|k| (hex::encode(k.id()), k.identity.clone()))
            .collect();
        config.recovery = Some(book);
        let fee_profile = ordinary_profile(&config);
        config.ordinary = Some(OrdinaryConfig {
            version: 1,
            fee_profile,
            account_template: crate::ordinary_state::AccountTemplate {
                recovery: recovery_config.clone(),
            },
            initial_grants: BTreeMap::new(),
            max_state_bytes: 4_000_000,
            max_grants: 16,
            max_receipts: 128,
            max_retained_profiles: 4,
            max_transport_bytes: 65_536,
            queue_max_entries: 32,
            queue_max_wire_bytes: 1_000_000,
            queue_max_signature_work: 100,
        });
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
            self.active.id(),
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
            self.active.id(),
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
            self.active.id(),
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
        self.sponsored_by(recovery, self.payer.id(), &self.payer, 0, nonce, limit)
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
    RecoveryBook::load(&app.storage).unwrap().unwrap()
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
    balances.get("udrt").copied().unwrap_or(0)
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

fn ordinary_profile(config: &ConsensusConfig) -> OrdinaryFeeProfile {
    OrdinaryFeeProfile {
        ordinary_fee_contract_version: 1,
        version: 1,
        activation_height: 1,
        denomination: Denomination::Udrt,
        gas_price: 2,
        minimum_gas: 10,
        max_transaction_gas: 1000,
        max_block_transaction_gas: GAS_LIMIT * 4,
        max_block_transaction_bytes: 1_000_000,
        max_block_signature_checks: 100,
        max_fee_cap: 2000,
        limits: Limits {
            max_wire_bytes: 65_536,
            max_actions: 16,
            max_identifier_bytes: 128,
            max_data_bytes: 1024,
            max_memo_bytes: 1024,
            max_consensus_key_bytes: 4096,
            max_proof_bytes: 8192,
            max_expiry_lifetime: 100,
            allowed_algorithms: ["mldsa65".into()].into_iter().collect(),
        },
        transaction_overhead: 2,
        receipt_metadata_cost: 5,
        wire_byte_cost: 0,
        read_byte_cost: 0,
        write_byte_cost: 0,
        action_costs: [10; 12],
        signature_costs: BTreeMap::from([("mldsa65".into(), 3)]),
        validator_proof_profile_digest: crate::ordinary_state::validator_profile_digest(
            config.lifecycle.as_ref().unwrap(),
        )
        .unwrap(),
        validator_proof_costs: BTreeMap::from([("mldsa65".into(), 4)]),
        account_creation_fee_udrt: 1_000,
    }
}
impl Fixture {
    fn ordinary(
        &self,
        state: &RecoveryState,
        key: &Key,
        actions: Vec<OrdinaryAction>,
        gas_limit: u64,
    ) -> SignedOrdinary {
        let p = &self.config.ordinary.as_ref().unwrap().fee_profile;
        let body = OrdinaryTransaction {
            domain: state.domain.clone(),
            authorization_generation: state.active_generation,
            spending_nonce: state.spending_nonce,
            key: key.identity.clone(),
            expiry_height: 40,
            ordinary_fee_contract_version: 1,
            fee_profile_version: p.version,
            fee_profile_digest: ordinary_fees::profile_digest(p).unwrap(),
            fee_denomination: Denomination::Udrt,
            maximum_fee: p.max_fee_cap,
            gas_limit,
            memo: "synthetic consensus check".into(),
            actions,
        };
        self.resign(body, key)
    }
    fn resign(&self, body: OrdinaryTransaction, key: &Key) -> SignedOrdinary {
        let p = &self.config.ordinary.as_ref().unwrap().fee_profile;
        let signature = ActivePQC::sign(
            &key.secret,
            &ordinary::signing_bytes(&body, &p.limits).unwrap(),
        );
        SignedOrdinary { body, signature }
    }
    fn ordinary_wire(&self, signed: &SignedOrdinary) -> Vec<u8> {
        crate::ordinary_transport::encode_transport(
            signed,
            &self.config.ordinary.as_ref().unwrap().fee_profile.limits,
            100_000,
        )
        .unwrap()
    }
}
fn send(recipient: [u8; 32], amount: u128) -> OrdinaryAction {
    OrdinaryAction::Send {
        recipient,
        denomination: Denomination::Udrt,
        amount,
    }
}
fn current(app: &ConsensusApplication, key: &Key) -> RecoveryState {
    book(app).accounts[&hex::encode(key.id())].recovery.clone()
}
fn assert_mirror(app: &ConsensusApplication, key: &Key, expected: u64) {
    let bytes = app
        .storage
        .db
        .get(format!("acct:nonce:{}", key.address()))
        .unwrap()
        .expect("explicit native nonce mirror");
    let native: u64 = bincode::deserialize(&bytes).unwrap();
    assert_eq!(native, expected);
    assert_eq!(current(app, key).spending_nonce, expected);
}

pub(crate) fn local_governance_candidate(
    fixture: &Fixture,
    genesis_digest: [u8; 32],
    activation_height: u64,
) -> GovernanceCandidateConfig {
    use crate::runtime::governance_candidate::{
        ActionClassLimit, BallotRules, Bounds, Cancellation, DepositRules, EntryPolicy,
        ParameterBounds, ProposerEligibility, ValidatorVoting, VoteDelegation,
        CANDIDATE_SCHEMA_VERSION, CLASS_PARAMETER_CHANGE, CLASS_VALIDATOR_REGISTRY,
    };
    // Synthetic values; production values are E05 inputs.
    GovernanceCandidateConfig {
        schema_version: CANDIDATE_SCHEMA_VERSION,
        chain_id: CHAIN.into(),
        genesis_digest,
        activation_height,
        fee_profile: dytallix_protocol_types::ordinary_fees_v3::FeeProfileV3 {
            base: fixture.config.ordinary.as_ref().unwrap().fee_profile.clone(),
            version: 2,
            activation_height,
            max_governance_action_bytes: 1_000,
            governance_action_costs: [1; 3],
        },
        ballot: BallotRules {
            version: 1,
            chain_id: CHAIN.into(),
            genesis_digest,
            quorum_bps: 5_000,
            approval_bps: 5_000,
            veto_bps: 3_334,
            voting_period_blocks: 2,
            timelock_blocks: 2,
            max_voters: 64,
        },
        deposit: DepositRules {
            deposit_period_blocks: 2,
            minimum_deposit_udgt: 5,
            max_action_bytes: 1_000,
            max_depositors: 4,
        },
        action_classes: vec![
            ActionClassLimit {
                class: CLASS_PARAMETER_CHANGE,
                max_data_bytes: 1_000,
                approval_digest: [8; 32],
            },
            ActionClassLimit {
                class: CLASS_VALIDATOR_REGISTRY,
                max_data_bytes: 1_000,
                approval_digest: [9; 32],
            },
        ],
        parameter_bounds: ParameterBounds {
            gas_price: Bounds { min: 1, max: 100 },
            resource_cost: Bounds { min: 0, max: 1_000_000 },
            account_creation_fee_udrt: Bounds { min: 1, max: 1_000_000 },
            min_self_bond: Bounds { min: 1, max: 1_000_000_000 },
            max_active: Bounds { min: 1, max: 64 },
        },
        entry_policy: EntryPolicy {
            proposer_eligibility:
                ProposerEligibility::RegisteredOwnerWithEffectiveBondAtFinalizedParent,
            validator_voting: ValidatorVoting::OwnEffectiveBondOnly,
            vote_delegation: VoteDelegation::Disabled,
            cancellation: Cancellation::Disabled,
        },
    }
}

#[test]
fn signed_send_and_data_commit_once_with_fee_conservation_and_nonce_mirror() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    assert_mirror(&app, &fixture.active, 0);
    let tx = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![
            send(fixture.payer.id(), 100),
            OrdinaryAction::Data {
                data: "committed data".into(),
            },
        ],
        1000,
    );
    let raw = fixture.ordinary_wire(&tx);
    let before = data(&app);
    assert_admitted(app.check_tx(&raw));
    assert_eq!(data(&app), before);
    let input = block(1, vec![raw]);
    let result = app.finalize_block(input.clone()).unwrap();
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    assert_eq!(data(&app), before, "finalize only stages writes");
    assert_eq!(app.finalize_block(input).unwrap(), result);
    app.commit().unwrap();
    let fee = result.tx_results[0].gas_used.max(10) as u128 * 2;
    assert!(fee > 0);
    assert_eq!(
        balance(&app, &fixture.active.address()),
        INITIAL_DRT - 100 - fee
    );
    assert_eq!(balance(&app, &fixture.payer.address()), INITIAL_DRT + 100);
    assert_mirror(&app, &fixture.active, 1);
    assert_mirror(&app, &fixture.payer, 0);
}

#[test]
fn accepted_second_action_out_of_gas_rolls_back_send_and_charges_once() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    // Overhead (2), signature (3), receipt (5), first action (10) fit; second action does not.
    let tx = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![
            send(fixture.payer.id(), 100),
            OrdinaryAction::Data {
                data: "not committed".into(),
            },
        ],
        25,
    );
    let result = commit(&mut app, 1, vec![fixture.ordinary_wire(&tx)]);
    assert_ne!(result.tx_results[0].code, 0);
    assert_eq!(result.tx_results[0].gas_used, 25);
    assert_eq!(balance(&app, &fixture.active.address()), INITIAL_DRT - 50);
    assert_eq!(balance(&app, &fixture.payer.address()), INITIAL_DRT);
    assert_mirror(&app, &fixture.active, 1);
}

#[test]
fn inactivity_failure_in_second_action_rolls_back_send_but_consumes_fee_and_nonce() {
    let mut fixture = Fixture::new();
    fixture
        .config
        .ordinary
        .as_mut()
        .unwrap()
        .initial_grants
        .insert(
            hex::encode(fixture.secondary.id()),
            DiscretionaryGrant {
                version: 1,
                owner: fixture.secondary.id(),
                beneficiary: fixture.active.id(),
                owner_generation: 0,
                period_blocks: 10,
                last_active_height: 0,
            },
        );
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let tx = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![
            send(fixture.payer.id(), 100),
            OrdinaryAction::DmsClaim {
                owner: fixture.secondary.id(),
                expected_grant_generation: 0,
            },
        ],
        1000,
    );
    let result = commit(&mut app, 1, vec![fixture.ordinary_wire(&tx)]);
    assert_ne!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    assert!(result.tx_results[0].gas_used > 0);
    let fee = result.tx_results[0].gas_used.max(10) as u128 * 2;
    assert_eq!(balance(&app, &fixture.active.address()), INITIAL_DRT - fee);
    assert_eq!(balance(&app, &fixture.payer.address()), INITIAL_DRT);
    assert_eq!(balance(&app, &fixture.secondary.address()), INITIAL_DRT);
    assert_mirror(&app, &fixture.active, 1);
    assert_mirror(&app, &fixture.secondary, 0);
}

#[test]
fn exact_and_resigned_replays_reject_before_and_after_restart_without_second_fee() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = fixture.initialized(&path);
    let tx = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![send(fixture.payer.id(), 100)],
        1000,
    );
    let raw = fixture.ordinary_wire(&tx);
    let first = commit(&mut app, 1, vec![raw.clone()]);
    assert_eq!(first.tx_results[0].code, 0);
    let actor = balance(&app, &fixture.active.address());
    let recipient = balance(&app, &fixture.payer.address());
    let resigned = fixture.ordinary_wire(&fixture.resign(tx.body.clone(), &fixture.active));
    assert_ne!(app.check_tx(&raw).code, 0);
    assert_ne!(app.check_tx(&resigned).code, 0);
    drop(app);
    let mut app = fixture.open(&path);
    assert_ne!(app.check_tx(&resigned).code, 0);
    let retry = commit(&mut app, 2, vec![raw, resigned]);
    assert!(retry.tx_results.iter().all(|r| r.code != 0));
    assert_eq!(balance(&app, &fixture.active.address()), actor);
    assert_eq!(balance(&app, &fixture.payer.address()), recipient);
    assert_mirror(&app, &fixture.active, 1);
}

#[test]
fn future_nonce_and_wrong_generation_reject_without_fee_or_counter_use() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let tx = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![send(fixture.payer.id(), 100)],
        1000,
    );
    let mut future = tx.body.clone();
    future.spending_nonce = 1;
    let mut stale = tx.body.clone();
    stale.authorization_generation = 1;
    let result = commit(
        &mut app,
        1,
        vec![
            fixture.ordinary_wire(&fixture.resign(future, &fixture.active)),
            fixture.ordinary_wire(&fixture.resign(stale, &fixture.active)),
        ],
    );
    assert!(result.tx_results.iter().all(|r| r.code != 0));
    assert_eq!(balance(&app, &fixture.active.address()), INITIAL_DRT);
    assert_eq!(balance(&app, &fixture.payer.address()), INITIAL_DRT);
    assert_mirror(&app, &fixture.active, 0);
}

#[test]
fn ordinary_commit_error_and_lost_acknowledgement_keep_one_durable_charge() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = fixture.initialized(&path);
    let tx = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![send(fixture.payer.id(), 100)],
        1000,
    );
    let input = block(1, vec![fixture.ordinary_wire(&tx)]);
    let before = data(&app);
    let result = app.finalize_block(input.clone()).unwrap();
    assert_eq!(result.tx_results[0].code, 0);
    assert!(app
        .commit_with(|_, _| anyhow::bail!("synthetic prewrite failure"))
        .is_err());
    assert_eq!(data(&app), before);
    assert_eq!(app.finalize_block(input.clone()).unwrap(), result);
    assert!(app
        .commit_with(|storage, batch| {
            write_sync(storage, batch)?;
            anyhow::bail!("synthetic lost acknowledgement")
        })
        .is_err());
    let durable = data(&app);
    drop(app);
    let mut app = fixture.open(&path);
    assert_eq!(app.info().unwrap().height, 1);
    assert_eq!(app.finalize_block(input).unwrap(), result);
    app.commit().unwrap();
    assert_eq!(data(&app), durable);
    assert_mirror(&app, &fixture.active, 1);
}

#[test]
fn ordinary_restart_before_commit_discards_effects_and_recomputes_identical_result() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = fixture.initialized(&path);
    let tx = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![send(fixture.payer.id(), 100)],
        1000,
    );
    let input = block(1, vec![fixture.ordinary_wire(&tx)]);
    let before = data(&app);
    let result = app.finalize_block(input.clone()).unwrap();
    assert_eq!(result.tx_results[0].code, 0);
    drop(app);
    let mut app = fixture.open(&path);
    assert_eq!(data(&app), before);
    assert_mirror(&app, &fixture.active, 0);
    assert_eq!(app.finalize_block(input).unwrap(), result);
    app.commit().unwrap();
    assert_mirror(&app, &fixture.active, 1);
}

#[test]
fn recovery_start_order_controls_ordinary_effects_within_the_same_block() {
    for start_first in [false, true] {
        let fixture = Fixture::new();
        let dir = tempfile::tempdir().unwrap();
        let mut app = fixture.initialized(&dir.path().join("db"));
        let enrolled = commit(
            &mut app,
            1,
            vec![wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT))],
        );
        assert_eq!(enrolled.tx_results[0].code, 0, "{:?}", enrolled.tx_results);
        assert_mirror(&app, &fixture.active, 1);
        let tx = fixture.ordinary(
            &current(&app, &fixture.active),
            &fixture.active,
            vec![send(fixture.secondary.id(), 100)],
            1000,
        );
        let ordinary = fixture.ordinary_wire(&tx);
        let start = wire(&fixture.sponsored(fixture.start(), 1, GAS_LIMIT));
        let txs = if start_first {
            vec![start, ordinary]
        } else {
            vec![ordinary, start]
        };
        let result = commit(&mut app, 2, txs);
        let ordinary_index = if start_first { 1 } else { 0 };
        let recovery_index = 1 - ordinary_index;
        assert_eq!(
            result.tx_results[recovery_index].code, 0,
            "{:?}",
            result.tx_results
        );
        assert_eq!(
            current(&app, &fixture.active).status,
            RecoveryStatus::PendingRecovery
        );
        if start_first {
            assert_ne!(result.tx_results[ordinary_index].code, 0);
            assert_eq!(balance(&app, &fixture.active.address()), INITIAL_DRT);
            assert_eq!(balance(&app, &fixture.secondary.address()), INITIAL_DRT);
            assert_mirror(&app, &fixture.active, 1);
        } else {
            assert_eq!(result.tx_results[ordinary_index].code, 0);
            let fee = result.tx_results[ordinary_index].gas_used.max(10) as u128 * 2;
            assert_eq!(
                balance(&app, &fixture.active.address()),
                INITIAL_DRT - 100 - fee
            );
            assert_eq!(
                balance(&app, &fixture.secondary.address()),
                INITIAL_DRT + 100
            );
            assert_mirror(&app, &fixture.active, 2);
        }
    }
}

#[test]
fn finalized_replacement_key_sends_from_stable_account_while_old_key_rejects() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    assert_eq!(
        commit(
            &mut app,
            1,
            vec![wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT))]
        )
        .tx_results[0]
            .code,
        0
    );
    assert_eq!(
        commit(
            &mut app,
            2,
            vec![wire(&fixture.sponsored(fixture.start(), 1, GAS_LIMIT))]
        )
        .tx_results[0]
            .code,
        0
    );
    commit(&mut app, 3, vec![]);
    let prior = current(&app, &fixture.active);
    let mut expected = prior.clone();
    expected.active_generation += 1;
    expected.active_key = fixture.replacement.identity.clone();
    let replacement = fixture.ordinary(
        &expected,
        &fixture.replacement,
        vec![send(fixture.secondary.id(), 100)],
        1000,
    );
    let old = fixture.ordinary(
        &prior,
        &fixture.active,
        vec![send(fixture.secondary.id(), 500)],
        1000,
    );
    let result = commit(
        &mut app,
        4,
        vec![
            wire(&fixture.sponsored(fixture.finalize(), 2, GAS_LIMIT)),
            fixture.ordinary_wire(&old),
            fixture.ordinary_wire(&replacement),
        ],
    );
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    assert_ne!(result.tx_results[1].code, 0);
    assert_eq!(result.tx_results[2].code, 0, "{:?}", result.tx_results);
    let active = current(&app, &fixture.active);
    assert_eq!(active.domain.account_id, fixture.active.id());
    assert_eq!(active.active_key, fixture.replacement.identity);
    assert_eq!(active.status, RecoveryStatus::Normal);
    assert_eq!(
        balance(&app, &fixture.secondary.address()),
        INITIAL_DRT + 100
    );
    let fee = result.tx_results[2].gas_used.max(10) as u128 * 2;
    assert_eq!(
        balance(&app, &fixture.active.address()),
        INITIAL_DRT - 100 - fee
    );
    assert_mirror(&app, &fixture.active, 2);
}

#[test]
fn protected_dms_debit_owner_rejects_beneficiary_charge_and_all_transfer_effects() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    assert_eq!(
        commit(
            &mut app,
            1,
            vec![wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT))]
        )
        .tx_results[0]
            .code,
        0
    );
    let grant = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![OrdinaryAction::DmsRegister {
            beneficiary: fixture.secondary.id(),
            period_blocks: 1,
        }],
        1000,
    );
    assert_eq!(
        commit(&mut app, 2, vec![fixture.ordinary_wire(&grant)]).tx_results[0].code,
        0
    );
    assert_eq!(
        commit(
            &mut app,
            3,
            vec![wire(&fixture.sponsored(fixture.start(), 1, GAS_LIMIT))]
        )
        .tx_results[0]
            .code,
        0
    );
    let owner_before = balance(&app, &fixture.active.address());
    let tx = fixture.ordinary(
        &current(&app, &fixture.secondary),
        &fixture.secondary,
        vec![OrdinaryAction::DmsClaim {
            owner: fixture.active.id(),
            expected_grant_generation: 1,
        }],
        1000,
    );
    let result = commit(&mut app, 4, vec![fixture.ordinary_wire(&tx)]);
    assert_ne!(result.tx_results[0].code, 0);
    assert_eq!(balance(&app, &fixture.active.address()), owner_before);
    assert_eq!(balance(&app, &fixture.secondary.address()), INITIAL_DRT);
    assert_mirror(&app, &fixture.active, 2);
    assert_mirror(&app, &fixture.secondary, 0);
}

#[test]
fn changed_predecessor_rejects_staged_ordinary_commit_without_partial_writes() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = fixture.initialized(&path);
    let tx = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![send(fixture.payer.id(), 100)],
        1000,
    );
    let result = app
        .finalize_block(block(1, vec![fixture.ordinary_wire(&tx)]))
        .unwrap();
    assert_eq!(result.tx_results[0].code, 0);
    // A synthetic storage fault changes the predecessor after proposal execution.
    let mut batch = WriteBatch::default();
    batch.put(
        format!("acct:nonce:{}", fixture.active.address()),
        bincode::serialize(&9u64).unwrap(),
    );
    write_sync(&app.storage, batch).unwrap();
    let altered = data(&app);
    assert!(app.commit().is_err());
    assert_eq!(data(&app), altered);
    assert_eq!(balance(&app, &fixture.active.address()), INITIAL_DRT);
    assert_eq!(balance(&app, &fixture.payer.address()), INITIAL_DRT);
    drop(app);
    assert!(
        ConsensusApplication::open(&path, fixture.config.clone(), fixture.genesis.clone()).is_err()
    );
}

#[test]
fn shared_signature_budget_bounds_ordinary_and_recovery_in_all_consensus_entry_points() {
    let mut fixture = Fixture::new();
    fixture
        .config
        .ordinary
        .as_mut()
        .unwrap()
        .fee_profile
        .max_block_signature_checks = 5;
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    // Enrollment needs four recovery signatures plus one sponsor signature.
    let recovery = wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT));
    let signed = fixture.ordinary(
        &current(&app, &fixture.secondary),
        &fixture.secondary,
        vec![OrdinaryAction::Data {
            data: "bounded".into(),
        }],
        1000,
    );
    let ordinary = fixture.ordinary_wire(&signed);
    let before = data(&app);
    assert_admitted(app.check_tx(&recovery));
    assert_admitted(app.check_tx(&ordinary));
    assert!(app
        .process_proposal(block(1, vec![recovery.clone()]))
        .unwrap());
    assert!(app
        .process_proposal(block(1, vec![ordinary.clone()]))
        .unwrap());
    let both = vec![recovery, ordinary];
    assert!(!app.process_proposal(block(1, both.clone())).unwrap());
    assert!(app.finalize_block(block(1, both)).is_err());
    assert_eq!(data(&app), before);
    assert_mirror(&app, &fixture.active, 0);
    assert_mirror(&app, &fixture.secondary, 0);
}

#[test]
fn invalid_ordinary_signatures_consume_shared_limits_without_fees() {
    for resource in ["gas", "bytes"] {
        let mut fixture = Fixture::new();
        let gas_limit = if resource == "gas" { 5 } else { 1000 };
        if resource == "gas" {
            let p = &mut fixture.config.ordinary.as_mut().unwrap().fee_profile;
            p.minimum_gas = 1;
            p.max_transaction_gas = 5;
            p.max_block_transaction_gas = 5;
        } else {
            let provisional = fixture.ordinary(
                fixture.state(fixture.active.id()),
                &fixture.active,
                vec![OrdinaryAction::Data {
                    data: "invalid fixture".into(),
                }],
                gas_limit,
            );
            let bytes = ordinary::encode(
                &provisional,
                &fixture.config.ordinary.as_ref().unwrap().fee_profile.limits,
            )
            .unwrap()
            .len();
            let p = &mut fixture.config.ordinary.as_mut().unwrap().fee_profile;
            p.limits.max_wire_bytes = bytes as u32;
            p.max_block_transaction_bytes = bytes as u64;
        }
        let dir = tempfile::tempdir().unwrap();
        let mut app = fixture.initialized(&dir.path().join("db"));
        let mut signed = fixture.ordinary(
            &current(&app, &fixture.active),
            &fixture.active,
            vec![OrdinaryAction::Data {
                data: "invalid fixture".into(),
            }],
            gas_limit,
        );
        signed.signature[0] ^= 1;
        let raw = fixture.ordinary_wire(&signed);
        let before = data(&app);
        let checked = app.check_tx(&raw);
        assert_ne!(checked.code, 0);
        assert!(
            checked.gas_used > 0,
            "invalid signature must retain measured work"
        );
        assert_eq!(data(&app), before);
        assert!(app.process_proposal(block(1, vec![raw.clone()])).unwrap());
        assert!(
            !app.process_proposal(block(1, vec![raw.clone(), raw.clone()]))
                .unwrap(),
            "shared {resource} budget"
        );
        assert!(app
            .finalize_block(block(1, vec![raw.clone(), raw.clone()]))
            .is_err());
        assert_eq!(data(&app), before);
        let result = commit(&mut app, 1, vec![raw]);
        assert_ne!(result.tx_results[0].code, 0);
        assert_eq!(balance(&app, &fixture.active.address()), INITIAL_DRT);
        assert_mirror(&app, &fixture.active, 0);
    }
}

#[test]
fn paid_failure_replay_and_resigning_preserve_one_charge_after_restart() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = fixture.initialized(&path);
    let signed = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![
            send(fixture.payer.id(), 100),
            OrdinaryAction::Data {
                data: "rollback".into(),
            },
        ],
        25,
    );
    let raw = fixture.ordinary_wire(&signed);
    let input = block(1, vec![raw.clone()]);
    let result = app.finalize_block(input.clone()).unwrap();
    assert_ne!(result.tx_results[0].code, 0);
    assert_eq!(result.tx_results[0].gas_used, 25);
    app.commit().unwrap();
    let durable = data(&app);
    assert_eq!(app.finalize_block(input.clone()).unwrap(), result);
    app.commit().unwrap();
    assert_eq!(data(&app), durable);
    drop(app);
    let mut app = fixture.open(&path);
    assert_eq!(app.finalize_block(input).unwrap(), result);
    app.commit().unwrap();
    assert_eq!(data(&app), durable);
    let resigned = fixture.ordinary_wire(&fixture.resign(signed.body, &fixture.active));
    assert_ne!(app.check_tx(&raw).code, 0);
    assert_ne!(app.check_tx(&resigned).code, 0);
    let retry = commit(&mut app, 2, vec![raw, resigned]);
    assert!(retry.tx_results.iter().all(|r| r.code != 0));
    assert_eq!(balance(&app, &fixture.active.address()), INITIAL_DRT - 50);
    assert_eq!(balance(&app, &fixture.payer.address()), INITIAL_DRT);
    assert_mirror(&app, &fixture.active, 1);
}

#[test]
fn independent_databases_commit_identical_mixed_inputs_and_roots() {
    let fixture = Fixture::new();
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let mut a = fixture.initialized(&first.path().join("db"));
    let mut b = fixture.initialized(&second.path().join("db"));
    assert_eq!(data(&a), data(&b));
    let fresh = Key::new();
    let signed = fixture.ordinary(
        &current(&a, &fixture.secondary),
        &fixture.secondary,
        vec![
            send(fixture.payer.id(), 100),
            send(fresh.id(), 5),
            OrdinaryAction::Data {
                data: "deterministic".into(),
            },
        ],
        1000,
    );
    let txs = vec![
        fixture.ordinary_wire(&signed),
        wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT)),
    ];
    let result_a = commit(&mut a, 1, txs.clone());
    let result_b = commit(&mut b, 1, txs);
    assert!(
        result_a.tx_results.iter().all(|r| r.code == 0),
        "{:?}",
        result_a.tx_results
    );
    assert_eq!(result_a, result_b);
    assert_eq!(a.info().unwrap(), b.info().unwrap());
    assert_eq!(data(&a), data(&b));
}

#[test]
fn earlier_accepted_transaction_survives_later_paid_failure_in_same_block() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let first = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![send(fixture.payer.id(), 100)],
        1000,
    );
    let mut after_first = current(&app, &fixture.active);
    after_first.spending_nonce += 1;
    let second = fixture.ordinary(
        &after_first,
        &fixture.active,
        vec![
            send(fixture.secondary.id(), 200),
            OrdinaryAction::Data {
                data: "failed second transaction".into(),
            },
        ],
        25,
    );
    let result = commit(
        &mut app,
        1,
        vec![
            fixture.ordinary_wire(&first),
            fixture.ordinary_wire(&second),
        ],
    );
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    assert_ne!(result.tx_results[1].code, 0);
    assert_eq!(result.tx_results[1].gas_used, 25);
    let first_fee = result.tx_results[0].gas_used.max(10) as u128 * 2;
    assert_eq!(
        balance(&app, &fixture.active.address()),
        INITIAL_DRT - 100 - first_fee - 50
    );
    assert_eq!(balance(&app, &fixture.payer.address()), INITIAL_DRT + 100);
    assert_eq!(balance(&app, &fixture.secondary.address()), INITIAL_DRT);
    assert_mirror(&app, &fixture.active, 2);
}

#[test]
fn proposal_reserves_ordinary_and_sponsor_caps_against_one_payer_balance() {
    let mut fixture = Fixture::new();
    fixture
        .config
        .ordinary
        .as_mut()
        .unwrap()
        .fee_profile
        .max_fee_cap = 6_000_000;
    fixture
        .config
        .recovery
        .as_mut()
        .unwrap()
        .profile
        .max_fee_cap = 6_000_000;
    let dir = tempfile::tempdir().unwrap();
    let app = fixture.initialized(&dir.path().join("db"));
    let signed = fixture.ordinary(
        &current(&app, &fixture.payer),
        &fixture.payer,
        vec![OrdinaryAction::Data {
            data: "shared cap".into(),
        }],
        1000,
    );
    let ordinary = fixture.ordinary_wire(&signed);
    let recovery = wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT));
    assert_admitted(app.check_tx(&ordinary));
    assert!(app
        .evict_ordinary_admission(&ordinary_id(&fixture, &signed), false)
        .unwrap());
    assert_admitted(app.check_tx(&recovery));
    let before = data(&app);
    for ordered in [
        vec![ordinary.clone(), recovery.clone()],
        vec![recovery.clone(), ordinary.clone()],
    ] {
        let selected = app
            .prepare_proposal(1, 10, 0, ordered.clone(), 1_048_576)
            .unwrap();
        assert_eq!(
            selected,
            vec![ordered[0].clone()],
            "independent nonces must share the fee reservation balance"
        );
        assert_eq!(data(&app), before);
    }
}

fn asset_balance(app: &ConsensusApplication, owner: &str, denomination: &str) -> u128 {
    let balances: BTreeMap<String, u128> = bincode::deserialize(
        &app.storage
            .db
            .get(format!("acct:balances:{owner}"))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    balances.get(denomination).copied().unwrap_or(0)
}
fn reward_state(app: &ConsensusApplication) -> RewardState {
    RewardState::decode(&app.storage.db.get(REWARD_STATE_KEY).unwrap().unwrap()).unwrap()
}
fn lifecycle_state(app: &ConsensusApplication) -> LifecycleState {
    LifecycleState::decode(&app.storage.db.get(LIFECYCLE_STATE_KEY).unwrap().unwrap()).unwrap()
}
fn successful_ordinary(
    app: &mut ConsensusApplication,
    fixture: &Fixture,
    key: &Key,
    height: u64,
    actions: Vec<OrdinaryAction>,
) -> FinalizeResult {
    let signed = fixture.ordinary(&current(app, key), key, actions, 1000);
    let result = commit(app, height, vec![fixture.ordinary_wire(&signed)]);
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    result
}

#[test]
fn signed_dms_register_ping_and_mature_claim_commit_liquid_assets_and_nonce_mirrors() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    successful_ordinary(
        &mut app,
        &fixture,
        &fixture.secondary,
        1,
        vec![OrdinaryAction::DmsRegister {
            beneficiary: fixture.payer.id(),
            period_blocks: 2,
        }],
    );
    successful_ordinary(
        &mut app,
        &fixture,
        &fixture.secondary,
        2,
        vec![OrdinaryAction::DmsPing],
    );
    commit(&mut app, 3, vec![]);
    commit(&mut app, 4, vec![]);
    let owner_drt = balance(&app, &fixture.secondary.address());
    let owner_dgt = asset_balance(&app, &fixture.secondary.address(), "udgt");
    let beneficiary_drt = balance(&app, &fixture.payer.address());
    let beneficiary_dgt = asset_balance(&app, &fixture.payer.address(), "udgt");
    let result = successful_ordinary(
        &mut app,
        &fixture,
        &fixture.payer,
        5,
        vec![OrdinaryAction::DmsClaim {
            owner: fixture.secondary.id(),
            expected_grant_generation: 0,
        }],
    );
    let fee = result.tx_results[0].gas_used.max(10) as u128 * 2;
    assert_eq!(balance(&app, &fixture.secondary.address()), 0);
    assert_eq!(asset_balance(&app, &fixture.secondary.address(), "udgt"), 0);
    assert_eq!(
        balance(&app, &fixture.payer.address()),
        beneficiary_drt + owner_drt - fee
    );
    assert_eq!(
        asset_balance(&app, &fixture.payer.address(), "udgt"),
        beneficiary_dgt + owner_dgt
    );
    assert_mirror(&app, &fixture.secondary, 2);
    assert_mirror(&app, &fixture.payer, 1);
}

#[test]
fn signed_reward_bond_begin_unbond_and_claim_preserve_h_plus_two_custody() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let consensus_key = fixture.config.validators[0].pubkey_base64.clone();
    let power = |app: &ConsensusApplication, height| {
        lifecycle_state(app)
            .validator_set(height)
            .unwrap()
            .into_iter()
            .find(|v| v.pubkey_base64 == consensus_key)
            .unwrap()
            .power
    };
    successful_ordinary(
        &mut app,
        &fixture,
        &fixture.secondary,
        1,
        vec![OrdinaryAction::RewardBond {
            validator_id: "validator-one".into(),
            amount_udgt: 100,
        }],
    );
    assert_eq!(
        asset_balance(&app, &fixture.secondary.address(), "udgt"),
        900
    );
    assert_eq!(power(&app, 2), 100);
    assert_eq!(power(&app, 3), 200);
    successful_ordinary(
        &mut app,
        &fixture,
        &fixture.secondary,
        2,
        vec![OrdinaryAction::RewardBeginUnbond {
            validator_id: "validator-one".into(),
            amount_udgt: 40,
        }],
    );
    assert_eq!(power(&app, 3), 200);
    assert_eq!(power(&app, 4), 160);
    commit(&mut app, 3, vec![]);
    commit(&mut app, 4, vec![]);
    let state = lifecycle_state(&app);
    let entry = state
        .unbonding
        .values()
        .find(|entry| entry.owner == fixture.secondary.address())
        .unwrap();
    assert_eq!(
        (
            entry.amount,
            entry.request_height,
            entry.effective_height,
            entry.last_exposure_height
        ),
        (40, 2, 4, Some(3))
    );
    assert_eq!(
        asset_balance(&app, &fixture.secondary.address(), "udgt"),
        900
    );
    let before_rewards = reward_state(&app);
    let before_balance = balance(&app, &fixture.secondary.address());
    assert!(
        before_rewards
            .unpaid
            .get(&fixture.secondary.address())
            .copied()
            .unwrap_or(0)
            > 0
    );
    let result = successful_ordinary(
        &mut app,
        &fixture,
        &fixture.secondary,
        5,
        vec![OrdinaryAction::RewardClaim],
    );
    let claimed = reward_state(&app).total_claimed - before_rewards.total_claimed;
    assert!(claimed > 0);
    assert_eq!(
        balance(&app, &fixture.secondary.address()),
        before_balance + claimed - result.tx_results[0].gas_used.max(10) as u128 * 2
    );
    assert_mirror(&app, &fixture.secondary, 3);
}

#[test]
fn paid_failure_can_retry_with_next_nonce_and_sufficient_signed_gas() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let actions = vec![
        send(fixture.payer.id(), 100),
        OrdinaryAction::Data {
            data: "retry".into(),
        },
    ];
    let signed = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        actions.clone(),
        25,
    );
    let failed = commit(&mut app, 1, vec![fixture.ordinary_wire(&signed)]);
    assert_ne!(failed.tx_results[0].code, 0);
    assert_mirror(&app, &fixture.active, 1);
    let successful = successful_ordinary(&mut app, &fixture, &fixture.active, 2, actions);
    let second_fee = successful.tx_results[0].gas_used.max(10) as u128 * 2;
    assert_eq!(
        balance(&app, &fixture.active.address()),
        INITIAL_DRT - 50 - 100 - second_fee
    );
    assert_eq!(balance(&app, &fixture.payer.address()), INITIAL_DRT + 100);
    assert_mirror(&app, &fixture.active, 2);
}

#[test]
fn mandatory_recovery_expiry_survives_later_paid_ordinary_failure() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    assert_eq!(
        commit(
            &mut app,
            1,
            vec![wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT))]
        )
        .tx_results[0]
            .code,
        0
    );
    assert_eq!(
        commit(
            &mut app,
            2,
            vec![wire(&fixture.sponsored(fixture.start(), 1, GAS_LIMIT))]
        )
        .tx_results[0]
            .code,
        0
    );
    for height in 3..=5 {
        commit(&mut app, height, vec![]);
    }
    assert_eq!(
        current(&app, &fixture.active).status,
        RecoveryStatus::PendingRecovery
    );
    let before = current(&app, &fixture.active);
    let signed = fixture.ordinary(
        &current(&app, &fixture.secondary),
        &fixture.secondary,
        vec![
            send(fixture.payer.id(), 100),
            OrdinaryAction::Data {
                data: "expiry block failure".into(),
            },
        ],
        25,
    );
    let result = commit(&mut app, 6, vec![fixture.ordinary_wire(&signed)]);
    assert_ne!(result.tx_results[0].code, 0);
    assert_eq!(result.tx_results[0].gas_used, 25);
    let after = current(&app, &fixture.active);
    assert_eq!(after.status, RecoveryStatus::RecoveryLocked);
    assert!(after.pending_recovery.is_none());
    assert_eq!(after.active_generation, before.active_generation + 1);
    assert_eq!(after.recovery_sequence, before.recovery_sequence + 1);
    assert_eq!(
        balance(&app, &fixture.secondary.address()),
        INITIAL_DRT - 50
    );
    assert_mirror(&app, &fixture.secondary, 1);
    assert_mirror(&app, &fixture.active, 1);
}

struct ValidatorProofKey {
    public: Vec<u8>,
    private: ml_dsa_65::PrivateKey,
}
impl ValidatorProofKey {
    fn new() -> Self {
        let (public, private) = ml_dsa_65::KG::try_keygen_with_rng(&mut rand_core::OsRng).unwrap();
        Self {
            public: public.into_bytes().to_vec(),
            private,
        }
    }
    fn proof(
        &self,
        operation: &str,
        validator_id: &str,
        owner: &Key,
        nonce: u64,
        amount: u128,
    ) -> Vec<u8> {
        use fips204::traits::Signer;
        let bytes = crate::runtime::validator_lifecycle::proof_sign_bytes(
            CHAIN,
            operation,
            validator_id,
            &owner.address(),
            &B64.encode(&self.public),
            nonce,
            40,
            amount,
        )
        .unwrap();
        self.private.try_sign(&bytes, &[]).unwrap().to_vec()
    }
}

#[test]
fn signed_validator_register_rotate_exit_and_mature_withdraw_preserve_stable_owner() {
    let mut fixture = Fixture::new();
    fixture.config.profile = "cometbft-penalty-local-qualification".into();
    fixture.config.penalty = Some(PenaltyConfig {
        version: 1,
        profile: "cometbft-penalty-local-qualification".into(),
        chain_id: CHAIN.into(),
        penalty_numerator: 1,
        penalty_denominator: 20,
        production_activation: false,
    });
    fixture
        .config
        .lifecycle
        .as_mut()
        .unwrap()
        .approved_operators
        .insert("validator-two".into(), fixture.secondary.address());
    fixture
        .config
        .ordinary
        .as_mut()
        .unwrap()
        .fee_profile
        .validator_proof_profile_digest =
        crate::ordinary_state::validator_profile_digest(fixture.config.lifecycle.as_ref().unwrap())
            .unwrap();
    let first_key = ValidatorProofKey::new();
    let next_key = ValidatorProofKey::new();
    let first_encoded = B64.encode(&first_key.public);
    let next_encoded = B64.encode(&next_key.public);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = fixture.initialized(&path);
    let power = |app: &ConsensusApplication, height, key: &str| {
        lifecycle_state(app)
            .validator_set(height)
            .unwrap()
            .into_iter()
            .find(|v| v.pubkey_base64 == key)
            .map_or(0, |v| v.power)
    };
    successful_ordinary(
        &mut app,
        &fixture,
        &fixture.secondary,
        1,
        vec![OrdinaryAction::ValidatorRegister {
            validator_id: "validator-two".into(),
            consensus_key: first_key.public.clone(),
            proof: first_key.proof("register", "validator-two", &fixture.secondary, 0, 100),
            proof_expiry_height: 40,
            amount_udgt: 100,
        }],
    );
    assert_eq!(
        asset_balance(&app, &fixture.secondary.address(), "udgt"),
        900
    );
    assert_eq!(power(&app, 2, &first_encoded), 0);
    assert_eq!(power(&app, 3, &first_encoded), 100);
    commit(&mut app, 2, vec![]);
    commit(&mut app, 3, vec![]);
    successful_ordinary(
        &mut app,
        &fixture,
        &fixture.secondary,
        4,
        vec![OrdinaryAction::ValidatorRotateKey {
            validator_id: "validator-two".into(),
            consensus_key: next_key.public.clone(),
            proof: next_key.proof("rotate", "validator-two", &fixture.secondary, 1, 0),
            proof_expiry_height: 40,
        }],
    );
    assert_eq!(power(&app, 5, &first_encoded), 100);
    assert_eq!(power(&app, 5, &next_encoded), 0);
    assert_eq!(power(&app, 6, &first_encoded), 0);
    assert_eq!(power(&app, 6, &next_encoded), 100);
    commit(&mut app, 5, vec![]);
    commit(&mut app, 6, vec![]);
    successful_ordinary(
        &mut app,
        &fixture,
        &fixture.secondary,
        7,
        vec![OrdinaryAction::ValidatorExit {
            validator_id: "validator-two".into(),
        }],
    );
    assert_eq!(power(&app, 8, &next_encoded), 100);
    assert_eq!(power(&app, 9, &next_encoded), 0);
    for height in 8..=13 {
        commit(&mut app, height, vec![]);
    }
    let state = lifecycle_state(&app);
    let unbond = state
        .unbonding
        .values()
        .find(|entry| entry.owner == fixture.secondary.address())
        .unwrap();
    assert_eq!(
        (
            unbond.amount,
            unbond.effective_height,
            unbond.last_exposure_height
        ),
        (100, 9, Some(8))
    );
    assert!(!unbond
        .maturity_satisfied(fixture.config.lifecycle.as_ref().unwrap(), 12, 120, true)
        .unwrap());
    assert!(unbond
        .maturity_satisfied(fixture.config.lifecycle.as_ref().unwrap(), 13, 130, true)
        .unwrap());
    let unbond_id = unbond.id.clone();
    drop(app);
    let mut app = fixture.open(&path);
    successful_ordinary(
        &mut app,
        &fixture,
        &fixture.secondary,
        14,
        vec![OrdinaryAction::ValidatorWithdraw { unbond_id }],
    );
    assert_eq!(
        asset_balance(&app, &fixture.secondary.address(), "udgt"),
        1000
    );
    assert_mirror(&app, &fixture.secondary, 4);
    let penalty =
        PenaltyState::decode(&app.storage.db.get(PENALTY_STATE_KEY).unwrap().unwrap()).unwrap();
    assert_eq!(penalty.released_total().unwrap(), 100);
    assert_eq!(penalty.deducted_total().unwrap(), 0);
    assert_eq!(
        current(&app, &fixture.secondary).domain.account_id,
        fixture.secondary.id()
    );

    // The next block start removes the released unbond, its tranches and its
    // receipt, keeping the release total (state model step 4).
    let owner = fixture.secondary.address();
    commit(&mut app, 15, vec![]);
    let penalty =
        PenaltyState::decode(&app.storage.db.get(PENALTY_STATE_KEY).unwrap().unwrap()).unwrap();
    assert!(penalty.releases.is_empty());
    assert!(penalty.tranches.values().all(|t| t.owner != owner || t.unbond_id.is_none()));
    assert_eq!((penalty.pruned_released, penalty.released_total().unwrap()), (100, 100));
    let state = lifecycle_state(&app);
    assert!(state.unbonding.values().all(|entry| entry.owner != owner));
    // History before the evidence horizon is gone; the restart below still
    // passes the complete check, which folds validator updates from genesis.
    assert!(state.history.base_height > 1);
    let rewards = reward_state(&app);
    assert!(!rewards.unbonding.contains_key(&owner));
    // Its staker slot is freed once it holds nothing else.
    let holds = rewards.positions.contains_key(&owner) || rewards.unpaid.contains_key(&owner);
    assert_eq!(state.reserved_owners.contains(&owner), holds);
    crate::supply::inspect_native(&app.storage).unwrap();
    drop(app);
    let reopened = fixture.open(&path);
    verify_recovery(&reopened.storage).unwrap();
}

fn ordinary_id(fixture: &Fixture, signed: &SignedOrdinary) -> String {
    hex::encode(
        ordinary::transaction_id(
            &signed.body,
            &fixture.config.ordinary.as_ref().unwrap().fee_profile.limits,
        )
        .unwrap(),
    )
}

#[test]
fn check_tx_retains_same_nonce_reservations_and_refuses_resigned_intent() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let app = fixture.initialized(&dir.path().join("db"));
    let first = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![send(fixture.payer.id(), 100)],
        1000,
    );
    let second = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![send(fixture.secondary.id(), 200)],
        1000,
    );
    let before = data(&app);
    assert_eq!(app.ordinary_admission_count().unwrap(), 0);
    assert_admitted(app.check_tx(&fixture.ordinary_wire(&first)));
    assert_eq!(app.ordinary_admission_count().unwrap(), 1);
    assert_admitted(app.check_tx(&fixture.ordinary_wire(&first)));
    // A re-signed copy has other bytes: it would occupy the engine mempool
    // beside the original, so it is refused (E04 gap 6).
    let resigned = fixture.resign(first.body.clone(), &fixture.active);
    assert_ne!(fixture.ordinary_wire(&resigned), fixture.ordinary_wire(&first));
    let refused = app.check_tx(&fixture.ordinary_wire(&resigned));
    assert_ne!(refused.code, 0);
    assert!(refused.log.contains("another signed envelope"), "{}", refused.log);
    assert_eq!(app.ordinary_admission_count().unwrap(), 1);
    let mut invalid = first.clone();
    invalid.signature[0] ^= 1;
    assert_ne!(app.check_tx(&fixture.ordinary_wire(&invalid)).code, 0);
    assert_eq!(
        app.ordinary_admission_count().unwrap(),
        1,
        "untrusted new admission cannot evict a valid reservation"
    );
    assert_admitted(app.check_tx(&fixture.ordinary_wire(&first)));
    assert_ne!(app.check_tx(&fixture.ordinary_wire(&second)).code, 0);
    assert_eq!(app.ordinary_admission_count().unwrap(), 1);
    assert_eq!(data(&app), before);
    assert_mirror(&app, &fixture.active, 0);
}

#[test]
fn check_tx_shares_payer_liquidity_between_ordinary_and_recovery_sponsors() {
    let mut fixture = Fixture::new();
    fixture
        .config
        .ordinary
        .as_mut()
        .unwrap()
        .fee_profile
        .max_fee_cap = 6_000_000;
    fixture
        .config
        .recovery
        .as_mut()
        .unwrap()
        .profile
        .max_fee_cap = 6_000_000;
    let dir = tempfile::tempdir().unwrap();
    let app = fixture.initialized(&dir.path().join("db"));
    let signed = fixture.ordinary(
        &current(&app, &fixture.payer),
        &fixture.payer,
        vec![OrdinaryAction::Data {
            data: "ordinary payer".into(),
        }],
        1000,
    );
    let sponsored = fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT);
    let ordinary = fixture.ordinary_wire(&signed);
    let recovery = wire(&sponsored);
    let before = data(&app);
    assert_admitted(app.check_tx(&ordinary));
    assert_ne!(app.check_tx(&recovery).code, 0);
    assert_eq!(app.ordinary_admission_count().unwrap(), 1);
    assert!(app
        .evict_ordinary_admission(&ordinary_id(&fixture, &signed), false)
        .unwrap());
    assert_admitted(app.check_tx(&recovery));
    assert_ne!(app.check_tx(&ordinary).code, 0);
    assert_eq!(app.ordinary_admission_count().unwrap(), 1);
    let sponsor_id = hex::encode(sponsor::authorization_id(&sponsored.sponsor).unwrap());
    assert!(app.evict_ordinary_admission(&sponsor_id, true).unwrap());
    assert_eq!(app.ordinary_admission_count().unwrap(), 0);
    assert_eq!(data(&app), before);
}

#[test]
fn trusted_admission_eviction_releases_capacity_without_changing_chain_state() {
    let mut fixture = Fixture::new();
    fixture.config.ordinary.as_mut().unwrap().queue_max_entries = 1;
    let dir = tempfile::tempdir().unwrap();
    let app = fixture.initialized(&dir.path().join("db"));
    let first = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![OrdinaryAction::Data {
            data: "first".into(),
        }],
        1000,
    );
    let second = fixture.ordinary(
        &current(&app, &fixture.payer),
        &fixture.payer,
        vec![OrdinaryAction::Data {
            data: "second".into(),
        }],
        1000,
    );
    let before = data(&app);
    assert_admitted(app.check_tx(&fixture.ordinary_wire(&first)));
    assert_ne!(app.check_tx(&fixture.ordinary_wire(&second)).code, 0);
    let id = ordinary_id(&fixture, &first);
    assert!(app.evict_ordinary_admission(&id, false).unwrap());
    assert!(!app.evict_ordinary_admission(&id, false).unwrap());
    assert_admitted(app.check_tx(&fixture.ordinary_wire(&second)));
    assert_eq!(app.ordinary_admission_count().unwrap(), 1);
    assert_eq!(data(&app), before);
}

#[test]
fn commit_head_change_clears_reservations_and_recheck_rejects_stale_nonce() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let admitted = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![send(fixture.payer.id(), 100)],
        1000,
    );
    let committed = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![send(fixture.secondary.id(), 200)],
        1000,
    );
    assert_admitted(app.check_tx(&fixture.ordinary_wire(&admitted)));
    assert_eq!(app.ordinary_admission_count().unwrap(), 1);
    // Consensus can select another currently valid intent; local reservations do not grant authority.
    let result = commit(&mut app, 1, vec![fixture.ordinary_wire(&committed)]);
    assert_eq!(result.tx_results[0].code, 0);
    let durable = data(&app);
    assert_ne!(
        app.recheck_ordinary_admission(&fixture.ordinary_wire(&admitted))
            .code,
        0
    );
    assert_eq!(app.ordinary_admission_count().unwrap(), 0);
    assert_eq!(data(&app), durable);
    let fresh = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![send(fixture.payer.id(), 100)],
        1000,
    );
    assert_admitted(app.check_tx(&fixture.ordinary_wire(&fresh)));
    assert_eq!(app.ordinary_admission_count().unwrap(), 1);
    commit(&mut app, 2, vec![]);
    assert_eq!(app.ordinary_admission_count().unwrap(), 0);
    assert_eq!(
        app.recheck_ordinary_admission(&fixture.ordinary_wire(&fresh))
            .code,
        0
    );
    assert_eq!(app.ordinary_admission_count().unwrap(), 1);
}

#[test]
fn invalid_signature_recheck_removes_retained_intent_without_fee_or_nonce() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let app = fixture.initialized(&dir.path().join("db"));
    let mut signed = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![OrdinaryAction::Data {
            data: "rechecked".into(),
        }],
        1000,
    );
    let before = data(&app);
    assert_admitted(app.check_tx(&fixture.ordinary_wire(&signed)));
    signed.signature[0] ^= 1;
    assert_ne!(
        app.recheck_ordinary_admission(&fixture.ordinary_wire(&signed))
            .code,
        0
    );
    assert_eq!(app.ordinary_admission_count().unwrap(), 0);
    assert_eq!(data(&app), before);
    assert_mirror(&app, &fixture.active, 0);
}

#[test]
fn receipt_retention_limit_rejects_next_nonce_before_fee_acceptance() {
    let mut fixture = Fixture::new();
    fixture.config.ordinary.as_mut().unwrap().max_receipts = 1;
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    successful_ordinary(
        &mut app,
        &fixture,
        &fixture.active,
        1,
        vec![OrdinaryAction::Data {
            data: "one retained receipt".into(),
        }],
    );
    let balance_before = balance(&app, &fixture.active.address());
    let signed = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![send(fixture.payer.id(), 100)],
        1000,
    );
    let raw = fixture.ordinary_wire(&signed);
    let rejected = app.check_tx(&raw);
    assert_ne!(rejected.code, 0);
    assert!(rejected.gas_used > 0);
    let result = commit(&mut app, 2, vec![raw]);
    assert_ne!(result.tx_results[0].code, 0);
    assert!(result.tx_results[0].gas_used > 0);
    assert_eq!(balance(&app, &fixture.active.address()), balance_before);
    assert_eq!(balance(&app, &fixture.payer.address()), INITIAL_DRT);
    assert_mirror(&app, &fixture.active, 1);
}

#[test]
fn ordinary_state_byte_exhaustion_rejects_whole_block_without_partial_writes() {
    let mut fixture = Fixture::new();
    let initial_book = fixture.config.recovery.as_ref().unwrap();
    let native_nonces = initial_book
        .accounts
        .values()
        .map(|a| (a.address.clone(), a.recovery.spending_nonce))
        .collect();
    let initial = crate::ordinary_state::OrdinaryState::genesis(
        fixture.config.ordinary.as_ref().unwrap().clone(),
        fixture.config.lifecycle.as_ref().unwrap(),
        initial_book,
        &native_nonces,
    )
    .unwrap();
    // Permit fresh genesis plus a small height change, but not a retained paid receipt.
    fixture.config.ordinary.as_mut().unwrap().max_state_bytes =
        initial.encode().unwrap().len() as u64 + 32;
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let signed = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![send(fixture.payer.id(), 100)],
        1000,
    );
    let before = data(&app);
    assert!(app
        .finalize_block(block(1, vec![fixture.ordinary_wire(&signed)]))
        .is_err());
    assert_eq!(data(&app), before);
    assert_eq!(balance(&app, &fixture.active.address()), INITIAL_DRT);
    assert_eq!(balance(&app, &fixture.payer.address()), INITIAL_DRT);
    assert_mirror(&app, &fixture.active, 0);
}

#[test]
fn combined_genesis_rejects_reward_principal_without_registered_stable_owner() {
    let mut fixture = Fixture::new();
    let orphan = Key::new();
    let mut genesis: serde_json::Value = serde_json::from_slice(&fixture.genesis).unwrap();
    genesis["accounts"].as_array_mut().unwrap().push(serde_json::json!({
        "address": orphan.address(), "balances": {"udgt":"1000", "udrt":INITIAL_DRT.to_string()},
        "vesting":{"kind":"unlocked"}
    }));
    genesis["staking"]["delegations"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "delegator":orphan.address(), "amount_udgt":"100"
        }));
    genesis["reward_v2"]["positions"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "owner":orphan.address(), "validator":"validator-one", "amount_udgt":"100"
        }));
    fixture.genesis = serde_json::to_vec(&genesis).unwrap();
    let digest: [u8; 32] = Sha256::digest(&fixture.genesis).into();
    fixture.config.app_state_sha256 = hex::encode(digest);
    fixture.config.validators[0].power = 200;
    for account in fixture
        .config
        .recovery
        .as_mut()
        .unwrap()
        .accounts
        .values_mut()
    {
        account.recovery.domain.genesis_digest = digest;
    }
    assert!(!fixture
        .config
        .recovery
        .as_ref()
        .unwrap()
        .accounts
        .contains_key(&hex::encode(orphan.id())));
    assert!(!fixture
        .config
        .recovery
        .as_ref()
        .unwrap()
        .origins
        .contains_key(&hex::encode(orphan.id())));
    fixture.config.validate().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let result = ConsensusApplication::open(
        &dir.path().join("db"),
        fixture.config.clone(),
        fixture.genesis.clone(),
    )
    .and_then(|mut app| app.init_chain(CHAIN, 1, &fixture.genesis, &fixture.config.validators));
    let error = match result {
        Ok(_) => panic!("unregistered reward principal must prevent activation"),
        Err(error) => error,
    };
    assert!(
        format!("{error:#}").contains("Ordinary principal owner lacks registered stable account"),
        "{error:#}"
    );
}

fn assert_admitted(result: TxResult) {
    assert_eq!(result.code, 0, "{result:?}");
}

#[test]
fn signed_operator_exit_cannot_release_protected_delegator_principal_or_charge_fee() {
    let mut fixture = Fixture::new();
    let (other_validator, _) = ml_dsa_65::KG::try_keygen_with_rng(&mut rand_core::OsRng).unwrap();
    let mut genesis: serde_json::Value = serde_json::from_slice(&fixture.genesis).unwrap();
    // Two accounts suffice: operator sponsors recovery; delegator also owns the second validator.
    genesis["accounts"]
        .as_array_mut()
        .unwrap()
        .retain(|a| a["address"].as_str() != Some(fixture.payer.address().as_str()));
    fixture
        .config
        .recovery
        .as_mut()
        .unwrap()
        .accounts
        .remove(&hex::encode(fixture.payer.id()));
    fixture
        .config
        .recovery
        .as_mut()
        .unwrap()
        .origins
        .remove(&hex::encode(fixture.payer.id()));
    genesis["staking"]["delegations"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"delegator":fixture.secondary.address(),"amount_udgt":"200"}));
    genesis["reward_v2"]["validators"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"address":"validator-two","active":true,"jailed":false}));
    genesis["reward_v2"]["positions"].as_array_mut().unwrap().extend([
        serde_json::json!({"owner":fixture.secondary.address(),"validator":"validator-one","amount_udgt":"100"}),
        serde_json::json!({"owner":fixture.secondary.address(),"validator":"validator-two","amount_udgt":"100"}),
    ]);
    fixture.genesis = serde_json::to_vec(&genesis).unwrap();
    let digest: [u8; 32] = Sha256::digest(&fixture.genesis).into();
    fixture.config.app_state_sha256 = hex::encode(digest);
    fixture.config.validators[0].power = 200;
    fixture.config.validators.push(ValidatorConfig {
        pubkey_type: "ml_dsa_65".into(),
        pubkey_base64: B64.encode(other_validator.into_bytes()),
        power: 100,
        reward_address: "validator-two".into(),
    });
    fixture
        .config
        .lifecycle
        .as_mut()
        .unwrap()
        .approved_operators
        .insert("validator-two".into(), fixture.secondary.address());
    fixture
        .config
        .ordinary
        .as_mut()
        .unwrap()
        .fee_profile
        .validator_proof_profile_digest =
        crate::ordinary_state::validator_profile_digest(fixture.config.lifecycle.as_ref().unwrap())
            .unwrap();
    for account in fixture
        .config
        .recovery
        .as_mut()
        .unwrap()
        .accounts
        .values_mut()
    {
        account.recovery.domain.genesis_digest = digest;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let enroll_operation = fixture.operation(
        fixture.secondary.id(),
        ActionKind::Enroll {
            active: ActiveAuthorization {
                generation: 0,
                nonce: 0,
            },
            policy: fixture.policy(),
        },
        40,
    );
    let enroll = fixture.signed(
        enroll_operation,
        &[&fixture.secondary],
        &fixture.guardians.iter().collect::<Vec<_>>(),
    );
    assert_eq!(
        commit(
            &mut app,
            1,
            vec![wire(&fixture.sponsored_by(
                enroll,
                fixture.active.id(),
                &fixture.active,
                0,
                0,
                GAS_LIMIT
            ))]
        )
        .tx_results[0]
            .code,
        0
    );
    let start_operation = fixture.operation(
        fixture.secondary.id(),
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
    let start = fixture.signed(
        start_operation,
        &[&fixture.guardians[0], &fixture.guardians[1]],
        &[&fixture.replacement],
    );
    assert_eq!(
        commit(
            &mut app,
            2,
            vec![wire(&fixture.sponsored_by(
                start,
                fixture.active.id(),
                &fixture.active,
                0,
                1,
                GAS_LIMIT
            ))]
        )
        .tx_results[0]
            .code,
        0
    );
    let before_fee_balance = balance(&app, &fixture.active.address());
    let before_operator = current(&app, &fixture.active);
    let before_delegator = current(&app, &fixture.secondary);
    let before_set = lifecycle_state(&app).validator_set(4).unwrap();
    let signed = fixture.ordinary(
        &before_operator,
        &fixture.active,
        vec![OrdinaryAction::ValidatorExit {
            validator_id: "validator-one".into(),
        }],
        1000,
    );
    let result = commit(&mut app, 3, vec![fixture.ordinary_wire(&signed)]);
    assert_ne!(result.tx_results[0].code, 0);
    assert!(
        result.tx_results[0]
            .log
            .to_lowercase()
            .contains("protected"),
        "{:?}",
        result.tx_results
    );
    assert_eq!(balance(&app, &fixture.active.address()), before_fee_balance);
    assert_mirror(&app, &fixture.active, before_operator.spending_nonce);
    assert_mirror(&app, &fixture.secondary, before_delegator.spending_nonce);
    assert_eq!(
        current(&app, &fixture.secondary).status,
        RecoveryStatus::PendingRecovery
    );
    assert_eq!(lifecycle_state(&app).validator_set(5).unwrap(), before_set);
    assert!(lifecycle_state(&app).unbonding.is_empty());
    assert_eq!(asset_balance(&app, &fixture.active.address(), "udgt"), 900);
    assert_eq!(
        asset_balance(&app, &fixture.secondary.address(), "udgt"),
        800
    );
}

#[test]
fn invalid_validator_proof_consumes_shared_signature_and_gas_work_without_fee() {
    for shared_signatures in [1, 2] {
        let mut fixture = Fixture::new();
        fixture
            .config
            .lifecycle
            .as_mut()
            .unwrap()
            .approved_operators
            .insert("validator-two".into(), fixture.secondary.address());
        fixture
            .config
            .ordinary
            .as_mut()
            .unwrap()
            .fee_profile
            .validator_proof_profile_digest = crate::ordinary_state::validator_profile_digest(
            fixture.config.lifecycle.as_ref().unwrap(),
        )
        .unwrap();
        fixture
            .config
            .ordinary
            .as_mut()
            .unwrap()
            .fee_profile
            .max_block_signature_checks = shared_signatures;
        let key = ValidatorProofKey::new();
        let mut invalid_proof = key.proof("register", "validator-two", &fixture.secondary, 0, 100);
        invalid_proof[0] ^= 1;
        let dir = tempfile::tempdir().unwrap();
        let mut app = fixture.initialized(&dir.path().join("db"));
        let signed = fixture.ordinary(
            &current(&app, &fixture.secondary),
            &fixture.secondary,
            vec![OrdinaryAction::ValidatorRegister {
                validator_id: "validator-two".into(),
                consensus_key: key.public,
                proof: invalid_proof,
                proof_expiry_height: 40,
                amount_udgt: 100,
            }],
            1000,
        );
        let raw = fixture.ordinary_wire(&signed);
        let before = data(&app);
        let original_set = lifecycle_state(&app).validator_set(2).unwrap();
        let checked = app.check_tx(&raw);
        assert_ne!(checked.code, 0);
        assert_eq!(data(&app), before);
        if shared_signatures == 1 {
            assert!(!app.process_proposal(block(1, vec![raw.clone()])).unwrap());
            assert!(app.finalize_block(block(1, vec![raw])).is_err());
            assert_eq!(data(&app), before);
        } else {
            assert!(checked.gas_used >= 9, "{:?}", checked); // Overhead2 + account signature3 + proof4.
            assert!(app.process_proposal(block(1, vec![raw.clone()])).unwrap());
            let result = commit(&mut app, 1, vec![raw]);
            assert_ne!(result.tx_results[0].code, 0);
            assert!(
                result.tx_results[0].gas_used >= 9,
                "{:?}",
                result.tx_results
            );
        }
        assert_eq!(balance(&app, &fixture.secondary.address()), INITIAL_DRT);
        assert_eq!(
            asset_balance(&app, &fixture.secondary.address(), "udgt"),
            1000
        );
        assert_mirror(&app, &fixture.secondary, 0);
        assert_eq!(
            lifecycle_state(&app)
                .validator_set(app.info().unwrap().height + 2)
                .unwrap(),
            original_set
        );
    }
}

#[test]
fn ordinary_queries_expose_only_committed_profiles_and_receipts_across_restart() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = fixture.initialized(&path);
    let profile_before = app.query_ordinary_profile().unwrap();
    assert_eq!(profile_before["enabled"], true);
    assert_eq!(profile_before["context"]["height"], "0");
    let signed = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![send(fixture.payer.id(), 100)],
        1000,
    );
    let id = ordinary_id(&fixture, &signed);
    assert!(app.query_ordinary_receipt(&id).unwrap().is_null());
    let result = app
        .finalize_block(block(1, vec![fixture.ordinary_wire(&signed)]))
        .unwrap();
    assert_eq!(result.tx_results[0].code, 0);
    assert!(app.query_ordinary_receipt(&id).unwrap().is_null());
    assert_eq!(app.query_ordinary_profile().unwrap(), profile_before);
    app.commit().unwrap();
    let receipt = app.query_ordinary_receipt(&id).unwrap();
    assert!(!receipt.is_null());
    assert_eq!(receipt["outcome"], "success");
    assert_eq!(receipt["transaction_id"], id);
    assert_eq!(
        receipt["charge"],
        (result.tx_results[0].gas_used.max(10) as u128 * 2).to_string()
    );
    assert_eq!(receipt["reserved_cap"], signed.body.maximum_fee.to_string());
    assert_eq!(
        receipt["gas_used"],
        result.tx_results[0].gas_used.to_string()
    );
    let committed_profile = app.query_ordinary_profile().unwrap();
    assert_eq!(committed_profile["context"]["height"], "1");
    assert_eq!(committed_profile["config"], profile_before["config"]);
    assert_eq!(receipt["context"], committed_profile["context"]);
    assert_eq!(
        app.query_ordinary_account(&hex::encode(fixture.active.id()))
            .unwrap()["context"],
        committed_profile["context"]
    );
    drop(app);
    let mut app = fixture.open(&path);
    assert_eq!(app.query_ordinary_receipt(&id).unwrap(), receipt);
    assert_eq!(app.query_ordinary_profile().unwrap(), committed_profile);
    let failure = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![
            send(fixture.payer.id(), 100),
            OrdinaryAction::Data {
                data: "uncommitted failure".into(),
            },
        ],
        25,
    );
    let failure_id = ordinary_id(&fixture, &failure);
    let failed = app
        .finalize_block(block(2, vec![fixture.ordinary_wire(&failure)]))
        .unwrap();
    assert_ne!(failed.tx_results[0].code, 0);
    assert!(app.query_ordinary_receipt(&failure_id).unwrap().is_null());
    assert_eq!(app.query_ordinary_receipt(&id).unwrap(), receipt);
    assert_eq!(app.query_ordinary_profile().unwrap(), committed_profile);
    drop(app);
    let app = fixture.open(&path);
    assert!(app.query_ordinary_receipt(&failure_id).unwrap().is_null());
    assert_eq!(app.query_ordinary_receipt(&id).unwrap(), receipt);
    assert_eq!(app.query_ordinary_profile().unwrap(), committed_profile);
}

fn assert_query_context(
    app: &ConsensusApplication,
    fixture: &Fixture,
    context: &serde_json::Value,
) {
    let head = app.info().unwrap();
    assert_eq!(
        *context,
        serde_json::json!({
            "chain_id": CHAIN,
            "genesis_digest": fixture.config.app_state_sha256,
            "height": head.height.to_string(),
            "app_hash": head.app_hash,
        })
    );
}

#[test]
fn ordinary_account_query_tracks_only_committed_nonce_protection_and_replacement_key() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = fixture.initialized(&path);
    let id = hex::encode(fixture.active.id());
    let initial = app.query_ordinary_account(&id).unwrap();
    assert_eq!(initial["version"], 1);
    assert_eq!(initial["account_id"], id);
    assert_eq!(initial["address"], fixture.active.address());
    assert_eq!(initial["authorization_generation"], "0");
    assert_eq!(initial["spending_nonce"], "0");
    assert_eq!(initial["protected"], false);
    assert_eq!(
        initial["current_key"],
        serde_json::to_value(&fixture.active.identity).unwrap()
    );
    assert_eq!(
        initial["domain"],
        serde_json::json!({"network":3,"chain_id":CHAIN,"genesis_digest":fixture.config.app_state_sha256,"account_id":id})
    );
    assert_eq!(
        initial["profile_digest"],
        hex::encode(
            ordinary_fees::profile_digest(&fixture.config.ordinary.as_ref().unwrap().fee_profile)
                .unwrap()
        )
    );
    assert_query_context(&app, &fixture, &initial["context"]);
    assert_eq!(
        initial["context"],
        app.query_ordinary_profile().unwrap()["context"]
    );
    let enrolled = app
        .finalize_block(block(
            1,
            vec![wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT))],
        ))
        .unwrap();
    assert_eq!(enrolled.tx_results[0].code, 0);
    assert_eq!(app.query_ordinary_account(&id).unwrap(), initial);
    app.commit().unwrap();
    let enrolled = app.query_ordinary_account(&id).unwrap();
    assert_eq!(enrolled["authorization_generation"], "1");
    assert_eq!(enrolled["spending_nonce"], "1");
    assert_eq!(enrolled["current_key"], initial["current_key"]);
    assert_eq!(enrolled["protected"], false);
    let started = app
        .finalize_block(block(
            2,
            vec![wire(&fixture.sponsored(fixture.start(), 1, GAS_LIMIT))],
        ))
        .unwrap();
    assert_eq!(started.tx_results[0].code, 0);
    assert_eq!(app.query_ordinary_account(&id).unwrap(), enrolled);
    app.commit().unwrap();
    let protected = app.query_ordinary_account(&id).unwrap();
    assert_eq!(protected["authorization_generation"], "2");
    assert_eq!(protected["spending_nonce"], "1");
    assert_eq!(protected["protected"], true);
    assert_eq!(protected["current_key"], initial["current_key"]);
    assert_query_context(&app, &fixture, &protected["context"]);
    drop(app);
    let mut app = fixture.open(&path);
    assert_eq!(app.query_ordinary_account(&id).unwrap(), protected);
    commit(&mut app, 3, vec![]);
    let before_final = app.query_ordinary_account(&id).unwrap();
    let mut replacement_state = current(&app, &fixture.active);
    replacement_state.active_generation += 1;
    replacement_state.active_key = fixture.replacement.identity.clone();
    let send = fixture.ordinary(
        &replacement_state,
        &fixture.replacement,
        vec![send(fixture.secondary.id(), 100)],
        1000,
    );
    let finalized = app
        .finalize_block(block(
            4,
            vec![
                wire(&fixture.sponsored(fixture.finalize(), 2, GAS_LIMIT)),
                fixture.ordinary_wire(&send),
            ],
        ))
        .unwrap();
    assert!(
        finalized.tx_results.iter().all(|r| r.code == 0),
        "{:?}",
        finalized.tx_results
    );
    assert_eq!(app.query_ordinary_account(&id).unwrap(), before_final);
    app.commit().unwrap();
    let replaced = app.query_ordinary_account(&id).unwrap();
    assert_eq!(replaced["account_id"], id);
    assert_eq!(replaced["address"], initial["address"]);
    assert_eq!(replaced["authorization_generation"], "3");
    assert_eq!(replaced["spending_nonce"], "2");
    assert_eq!(replaced["protected"], false);
    assert_ne!(replaced["current_key"], initial["current_key"]);
    assert_eq!(
        replaced["current_key"],
        serde_json::to_value(&fixture.replacement.identity).unwrap()
    );
    assert_query_context(&app, &fixture, &replaced["context"]);
    assert_eq!(
        replaced["context"],
        app.query_ordinary_profile().unwrap()["context"]
    );
    drop(app);
    let app = fixture.open(&path);
    assert_eq!(app.query_ordinary_account(&id).unwrap(), replaced);
}

#[test]
fn ordinary_public_queries_reject_malformed_ids_and_return_null_for_absent_accounts() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let app = fixture.initialized(&dir.path().join("db"));
    let before = data(&app);
    for id in [
        String::new(),
        "0".into(),
        "00".repeat(31),
        "AB".repeat(32),
        "g".repeat(64),
        "00".repeat(33),
    ] {
        assert!(app.query_ordinary_account(&id).is_err(), "{id}");
        assert!(app.query_ordinary_receipt(&id).is_err(), "{id}");
    }
    let missing = hex::encode(Key::new().id());
    assert!(app.query_ordinary_account(&missing).unwrap().is_null());
    assert!(app.query_ordinary_receipt(&missing).unwrap().is_null());
    let account = app
        .query_ordinary_account(&hex::encode(fixture.payer.id()))
        .unwrap();
    let profile = app.query_ordinary_profile().unwrap();
    assert_query_context(&app, &fixture, &account["context"]);
    assert_eq!(account["context"], profile["context"]);
    assert_eq!(data(&app), before);
}

#[test]
fn committed_public_receipt_preserves_fee_and_cap_above_u64_as_decimal_strings() {
    let mut fixture = Fixture::new();
    let price = u64::MAX;
    let cap = u128::from(price) * 1000 + 17;
    let initial_balance = cap + 1000;
    fixture
        .config
        .ordinary
        .as_mut()
        .unwrap()
        .fee_profile
        .gas_price = price;
    fixture
        .config
        .ordinary
        .as_mut()
        .unwrap()
        .fee_profile
        .max_fee_cap = cap;
    let mut genesis: serde_json::Value = serde_json::from_slice(&fixture.genesis).unwrap();
    let actor = fixture.active.address();
    for account in genesis["accounts"].as_array_mut().unwrap() {
        if account["address"] == actor {
            account["balances"]["udrt"] = serde_json::json!(initial_balance.to_string());
        }
    }
    fixture.genesis = serde_json::to_vec(&genesis).unwrap();
    let digest: [u8; 32] = Sha256::digest(&fixture.genesis).into();
    fixture.config.app_state_sha256 = hex::encode(digest);
    for account in fixture
        .config
        .recovery
        .as_mut()
        .unwrap()
        .accounts
        .values_mut()
    {
        account.recovery.domain.genesis_digest = digest;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = fixture.initialized(&path);
    let signed = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![OrdinaryAction::Data {
            data: "wide exact receipt".into(),
        }],
        1000,
    );
    let id = ordinary_id(&fixture, &signed);
    let result = commit(&mut app, 1, vec![fixture.ordinary_wire(&signed)]);
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    let expected_charge =
        u128::try_from(result.tx_results[0].gas_used.max(10)).unwrap() * u128::from(price);
    assert!(expected_charge > u128::from(u64::MAX));
    let receipt = app.query_ordinary_receipt(&id).unwrap();
    assert_eq!(receipt["charge"], expected_charge.to_string());
    assert_eq!(receipt["reserved_cap"], cap.to_string());
    assert_eq!(receipt["released_cap"], (cap - expected_charge).to_string());
    assert_eq!(balance(&app, &actor), initial_balance - expected_charge);
    let text = serde_json::to_string(&receipt).unwrap();
    let decoded: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(decoded, receipt);
    assert_eq!(
        decoded["charge"].as_str().unwrap().parse::<u128>().unwrap(),
        expected_charge
    );
    assert_eq!(
        decoded["reserved_cap"]
            .as_str()
            .unwrap()
            .parse::<u128>()
            .unwrap(),
        cap
    );
    drop(app);
    let app = fixture.open(&path);
    assert_eq!(app.query_ordinary_receipt(&id).unwrap(), receipt);
}

#[path = "ordinary_client_fixture_tests.rs"]
mod client_fixture_tests;

#[path = "emergency_consensus_tests.rs"]
mod emergency_tests;

#[path = "upgrade_consensus_tests.rs"]
mod upgrade_tests;

#[path = "release_handover_consensus_tests.rs"]
mod release_handover_tests;

#[path = "startup_preflight_tests.rs"]
mod startup_preflight_tests;

/// Receipts expire with their transactions, so max_receipts bounds the retained
/// window instead of the chain's lifetime usage.
#[test]
fn expired_receipts_are_pruned_and_free_retention_capacity() {
    let mut fixture = Fixture::new();
    let ordinary = fixture.config.ordinary.as_mut().unwrap();
    ordinary.max_receipts = 1;
    ordinary.fee_profile.limits.max_expiry_lifetime = 3;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut app = fixture.initialized(&path);
    let data_tx = |app: &ConsensusApplication, expiry: u64, data: &str| {
        let mut signed = fixture.ordinary(
            &current(app, &fixture.active),
            &fixture.active,
            vec![OrdinaryAction::Data { data: data.into() }],
            1000,
        );
        signed.body.expiry_height = expiry;
        fixture.ordinary_wire(&fixture.resign(signed.body, &fixture.active))
    };

    let first = data_tx(&app, 3, "first");
    let result = commit(&mut app, 1, vec![first.clone()]);
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    // Inside the window (1 + 3 > 2) the retained receipt fills capacity.
    let second = data_tx(&app, 5, "second");
    let blocked = commit(&mut app, 2, vec![second]);
    assert_ne!(blocked.tx_results[0].code, 0);
    commit(&mut app, 3, vec![]);
    // At height 4 the first receipt is pruned (1 + 3 <= 4) and capacity returns.
    let third = data_tx(&app, 6, "third");
    let accepted = commit(&mut app, 4, vec![third]);
    assert_eq!(accepted.tx_results[0].code, 0, "{:?}", accepted.tx_results);
    assert_mirror(&app, &fixture.active, 2);
    // The pruned transaction cannot be replayed: it expired at height 3.
    assert_ne!(app.check_tx(&first).code, 0);

    // The complete history check accepts heights whose receipts were pruned.
    drop(app);
    let app = fixture.open(&path);
    assert_eq!(app.info().unwrap().height, 4);
}

/// Native account keys in the pending block's writes.
fn pending_account_writes(app: &ConsensusApplication) -> BTreeSet<String> {
    app.pending
        .as_ref()
        .unwrap()
        .writes
        .keys()
        .filter_map(|key| std::str::from_utf8(key).ok())
        .filter_map(|key| {
            key.strip_prefix("acct:balances:")
                .or_else(|| key.strip_prefix("acct:nonce:"))
        })
        .map(str::to_owned)
        .collect()
}

#[test]
fn blocks_write_native_accounts_only_for_touched_accounts() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let untouched = fixture.secondary.address();

    // Sponsored recovery: its target and sponsor.
    let enroll = wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT));
    let result = app.finalize_block(block(1, vec![enroll])).unwrap();
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    assert_eq!(
        pending_account_writes(&app),
        BTreeSet::from([fixture.active.address(), fixture.payer.address()])
    );
    app.commit().unwrap();
    assert_mirror(&app, &fixture.active, 1);

    // Ordinary send: its sender and recipient.
    let tx = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![send(fixture.payer.id(), 100)],
        1000,
    );
    let result = app
        .finalize_block(block(2, vec![fixture.ordinary_wire(&tx)]))
        .unwrap();
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    let written = pending_account_writes(&app);
    assert_eq!(
        written,
        BTreeSet::from([fixture.active.address(), fixture.payer.address()])
    );
    assert!(!written.contains(&untouched));
    app.commit().unwrap();
    assert_mirror(&app, &fixture.active, 2);
    assert_mirror(&app, &fixture.secondary, 0);

    // An empty block writes no native account.
    app.finalize_block(block(3, vec![])).unwrap();
    assert!(pending_account_writes(&app).is_empty());
    app.commit().unwrap();
}

#[test]
fn outside_write_to_an_untouched_mirror_is_caught_by_the_complete_check() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    commit(&mut app, 1, vec![]);
    app.storage
        .db
        .put(
            format!("acct:nonce:{}", fixture.secondary.address()),
            bincode::serialize(&7u64).unwrap(),
        )
        .unwrap();
    let error = app.finalize_block(block(2, vec![])).unwrap_err();
    assert!(
        format!("{error:#}").contains("nonce"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn origins_are_stored_per_account_and_not_in_the_ordinary_state() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let stored = book(&app);
    assert_eq!(stored.origins.len(), 3);
    assert_eq!(stored.origins, fixture.config.recovery.as_ref().unwrap().origins);
    let key = format!("recovery:v2:origin:{}", hex::encode(fixture.active.id()));
    let raw = app.storage.db.get(&key).unwrap().expect("per-account origin entry");
    assert_eq!(
        serde_json::from_slice::<KeyIdentity>(&raw).unwrap(),
        fixture.active.identity
    );
    let state = app.storage.db.get(ORDINARY_STATE_KEY).unwrap().unwrap();
    assert!(!String::from_utf8(state).unwrap().contains("origins"));

    // An origin that differs from the committed configuration is rejected.
    let mut changed = fixture.active.identity.clone();
    changed.public_key[0] ^= 1;
    app.storage
        .db
        .put(&key, serde_json::to_vec(&changed).unwrap())
        .unwrap();
    let error = app.finalize_block(block(1, vec![])).unwrap_err();
    assert!(format!("{error:#}").contains("origin or configuration differs"), "{error:#}");
}

const CREATION_FEE: u128 = 1_000;

fn burned(app: &ConsensusApplication) -> u128 {
    crate::supply::inspect_native(&app.storage).unwrap().drt.burned
}
/// Fees a block's ordinary transactions paid (fixture price 2, minimum gas
/// 10). Every fee is burned (fees v1).
fn charged(result: &FinalizeResult) -> u128 {
    result
        .tx_results
        .iter()
        .filter(|r| r.code == 0 || r.code == 3)
        .map(|r| r.gas_used.max(10) as u128 * 2)
        .sum()
}

#[test]
fn send_to_a_new_address_creates_the_account_and_burns_the_fee_once() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let fresh = Key::new();
    let other = Key::new();
    assert!(app
        .storage
        .db
        .get(format!("acct:balances:{}", fresh.address()))
        .unwrap()
        .is_none());

    let tx = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![send(fresh.id(), 100)],
        1000,
    );
    let raw = fixture.ordinary_wire(&tx);
    assert_admitted(app.check_tx(&raw));
    let result = commit(&mut app, 1, vec![raw]);
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    let gas_fee = result.tx_results[0].gas_used.max(10) as u128 * 2;
    assert_eq!(balance(&app, &fresh.address()), 100);
    assert_eq!(ordinary_nonce(&app, &fresh.address()), 0);
    assert!(!book(&app).accounts.contains_key(&hex::encode(fresh.id())));
    assert_eq!(
        balance(&app, &fixture.active.address()),
        INITIAL_DRT - 100 - gas_fee - CREATION_FEE
    );
    assert_eq!(burned(&app), CREATION_FEE + gas_fee);

    // An existing recipient costs nothing extra; two transfers to one new
    // recipient in the same transaction burn one fee.
    let tx = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![send(fresh.id(), 50), send(other.id(), 7), send(other.id(), 3)],
        1000,
    );
    let result = commit(&mut app, 2, vec![fixture.ordinary_wire(&tx)]);
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    assert_eq!(balance(&app, &fresh.address()), 150);
    assert_eq!(balance(&app, &other.address()), 10);
    assert_eq!(burned(&app), 2 * CREATION_FEE + gas_fee + charged(&result));

    // A fresh process passes the complete check.
    drop(app);
    let reopened = fixture.open(&dir.path().join("db"));
    verify_recovery(&reopened.storage).unwrap();
}

#[test]
fn validators_are_paid_by_power_and_every_fee_is_burned() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    for height in 1..=3 {
        commit(&mut app, height, vec![]);
    }
    // The only validator's operator is owed the whole validator share.
    let owner = fixture.active.address();
    let rewards = reward_state(&app);
    let supply = crate::supply::inspect_native(&app.storage).unwrap();
    let owed = rewards.validator_payouts.unpaid[&owner];
    assert!(owed > 0);
    assert_eq!(
        owed + rewards.validator_payouts.reserve,
        supply.drt.pools["validator_rewards"]
    );
    let before = balance(&app, &owner);
    let tx = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![OrdinaryAction::RewardClaim],
        1000,
    );
    let result = commit(&mut app, 4, vec![fixture.ordinary_wire(&tx)]);
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    let after = reward_state(&app);
    let staking = after.total_claimed - rewards.total_claimed;
    let validator = after.validator_payouts.total_claimed;
    // Block 4's share was allocated at block start, before the claim.
    assert!(validator > owed);
    assert!(!after.validator_payouts.unpaid.contains_key(&owner));
    let fee = charged(&result);
    assert_eq!(balance(&app, &owner), before + staking + validator - fee);
    let supply = crate::supply::inspect_native(&app.storage).unwrap();
    assert_eq!(supply.drt.withheld_fees, 0);
    assert_eq!(supply.drt.burned, fee);
    assert_eq!(
        supply.drt.pools["validator_rewards"],
        after.validator_payouts.reserve
    );
    drop(app);
    let reopened = fixture.open(&dir.path().join("db"));
    verify_recovery(&reopened.storage).unwrap();
    // A committed state that still withholds a fee is refused, even when
    // the amount is conserved.
    let moved = 5u128;
    let mut balances: BTreeMap<String, u128> = bincode::deserialize(
        &reopened
            .storage
            .db
            .get(format!("acct:balances:{owner}"))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    *balances.get_mut("udrt").unwrap() -= moved;
    reopened
        .storage
        .db
        .put(format!("acct:balances:{owner}"), bincode::serialize(&balances).unwrap())
        .unwrap();
    // Keep the running account totals consistent, so the fee rule is reached.
    let key = crate::supply::ACCOUNT_TOTALS_KEY;
    let mut totals = crate::supply::AccountTotals::decode(
        &reopened.storage.db.get(key).unwrap().unwrap(),
    )
    .unwrap();
    totals.udrt -= moved;
    reopened.storage.db.put(key, totals.encode().unwrap()).unwrap();
    reopened
        .storage
        .db
        .put(crate::settlement::FEE_KEY, bincode::serialize(&moved).unwrap())
        .unwrap();
    let error = verify_recovery(&reopened.storage).unwrap_err().to_string();
    assert!(error.contains("unburned"), "{error}");
}

/// Without a penalty profile, evidence of either kind is accepted and
/// recorded with no stake or set effect (P01, 27 September 2026); the chain
/// keeps running.
#[test]
fn evidence_without_penalty_rules_is_recorded_and_never_stops_the_chain() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    commit(&mut app, 1, vec![]);
    commit(&mut app, 2, vec![]);
    let fact = |kind: &str, power: i64| EvidenceFact {
        kind: kind.into(),
        validator_address: "ab".repeat(20),
        height: 1,
        time_seconds: 10,
        time_nanos: 0,
        power,
        total_power: 100,
    };
    // A power the chain never had is still recorded: no historical check
    // can stop a block CometBFT has already verified.
    let facts = vec![fact("duplicate_vote", 100), fact("light_client_attack", 7)];
    let lifecycle_before = lifecycle_state(&app);
    let mut input = block(3, vec![]);
    input.misbehavior = facts.clone();
    assert!(app.process_proposal(input.clone()).unwrap());
    app.finalize_block(input).unwrap();
    app.commit().unwrap();
    for (index, fact) in facts.iter().enumerate() {
        let key = crate::settlement::evidence_key(3, index).unwrap();
        assert_eq!(
            app.storage.db.get(&key).unwrap(),
            Some(crate::settlement::encode_evidence(fact).unwrap())
        );
    }
    let lifecycle_after = lifecycle_state(&app);
    assert_eq!(lifecycle_after.effective, lifecycle_before.effective);
    commit(&mut app, 4, vec![]);
    drop(app);
    let reopened = fixture.open(&dir.path().join("db"));
    verify_recovery(&reopened.storage).unwrap();
    // A changed or extra record is refused by the complete check.
    let key = crate::settlement::evidence_key(3, 1).unwrap();
    let changed = crate::settlement::encode_evidence(&fact("light_client_attack", 8)).unwrap();
    reopened.storage.db.put(&key, &changed).unwrap();
    assert!(verify_recovery(&reopened.storage).is_err());
    let original = crate::settlement::encode_evidence(&facts[1]).unwrap();
    reopened.storage.db.put(&key, &original).unwrap();
    let extra = crate::settlement::evidence_key(4, 0).unwrap();
    reopened.storage.db.put(&extra, &original).unwrap();
    assert!(verify_recovery(&reopened.storage).is_err());
    // An unknown kind is still refused.
    let mut input = block(5, vec![]);
    input.misbehavior = vec![fact("surround_vote", 1)];
    reopened.storage.db.delete(&extra).unwrap();
    let mut app = reopened;
    assert!(!app.process_proposal(input).unwrap());
}

#[test]
fn failed_transfer_to_a_new_address_creates_and_burns_nothing() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let fresh = Key::new();
    // Accepted, then out of gas at the second action: the transfer rolls back.
    let tx = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![
            send(fresh.id(), 100),
            OrdinaryAction::Data {
                data: "not committed".into(),
            },
        ],
        25,
    );
    let result = commit(&mut app, 1, vec![fixture.ordinary_wire(&tx)]);
    assert_ne!(result.tx_results[0].code, 0);
    assert_eq!(result.tx_results[0].gas_used, 25);
    assert_eq!(balance(&app, &fixture.active.address()), INITIAL_DRT - 50);
    assert!(app
        .storage
        .db
        .get(format!("acct:balances:{}", fresh.address()))
        .unwrap()
        .is_none());
    // No creation fee; only the transaction fee is burned.
    assert_eq!(burned(&app), 50);
}

#[test]
fn creation_fee_is_reserved_before_acceptance() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let fresh = Key::new();
    // The amount and fee cap fit the balance; adding the creation fee does not.
    let amount = INITIAL_DRT - 2_000 - 500;
    let to_new = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![send(fresh.id(), amount)],
        1000,
    );
    let raw = fixture.ordinary_wire(&to_new);
    assert_ne!(app.check_tx(&raw).code, 0);
    let result = commit(&mut app, 1, vec![raw]);
    assert_ne!(result.tx_results[0].code, 0);
    assert!(result.tx_results[0].log.contains("Insufficient"), "{:?}", result.tx_results);
    assert_eq!(balance(&app, &fixture.active.address()), INITIAL_DRT);
    assert!(app
        .storage
        .db
        .get(format!("acct:balances:{}", fresh.address()))
        .unwrap()
        .is_none());
    assert_mirror(&app, &fixture.active, 0);
    // The same amount to an existing account is accepted.
    let to_existing = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![send(fixture.payer.id(), amount)],
        1000,
    );
    let result = commit(&mut app, 2, vec![fixture.ordinary_wire(&to_existing)]);
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
}

#[test]
fn complete_check_rejects_a_created_account_with_a_nonzero_nonce() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let fresh = Key::new();
    let tx = fixture.ordinary(
        &current(&app, &fixture.active),
        &fixture.active,
        vec![send(fresh.id(), 100)],
        1000,
    );
    let result = commit(&mut app, 1, vec![fixture.ordinary_wire(&tx)]);
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    app.storage
        .db
        .put(
            format!("acct:nonce:{}", fresh.address()),
            bincode::serialize(&1u64).unwrap(),
        )
        .unwrap();
    let error = app.finalize_block(block(2, vec![])).unwrap_err();
    assert!(format!("{error:#}").contains("nonzero nonce"), "{error:#}");
}

/// Signing state of an account with no recovery record: the chain domain with
/// its ID, generation and nonce zero.
fn uninitialized(app: &ConsensusApplication, fixture: &Fixture, key: &Key) -> RecoveryState {
    let mut state = current(app, &fixture.active);
    state.domain.account_id = key.id();
    state.active_generation = 0;
    state.spending_nonce = 0;
    state
}
/// A transfer from the active account that creates `key`'s account.
fn funding(app: &ConsensusApplication, fixture: &Fixture, key: &Key, amount: u128) -> Vec<u8> {
    let tx = fixture.ordinary(
        &current(app, &fixture.active),
        &fixture.active,
        vec![send(key.id(), amount)],
        1000,
    );
    fixture.ordinary_wire(&tx)
}
fn first_spend(
    app: &ConsensusApplication,
    fixture: &Fixture,
    key: &Key,
    actions: Vec<OrdinaryAction>,
    gas_limit: u64,
) -> Vec<u8> {
    let tx = fixture.ordinary(&uninitialized(app, fixture, key), key, actions, gas_limit);
    fixture.ordinary_wire(&tx)
}
fn registered(app: &ConsensusApplication, key: &Key) -> bool {
    book(app).accounts.contains_key(&hex::encode(key.id()))
}

#[test]
fn first_spend_initializes_a_created_account_from_the_chain_template() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let fresh = Key::new();
    let raw = funding(&app, &fixture, &fresh, 50_000);
    assert_eq!(commit(&mut app, 1, vec![raw]).tx_results[0].code, 0);
    let id = hex::encode(fresh.id());
    assert!(app.query_ordinary_account(&id).unwrap().is_null());

    let payer_before = balance(&app, &fixture.payer.address());
    let raw = first_spend(&app, &fixture, &fresh, vec![send(fixture.payer.id(), 100)], 1000);
    assert_admitted(app.check_tx(&raw));
    let result = commit(&mut app, 2, vec![raw]);
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    let gas_fee = result.tx_results[0].gas_used.max(10) as u128 * 2;
    let stored = book(&app).accounts[&id].clone();
    assert_eq!(stored.address, fresh.address());
    assert_eq!(stored.recovery.domain, uninitialized(&app, &fixture, &fresh).domain);
    assert_eq!(
        stored.recovery.config,
        fixture.config.ordinary.as_ref().unwrap().account_template.recovery
    );
    assert_eq!(stored.recovery.active_key, fresh.identity);
    assert_eq!(book(&app).origins[&id], fresh.identity);
    assert_mirror(&app, &fresh, 1);
    assert_eq!(balance(&app, &fresh.address()), 50_000 - 100 - gas_fee);
    assert_eq!(balance(&app, &fixture.payer.address()), payer_before + 100);
    assert!(!app.query_ordinary_account(&id).unwrap().is_null());

    // From now on it spends like any registered account.
    let tx = fixture.ordinary(&current(&app, &fresh), &fresh, vec![send(fixture.payer.id(), 1)], 1000);
    let result = commit(&mut app, 3, vec![fixture.ordinary_wire(&tx)]);
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    assert_mirror(&app, &fresh, 2);

    // A fresh process passes the complete check and continues.
    drop(app);
    let mut reopened = fixture.open(&dir.path().join("db"));
    verify_recovery(&reopened.storage).unwrap();
    let tx = fixture.ordinary(&current(&reopened, &fresh), &fresh, vec![send(fixture.payer.id(), 1)], 1000);
    let result = commit(&mut reopened, 4, vec![fixture.ordinary_wire(&tx)]);
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    assert_mirror(&reopened, &fresh, 3);
}

#[test]
fn first_spend_by_a_foreign_key_domain_or_counter_is_rejected_without_a_fee() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let fresh = Key::new();
    let payer = fixture.payer.id();
    // Never funded: no native account, so nothing to pay with.
    let raw = first_spend(&app, &fixture, &fresh, vec![send(payer, 1)], 1000);
    assert_ne!(app.check_tx(&raw).code, 0);
    let result = commit(&mut app, 1, vec![raw]);
    assert!(result.tx_results[0].log.contains("no funds"), "{:?}", result.tx_results);
    assert!(app
        .storage
        .db
        .get(format!("acct:balances:{}", fresh.address()))
        .unwrap()
        .is_none());

    let raw = funding(&app, &fixture, &fresh, 50_000);
    assert_eq!(commit(&mut app, 2, vec![raw]).tx_results[0].code, 0);
    let base = uninitialized(&app, &fixture, &fresh);
    let other = Key::new();
    let mut cases = vec![("foreign key", fixture.ordinary(&base, &other, vec![send(payer, 1)], 1000))];
    let mut state = base.clone();
    state.domain.genesis_digest[0] ^= 1;
    cases.push(("domain", fixture.ordinary(&state, &fresh, vec![send(payer, 1)], 1000)));
    let mut state = base.clone();
    state.spending_nonce = 1;
    cases.push(("nonce", fixture.ordinary(&state, &fresh, vec![send(payer, 1)], 1000)));
    let mut state = base.clone();
    state.active_generation = 1;
    cases.push(("generation", fixture.ordinary(&state, &fresh, vec![send(payer, 1)], 1000)));
    let active_before = balance(&app, &fixture.active.address());
    for (height, (case, tx)) in (3..).zip(cases) {
        let raw = fixture.ordinary_wire(&tx);
        assert_ne!(app.check_tx(&raw).code, 0, "{case}");
        let result = commit(&mut app, height, vec![raw]);
        assert_ne!(result.tx_results[0].code, 0, "{case}");
        assert!(!registered(&app, &fresh), "{case}");
        assert_eq!(balance(&app, &fresh.address()), 50_000, "{case}");
        assert_eq!(ordinary_nonce(&app, &fresh.address()), 0, "{case}");
    }
    assert_eq!(balance(&app, &fixture.active.address()), active_before);
}

#[test]
fn failed_first_spend_still_initializes_the_account_and_consumes_its_nonce() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let fresh = Key::new();
    let raw = funding(&app, &fixture, &fresh, 50_000);
    assert_eq!(commit(&mut app, 1, vec![raw]).tx_results[0].code, 0);
    let payer_before = balance(&app, &fixture.payer.address());
    // Accepted, then out of gas at the second action: the transfer rolls back
    // but the fee is paid and the nonce is consumed.
    let raw = first_spend(
        &app,
        &fixture,
        &fresh,
        vec![
            send(fixture.payer.id(), 100),
            OrdinaryAction::Data {
                data: "not committed".into(),
            },
        ],
        25,
    );
    let result = commit(&mut app, 2, vec![raw]);
    assert_ne!(result.tx_results[0].code, 0);
    assert_eq!(result.tx_results[0].gas_used, 25);
    assert!(registered(&app, &fresh));
    assert_mirror(&app, &fresh, 1);
    assert_eq!(balance(&app, &fresh.address()), 50_000 - 50);
    assert_eq!(balance(&app, &fixture.payer.address()), payer_before);
}

#[test]
fn an_account_can_be_created_and_first_spent_in_one_block() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let fresh = Key::new();
    let fund = funding(&app, &fixture, &fresh, 50_000);
    let spend = first_spend(&app, &fixture, &fresh, vec![send(fixture.payer.id(), 100)], 1000);
    // The spend cannot come before the transfer that funds it.
    let result = commit(&mut app, 1, vec![spend.clone(), fund.clone()]);
    assert_ne!(result.tx_results[0].code, 0);
    assert_eq!(result.tx_results[1].code, 0, "{:?}", result.tx_results);
    assert!(!registered(&app, &fresh));
    let mut fees = charged(&result);

    let other = Key::new();
    let fund = funding(&app, &fixture, &other, 50_000);
    let spend = first_spend(&app, &fixture, &other, vec![send(fixture.payer.id(), 100)], 1000);
    let result = commit(&mut app, 2, vec![fund, spend]);
    assert!(result.tx_results.iter().all(|r| r.code == 0), "{:?}", result.tx_results);
    assert_mirror(&app, &other, 1);
    fees += charged(&result);
    assert_eq!(burned(&app), 2 * CREATION_FEE + fees);
    drop(app);
    let reopened = fixture.open(&dir.path().join("db"));
    verify_recovery(&reopened.storage).unwrap();
}

#[test]
fn proposal_rolls_back_a_first_spend_it_does_not_select() {
    let mut fixture = Fixture::new();
    // One reservation per block, so a second candidate executes and is dropped.
    fixture.config.ordinary.as_mut().unwrap().queue_max_entries = 1;
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let fresh = Key::new();
    let raw = funding(&app, &fixture, &fresh, 50_000);
    assert_eq!(commit(&mut app, 1, vec![raw]).tx_results[0].code, 0);
    let data_tx = fixture.ordinary(
        &current(&app, &fixture.payer),
        &fixture.payer,
        vec![OrdinaryAction::Data {
            data: "first".into(),
        }],
        1000,
    );
    let data_tx = fixture.ordinary_wire(&data_tx);
    let spend = first_spend(&app, &fixture, &fresh, vec![send(fixture.payer.id(), 1)], 1000);
    let before = data(&app);
    // The dropped candidate's record and origin are rolled back; the test
    // build also compares the rollback with a full clone.
    for ordered in [
        vec![data_tx.clone(), spend.clone()],
        vec![spend.clone(), data_tx.clone()],
    ] {
        let selected = app
            .prepare_proposal(2, 20, 0, ordered.clone(), 1_048_576)
            .unwrap();
        assert_eq!(selected, vec![ordered[0].clone()]);
        assert_eq!(data(&app), before);
    }
}

#[test]
fn initialized_account_must_keep_the_chain_template() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let fresh = Key::new();
    let raw = funding(&app, &fixture, &fresh, 50_000);
    assert_eq!(commit(&mut app, 1, vec![raw]).tx_results[0].code, 0);
    let raw = first_spend(&app, &fixture, &fresh, vec![send(fixture.payer.id(), 1)], 1000);
    assert_eq!(commit(&mut app, 2, vec![raw]).tx_results[0].code, 0);
    let key = format!("recovery:v2:account:{}", hex::encode(fresh.id()));
    let mut account: RecoveryAccount =
        serde_json::from_slice(&app.storage.db.get(&key).unwrap().unwrap()).unwrap();
    account.recovery.config.recovery_delay += 1;
    app.storage
        .db
        .put(&key, serde_json::to_vec(&account).unwrap())
        .unwrap();
    let error = app.finalize_block(block(3, vec![])).unwrap_err();
    assert!(format!("{error:#}").contains("chain template"), "{error:#}");
}

#[test]
fn recovery_enrolls_an_initialized_implicit_account_and_refuses_an_uninitialized_one() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let fresh = Key::new();
    let raw = funding(&app, &fixture, &fresh, 50_000);
    assert_eq!(commit(&mut app, 1, vec![raw]).tx_results[0].code, 0);
    let guardians: Vec<_> = fixture.guardians.iter().collect();
    let enroll = |state: &RecoveryState| {
        let operation = RecoveryOperation {
            domain: state.domain.clone(),
            action: Action {
                submission_expiry: 40,
                kind: ActionKind::Enroll {
                    active: ActiveAuthorization {
                        generation: state.active_generation,
                        nonce: state.spending_nonce,
                    },
                    policy: fixture.policy(),
                },
            },
        };
        fixture.signed(operation, &[&fresh], &guardians)
    };

    // Before its first spend the account has no recovery record: it can be
    // neither a recovery target nor a sponsor, and nothing is charged.
    let payer_before = balance(&app, &fixture.payer.address());
    let early = wire(&fixture.sponsored(enroll(&uninitialized(&app, &fixture, &fresh)), 0, GAS_LIMIT));
    let as_sponsor = wire(&fixture.sponsored_by(fixture.enroll(), fresh.id(), &fresh, 0, 0, GAS_LIMIT));
    for (raw, role) in [(early, "target"), (as_sponsor, "sponsor")] {
        assert_ne!(app.check_tx(&raw).code, 0, "{role}");
        let height = current_info(&app.storage).unwrap().height + 1;
        let result = commit(&mut app, height, vec![raw]);
        let log = &result.tx_results[0].log;
        assert!(
            log.contains(&format!("Recovery {role} has no recovery record"))
                && log.contains("first ordinary transaction"),
            "{role}: {log}"
        );
    }
    assert_eq!(balance(&app, &fixture.payer.address()), payer_before);
    assert_eq!(balance(&app, &fresh.address()), 50_000);
    assert!(!registered(&app, &fresh));

    // Once initialized, it enrolls guardians like a genesis account.
    let raw = first_spend(&app, &fixture, &fresh, vec![send(fixture.payer.id(), 1)], 1000);
    assert_eq!(commit(&mut app, 4, vec![raw]).tx_results[0].code, 0);
    let raw = wire(&fixture.sponsored(enroll(&current(&app, &fresh)), 0, GAS_LIMIT));
    assert_admitted(app.check_tx(&raw));
    let result = commit(&mut app, 5, vec![raw]);
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    let state = current(&app, &fresh);
    assert_eq!(state.policy, Some(fixture.policy()));
    assert_mirror(&app, &fresh, state.spending_nonce);
    drop(app);
    let reopened = fixture.open(&dir.path().join("db"));
    verify_recovery(&reopened.storage).unwrap();
}

#[test]
fn block_account_reads_do_not_grow_with_initialized_accounts() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let reads = || crate::recovery_store::STORED_READS.with(|n| n.get());
    let loads = || crate::recovery_store::COMPLETE_LOADS.with(|n| n.get());
    let send_block = |app: &mut ConsensusApplication, height: u64| {
        let tx = fixture.ordinary(
            &current(app, &fixture.active),
            &fixture.active,
            vec![send(fixture.payer.id(), 1)],
            1000,
        );
        let raw = fixture.ordinary_wire(&tx);
        let (before, loaded) = (reads(), loads());
        assert_admitted(app.check_tx(&raw));
        let proposed = app
            .prepare_proposal(height, height as i64 * 10, 0, vec![raw.clone()], 1_048_576)
            .unwrap();
        assert_eq!(proposed, vec![raw.clone()]);
        let result = commit(app, height, vec![raw]);
        assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
        assert_eq!(loads(), loaded, "admission, proposal and commit load no complete book");
        reads() - before
    };
    // Opening the committed book re-checks the previous block's receipts, so
    // each measured block follows a block with one receipt.
    send_block(&mut app, 1);
    let first = send_block(&mut app, 2);
    let mut height = 3;
    for _ in 0..12 {
        let key = Key::new();
        let raw = funding(&app, &fixture, &key, 50_000);
        assert_eq!(commit(&mut app, height, vec![raw]).tx_results[0].code, 0);
        let raw = first_spend(&app, &fixture, &key, vec![send(fixture.payer.id(), 1)], 1000);
        assert_eq!(commit(&mut app, height + 1, vec![raw]).tx_results[0].code, 0);
        assert!(registered(&app, &key));
        height += 2;
    }
    assert_eq!(book(&app).accounts.len(), 15);
    assert_eq!(send_block(&mut app, height), first);
}

#[test]
fn sponsor_receipts_are_pruned_after_their_operation_expires_and_history_still_verifies() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = fixture.initialized(&dir.path().join("db"));
    let raw = wire(&fixture.sponsored(fixture.enroll(), 0, GAS_LIMIT));
    let result = commit(&mut app, 1, vec![raw.clone()]);
    assert_eq!(result.tx_results[0].code, 0, "{:?}", result.tx_results);
    let committed = book(&app);
    let (id, receipt) = committed.sponsor_receipts.iter().next().unwrap();
    let (id, expiry) = (id.clone(), receipt.retained_until);
    assert_eq!(expiry, fixture.enroll().operation.action.submission_expiry);
    assert!(committed.operation_success.values().any(|entry| *entry == id));
    for height in 2..expiry {
        commit(&mut app, height, vec![]);
    }
    assert!(book(&app).sponsor_receipts.contains_key(&id));
    // At the operation's expiry the receipt and its success entry are
    // pruned; a replay is rejected by expiry and by the sponsor nonce.
    let result = commit(&mut app, expiry, vec![raw]);
    assert_ne!(result.tx_results[0].code, 0);
    let pruned = book(&app);
    assert!(pruned.sponsor_receipts.is_empty() && pruned.operation_success.is_empty());
    assert_eq!(pruned.accounts[&hex::encode(fixture.payer.id())].sponsor_nonce, 1);
    assert!(current(&app, &fixture.active).policy.is_some());
    // The complete history check accepts the pruned receipt after a restart.
    drop(app);
    let reopened = fixture.open(&dir.path().join("db"));
    verify_recovery(&reopened.storage).unwrap();
}

// Governance T6 through the engine: CheckTx, finalize and commit with real
// signatures. Values are synthetic.
mod governance {
    use super::*;
    use crate::governance_actions::{self, ParameterChange, RegistryChange};
    use crate::runtime::governance_candidate::{CLASS_PARAMETER_CHANGE, CLASS_VALIDATOR_REGISTRY};
    use crate::runtime::governance_store::{self, GovernedParameters, Header};
    use dytallix_protocol_types::{
        ordinary_fees_v3::{self, ORDINARY_FEE_CONTRACT_VERSION},
        ordinary_v3 as v3,
    };

    fn governed() -> Fixture {
        let mut f = Fixture::new();
        let digest: [u8; 32] = Sha256::digest(&f.genesis).into();
        f.config.governance = Some(local_governance_candidate(&f, digest, 1));
        f
    }
    fn tx(app: &ConsensusApplication, key: &Key, action: v3::Action) -> Vec<u8> {
        let candidate = app.config.governance.as_ref().unwrap();
        let parameters = GovernedParameters::read(&app.storage).unwrap();
        let profile = governance_actions::governance_fee(candidate, &parameters);
        let state = current(app, key);
        let body = v3::OrdinaryTransaction {
            domain: state.domain.clone(),
            authorization_generation: state.active_generation,
            spending_nonce: state.spending_nonce,
            key: key.identity.clone(),
            expiry_height: header(app).height + 50,
            ordinary_fee_contract_version: ORDINARY_FEE_CONTRACT_VERSION,
            fee_profile_version: profile.version,
            fee_profile_digest: ordinary_fees_v3::profile_digest(profile).unwrap(),
            fee_denomination: v3::Denomination::Udrt,
            maximum_fee: profile.base.max_fee_cap,
            gas_limit: profile.base.max_transaction_gas,
            memo: String::new(),
            actions: vec![action],
        };
        let limits = profile.limits();
        let signature =
            ActivePQC::sign(&key.secret, &v3::signing_bytes(&body, &limits).unwrap());
        crate::ordinary_transport::encode_transport_v3(
            &v3::SignedOrdinary { body, signature },
            &limits,
            65_536,
        )
        .unwrap()
    }
    fn propose(app: &ConsensusApplication, key: &Key, id: u64, class: u16, data: Vec<u8>) -> Vec<u8> {
        let digest = v3::governance_action_digest(class, &data).unwrap();
        tx(
            app,
            key,
            v3::Action::GovernanceProposal {
                proposal_id: id,
                action_class: class,
                action_data: data,
                action_digest: digest,
            },
        )
    }
    fn deposit(app: &ConsensusApplication, key: &Key, id: u64, amount: u128) -> Vec<u8> {
        tx(
            app,
            key,
            v3::Action::GovernanceDeposit {
                proposal_id: id,
                amount_udgt: amount,
            },
        )
    }
    fn vote(app: &ConsensusApplication, key: &Key, id: u64, choice: v3::VoteChoice) -> Vec<u8> {
        tx(app, key, v3::Action::GovernanceVote { proposal_id: id, choice })
    }
    fn udgt(app: &ConsensusApplication, owner: &str) -> u128 {
        let balances: BTreeMap<String, u128> = bincode::deserialize(
            &app.storage
                .db
                .get(format!("acct:balances:{owner}"))
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        balances.get("udgt").copied().unwrap_or(0)
    }
    fn header(app: &ConsensusApplication) -> Header {
        Header::read(&app.storage).unwrap().unwrap()
    }
    fn codes(result: &FinalizeResult) -> Vec<u32> {
        result.tx_results.iter().map(|r| r.code).collect()
    }
    fn min_self_bond(app: &ConsensusApplication) -> u128 {
        lifecycle_state(app).config.min_self_bond
    }

    /// Submission, deposit, vote, tally, timelock and execution at their exact
    /// heights; the deposit is refunded once and the record then removed.
    /// Returns every block's transactions and app hash.
    fn run_min_self_bond(f: &Fixture, dir: &std::path::Path) -> Vec<(Vec<Vec<u8>>, String)> {
        let mut app = f.initialized(dir);
        let data = governance_actions::encode(&ParameterChange::MinSelfBond(50)).unwrap();
        let proposal = propose(&app, &f.active, 1, CLASS_PARAMETER_CHANGE, data);
        assert_eq!(app.check_tx_result(&proposal).unwrap().code, 0);
        let mut blocks = Vec::new();
        let mut run = |app: &mut ConsensusApplication, height, txs: Vec<Vec<u8>>| {
            let result = commit(app, height, txs.clone());
            blocks.push((txs, result.app_hash.clone()));
            result
        };
        assert_eq!(codes(&run(&mut app, 1, vec![proposal])), vec![0]);
        assert_eq!(header(&app).next_proposal_id, 2);
        let payer_dgt = udgt(&app, &f.payer.address());
        let deposit = deposit(&app, &f.payer, 1, 5);
        assert_eq!(codes(&run(&mut app, 2, vec![deposit])), vec![0]);
        assert_eq!(udgt(&app, &f.payer.address()), payer_dgt - 5);
        assert_eq!(header(&app).held_udgt, 5);
        // Deposits close at 3 and the ballot opens there, with the bond
        // snapshot of height 2: the active account's 100 uDGT.
        let yes = vote(&app, &f.active, 1, v3::VoteChoice::Yes);
        assert_eq!(codes(&run(&mut app, 3, vec![yes])), vec![0]);
        run(&mut app, 4, vec![]);
        run(&mut app, 5, vec![]);
        // The ballot closes at 6 and passes; the timelock runs to 8.
        run(&mut app, 6, vec![]);
        assert_eq!(min_self_bond(&app), 10);
        assert_eq!(header(&app).held_udgt, 5);
        run(&mut app, 7, vec![]);
        run(&mut app, 8, vec![]);
        assert_eq!(min_self_bond(&app), 50);
        assert_eq!(
            GovernedParameters::read(&app.storage).unwrap().min_self_bond,
            Some(50)
        );
        assert_eq!(udgt(&app, &f.payer.address()), payer_dgt);
        assert_eq!(header(&app).held_udgt, 0);
        assert_eq!(header(&app).proposals, 1);
        run(&mut app, 9, vec![]);
        assert_eq!(header(&app).proposals, 0);
        assert!(app
            .storage
            .db
            .iterator(IteratorMode::From(b"governance:v2:", rocksdb::Direction::Forward))
            .map(|e| e.unwrap().0)
            .take_while(|k| k.starts_with(b"governance:v2:"))
            .all(|k| k.as_ref() == governance_store::HEADER_KEY.as_bytes()
                || k.as_ref() == governance_store::PARAMETERS_KEY.as_bytes()));
        drop(app);
        // A restart runs the complete check over the pruned state.
        let reopened = f.open(dir);
        assert_eq!(min_self_bond(&reopened), 50);
        blocks
    }

    #[test]
    fn passed_parameter_change_executes_after_its_timelock_and_refunds_once() {
        let f = governed();
        let one = tempfile::tempdir().unwrap();
        let blocks = run_min_self_bond(&f, one.path());
        // The same blocks on an independent database give the same hashes.
        let two = tempfile::tempdir().unwrap();
        let mut replay = f.initialized(two.path());
        for (height, (txs, hash)) in blocks.into_iter().enumerate() {
            let result = commit(&mut replay, height as u64 + 1, txs);
            assert_eq!(result.app_hash, hash, "block {} differs", height + 1);
        }
    }

    #[test]
    fn rule_failures_after_acceptance_are_charged_and_change_no_governance_state() {
        let f = governed();
        let dir = tempfile::tempdir().unwrap();
        let mut app = f.initialized(dir.path());
        // The payer has no bond, so it cannot propose; it still pays.
        let data = governance_actions::encode(&ParameterChange::MinSelfBond(50)).unwrap();
        let drt = balance(&app, &f.payer.address());
        let unbonded = propose(&app, &f.payer, 1, CLASS_PARAMETER_CHANGE, data.clone());
        assert_eq!(codes(&commit(&mut app, 1, vec![unbonded])), vec![3]);
        assert!(balance(&app, &f.payer.address()) < drt);
        assert_eq!(ordinary_nonce(&app, &f.payer.address()), 1);
        assert_eq!(header(&app).next_proposal_id, 1);
        // Out of bounds, a disabled class and a wrong ID are paid failures.
        let too_high =
            governance_actions::encode(&ParameterChange::MinSelfBond(2_000_000_000)).unwrap();
        let block = vec![
            propose(&app, &f.active, 1, CLASS_PARAMETER_CHANGE, too_high),
        ];
        assert_eq!(codes(&commit(&mut app, 2, block)), vec![3]);
        let block = vec![propose(&app, &f.active, 1, 7, vec![1])];
        assert_eq!(codes(&commit(&mut app, 3, block)), vec![3]);
        let block = vec![propose(&app, &f.active, 2, CLASS_PARAMETER_CHANGE, data.clone())];
        assert_eq!(codes(&commit(&mut app, 4, block)), vec![3]);
        let block = vec![propose(&app, &f.active, 1, CLASS_PARAMETER_CHANGE, data)];
        assert_eq!(codes(&commit(&mut app, 5, block)), vec![0]);
        // Voting before the ballot opens, a deposit of zero, and a vote by an
        // owner outside the snapshot are all paid failures.
        let block = vec![vote(&app, &f.active, 1, v3::VoteChoice::Yes)];
        assert_eq!(codes(&commit(&mut app, 6, block)), vec![3]);
        let block = vec![deposit(&app, &f.payer, 1, 0)];
        assert_eq!(codes(&commit(&mut app, 7, block)), vec![1], "zero deposit is malformed");
        let block = vec![deposit(&app, &f.payer, 1, 5)];
        assert_eq!(codes(&commit(&mut app, 6 + 2, block)), vec![3], "deposit period closed");
        assert_eq!(header(&app).held_udgt, 0);
    }

    #[test]
    fn below_minimum_and_rejected_proposals_refund_every_depositor() {
        let f = governed();
        let dir = tempfile::tempdir().unwrap();
        let mut app = f.initialized(dir.path());
        let data = governance_actions::encode(&ParameterChange::MaxActive(3)).unwrap();
        let before = udgt(&app, &f.payer.address());
        let block = vec![propose(&app, &f.active, 1, CLASS_PARAMETER_CHANGE, data.clone())];
        commit(&mut app, 1, block);
        let txs = vec![deposit(&app, &f.payer, 1, 4)];
        commit(&mut app, 2, txs);
        assert_eq!(header(&app).held_udgt, 4);
        // Below the minimum of 5 at close: refunded at 3.
        commit(&mut app, 3, vec![]);
        assert_eq!(header(&app).held_udgt, 0);
        assert_eq!(udgt(&app, &f.payer.address()), before);
        // A funded proposal voted down is refunded at its close.
        let block = vec![propose(&app, &f.active, 2, CLASS_PARAMETER_CHANGE, data)];
        commit(&mut app, 4, block);
        let txs = vec![deposit(&app, &f.payer, 2, 3), deposit(&app, &f.secondary, 2, 3)];
        commit(&mut app, 5, txs);
        let txs = vec![vote(&app, &f.active, 2, v3::VoteChoice::NoWithVeto)];
        commit(&mut app, 6, txs);
        commit(&mut app, 7, vec![]);
        commit(&mut app, 8, vec![]);
        assert_eq!(header(&app).held_udgt, 6);
        commit(&mut app, 9, vec![]);
        assert_eq!(header(&app).held_udgt, 0);
        assert_eq!(udgt(&app, &f.payer.address()), before);
        assert_eq!(
            lifecycle_state(&app).config.max_active,
            4
        );
        drop(app);
        f.open(dir.path());
    }

    #[test]
    fn inconsistent_execution_fails_refunds_and_leaves_state() {
        let f = governed();
        let dir = tempfile::tempdir().unwrap();
        let mut app = f.initialized(dir.path());
        // A minimum above the only validator's self-bond (100) passes the
        // genesis bounds but not the current state.
        let data = governance_actions::encode(&ParameterChange::MinSelfBond(500)).unwrap();
        let block = vec![propose(&app, &f.active, 1, CLASS_PARAMETER_CHANGE, data)];
        commit(&mut app, 1, block);
        let txs = vec![deposit(&app, &f.payer, 1, 5)];
        commit(&mut app, 2, txs);
        let txs = vec![vote(&app, &f.active, 1, v3::VoteChoice::Yes)];
        commit(&mut app, 3, txs);
        for height in 4..=8 {
            commit(&mut app, height, vec![]);
        }
        assert_eq!(min_self_bond(&app), 10);
        assert_eq!(GovernedParameters::read(&app.storage).unwrap(), GovernedParameters::default());
        assert_eq!(header(&app).held_udgt, 0);
        let removed = governance_actions::encode(&RegistryChange::Remove {
            validator_id: "validator-one".into(),
        })
        .unwrap();
        let block = vec![propose(&app, &f.active, 2, CLASS_VALIDATOR_REGISTRY, removed)];
        assert_eq!(codes(&commit(&mut app, 9, block)), vec![0]);
    }

    #[test]
    fn fee_change_replaces_both_profiles_at_its_execution_height() {
        let f = governed();
        let dir = tempfile::tempdir().unwrap();
        let mut app = f.initialized(dir.path());
        // Clients read the v3 profile they sign against (clients v1).
        let view = |app: &ConsensusApplication| {
            serde_json::from_value::<dytallix_protocol_types::ordinary_client::GovernanceProfileView>(
                app.query_governance_profile().unwrap(),
            )
            .unwrap()
        };
        let candidate = f.config.governance.as_ref().unwrap().fee_profile.clone();
        assert_eq!(view(&app).fee_profile, Some(candidate.clone()));
        assert!(view(&app).enabled);
        assert_eq!(view(&app).next_proposal_id, 1);
        let base = &f.config.ordinary.as_ref().unwrap().fee_profile;
        let values = governance_actions::FeeValues {
            gas_price: 3,
            transaction_overhead: base.transaction_overhead,
            receipt_metadata_cost: base.receipt_metadata_cost,
            wire_byte_cost: base.wire_byte_cost,
            read_byte_cost: base.read_byte_cost,
            write_byte_cost: base.write_byte_cost,
            action_costs: base.action_costs,
            signature_costs: base.signature_costs.clone(),
            validator_proof_costs: base.validator_proof_costs.clone(),
            governance_action_costs: [2; 3],
            account_creation_fee_udrt: 2_000,
        };
        let data = governance_actions::encode(&ParameterChange::Fees(values)).unwrap();
        let block = vec![propose(&app, &f.active, 1, CLASS_PARAMETER_CHANGE, data)];
        commit(&mut app, 1, block);
        // An ordinary receipt under the old profile is still retained when
        // the change executes.
        let old = f.ordinary(
            &current(&app, &f.secondary),
            &f.secondary,
            vec![send(f.payer.id(), 1)],
            1000,
        );
        let txs = vec![deposit(&app, &f.payer, 1, 5), f.ordinary_wire(&old)];
        assert_eq!(codes(&commit(&mut app, 2, txs)), vec![0, 0]);
        let txs = vec![vote(&app, &f.active, 1, v3::VoteChoice::Yes)];
        commit(&mut app, 3, txs);
        for height in 4..=7 {
            commit(&mut app, height, vec![]);
        }
        // The view is committed state: it shows the old profile until the
        // change executes, and a request signed for it is refused at height 8.
        assert_eq!(view(&app).fee_profile, Some(candidate.clone()));
        let stale = vote(&app, &f.secondary, 1, v3::VoteChoice::Yes);
        let result = commit(&mut app, 8, vec![stale]);
        assert_eq!(codes(&result), vec![1]);
        let parameters = GovernedParameters::read(&app.storage).unwrap();
        let ordinary = parameters.ordinary_fee.clone().unwrap();
        assert_eq!((ordinary.gas_price, ordinary.version, ordinary.activation_height), (3, 2, 8));
        assert_eq!(ordinary.account_creation_fee_udrt, 2_000);
        let v3_profile = parameters.governance_fee.clone().unwrap();
        assert_eq!(v3_profile.base, ordinary);
        assert_eq!((v3_profile.version, v3_profile.activation_height), (3, 8));
        assert_eq!(view(&app).fee_profile, Some(v3_profile.clone()));
        assert_eq!(view(&app).next_proposal_id, 2);
        // The new profile is in force: a send pays at gas price 3.
        let drt = balance(&app, &f.payer.address());
        let mut signed = f.ordinary(&current(&app, &f.payer), &f.payer, vec![send(f.secondary.id(), 1)], 1000);
        signed.body.fee_profile_version = ordinary.version;
        signed.body.fee_profile_digest = ordinary_fees::profile_digest(&ordinary).unwrap();
        signed.body.maximum_fee = 2000;
        signed.body.gas_limit = 600;
        let signature = ActivePQC::sign(
            &f.payer.secret,
            &ordinary::signing_bytes(&signed.body, &ordinary.limits).unwrap(),
        );
        signed.signature = signature;
        let result = commit(&mut app, 9, vec![f.ordinary_wire(&signed)]);
        assert_eq!(codes(&result), vec![0], "{:?}", result.tx_results);
        let paid = drt - balance(&app, &f.payer.address()) - 1;
        assert_eq!(paid % 3, 0, "fee is charged at the governed gas price");
        // Both profiles are retained while their receipts are; an expired
        // receipt takes its profile with it.
        let mut state = load_ordinary(&app.storage, &app.config, Some(&book(&app)), false)
            .unwrap()
            .unwrap();
        assert_eq!(state.history.profiles().len(), 2);
        state.history.prune_expired(2 + state.receipt_window(), state.receipt_window());
        assert_eq!(state.history.profiles().len(), 1);
        assert!(state.history.profiles().values().all(|p| *p == ordinary));
        drop(app);
        f.open(dir.path());
    }

    use bincode::Options as _;
    fn fixint<T: serde::Serialize>(value: &T) -> Vec<u8> {
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .serialize(value)
            .unwrap()
    }

    #[test]
    fn complete_check_refuses_inconsistent_governance_entries() {
        let f = governed();
        let dir = tempfile::tempdir().unwrap();
        let mut app = f.initialized(dir.path());
        let data = governance_actions::encode(&ParameterChange::MaxActive(3)).unwrap();
        let txs = vec![propose(&app, &f.active, 1, CLASS_PARAMETER_CHANGE, data)];
        commit(&mut app, 1, txs);
        let txs = vec![deposit(&app, &f.payer, 1, 5)];
        commit(&mut app, 2, txs);
        let txs = vec![vote(&app, &f.active, 1, v3::VoteChoice::Yes)];
        commit(&mut app, 3, txs);
        let rules = app.config.governance.as_ref().unwrap().store_rules();
        governance_store::validate_complete(&app.storage, &rules).unwrap();
        let entries: BTreeMap<Vec<u8>, Vec<u8>> = app
            .storage
            .db
            .iterator(IteratorMode::From(b"governance:v2:", rocksdb::Direction::Forward))
            .map(|e| {
                let (k, v) = e.unwrap();
                (k.to_vec(), v.to_vec())
            })
            .take_while(|(k, _)| k.starts_with(b"governance:v2:"))
            .collect();
        let key = |prefix: &str| {
            entries
                .keys()
                .find(|k| k.starts_with(prefix.as_bytes()))
                .unwrap()
                .clone()
        };
        let proposal_key = key("governance:v2:proposal:");
        let mut proposal: governance_store::Proposal = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .deserialize(&entries[&proposal_key])
            .unwrap();
        if let governance_store::Phase::Voting { tally, .. } = &mut proposal.phase {
            tally.yes += 1;
        }
        let mut header = header(&app);
        header.held_udgt += 1;
        let corruptions: Vec<(Vec<u8>, Option<Vec<u8>>)> = vec![
            (proposal_key, Some(fixint(&proposal))),
            (key("governance:v2:due:"), None),
            (key("governance:v2:vote:"), Some(fixint(&governance_store::VoteChoice::No))),
            (governance_store::HEADER_KEY.as_bytes().to_vec(), Some(fixint(&header))),
            (key("governance:v2:snapshot:"), None),
            (b"governance:v2:other".to_vec(), Some(vec![1])),
        ];
        for (key, value) in corruptions {
            let original = entries.get(&key).cloned();
            match &value {
                Some(value) => app.storage.db.put(&key, value).unwrap(),
                None => app.storage.db.delete(&key).unwrap(),
            }
            assert!(
                governance_store::validate_complete(&app.storage, &rules).is_err(),
                "{}",
                String::from_utf8_lossy(&key)
            );
            match original {
                Some(value) => app.storage.db.put(&key, value).unwrap(),
                None => app.storage.db.delete(&key).unwrap(),
            }
        }
        governance_store::validate_complete(&app.storage, &rules).unwrap();
    }

    #[test]
    fn a_refund_at_block_start_is_spendable_in_the_same_block() {
        let f = governed();
        let dir = tempfile::tempdir().unwrap();
        let mut app = f.initialized(dir.path());
        let data = governance_actions::encode(&ParameterChange::MaxActive(3)).unwrap();
        let txs = vec![propose(&app, &f.active, 1, CLASS_PARAMETER_CHANGE, data)];
        commit(&mut app, 1, txs);
        let all = udgt(&app, &f.payer.address());
        let txs = vec![deposit(&app, &f.payer, 1, 4)];
        commit(&mut app, 2, txs);
        // The below-minimum refund runs at 3 before this transfer of the
        // payer's whole balance, so the transfer is funded.
        let mut signed = f.ordinary(
            &current(&app, &f.payer),
            &f.payer,
            vec![OrdinaryAction::Send {
                recipient: f.secondary.id(),
                denomination: Denomination::Udgt,
                amount: all,
            }],
            1000,
        );
        signed = f.resign(signed.body, &f.payer);
        let result = commit(&mut app, 3, vec![f.ordinary_wire(&signed)]);
        assert_eq!(codes(&result), vec![0], "{:?}", result.tx_results);
        assert_eq!(udgt(&app, &f.payer.address()), 0);
    }

    #[test]
    fn proposer_selects_paid_governance_transactions_and_drops_denied_ones() {
        let f = governed();
        let dir = tempfile::tempdir().unwrap();
        let app = f.initialized(dir.path());
        let data = governance_actions::encode(&ParameterChange::MaxActive(3)).unwrap();
        let valid = propose(&app, &f.active, 1, CLASS_PARAMETER_CHANGE, data.clone());
        // Unfunded for its signed cap and not registered: denied, not included.
        let stranger = Key::new();
        let denied = {
            let mut raw = propose(&app, &f.active, 1, CLASS_PARAMETER_CHANGE, data.clone());
            raw.truncate(raw.len() - 3);
            raw
        };
        let paid = propose(&app, &f.payer, 1, CLASS_PARAMETER_CHANGE, data);
        let _ = stranger;
        let out = app
            .prepare_proposal(1, 10, 0, vec![valid.clone(), denied, paid.clone()], 1_048_576)
            .unwrap();
        assert_eq!(out, vec![valid, paid]);
        assert!(app.process_proposal(block(1, out)).unwrap());
    }

    #[test]
    fn validator_registry_addition_executes_and_survives_restart() {
        let f = governed();
        let dir = tempfile::tempdir().unwrap();
        let mut app = f.initialized(dir.path());
        let data = governance_actions::encode(&RegistryChange::Add {
            validator_id: "validator-two".into(),
            owner: f.payer.address(),
        })
        .unwrap();
        let txs = vec![propose(&app, &f.active, 1, CLASS_VALIDATOR_REGISTRY, data)];
        commit(&mut app, 1, txs);
        let txs = vec![deposit(&app, &f.payer, 1, 5)];
        commit(&mut app, 2, txs);
        let txs = vec![vote(&app, &f.active, 1, v3::VoteChoice::Yes)];
        commit(&mut app, 3, txs);
        for height in 4..=8 {
            commit(&mut app, height, vec![]);
        }
        let operators = lifecycle_state(&app).config.approved_operators;
        assert_eq!(operators.get("validator-two"), Some(&f.payer.address()));
        assert_eq!(
            GovernedParameters::read(&app.storage).unwrap().approved_operators,
            Some(operators)
        );
        drop(app);
        let reopened = f.open(dir.path());
        assert!(lifecycle_state(&reopened)
            .config
            .approved_operators
            .contains_key("validator-two"));
    }

    #[test]
    fn many_proposals_keep_retained_governance_state_bounded() {
        let f = governed();
        let dir = tempfile::tempdir().unwrap();
        let mut app = f.initialized(dir.path());
        let data = governance_actions::encode(&ParameterChange::MaxActive(3)).unwrap();
        let mut peak = 0;
        // Within one issuance epoch (100 blocks), so no observation is due.
        for round in 0..95u64 {
            let height = round + 1;
            let id = header(&app).next_proposal_id;
            let txs = vec![propose(&app, &f.active, id, CLASS_PARAMETER_CHANGE, data.clone())];
            assert_eq!(codes(&commit(&mut app, height, txs)), vec![0]);
            let retained = app
                .storage
                .db
                .iterator(IteratorMode::From(b"governance:v2:", rocksdb::Direction::Forward))
                .take_while(|e| e.as_ref().unwrap().0.starts_with(b"governance:v2:"))
                .count();
            peak = peak.max(retained);
        }
        assert_eq!(header(&app).next_proposal_id, 96);
        // Header, and three proposals (admitted, collecting, finished) each
        // with one due entry.
        assert!(peak <= 7, "retained governance entries peaked at {peak}");
        drop(app);
        f.open(dir.path());
    }
}

/// Without a governance candidate the v3 profile view is disabled.
#[test]
fn governance_profile_view_is_disabled_without_governance() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let app = fixture.initialized(&dir.path().join("db"));
    let view: dytallix_protocol_types::ordinary_client::GovernanceProfileView =
        serde_json::from_value(app.query_governance_profile().unwrap()).unwrap();
    assert!(!view.enabled && view.fee_profile.is_none() && view.next_proposal_id == 0);
    assert_eq!(view.context.height, 0);
}
