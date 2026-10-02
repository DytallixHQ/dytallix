//! Governance v1 configuration (T6, `docs/architecture/governance-v1.md`).
//! Every numeric value is an E05 input; no field has a default. Rules approved
//! on 25 and 27 September 2026 are fixed by these types: bonded-stake voting
//! by each owner, no delegation, no cancellation, parameter change and
//! validator registry as the only action classes.
use anyhow::{ensure, Context, Result};
use dytallix_protocol_types::ordinary_fees_v3::FeeProfileV3;
use serde::{Deserialize, Serialize};

pub const CANDIDATE_SCHEMA_VERSION: u16 = 2;
const DGT_SUPPLY: u128 = dytallix_protocol_types::units::DGT_TOTAL_BASE_UNITS;

/// Action class codes. Upgrades stay root-signed; treasury spending is
/// POST MAINNET.
pub const CLASS_PARAMETER_CHANGE: u16 = 1;
pub const CLASS_VALIDATOR_REGISTRY: u16 = 2;
pub const IMPLEMENTED_CLASSES: [u16; 2] = [CLASS_PARAMETER_CHANGE, CLASS_VALIDATOR_REGISTRY];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BallotRules {
    pub version: u16,
    pub chain_id: String,
    pub genesis_digest: [u8; 32],
    pub quorum_bps: u16,
    pub approval_bps: u16,
    pub veto_bps: u16,
    pub voting_period_blocks: u64,
    pub timelock_blocks: u64,
    pub max_voters: u32,
}
impl BallotRules {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "Unsupported governance ballot version");
        ensure!(
            !self.chain_id.is_empty()
                && self.chain_id.len() <= 128
                && !self.chain_id.chars().any(char::is_control),
            "Invalid governance chain ID"
        );
        ensure!(
            self.genesis_digest != [0; 32],
            "Governance genesis digest is absent"
        );
        // A zero threshold is never usable (E05-a): a zero veto threshold
        // fails every proposal, and zero quorum or approval pass any vote.
        ensure!(
            (1..=10_000).contains(&self.quorum_bps)
                && (1..=10_000).contains(&self.approval_bps)
                && (1..=10_000).contains(&self.veto_bps),
            "Governance basis points must be from 1 through 10000"
        );
        ensure!(
            self.voting_period_blocks > 0 && self.timelock_blocks > 0 && self.max_voters > 0,
            "Governance periods and voter capacity must be explicit and positive"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DepositRules {
    pub deposit_period_blocks: u64,
    pub minimum_deposit_udgt: u128,
    pub max_action_bytes: u32,
    pub max_depositors: u32,
}
impl DepositRules {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.deposit_period_blocks > 0,
            "Deposit period must be positive"
        );
        ensure!(
            self.minimum_deposit_udgt > 0 && self.minimum_deposit_udgt <= DGT_SUPPLY,
            "Minimum governance deposit is outside DGT supply"
        );
        ensure!(
            self.max_action_bytes > 0 && self.max_depositors > 0,
            "Governance action and depositor bounds must be positive"
        );
        Ok(())
    }
}

/// An enabled action class and its byte bound. `approval_digest` identifies
/// the P01 approval record for the class.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionClassLimit {
    pub class: u16,
    pub max_data_bytes: u32,
    pub approval_digest: [u8; 32],
}

/// Inclusive bounds fixed at genesis. A governed value outside them fails
/// execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds<T> {
    pub min: T,
    pub max: T,
}
impl<T: PartialOrd + Copy> Bounds<T> {
    pub fn contains(&self, value: T) -> bool {
        self.min <= value && value <= self.max
    }
    fn validate(&self, floor: T) -> Result<()> {
        ensure!(
            floor <= self.min && self.min <= self.max,
            "Governed parameter bounds are inverted or below their floor"
        );
        Ok(())
    }
}

