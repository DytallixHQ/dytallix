//! Combined ordinary/recovery consensus with real signatures and synthetic profiles.
//! These local checks do not qualify production prices, custody, or deployment.
use super::*;
use crate::crypto::{ActivePQC, PQC};
use crate::ordinary_authority::DiscretionaryGrant;
use crate::ordinary_state::OrdinaryConfig;
use crate::runtime::governance_state::{GovernanceState, GOVERNANCE_STATE_VERSION};
use crate::recovery_fees::{RecoveryAccount, RecoveryBook, STATE_KEY};
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
        config.recovery = Some(RecoveryBook::new(profile, accounts).unwrap());
        let fee_profile = ordinary_profile(&config);
        config.ordinary = Some(OrdinaryConfig {
            version: 1,
            fee_profile,
            origins: [&active, &payer, &secondary]
                .into_iter()
                .map(|k| (hex::encode(k.id()), k.identity.clone()))
                .collect(),
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

fn local_governance_candidate(
    fixture: &Fixture,
    genesis_digest: [u8; 32],
    activation_height: u64,
) -> GovernanceCandidateConfig {
    use crate::runtime::{
        governance_ballot::Rules as BallotRules,
        governance_candidate::{
            ActionClassLimit, Cancellation, EntryPolicy, PendingPolicies, PolicyDecision,
            ProposerEligibility, ValidatorVoting, VoteDelegation, CANDIDATE_SCHEMA_VERSION,
        },
        governance_deposit_stage::DepositRules,
    };
    GovernanceCandidateConfig {
        schema_version: CANDIDATE_SCHEMA_VERSION,
        chain_id: CHAIN.into(),
        genesis_digest,
        activation_height,
        fee_profile: dytallix_protocol_types::ordinary_fees_v3::FeeProfileV3 {
            base: fixture.config.ordinary.as_ref().unwrap().fee_profile.clone(),
            version: 2,
            activation_height,
            max_governance_action_bytes: 100,
            governance_action_costs: [1; 3],
        },
        ballot: BallotRules {
            version: 1,
            chain_id: CHAIN.into(),
            genesis_digest,
            quorum_bps: 1,
            approval_bps: 1,
            veto_bps: 1,
            voting_period_blocks: 2,
            timelock_blocks: 2,
            max_voters: 3,
        },
        deposit: DepositRules {
            deposit_period_blocks: 2,
            minimum_deposit_udgt: 5,
            max_action_bytes: 100,
        },
        action_classes: vec![ActionClassLimit {
            class: 17,
            max_data_bytes: 3,
            approval_digest: [8; 32],
        }],
        entry_policy: EntryPolicy {
            proposer_eligibility:
                ProposerEligibility::RegisteredOwnerWithEffectiveBondAtFinalizedParent,
            validator_voting: ValidatorVoting::OwnEffectiveBondOnly,
            vote_delegation: VoteDelegation::Disabled,
            cancellation: Cancellation::Disabled,
        },
        pending_policies: PendingPolicies {
            exact_state_transitions: PolicyDecision::Pending,
        },
    }
}

#[test]
fn governance_parent_binds_genesis_state_height_and_nonce_mirrors() {
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let app = f.initialized(dir.path());
    let _guard = app.storage.lock_execution().unwrap();
    let genesis_digest: [u8; 32] = Sha256::digest(&f.genesis).into();
    let mut configured = f.config.clone();
    configured.governance = Some(local_governance_candidate(&f, genesis_digest, 1));
    app.storage
        .db
        .put(MODE_KEY, serde_json::to_vec(&configured).unwrap())
        .unwrap();
    let governance = GovernanceState::new(
        GOVERNANCE_STATE_VERSION,
        CHAIN.into(),
        genesis_digest,
        0,
    )
    .unwrap();
    let bind_state = |governance: &GovernanceState| {
        app.storage
            .db
            .put(GOVERNANCE_STATE_KEY, governance.encode().unwrap())
            .unwrap();
        let state = state_digest(&app.storage, &Writes::new(), true).unwrap();
        let hash = digest(b"dytallix-cometbft-genesis-v1", &state).unwrap();
        app.storage.db.put(GENESIS_APP_HASH_KEY, hash.as_bytes()).unwrap();
        hash
    };
    let expected_hash = bind_state(&governance);
    let parent = committed_governance_parent(
        app.storage.clone(),
        &configured,
        genesis_digest,
    )
    .unwrap();
    assert_eq!(parent.height, 0);
    assert_eq!(parent.app_hash, hex::decode(expected_hash).unwrap().as_slice());
    assert_eq!(parent.recovery.last_height, 0);
    assert_eq!(parent.lifecycle.last_height, 0);
    assert_eq!(parent.governance, governance);
    assert_eq!(parent.native_nonces.len(), parent.recovery.accounts.len());
    let mut alternate = configured.clone();
    alternate.governance.as_mut().unwrap().activation_height += 1;
    assert!(committed_governance_parent(app.storage.clone(), &alternate, genesis_digest)
        .err().unwrap()
        .to_string()
        .contains("committed configuration"));

    app.storage.db.put(GENESIS_APP_HASH_KEY, b"00".repeat(32)).unwrap();
    assert!(committed_governance_parent(app.storage.clone(), &configured, genesis_digest)
        .err().unwrap()
        .to_string()
        .contains("app hash"));

    let stale = GovernanceState::new(
        GOVERNANCE_STATE_VERSION,
        CHAIN.into(),
        genesis_digest,
        1,
    )
    .unwrap();
    bind_state(&stale);
    assert!(committed_governance_parent(app.storage.clone(), &configured, genesis_digest)
        .err().unwrap()
        .to_string()
        .contains("state, authority, or lifecycle"));

    bind_state(&governance);
    app.storage
        .db
        .put(
            format!("acct:nonce:{}", f.active.address()),
            bincode::serialize(&1u64).unwrap(),
        )
        .unwrap();
    let state = state_digest(&app.storage, &Writes::new(), true).unwrap();
    let hash = digest(b"dytallix-cometbft-genesis-v1", &state).unwrap();
    app.storage.db.put(GENESIS_APP_HASH_KEY, hash.as_bytes()).unwrap();
    assert!(committed_governance_parent(app.storage.clone(), &configured, genesis_digest)
        .err().unwrap()
        .to_string()
        .contains("nonce"));
}

#[test]
fn signed_governance_deposit_uses_verified_committed_parent() {
    use crate::runtime::governance_ordered_admission::{
        plan_ordered_admission_block, AdmissionAction,
    };
    use dytallix_protocol_types::{
        ordinary_fees_v3::{self, ORDINARY_FEE_CONTRACT_VERSION},
        ordinary_v3 as v3,
    };

    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = f.initialized(dir.path());
    commit(&mut app, 1, vec![]);
    let _guard = app.storage.lock_execution().unwrap();
    let genesis_digest: [u8; 32] = Sha256::digest(&f.genesis).into();
    let candidate = local_governance_candidate(&f, genesis_digest, 2);
    candidate.validate_shape().unwrap();
    let deposit = candidate.deposit.clone();
    let mut configured = f.config.clone();
    configured.governance = Some(candidate.clone());
    app.storage
        .db
        .put(MODE_KEY, serde_json::to_vec(&configured).unwrap())
        .unwrap();
    let initial = GovernanceState::new(
        GOVERNANCE_STATE_VERSION,
        CHAIN.into(),
        genesis_digest,
        0,
    )
    .unwrap();
    let action_data = vec![1, 2, 3];
    let planned = plan_ordered_admission_block(
        &initial,
        1,
        &deposit,
        3,
        &BTreeMap::new(),
        None,
        &[AdmissionAction::Proposal {
            proposal_id: 1,
            action_class: 17,
            action_digest: v3::governance_action_digest(17, &action_data).unwrap(),
            action_data,
        }],
    )
    .unwrap();
    app.storage
        .db
        .put(GOVERNANCE_STATE_KEY, planned.state_bytes)
        .unwrap();
    let state = state_digest(&app.storage, &Writes::new(), true).unwrap();
    let mut head = read_head(&app.storage).unwrap().unwrap();
    head.state_digest = state.clone();
    head.app_hash = app_hash(&state, &head.anchor).unwrap();
    app.storage.db.put(HEAD_KEY, serde_json::to_vec(&head).unwrap()).unwrap();
    let mut record: BlockRecord = decode(
        &app.storage.db.get(record_key(1)).unwrap().unwrap(),
    )
    .unwrap();
    record.head = head;
    record.result.app_hash = record.head.app_hash.clone();
    app.storage
        .db
        .put(record_key(1), serde_json::to_vec(&record).unwrap())
        .unwrap();
    let parent = committed_governance_parent(
        app.storage.clone(),
        &configured,
        genesis_digest,
    )
    .unwrap();
    let recovery = &parent.recovery.accounts[&hex::encode(f.active.id())].recovery;
    let mut signed = v3::SignedOrdinary {
        body: v3::OrdinaryTransaction {
            domain: recovery.domain.clone(),
            authorization_generation: recovery.active_generation,
            spending_nonce: recovery.spending_nonce,
            key: f.active.identity.clone(),
            expiry_height: 5,
            ordinary_fee_contract_version: ORDINARY_FEE_CONTRACT_VERSION,
            fee_profile_version: candidate.fee_profile.version,
            fee_profile_digest: ordinary_fees_v3::profile_digest(&candidate.fee_profile).unwrap(),
            fee_denomination: v3::Denomination::Udrt,
            maximum_fee: candidate.fee_profile.base.max_fee_cap,
            gas_limit: candidate.fee_profile.base.max_transaction_gas,
            memo: String::new(),
            actions: vec![v3::Action::GovernanceDeposit {
                proposal_id: 1,
                amount_udgt: 5,
            }],
        },
        signature: Vec::new(),
    };
    let resign = |signed: &mut v3::SignedOrdinary| {
        signed.signature = ActivePQC::sign(
            &f.active.secret,
            &v3::signing_bytes(&signed.body, &candidate.fee_profile.limits()).unwrap(),
        );
    };
    resign(&mut signed);
    let assessed = parent.assess_signed(&signed).unwrap();
    assert_eq!(assessed.actor(), f.active.id());
    assert!(matches!(
        assessed.ordered_action(),
        AdmissionAction::Deposit { proposal_id: 1, amount_udgt: 5, .. }
    ));
    signed.body.spending_nonce += 1;
    resign(&mut signed);
    assert!(parent.assess_signed(&signed)
        .err().unwrap()
        .to_string()
        .contains("nonce is stale"));
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
    let signed = fixture.ordinary(
        &current(&a, &fixture.secondary),
        &fixture.secondary,
        vec![
            send(fixture.payer.id(), 100),
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
fn check_tx_retains_same_nonce_reservations_and_deduplicates_resigned_intent() {
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
    let resigned = fixture.resign(first.body.clone(), &fixture.active);
    assert_admitted(app.check_tx(&fixture.ordinary_wire(&resigned)));
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
        .ordinary
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
        .ordinary
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
