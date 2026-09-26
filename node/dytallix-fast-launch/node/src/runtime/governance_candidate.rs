//! Inert E04 governance candidate input. This module does not activate v3,
//! authorize an action class, or write consensus state.
use super::governance_ballot::Rules as BallotRules;
use super::governance_deposit_stage::DepositRules;
use anyhow::{bail, ensure, Context, Result};
use dytallix_protocol_types::ordinary_fees_v3::FeeProfileV3;
use serde::{Deserialize, Serialize};

pub const CANDIDATE_SCHEMA_VERSION: u16 = 1;

/// An action class needs a separate approval and an exact byte bound.
/// No action class is approved in the current E04 decision record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionClassLimit {
    pub class: u16,
    pub max_data_bytes: u32,
    pub approval_digest: [u8; 32],
}

/// Exact proposal, vote, delegation, and cancellation rules approved for
/// engineering. This type does not implement any of those state transitions.
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

/// Exact execution transitions remain unapproved. There is no `Approved`
/// variant that can turn a document reference into runtime authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDecision {
    Pending,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingPolicies {
    pub exact_state_transitions: PolicyDecision,
}

/// Every numeric input must come from a later candidate decision. No field
/// has a default. `validate_shape` checks the input but grants no activation.
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
    pub entry_policy: EntryPolicy,
    pub pending_policies: PendingPolicies,
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

    /// Current decisions permit local engineering only. Exact state
    /// transitions remain pending, so this input cannot activate governance.
    pub fn validate_for_activation(&self) -> Result<()> {
        self.validate_shape()?;
        ensure!(
            !self.action_classes.is_empty(),
            "Governance action classes are not approved"
        );
        bail!("Governance exact state transitions remain unapproved")
    }
}

/// The caller must supply a candidate. Missing configuration fails closed.
pub fn require_candidate_config(input: Option<&GovernanceCandidateConfig>) -> Result<()> {
    input
        .context("Governance candidate configuration is absent")?
        .validate_for_activation()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dytallix_protocol_types::{ordinary, ordinary_fees::FeeProfile};
    use std::collections::{BTreeMap, BTreeSet};

    // Test values have no production authority.
    fn example() -> GovernanceCandidateConfig {
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
            },
            action_classes: vec![],
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
    fn missing_and_pending_config_cannot_activate() {
        assert!(require_candidate_config(None).is_err());
        let config = example();
        assert!(config.validate_shape().is_ok());
        assert!(require_candidate_config(Some(&config)).is_err());
        let mut with_class = config;
        with_class.action_classes.push(ActionClassLimit {
            class: 1,
            max_data_bytes: 1,
            approval_digest: [4; 32],
        });
        assert!(with_class.validate_shape().is_ok());
        assert!(with_class.validate_for_activation().is_err());
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
        config.activation_height = 0;
        assert!(config.validate_shape().is_err());
        config = example();
        config.fee_profile.governance_action_costs[0] = 0;
        assert!(config.validate_shape().is_err());
        config = example();
        config.action_classes = vec![
            ActionClassLimit {
                class: 1,
                max_data_bytes: 1,
                approval_digest: [4; 32],
            },
            ActionClassLimit {
                class: 1,
                max_data_bytes: 1,
                approval_digest: [4; 32],
            },
        ];
        assert!(config.validate_shape().is_err());
    }

    #[test]
    fn incomplete_json_cannot_supply_defaults() {
        let config = example();
        let mut value = serde_json::to_value(config).unwrap();
        value.as_object_mut().unwrap().remove("pending_policies");
        assert!(serde_json::from_value::<GovernanceCandidateConfig>(value).is_err());
    }
}