/// Bounds for the parameters governance may change (P01, 27 September
/// 2026): new ordinary fee profile versions and two validator limits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterBounds {
    pub gas_price: Bounds<u64>,
    /// Every per-resource cost in a governed fee profile.
    pub resource_cost: Bounds<u64>,
    pub account_creation_fee_udrt: Bounds<u128>,
    pub min_self_bond: Bounds<u128>,
    pub max_active: Bounds<u64>,
    /// What a reference basic Send may cost, in uDRT, under a governed fee
    /// profile and at genesis (P01, 30 September and 2 October 2026).
    pub reference_send_fee_udrt: Bounds<u128>,
}
impl ParameterBounds {
    pub fn validate(&self) -> Result<()> {
        self.gas_price.validate(1)?;
        // A resource may be free, as in a genesis profile.
        self.resource_cost.validate(0)?;
        self.account_creation_fee_udrt.validate(1)?;
        self.reference_send_fee_udrt.validate(1)?;
        self.min_self_bond.validate(1)?;
        self.max_active.validate(1)?;
        ensure!(
            self.max_active.max <= 64,
            "Governed active validator bound exceeds 64"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposerEligibility {
    RegisteredOwnerWithEffectiveBondAtFinalizedParent,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidatorVoting {
    OwnEffectiveBondOnly,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoteDelegation {
    Disabled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cancellation {
    Disabled,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryPolicy {
    pub proposer_eligibility: ProposerEligibility,
    pub validator_voting: ValidatorVoting,
    pub vote_delegation: VoteDelegation,
    pub cancellation: Cancellation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernanceCandidateConfig {
    pub schema_version: u16,
    pub chain_id: String,
    pub genesis_digest: [u8; 32],
    pub activation_height: u64,
    pub fee_profile: FeeProfileV3,
    pub ballot: BallotRules,
    pub deposit: DepositRules,
    pub action_classes: Vec<ActionClassLimit>,
    pub parameter_bounds: ParameterBounds,
    pub entry_policy: EntryPolicy,
}

impl GovernanceCandidateConfig {
    pub fn validate_shape(&self) -> Result<()> {
        ensure!(
            self.schema_version == CANDIDATE_SCHEMA_VERSION,
            "Unsupported governance candidate schema"
        );
        ensure!(
            !self.chain_id.is_empty()
                && self.chain_id.len() <= 128
                && !self.chain_id.chars().any(char::is_control),
            "Invalid governance candidate chain ID"
        );
        ensure!(
            self.genesis_digest != [0; 32],
            "Governance candidate genesis digest is absent"
        );
        ensure!(
            self.activation_height > 0,
            "Governance candidate activation height is absent"
        );
        self.fee_profile
            .validate()
            .map_err(|e| anyhow::anyhow!("Invalid governance fee profile: {e}"))?;
        self.ballot.validate()?;
        self.deposit.validate()?;
        self.parameter_bounds.validate()?;
        ensure!(
            self.ballot.chain_id == self.chain_id
                && self.ballot.genesis_digest == self.genesis_digest,
            "Governance ballot and candidate chain differ"
        );
        ensure!(
            self.fee_profile.activation_height == self.activation_height
                && self.fee_profile.max_governance_action_bytes == self.deposit.max_action_bytes,
            "Governance fee, deposit and activation bounds differ"
        );
        ensure!(
            self.fee_profile
                .governance_action_costs
                .iter()
                .all(|cost| *cost > 0),
            "Governance action costs must be explicit and positive"
        );
        self.activation_height
            .checked_add(self.deposit.deposit_period_blocks)
            .and_then(|h| h.checked_add(self.ballot.voting_period_blocks))
            .and_then(|h| h.checked_add(self.ballot.timelock_blocks))
            .and_then(|h| h.checked_add(2))
            .context("Governance candidate height range overflows")?;
        ensure!(
            self.action_classes
                .windows(2)
                .all(|pair| pair[0].class < pair[1].class),
            "Governance action classes must be sorted and unique"
        );
        for class in &self.action_classes {
            ensure!(
                class.class > 0
                    && class.max_data_bytes > 0
                    && class.max_data_bytes <= self.deposit.max_action_bytes
                    && class.approval_digest != [0; 32],
                "Invalid governance action-class approval or byte bound"
            );
        }
        Ok(())
    }

    /// A candidate activates when every enabled class has an executor.
    pub fn validate_for_activation(&self) -> Result<()> {
        self.validate_shape()?;
        ensure!(
            !self.action_classes.is_empty(),
            "Governance enables no action class"
        );
        ensure!(
            self.action_classes
                .iter()
                .all(|class| IMPLEMENTED_CLASSES.contains(&class.class)),
            "Governance action class has no approved executor"
        );
        Ok(())
    }

    pub fn class(&self, class: u16) -> Option<&ActionClassLimit> {
        self.action_classes.iter().find(|item| item.class == class)
    }

    pub fn store_rules(&self) -> super::governance_store::StoreRules {
        super::governance_store::StoreRules {
            deposit: self.deposit.clone(),
            ballot: self.ballot.clone(),
        }
    }
}

/// The caller must supply a candidate. Missing configuration fails closed.
pub fn require_candidate_config(input: Option<&GovernanceCandidateConfig>) -> Result<()> {
    input
        .context("Governance candidate configuration is absent")?
        .validate_for_activation()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use dytallix_protocol_types::{ordinary, ordinary_fees::FeeProfile};
    use std::collections::{BTreeMap, BTreeSet};

    // Test values have no production authority.
    pub(crate) fn example() -> GovernanceCandidateConfig {
        let base = FeeProfile {
            ordinary_fee_contract_version: 1,
            version: 1,
            activation_height: 1,
            denomination: ordinary::Denomination::Udrt,
            gas_price: 1,
            minimum_gas: 1,
            max_transaction_gas: 1000,
            max_block_transaction_gas: 1000,
            max_block_transaction_bytes: 100_000,
            max_block_signature_checks: 1,
            max_fee_cap: 1000,
            limits: ordinary::Limits {
                max_wire_bytes: 100_000,
                max_actions: 1,
                max_identifier_bytes: 100,
                max_data_bytes: 100,
                max_memo_bytes: 100,
                max_consensus_key_bytes: 100,
                max_proof_bytes: 100,
                max_expiry_lifetime: 100,
                allowed_algorithms: BTreeSet::from(["mldsa65".into()]),
            },
            transaction_overhead: 1,
            receipt_metadata_cost: 1,
            wire_byte_cost: 1,
            read_byte_cost: 1,
            write_byte_cost: 1,
            action_costs: [1; 12],
            signature_costs: BTreeMap::from([("mldsa65".into(), 1)]),
            validator_proof_profile_digest: [1; 32],
            validator_proof_costs: BTreeMap::from([("mldsa65".into(), 1)]),
            account_creation_fee_udrt: 1_000,
        };
        GovernanceCandidateConfig {
            schema_version: CANDIDATE_SCHEMA_VERSION,
            chain_id: "test-chain".into(),
            genesis_digest: [2; 32],
            activation_height: 2,
            fee_profile: FeeProfileV3 {
                base,
                version: 2,
                activation_height: 2,
                max_governance_action_bytes: 100,
                governance_action_costs: [1; 3],
            },
            ballot: BallotRules {
                version: 1,
                chain_id: "test-chain".into(),
                genesis_digest: [2; 32],
                quorum_bps: 1,
                approval_bps: 1,
                veto_bps: 1,
                voting_period_blocks: 1,
                timelock_blocks: 1,
                max_voters: 1,
            },
            deposit: DepositRules {
                deposit_period_blocks: 1,
                minimum_deposit_udgt: 1,
                max_action_bytes: 100,
                max_depositors: 4,
            },
            action_classes: vec![ActionClassLimit {
                class: CLASS_PARAMETER_CHANGE,
                max_data_bytes: 100,
                approval_digest: [4; 32],
            }],
            parameter_bounds: ParameterBounds {
                gas_price: Bounds { min: 1, max: 10 },
                resource_cost: Bounds { min: 1, max: 100 },
                account_creation_fee_udrt: Bounds {
                    min: 1,
                    max: 10_000,
                },
                min_self_bond: Bounds { min: 1, max: 1_000 },
                max_active: Bounds { min: 1, max: 64 },
                reference_send_fee_udrt: Bounds {
                    min: 1,
                    max: 1_000_000_000,
                },
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
    fn activation_needs_an_enabled_class_with_an_executor() {
        assert!(require_candidate_config(None).is_err());
        let config = example();
        assert!(config.validate_for_activation().is_ok());
        let mut none = config.clone();
        none.action_classes.clear();
        assert!(none.validate_shape().is_ok());
        assert!(none.validate_for_activation().is_err());
        let mut unknown = config;
        unknown.action_classes.push(ActionClassLimit {
            class: 3,
            max_data_bytes: 1,
            approval_digest: [4; 32],
        });
        assert!(unknown.validate_shape().is_ok());
        assert!(unknown.validate_for_activation().is_err());
    }

    #[test]
    fn invalid_bindings_and_limits_fail_closed() {
        let mut config = example();
        config.ballot.genesis_digest = [3; 32];
        assert!(config.validate_shape().is_err());
        config = example();
        config.deposit.deposit_period_blocks = 0;
        assert!(config.validate_shape().is_err());
        config = example();
        config.deposit.max_depositors = 0;
        assert!(config.validate_shape().is_err());
        config = example();
        config.activation_height = 0;
        assert!(config.validate_shape().is_err());
        config = example();
        config.fee_profile.governance_action_costs[0] = 0;
        assert!(config.validate_shape().is_err());
        config = example();
        config.parameter_bounds.max_active.max = 65;
        assert!(config.validate_shape().is_err());
        config = example();
        config.parameter_bounds.gas_price = Bounds { min: 5, max: 4 };
        assert!(config.validate_shape().is_err());
        config = example();
        let class = config.action_classes[0].clone();
        config.action_classes.push(class);
        assert!(config.validate_shape().is_err());
    }

    #[test]
    fn incomplete_json_cannot_supply_defaults() {
        let config = example();
        let mut value = serde_json::to_value(config).unwrap();
        value.as_object_mut().unwrap().remove("parameter_bounds");
        assert!(serde_json::from_value::<GovernanceCandidateConfig>(value).is_err());
    }
}
