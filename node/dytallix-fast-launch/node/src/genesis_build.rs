//! Deterministic genesis builder (E05-d): an inputs file of values and records
//! to the three genesis files, the native genesis, the application
//! configuration and the engine genesis, plus a manifest of their digests.
//!
//! Each build has one mode (production activation v1, A7). A development
//! build writes a rehearsal in the local-qualification and development
//! profiles; a production build writes a production-profile genesis, which
//! opens only from its root genesis signed three of five. Neither output is
//! accepted by being built: approved values for every open input, accepted
//! records, the release, the root signatures and gate acceptance remain
//! required. Every value comes from the inputs file, which the E05 resolver
//! writes from approved values, labeled proposals and records.
//! Nothing is defaulted here: the builder only fixes code constants and
//! derives values that approved rules determine. The same inputs always give
//! the same bytes.
use crate::addr::{AccountAddress, AddressNetwork, OriginKeyAlgorithm};
use crate::consensus_settlement::{
    ConsensusApplication, ConsensusConfig, ValidatorConfig, MAX_CONFIG_BYTES,
    MAX_ENGINE_GENESIS_BYTES, MAX_GENESIS_BYTES,
};
use crate::emergency_freeze::{self as emergency, AuthorityPolicy, AutomaticTransitionPolicy};
use crate::ordinary_state::{validator_profile_digest, AccountTemplate, OrdinaryConfig};
use crate::recovery_fees::{RecoveryAccount, RecoveryBook};
use crate::release_handover as handover;
use crate::runtime::governance_candidate::{
    ActionClassLimit, BallotRules, Bounds, Cancellation, DepositRules, EntryPolicy,
    GovernanceCandidateConfig, ParameterBounds, ProposerEligibility, ValidatorVoting,
    VoteDelegation, CANDIDATE_SCHEMA_VERSION,
};
use crate::runtime::issuance_timing::{ControllerInputs, TimingGenesis};
use crate::runtime::penalty_custody::PenaltyConfig;
use crate::runtime::validator_lifecycle::LifecycleConfig;
use crate::upgrade;
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use dytallix_protocol_types::ordinary::{Denomination, Limits};
use dytallix_protocol_types::ordinary_fees::FeeProfile;
use dytallix_protocol_types::ordinary_fees_v3::FeeProfileV3;
use dytallix_protocol_types::recovery::{
    KeyIdentity, RecoveryConfig, RecoveryDomain, RecoveryState,
};
use dytallix_protocol_types::recovery_sponsor::FeeProfile as RecoveryProfile;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_with::{serde_as, DisplayFromStr};
use sha2::{Digest, Sha256, Sha512};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const INPUTS_SCHEMA: &str = "dytallix.genesis-build-inputs.v1";
pub const MANIFEST_SCHEMA: &str = "dytallix.genesis-build-manifest.v1";
/// A development build's mode: a rehearsal in the development profiles.
pub const MODE_REHEARSAL: &str = "rehearsal";
/// A production build's mode: the production profiles (production
/// activation v1, A7).
pub const MODE_PRODUCTION: &str = "production";
/// The mode this build writes; neither build writes the other's.
pub const MODE: &str = if crate::build_profile::PRODUCTION {
    MODE_PRODUCTION
} else {
    MODE_REHEARSAL
};
const ENGINE: &str = "cometbft-v0.40.0";
/// All DGT is issued at genesis (D05-Q02): 1,000,000,000 DGT in uDGT.
const DGT_TOTAL_UDGT: u128 = 1_000_000_000_000_000;
const MLDSA65: &str = "mldsa65";
const MLDSA65_PUBLIC_KEY_BYTES: usize = 1952;
/// `{"type":"ordinary_v2","envelope_base64":""}` around a base64 envelope.
const TRANSPORT_OVERHEAD: u64 = 43;
/// The engine accepts at most 64 validators.
const MAX_VALIDATORS: usize = 64;
const BUILD_DOMAIN: &[u8] = b"DYTALLIX/GENESIS-BUILD/v1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Network {
    Mainnet,
    Testnet,
    Development,
}
impl Network {
    fn address(self) -> AddressNetwork {
        match self {
            Self::Mainnet => AddressNetwork::Mainnet,
            Self::Testnet => AddressNetwork::Testnet,
            Self::Development => AddressNetwork::Development,
        }
    }
    /// The recovery domain's network code.
    fn code(self) -> u8 {
        match self {
            Self::Mainnet => 1,
            Self::Testnet => 2,
            Self::Development => 3,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inputs {
    pub schema: String,
    pub mode: String,
    pub chain_id: String,
    pub genesis_time: String,
    pub network: Network,
    pub application: Application,
    pub lifecycle: Lifecycle,
    pub penalty: Penalty,
    pub recovery_profile: RecoveryFees,
    pub ordinary: Ordinary,
    pub account_template: Template,
    pub governance: Governance,
    pub reward: Reward,
    pub issuance: Issuance,
    pub engine: Engine,
    pub root: Option<Root>,
    #[serde(with = "amount")]
    pub drt_bootstrap_total_udrt: u128,
    pub accounts: Vec<Account>,
    pub validators: Vec<Validator>,
    pub delegations: Vec<Delegation>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Application {
    pub max_tx_bytes: usize,
    pub max_block_bytes: usize,
    pub max_txs: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lifecycle {
    #[serde(with = "amount")]
    pub min_self_bond: u128,
    pub max_active: usize,
    pub evidence_max_age_blocks: u64,
    pub evidence_max_age_seconds: u64,
    pub processing_margin_blocks: u64,
    pub processing_margin_seconds: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Penalty {
    pub numerator: u64,
    pub denominator: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryFees {
    pub gas_price: u64,
    pub minimum_gas: u64,
    pub max_transaction_gas: u64,
    pub max_block_gas: u64,
    pub max_block_recovery_bytes: u64,
    pub max_block_recovery_signatures: u64,
    pub max_pending_accounts: u64,
    pub max_due_expiry_events_per_height: u64,
    pub mandatory_expiry_gas_budget: u64,
    pub expiry_event_gas_cost: u64,
    pub action_costs: [u64; 9],
    pub wire_byte_cost: u64,
    pub read_byte_cost: u64,
    pub write_byte_cost: u64,
    pub signature_costs: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ordinary {
    pub gas_price: u64,
    pub minimum_gas: u64,
    pub max_transaction_gas: u64,
    pub max_block_transaction_gas: u64,
    pub max_block_transaction_bytes: u64,
    pub max_block_signature_checks: u64,
    pub limits: OrdinaryLimits,
    pub transaction_overhead: u64,
    pub receipt_metadata_cost: u64,
    pub wire_byte_cost: u64,
    pub read_byte_cost: u64,
    pub write_byte_cost: u64,
    pub action_costs: [u64; 12],
    pub signature_costs: BTreeMap<String, u64>,
    pub validator_proof_costs: BTreeMap<String, u64>,
    #[serde(with = "amount")]
    pub account_creation_fee_udrt: u128,
    pub max_state_bytes: u64,
    pub max_grants: u32,
    pub max_receipts: u32,
    pub max_retained_profiles: u32,
    pub queue_max_entries: u32,
    pub queue_max_wire_bytes: u64,
    pub queue_max_signature_work: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrdinaryLimits {
    pub max_wire_bytes: u32,
    pub max_actions: u16,
    pub max_identifier_bytes: u16,
    pub max_data_bytes: u32,
    pub max_memo_bytes: u32,
    pub max_consensus_key_bytes: u32,
    pub max_proof_bytes: u32,
    pub max_expiry_lifetime: u64,
    pub allowed_algorithms: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    pub timing_version: u64,
    pub recovery_delay: u64,
    pub finalization_window: u64,
    pub policy_delay: u64,
    pub policy_window: u64,
    pub submission_lifetime: u64,
    pub algorithms: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Governance {
    pub max_governance_action_bytes: u32,
    pub governance_action_costs: [u64; 3],
    pub quorum_bps: u16,
    pub approval_bps: u16,
    pub veto_bps: u16,
    pub voting_period_blocks: u64,
    pub timelock_blocks: u64,
    pub max_voters: u32,
    pub deposit_period_blocks: u64,
    #[serde(with = "amount")]
    pub minimum_deposit_udgt: u128,
    pub max_action_bytes: u32,
    pub max_depositors: u32,
    pub action_classes: Vec<ActionClass>,
    pub bounds: GovernanceBounds,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionClass {
    pub class: u16,
    pub max_data_bytes: u32,
    /// SHA-256 of the class's P01 approval record, lowercase hex.
    pub approval_digest: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernanceBounds {
    pub gas_price: Bounds<u64>,
    pub resource_cost: Bounds<u64>,
    pub account_creation_fee_udrt: AmountBounds,
    pub min_self_bond: AmountBounds,
    pub max_active: Bounds<u64>,
    pub reference_send_fee_udrt: AmountBounds,
}

#[serde_as]
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmountBounds {
    #[serde_as(as = "DisplayFromStr")]
    pub min: u128,
    #[serde_as(as = "DisplayFromStr")]
    pub max: u128,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reward {
    pub max_positions: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Issuance {
    pub epoch_blocks: u64,
    pub max_recorded_epochs: u64,
    #[serde(with = "amount")]
    pub initial_epoch_budget_udrt: u128,
    pub controller: ControllerInputs,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Engine {
    pub block_max_bytes: i64,
    pub block_max_gas: i64,
    pub evidence_max_bytes: i64,
}

/// The root-signed controls. Their keys come from the custodian intakes.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Root {
    /// SHA-512 of the release manifest (E06), lowercase hex.
    pub release_sha512: String,
    pub emergency: EmergencyRoot,
    pub upgrade: UpgradeRoot,
    pub handover: HandoverRoot,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmergencyRoot {
    pub authority_epoch: u64,
    pub freeze: AuthorityPolicy,
    pub resume: AuthorityPolicy,
    pub max_validity_blocks: u64,
    pub max_anchor_age_blocks: u64,
}

/// Upgrade schema 2 (A3): its authority is the upgrade custodians'.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradeRoot {
    pub authority_epoch: u64,
    pub authority: AuthorityPolicy,
    pub migration_bounds: upgrade::v2::MigrationBounds,
    pub min_notice_blocks: u64,
    pub max_validity_blocks: u64,
    pub max_anchor_age_blocks: u64,
}

/// Release handover schema 2 (A3). The upgrade custodians control it
/// (P01, 30 September 2026), so its authority and epoch are the upgrade's.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandoverRoot {
    pub max_signatures: usize,
    pub min_notice_blocks: u64,
    pub max_validity_blocks: u64,
    pub max_anchor_age_blocks: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
    /// A public label from the records, used by validators and delegations.
    pub label: String,
    /// The account's ML-DSA-65 origin key; its address derives from it.
    pub origin_public_key_base64: String,
    #[serde(with = "amount")]
    pub udgt: u128,
    #[serde(with = "amount")]
    pub udrt: u128,
    pub vesting: Vesting,
}

#[serde_as]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Vesting {
    Unlocked,
    LinearAfterCliff {
        #[serde_as(as = "DisplayFromStr")]
        total_amount: u128,
        start_time: u64,
        cliff_duration: u64,
        vesting_duration: u64,
        allow_staking: bool,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Validator {
    pub validator_id: String,
    /// The label of the operator's owner account, which holds its self-bond.
    pub operator: String,
    pub consensus_public_key_base64: String,
    #[serde(with = "amount")]
    pub self_bond_udgt: u128,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delegation {
    pub account: String,
    pub validator_id: String,
    #[serde(with = "amount")]
    pub amount_udgt: u128,
}

/// A canonical decimal string: no sign, no leading zeros.
mod amount {
    use serde::{Deserialize, Deserializer};
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u128, D::Error> {
        let s = String::deserialize(d)?;
        let canonical = s == "0"
            || (!s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit()));
        if !canonical {
            return Err(serde::de::Error::custom(
                "amount must be a canonical decimal string",
            ));
        }
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// The three genesis files and their manifest.
pub struct Built {
    pub native_genesis: Vec<u8>,
    pub application_config: Vec<u8>,
    pub engine_genesis: Vec<u8>,
    pub manifest: Vec<u8>,
    pub config: ConsensusConfig,
}

fn identifier(value: &str, what: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
        "{what} must be 1 to 128 characters from [A-Za-z0-9._-]: {value:?}"
    );
    Ok(())
}

fn hex_digest(value: &str, bytes: usize, what: &str) -> Result<Vec<u8>> {
    ensure!(
        value.len() == bytes * 2
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "{what} must be {bytes} bytes of lowercase hexadecimal"
    );
    Ok(hex::decode(value)?)
}

fn mldsa65_key(value: &str, what: &str) -> Result<Vec<u8>> {
    let raw = B64
        .decode(value)
        .with_context(|| format!("{what}: invalid base64"))?;
    ensure!(
        raw.len() == MLDSA65_PUBLIC_KEY_BYTES && B64.encode(&raw) == value,
        "{what}: an ML-DSA-65 public key is {MLDSA65_PUBLIC_KEY_BYTES} bytes in canonical base64"
    );
    Ok(raw)
}

/// Whole seconds in UTC, as the engine writes them: `YYYY-MM-DDTHH:MM:SSZ`.
fn genesis_time(value: &str) -> Result<()> {
    let b = value.as_bytes();
    let digits = |r: std::ops::Range<usize>| r.into_iter().all(|i| b[i].is_ascii_digit());
    ensure!(
        b.len() == 20
            && digits(0..4)
            && b[4] == b'-'
            && digits(5..7)
            && b[7] == b'-'
            && digits(8..10)
            && b[10] == b'T'
            && digits(11..13)
            && b[13] == b':'
            && digits(14..16)
            && b[16] == b':'
            && digits(17..19)
            && b[19] == b'Z',
        "genesis_time must be whole seconds in UTC, as YYYY-MM-DDTHH:MM:SSZ"
    );
    let field = |r: std::ops::Range<usize>| value[r].parse::<u32>().expect("digits");
    ensure!(
        (1..=12).contains(&field(5..7))
            && (1..=31).contains(&field(8..10))
            && field(11..13) < 24
            && field(14..16) < 60
            && field(17..19) < 60,
        "genesis_time is out of range"
    );
    Ok(())
}

fn digest_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn prefixed(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

struct Member {
    address: String,
    account_id: [u8; 32],
    identity: KeyIdentity,
}

/// Build the three genesis files from `inputs`. The node's own configuration
/// validation runs on the result; `verify` also starts a chain from it.
pub fn build(inputs: &Inputs, inputs_bytes: &[u8]) -> Result<Built> {
    ensure!(inputs.schema == INPUTS_SCHEMA, "Unsupported inputs schema");
    ensure!(
        inputs.mode == MODE,
        "This build writes only the {MODE} mode"
    );
    let chain = inputs.chain_id.as_str();
    identifier(chain, "chain_id")?;
    ensure!(chain.len() <= 50, "chain_id is at most 50 bytes");
    ensure!(
        crate::build_profile::chain_id_allowed(chain),
        "A development build refuses a chain ID naming mainnet or production"
    );
    // A production configuration carries every root control (P01, 1 October
    // 2026).
    ensure!(
        !crate::build_profile::PRODUCTION || inputs.root.is_some(),
        "A production genesis needs the root controls"
    );
    genesis_time(&inputs.genesis_time)?;
    ensure!(
        inputs.account_template.algorithms
            == BTreeMap::from([(MLDSA65.to_string(), MLDSA65_PUBLIC_KEY_BYTES)]),
        "Accounts use ML-DSA-65 only"
    );

    // Accounts: the address and account ID derive from the origin key.
    let mut members = Vec::new();
    let mut labels = BTreeMap::new();
    let mut addresses = BTreeSet::new();
    for account in &inputs.accounts {
        identifier(&account.label, "account label")?;
        let key = mldsa65_key(&account.origin_public_key_base64, &account.label)?;
        let address = AccountAddress::from_origin_key(
            inputs.network.address(),
            chain,
            OriginKeyAlgorithm::MlDsa65,
            &key,
        )?;
        let encoded = address.encode();
        ensure!(
            addresses.insert(encoded.clone()),
            "Two accounts share an origin key"
        );
        ensure!(
            labels
                .insert(account.label.clone(), members.len())
                .is_none(),
            "Duplicate account label {}",
            account.label
        );
        if let Vesting::LinearAfterCliff { total_amount, .. } = &account.vesting {
            ensure!(
                *total_amount == account.udgt,
                "{}: vesting covers the whole DGT balance",
                account.label
            );
        }
        members.push(Member {
            address: encoded,
            account_id: *address.account_id(),
            identity: KeyIdentity {
                algorithm: MLDSA65.into(),
                public_key: key,
            },
        });
    }
    let dgt: u128 = inputs
        .accounts
        .iter()
        .try_fold(0u128, |sum, a| sum.checked_add(a.udgt))
        .context("DGT total overflows")?;
    ensure!(
        dgt == DGT_TOTAL_UDGT,
        "Genesis must issue exactly 1,000,000,000 DGT (D05-Q02); it issues {dgt} uDGT"
    );
    let drt: u128 = inputs
        .accounts
        .iter()
        .try_fold(0u128, |sum, a| sum.checked_add(a.udrt))
        .context("DRT total overflows")?;
    ensure!(
        drt == inputs.drt_bootstrap_total_udrt,
        "Account DRT ({drt}) must equal the approved bootstrap ({})",
        inputs.drt_bootstrap_total_udrt
    );
    let member = |label: &str| -> Result<&Member> {
        labels
            .get(label)
            .map(|&i| &members[i])
            .with_context(|| format!("Unknown account label {label}"))
    };

    // Validators, self-bonds and delegations become reward positions.
    ensure!(
        !inputs.validators.is_empty() && inputs.validators.len() <= MAX_VALIDATORS,
        "Genesis needs 1 to {MAX_VALIDATORS} validators"
    );
    let mut operators = BTreeMap::new();
    let mut consensus_keys = BTreeSet::new();
    let mut positions: BTreeMap<(String, String), u128> = BTreeMap::new();
    for v in &inputs.validators {
        identifier(&v.validator_id, "validator_id")?;
        let key = mldsa65_key(&v.consensus_public_key_base64, &v.validator_id)?;
        ensure!(
            consensus_keys.insert(key),
            "Two validators share a consensus key"
        );
        let owner = member(&v.operator)?.address.clone();
        ensure!(
            operators
                .insert(v.validator_id.clone(), owner.clone())
                .is_none(),
            "Duplicate validator {}",
            v.validator_id
        );
        ensure!(
            v.self_bond_udgt >= inputs.lifecycle.min_self_bond,
            "{}: self-bond is below min_self_bond",
            v.validator_id
        );
        positions.insert((owner, v.validator_id.clone()), v.self_bond_udgt);
    }
    for d in &inputs.delegations {
        ensure!(
            operators.contains_key(&d.validator_id),
            "Delegation to unknown validator {}",
            d.validator_id
        );
        ensure!(d.amount_udgt > 0, "A delegation must be positive");
        let owner = member(&d.account)?.address.clone();
        ensure!(
            positions
                .insert((owner, d.validator_id.clone()), d.amount_udgt)
                .is_none(),
            "{} already stakes with {}",
            d.account,
            d.validator_id
        );
    }
    let mut staked: BTreeMap<String, u128> = BTreeMap::new();
    let mut power: BTreeMap<String, u128> = BTreeMap::new();
    for ((owner, validator), amount) in &positions {
        let s = staked.entry(owner.clone()).or_default();
        *s = s.checked_add(*amount).context("Stake overflows")?;
        let p = power.entry(validator.clone()).or_default();
        *p = p.checked_add(*amount).context("Power overflows")?;
    }
    for (account, m) in inputs.accounts.iter().zip(&members) {
        ensure!(
            staked.get(&m.address).copied().unwrap_or(0) <= account.udgt,
            "{}: stakes more DGT than it holds",
            account.label
        );
    }

    // Native genesis: compact JSON with no trailing newline, so the engine
    // carries it unchanged as `app_state`.
    let timing = TimingGenesis {
        version: 1,
        profile: crate::build_profile::MONETARY_PROFILE.into(),
        decimals: 6,
        epoch_blocks: inputs.issuance.epoch_blocks,
        initial_epoch_budget_udrt: u64::try_from(inputs.issuance.initial_epoch_budget_udrt)
            .context("The first epoch budget exceeds u64")?,
        controller: inputs.issuance.controller.clone(),
        max_recorded_epochs: inputs.issuance.max_recorded_epochs,
    };
    timing.validate()?;
    let max_validators = usize::try_from(inputs.governance.bounds.max_active.max)?;
    let native = json!({
        "chain_id": chain,
        "accounts": inputs.accounts.iter().zip(&members).map(|(a, m)| json!({
            "address": m.address,
            "balances": {"udgt": a.udgt.to_string(), "udrt": a.udrt.to_string()},
            "vesting": serde_json::to_value(&a.vesting).expect("vesting serializes"),
        })).collect::<Vec<_>>(),
        "staking": {"delegations": staked.iter().map(|(owner, amount)| json!({
            "delegator": owner, "amount_udgt": amount.to_string(),
        })).collect::<Vec<_>>()},
        "reward_v2": {
            "version": 2, "activation_height": 1, "decimals": 6,
            "profile": crate::build_profile::MONETARY_PROFILE,
            // Room for every validator governance may activate (E05-a rule 4).
            "max_validators": max_validators,
            "max_positions": inputs.reward.max_positions,
            "validators": inputs.validators.iter().map(|v| json!({
                "address": v.validator_id, "active": true, "jailed": false,
            })).collect::<Vec<_>>(),
            "positions": positions.iter().map(|((owner, validator), amount)| json!({
                "owner": owner, "validator": validator, "amount_udgt": amount.to_string(),
            })).collect::<Vec<_>>(),
        },
        "adaptive_issuance": serde_json::to_value(&timing)?,
    });
    let native_genesis = serde_json::to_vec(&native)?;
    let app_digest: [u8; 32] = Sha256::digest(&native_genesis).into();
    let app_state_sha256 = hex::encode(app_digest);

    // Application configuration.
    let lifecycle = LifecycleConfig {
        version: 1,
        profile: crate::build_profile::LIFECYCLE_PROFILE.into(),
        chain_id: chain.into(),
        approved_operators: operators.clone(),
        min_self_bond: inputs.lifecycle.min_self_bond,
        max_active: inputs.lifecycle.max_active,
        evidence_max_age_blocks: inputs.lifecycle.evidence_max_age_blocks,
        evidence_max_age_seconds: inputs.lifecycle.evidence_max_age_seconds,
        processing_margin_blocks: inputs.lifecycle.processing_margin_blocks,
        processing_margin_seconds: inputs.lifecycle.processing_margin_seconds,
    };
    let template = RecoveryConfig {
        timing_version: inputs.account_template.timing_version,
        recovery_delay: inputs.account_template.recovery_delay,
        finalization_window: inputs.account_template.finalization_window,
        policy_delay: inputs.account_template.policy_delay,
        policy_window: inputs.account_template.policy_window,
        submission_lifetime: inputs.account_template.submission_lifetime,
        algorithms: inputs.account_template.algorithms.clone(),
    };
    let r = &inputs.recovery_profile;
    let recovery_profile = RecoveryProfile {
        version: 1,
        activation_height: 1,
        denomination: "udrt".into(),
        gas_price: r.gas_price,
        minimum_gas: r.minimum_gas,
        max_transaction_gas: r.max_transaction_gas,
        max_block_gas: r.max_block_gas,
        max_block_recovery_bytes: r.max_block_recovery_bytes,
        max_block_recovery_signatures: r.max_block_recovery_signatures,
        // Covers the largest fee a sponsor signs (E05-a rule 3).
        max_fee_cap: u128::from(r.max_transaction_gas) * u128::from(r.gas_price),
        max_pending_accounts: r.max_pending_accounts,
        max_due_expiry_events_per_height: r.max_due_expiry_events_per_height,
        mandatory_expiry_gas_budget: r.mandatory_expiry_gas_budget,
        expiry_event_gas_cost: r.expiry_event_gas_cost,
        action_costs: r.action_costs,
        wire_byte_cost: r.wire_byte_cost,
        read_byte_cost: r.read_byte_cost,
        write_byte_cost: r.write_byte_cost,
        signature_costs: r.signature_costs.clone(),
    };
    let accounts = members
        .iter()
        .map(|m| {
            Ok(RecoveryAccount {
                address: m.address.clone(),
                sponsor_nonce: 0,
                recovery: RecoveryState::new(
                    RecoveryDomain {
                        network: inputs.network.code(),
                        chain_id: chain.into(),
                        genesis_digest: app_digest,
                        account_id: m.account_id,
                    },
                    template.clone(),
                    m.identity.clone(),
                    0,
                )?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut book = RecoveryBook::new(recovery_profile, accounts)?;
    book.origins = members
        .iter()
        .map(|m| (hex::encode(m.account_id), m.identity.clone()))
        .collect();
    let o = &inputs.ordinary;
    let g = &inputs.governance;
    let fee_profile = FeeProfile {
        ordinary_fee_contract_version: 1,
        version: 1,
        activation_height: 1,
        denomination: Denomination::Udrt,
        gas_price: o.gas_price,
        minimum_gas: o.minimum_gas,
        max_transaction_gas: o.max_transaction_gas,
        max_block_transaction_gas: o.max_block_transaction_gas,
        max_block_transaction_bytes: o.max_block_transaction_bytes,
        max_block_signature_checks: o.max_block_signature_checks,
        // Covers the largest signable fee at the highest governed gas price
        // (E05-a rule 3): governance can raise the price but not the cap.
        max_fee_cap: u128::from(o.max_transaction_gas)
            * u128::from(g.bounds.gas_price.max.max(o.gas_price)),
        limits: Limits {
            max_wire_bytes: o.limits.max_wire_bytes,
            max_actions: o.limits.max_actions,
            max_identifier_bytes: o.limits.max_identifier_bytes,
            max_data_bytes: o.limits.max_data_bytes,
            max_memo_bytes: o.limits.max_memo_bytes,
            max_consensus_key_bytes: o.limits.max_consensus_key_bytes,
            max_proof_bytes: o.limits.max_proof_bytes,
            max_expiry_lifetime: o.limits.max_expiry_lifetime,
            allowed_algorithms: o.limits.allowed_algorithms.clone(),
        },
        transaction_overhead: o.transaction_overhead,
        receipt_metadata_cost: o.receipt_metadata_cost,
        wire_byte_cost: o.wire_byte_cost,
        read_byte_cost: o.read_byte_cost,
        write_byte_cost: o.write_byte_cost,
        action_costs: o.action_costs,
        signature_costs: o.signature_costs.clone(),
        validator_proof_profile_digest: validator_profile_digest(&lifecycle)?,
        validator_proof_costs: o.validator_proof_costs.clone(),
        account_creation_fee_udrt: o.account_creation_fee_udrt,
    };
    let ordinary = OrdinaryConfig {
        version: 1,
        fee_profile: fee_profile.clone(),
        account_template: AccountTemplate { recovery: template },
        initial_grants: BTreeMap::new(),
        max_state_bytes: o.max_state_bytes,
        max_grants: o.max_grants,
        max_receipts: o.max_receipts,
        max_retained_profiles: o.max_retained_profiles,
        // A full envelope in base64 transport (E05-a rule 5).
        max_transport_bytes: u64::from(o.limits.max_wire_bytes).div_ceil(3) * 4
            + TRANSPORT_OVERHEAD,
        queue_max_entries: o.queue_max_entries,
        queue_max_wire_bytes: o.queue_max_wire_bytes,
        queue_max_signature_work: o.queue_max_signature_work,
    };
    ensure!(
        g.max_governance_action_bytes == g.max_action_bytes,
        "The v3 fee profile and deposit rules carry the same governance action bound"
    );
    let governance = GovernanceCandidateConfig {
        schema_version: CANDIDATE_SCHEMA_VERSION,
        chain_id: chain.into(),
        genesis_digest: app_digest,
        activation_height: 1,
        fee_profile: FeeProfileV3 {
            base: fee_profile,
            version: 1,
            activation_height: 1,
            max_governance_action_bytes: g.max_governance_action_bytes,
            governance_action_costs: g.governance_action_costs,
        },
        ballot: BallotRules {
            version: 1,
            chain_id: chain.into(),
            genesis_digest: app_digest,
            quorum_bps: g.quorum_bps,
            approval_bps: g.approval_bps,
            veto_bps: g.veto_bps,
            voting_period_blocks: g.voting_period_blocks,
            timelock_blocks: g.timelock_blocks,
            max_voters: g.max_voters,
        },
        deposit: DepositRules {
            deposit_period_blocks: g.deposit_period_blocks,
            minimum_deposit_udgt: g.minimum_deposit_udgt,
            max_action_bytes: g.max_action_bytes,
            max_depositors: g.max_depositors,
        },
        action_classes: g
            .action_classes
            .iter()
            .map(|c| {
                Ok(ActionClassLimit {
                    class: c.class,
                    max_data_bytes: c.max_data_bytes,
                    approval_digest: hex_digest(&c.approval_digest, 32, "approval_digest")?
                        .try_into()
                        .expect("32 bytes"),
                })
            })
            .collect::<Result<Vec<_>>>()?,
        parameter_bounds: ParameterBounds {
            gas_price: g.bounds.gas_price.clone(),
            resource_cost: g.bounds.resource_cost.clone(),
            account_creation_fee_udrt: Bounds {
                min: g.bounds.account_creation_fee_udrt.min,
                max: g.bounds.account_creation_fee_udrt.max,
            },
            min_self_bond: Bounds {
                min: g.bounds.min_self_bond.min,
                max: g.bounds.min_self_bond.max,
            },
            max_active: g.bounds.max_active.clone(),
            reference_send_fee_udrt: Bounds {
                min: g.bounds.reference_send_fee_udrt.min,
                max: g.bounds.reference_send_fee_udrt.max,
            },
        },
        entry_policy: EntryPolicy {
            proposer_eligibility:
                ProposerEligibility::RegisteredOwnerWithEffectiveBondAtFinalizedParent,
            validator_voting: ValidatorVoting::OwnEffectiveBondOnly,
            vote_delegation: VoteDelegation::Disabled,
            cancellation: Cancellation::Disabled,
        },
    };
    let (emergency, upgrade, release_handover) = match &inputs.root {
        None => (None, None, None),
        Some(root) => {
            hex_digest(&root.release_sha512, 64, "release_sha512")?;
            let e = &root.emergency;
            // A root control carries its threshold's signatures and must fit
            // one transaction (E05-a rule 6), so each bound is the largest
            // transaction and each signature count the threshold.
            // Each build writes its own control policies (A4): development
            // policies are development-only and keep the development name
            // of the approved transition rule.
            let production = crate::build_profile::PRODUCTION;
            let emergency = emergency::Policy {
                schema: 2,
                development_only: !production,
                chain_id: chain.into(),
                release_sha512: root.release_sha512.clone(),
                initial_sequence: 1,
                freeze_authority: e.freeze.clone(),
                resume_authority: e.resume.clone(),
                max_control_bytes: inputs.application.max_tx_bytes,
                max_signatures: e.freeze.threshold.max(e.resume.threshold),
                automatic_transition_policy: if production {
                    AutomaticTransitionPolicy::ContinuePreviouslyApprovedRules
                } else {
                    AutomaticTransitionPolicy::ContinueExisting
                },
                v2: Some(emergency::PolicyV2 {
                    genesis_sha256: app_state_sha256.clone(),
                    authority_epoch: e.authority_epoch,
                    max_validity_blocks: e.max_validity_blocks,
                    max_anchor_age_blocks: e.max_anchor_age_blocks,
                }),
            };
            // Upgrade and handover schema 2 (A3) in both builds: anchored
            // windows and the minimum notice.
            let u = &root.upgrade;
            let upgrade = upgrade::Policy::V2(upgrade::v2::Policy {
                schema: 2,
                chain_id: chain.into(),
                genesis_sha256: app_state_sha256.clone(),
                initial_release_sha512: root.release_sha512.clone(),
                authority_epoch: u.authority_epoch,
                authority: u.authority.clone(),
                initial_sequence: 1,
                max_control_bytes: inputs.application.max_tx_bytes,
                max_signatures: u.authority.threshold,
                migration_bounds: u.migration_bounds.clone(),
                min_notice_blocks: u.min_notice_blocks,
                max_validity_blocks: u.max_validity_blocks,
                max_anchor_age_blocks: u.max_anchor_age_blocks,
            });
            let h = &root.handover;
            let handover = handover::Policy {
                schema: 2,
                development_only: !production,
                chain_id: chain.into(),
                genesis_sha256: app_state_sha256.clone(),
                initial_release_sha512: root.release_sha512.clone(),
                initial_schema: 0,
                authority_epoch: u.authority_epoch,
                authority: u.authority.clone(),
                initial_sequence: 1,
                max_control_bytes: inputs.application.max_tx_bytes,
                max_signatures: h.max_signatures,
                v2: Some(handover::PolicyV2 {
                    min_notice_blocks: h.min_notice_blocks,
                    max_validity_blocks: h.max_validity_blocks,
                    max_anchor_age_blocks: h.max_anchor_age_blocks,
                }),
            };
            (Some(emergency), Some(upgrade), Some(handover))
        }
    };
    let validators = inputs
        .validators
        .iter()
        .map(|v| {
            Ok(ValidatorConfig {
                pubkey_type: "ml_dsa_65".into(),
                pubkey_base64: v.consensus_public_key_base64.clone(),
                // Voting power is the stake bonded to the validator, in uDGT.
                power: i64::try_from(power[&v.validator_id])
                    .context("Validator power exceeds i64")?,
                reward_address: v.validator_id.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let config = ConsensusConfig {
        profile: crate::build_profile::PENALTY_PROFILE.into(),
        engine: ENGINE.into(),
        chain_id: chain.into(),
        app_state_sha256: app_state_sha256.clone(),
        // The top-level price only has a range check; it equals the ordinary one.
        gas_price: o.gas_price,
        max_tx_bytes: inputs.application.max_tx_bytes,
        max_block_bytes: inputs.application.max_block_bytes,
        max_txs: inputs.application.max_txs,
        validators: validators.clone(),
        lifecycle: Some(lifecycle),
        penalty: Some(PenaltyConfig {
            version: 1,
            profile: crate::build_profile::PENALTY_PROFILE.into(),
            chain_id: chain.into(),
            penalty_numerator: inputs.penalty.numerator,
            penalty_denominator: inputs.penalty.denominator,
            // Penalties v1 is complete; production builds activate it.
            production_activation: crate::build_profile::PRODUCTION,
        }),
        recovery: Some(book),
        ordinary: Some(ordinary),
        governance: Some(governance),
        emergency,
        upgrade,
        release_handover,
    };
    config.validate()?;
    let application_config = serde_json::to_vec(&config)?;
    ensure!(
        application_config.len() <= MAX_CONFIG_BYTES,
        "Configuration exceeds its bound"
    );

    let engine_genesis = engine_genesis(inputs, &validators, &native_genesis)?;
    genesis_sizes(&native_genesis, &engine_genesis)?;

    let mut hasher = Sha256::new();
    hasher.update(BUILD_DOMAIN);
    for file in [&native_genesis, &application_config, &engine_genesis] {
        prefixed(&mut hasher, file);
    }
    let file = |bytes: &[u8]| json!({"bytes": bytes.len(), "sha256": digest_hex(bytes), "sha512": hex::encode(Sha512::digest(bytes))});
    let manifest = json!({
        "schema": MANIFEST_SCHEMA,
        "mode": MODE,
        "production": crate::build_profile::PRODUCTION,
        "boundary": if crate::build_profile::PRODUCTION {
            "Production-profile output. Building accepts nothing: approved values for every open input, accepted records, the release, the root genesis signatures and gate acceptance remain required."
        } else {
            "Rehearsal output in the development profiles. Not a production genesis: a production build, approved values for every open input, accepted records, the root genesis signatures and gate acceptance remain required."
        },
        "chain_id": chain,
        "genesis_time": inputs.genesis_time,
        "inputs_sha256": digest_hex(inputs_bytes),
        "files": {
            "native-genesis.json": file(&native_genesis),
            "application-config.json": file(&application_config),
            "genesis.json": file(&engine_genesis),
        },
        "build_digest": hex::encode(hasher.finalize()),
    });
    let mut manifest = serde_json::to_vec_pretty(&manifest)?;
    manifest.push(b'\n');
    Ok(Built {
        native_genesis,
        application_config,
        engine_genesis,
        manifest,
        config,
    })
}

/// The engine genesis in the exact bytes the Go engine writes (compact, its
/// field order, 64-bit integers as strings, durations in nanoseconds) with the
/// native genesis spliced in as `app_state`. The Go fixture test re-encodes it
/// and requires the same bytes.
/// Each genesis must fit the bound of the component that reads it, or the
/// chain cannot start (production activation v1, A6).
fn genesis_sizes(native: &[u8], engine: &[u8]) -> Result<()> {
    ensure!(
        native.len() <= MAX_GENESIS_BYTES,
        "Native genesis exceeds its bound"
    );
    ensure!(
        engine.len() <= MAX_ENGINE_GENESIS_BYTES,
        "Engine genesis exceeds the engine's bound"
    );
    Ok(())
}

fn engine_genesis(
    inputs: &Inputs,
    validators: &[ValidatorConfig],
    native: &[u8],
) -> Result<Vec<u8>> {
    let e = &inputs.engine;
    ensure!(
        e.block_max_bytes > 0 && e.block_max_bytes <= 104_857_600,
        "Engine block max_bytes is 1 to 104,857,600"
    );
    ensure!(e.block_max_gas >= -1, "Engine block max_gas is at least -1");
    ensure!(
        (0..=e.block_max_bytes).contains(&e.evidence_max_bytes),
        "Engine evidence max_bytes is 0 to the block bound"
    );
    let l = &inputs.lifecycle;
    // The application requires these to equal the lifecycle's evidence limits
    // exactly, in whole seconds.
    let duration = i64::try_from(l.evidence_max_age_seconds)
        .ok()
        .and_then(|s| s.checked_mul(1_000_000_000))
        .context("Evidence age exceeds the engine's duration range")?;
    let mut entries = Vec::new();
    for v in validators {
        let key = B64.decode(&v.pubkey_base64)?;
        let address = hex::encode_upper(&Sha256::digest(&key)[..20]);
        let name = &v.reward_address;
        entries.push(format!(
            r#"{{"address":"{address}","pub_key":{{"type":"cometbft/PubKeyMlDsa65","value":"{}"}},"power":"{}","name":"{name}"}}"#,
            v.pubkey_base64, v.power
        ));
    }
    let head = format!(
        concat!(
            r#"{{"genesis_time":"{time}","chain_id":"{chain}","initial_height":"1","#,
            r#""consensus_params":{{"block":{{"max_bytes":"{block_bytes}","max_gas":"{block_gas}"}},"#,
            r#""evidence":{{"max_age_num_blocks":"{age_blocks}","max_age_duration":"{duration}","max_bytes":"{evidence_bytes}"}},"#,
            r#""validator":{{"pub_key_types":["ml_dsa_65"]}},"version":{{"app":"0"}},"#,
            r#""abci":{{"vote_extensions_enable_height":"0"}},"authority":{{"authority":""}}}},"#,
            r#""validators":[{validators}],"app_hash":"","app_state":"#
        ),
        time = inputs.genesis_time,
        chain = inputs.chain_id,
        block_bytes = e.block_max_bytes,
        block_gas = e.block_max_gas,
        age_blocks = l.evidence_max_age_blocks,
        duration = duration,
        evidence_bytes = e.evidence_max_bytes,
        validators = entries.join(","),
    );
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(native);
    bytes.push(b'}');
    Ok(bytes)
}

/// Start a chain from the build: open the application on a fresh database and
/// run InitChain with the engine genesis's validators. The root-signed
/// controls open only with the development root helper, so this check runs
/// without them; `ConsensusConfig::validate` has already checked them in
/// `build`. Returns the genesis application hash. A production genesis opens
/// only from its root genesis signed three of five, so a production build
/// has no unsigned start; the signed production test opens it.
pub fn verify(built: &Built, db: &Path) -> Result<String> {
    crate::build_profile::development_entry()?;
    ensure!(
        !db.exists(),
        "The verification database path must not exist"
    );
    let mut config = built.config.clone();
    config.emergency = None;
    config.upgrade = None;
    config.release_handover = None;
    let mut app = ConsensusApplication::open(db, config.clone(), built.native_genesis.clone())?;
    let info = app.init_chain(
        &config.chain_id,
        1,
        &built.native_genesis,
        &config.validators,
    )?;
    Ok(info.app_hash)
}

/// Parse an inputs file strictly.
pub fn read_inputs(bytes: &[u8]) -> Result<Inputs> {
    let inputs: Inputs = serde_json::from_slice(bytes).context("Invalid genesis build inputs")?;
    if inputs.schema != INPUTS_SCHEMA {
        bail!("Unsupported inputs schema {}", inputs.schema);
    }
    Ok(inputs)
}

#[cfg(test)]
#[path = "genesis_build_tests.rs"]
mod tests;
