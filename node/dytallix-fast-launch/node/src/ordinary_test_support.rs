//! Ordinary v2 for consensus test fixtures that used the removed legacy
//! signed-transaction path (E04 gap 14, L-b). A fixture registers its genesis
//! accounts in a recovery book, enables ordinary v2 and signs transactions
//! from an account's committed or initial state. Synthetic profiles only.
use super::*;
use crate::crypto::{ActivePQC, PQC};
use crate::ordinary_state::{AccountTemplate, OrdinaryConfig};
use crate::recovery_fees::{RecoveryAccount, RecoveryBook};
use dytallix_protocol_types::address::{AccountAddress, AddressNetwork};
use dytallix_protocol_types::ordinary::{
    self, Action, Denomination, Limits, OrdinaryTransaction, SignedOrdinary,
};
use dytallix_protocol_types::ordinary_fees::{self, FeeProfile};
use dytallix_protocol_types::recovery::{
    KeyIdentity, RecoveryConfig, RecoveryDomain, RecoveryState,
};
use dytallix_protocol_types::recovery_sponsor::FeeProfile as RecoveryProfile;

/// Validity of a signed transaction, in blocks after its signing height.
const EXPIRY_BLOCKS: u64 = 50;

/// The address of a development ML-DSA-65 origin key.
pub(crate) fn address(chain: &str, public: &[u8]) -> String {
    crate::addr::initial_address(
        crate::addr::AddressNetwork::Development,
        chain,
        crate::addr::OriginKeyAlgorithm::MlDsa65,
        public,
    )
    .unwrap()
}
pub(crate) fn account_id(chain: &str, public: &[u8]) -> [u8; 32] {
    *AccountAddress::decode(AddressNetwork::Development, &address(chain, public))
        .unwrap()
        .account_id()
}
fn identity(public: &[u8]) -> KeyIdentity {
    KeyIdentity {
        algorithm: "mldsa65".into(),
        public_key: public.to_vec(),
    }
}
fn template() -> RecoveryConfig {
    RecoveryConfig {
        timing_version: 1,
        recovery_delay: 2,
        finalization_window: 2,
        policy_delay: 2,
        policy_window: 2,
        submission_lifetime: 50,
        algorithms: BTreeMap::from([("mldsa65".into(), 1952)]),
    }
}
fn recovery_profile() -> RecoveryProfile {
    RecoveryProfile {
        version: 1,
        activation_height: 1,
        denomination: "udrt".into(),
        gas_price: 2,
        minimum_gas: 10,
        max_transaction_gas: 1_000_000,
        max_block_gas: 4_000_000,
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
/// The synthetic ordinary fee profile: every transaction costs the same
/// small amount, so balances stay easy to follow.
pub(crate) fn fee_profile(config: &ConsensusConfig) -> FeeProfile {
    FeeProfile {
        ordinary_fee_contract_version: 1,
        version: 1,
        activation_height: 1,
        denomination: Denomination::Udrt,
        gas_price: 2,
        minimum_gas: 10,
        max_transaction_gas: 1000,
        max_block_transaction_gas: 4_000_000,
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
            config.lifecycle.as_ref().expect("ordinary v2 requires lifecycle"),
        )
        .unwrap(),
        validator_proof_costs: BTreeMap::from([("mldsa65".into(), 4)]),
        account_creation_fee_udrt: 1_000,
    }
}

/// Register the genesis accounts of `publics` and enable ordinary v2.
/// `config.app_state_sha256` must already name the final genesis bytes.
pub(crate) fn enable(config: &mut ConsensusConfig, publics: &[&[u8]]) {
    let digest: [u8; 32] = hex::decode(&config.app_state_sha256)
        .unwrap()
        .try_into()
        .unwrap();
    let chain = config.chain_id.clone();
    let accounts = publics
        .iter()
        .map(|public| RecoveryAccount {
            address: address(&chain, public),
            sponsor_nonce: 0,
            recovery: RecoveryState::new(
                RecoveryDomain {
                    network: 3,
                    chain_id: chain.clone(),
                    genesis_digest: digest,
                    account_id: account_id(&chain, public),
                },
                template(),
                identity(public),
                0,
            )
            .unwrap(),
        })
        .collect();
    let mut book = RecoveryBook::new(recovery_profile(), accounts).unwrap();
    book.origins = publics
        .iter()
        .map(|public| (hex::encode(account_id(&chain, public)), identity(public)))
        .collect();
    config.recovery = Some(book);
    config.ordinary = Some(OrdinaryConfig {
        version: 1,
        fee_profile: fee_profile(config),
        account_template: AccountTemplate {
            recovery: template(),
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
}

/// The ordinary transaction the account of `public` signs next, from its
/// committed recovery state.
pub(crate) fn transaction(
    app: &ConsensusApplication,
    secret: &[u8],
    public: &[u8],
    actions: Vec<Action>,
) -> Vec<u8> {
    let book = RecoveryBook::load(&app.storage).unwrap().unwrap();
    let state = &book.accounts[&hex::encode(account_id(&app.config.chain_id, public))].recovery;
    let height = app.info().unwrap().height;
    sign(&app.config, state, secret, public, actions, height)
}
/// The account's transaction `ahead` nonces after its next one, for a later
/// transaction in the same block.
pub(crate) fn transaction_ahead(
    app: &ConsensusApplication,
    secret: &[u8],
    public: &[u8],
    actions: Vec<Action>,
    ahead: u64,
) -> Vec<u8> {
    let book = RecoveryBook::load(&app.storage).unwrap().unwrap();
    let mut state = book.accounts[&hex::encode(account_id(&app.config.chain_id, public))]
        .recovery
        .clone();
    state.spending_nonce += ahead;
    let height = app.info().unwrap().height;
    sign(&app.config, &state, secret, public, actions, height)
}
/// The account's first ordinary transaction, from its genesis state.
pub(crate) fn first_transaction(
    config: &ConsensusConfig,
    secret: &[u8],
    public: &[u8],
    actions: Vec<Action>,
) -> Vec<u8> {
    let book = config.recovery.as_ref().expect("ordinary v2 enabled");
    let state = &book.accounts[&hex::encode(account_id(&config.chain_id, public))].recovery;
    sign(config, state, secret, public, actions, 0)
}
/// The account's committed spending nonce, which validator key proofs bind.
pub(crate) fn nonce(app: &ConsensusApplication, public: &[u8]) -> u64 {
    RecoveryBook::load(&app.storage).unwrap().unwrap().accounts
        [&hex::encode(account_id(&app.config.chain_id, public))]
        .recovery
        .spending_nonce
}
fn sign(
    config: &ConsensusConfig,
    state: &RecoveryState,
    secret: &[u8],
    public: &[u8],
    actions: Vec<Action>,
    height: u64,
) -> Vec<u8> {
    let p = &config.ordinary.as_ref().expect("ordinary v2 enabled").fee_profile;
    let body = OrdinaryTransaction {
        domain: state.domain.clone(),
        authorization_generation: state.active_generation,
        spending_nonce: state.spending_nonce,
        key: identity(public),
        expiry_height: height + EXPIRY_BLOCKS,
        ordinary_fee_contract_version: 1,
        fee_profile_version: p.version,
        fee_profile_digest: ordinary_fees::profile_digest(p).unwrap(),
        fee_denomination: Denomination::Udrt,
        maximum_fee: p.max_fee_cap,
        gas_limit: p.max_transaction_gas,
        memo: "consensus test fixture".into(),
        actions,
    };
    let signature = ActivePQC::sign(secret, &ordinary::signing_bytes(&body, &p.limits).unwrap());
    crate::ordinary_transport::encode_transport(
        &SignedOrdinary { body, signature },
        &p.limits,
        100_000,
    )
    .unwrap()
}
