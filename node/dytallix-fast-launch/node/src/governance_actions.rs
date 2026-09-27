//! Governance action classes (T6, P01 27 September 2026): parameter change
//! and validator registry. Upgrades stay root-signed; treasury spending is
//! POST MAINNET.
//!
//! An action is checked when proposed and again when it executes. At
//! execution it applies to a copy of the block state, which must still pass
//! the lifecycle and fee-profile checks; otherwise it fails execution and the
//! deposits are refunded. A change takes effect at its execution height,
//! before that block's transactions.
use crate::{
    ordinary_state::OrdinaryState,
    recovery_fees::RecoveryBook,
    runtime::{
        governance_candidate::{
            GovernanceCandidateConfig, CLASS_PARAMETER_CHANGE, CLASS_VALIDATOR_REGISTRY,
        },
        governance_store::{GovernedParameters, Proposal, Rule},
    },
    settlement::Settlement,
};
use bincode::Options;
use dytallix_protocol_types::{ordinary_fees::FeeProfile, ordinary_fees_v3::FeeProfileV3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The fee values governance may set (gas price, per-resource costs and the
/// account creation fee). Every other profile field stays unchanged.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeeValues {
    pub gas_price: u64,
    pub transaction_overhead: u64,
    pub receipt_metadata_cost: u64,
    pub wire_byte_cost: u64,
    pub read_byte_cost: u64,
    pub write_byte_cost: u64,
    pub action_costs: [u64; 12],
    pub signature_costs: BTreeMap<String, u64>,
    pub validator_proof_costs: BTreeMap<String, u64>,
    pub governance_action_costs: [u64; 3],
    pub account_creation_fee_udrt: u128,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParameterChange {
    Fees(FeeValues),
    MinSelfBond(u128),
    MaxActive(u64),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegistryChange {
    /// Approve an operator: validator ID and its owner's native address.
    Add { validator_id: String, owner: String },
    /// Withdraw an approval that no retained validator record uses.
    Remove { validator_id: String },
}

fn options() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .reject_trailing_bytes()
}
/// Canonical action bytes for a client building a proposal.
pub fn encode<T: Serialize>(value: &T) -> anyhow::Result<Vec<u8>> {
    Ok(options().serialize(value)?)
}
fn decode<T: Serialize + for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, Rule> {
    let value: T = options()
        .with_limit(bytes.len() as u64)
        .deserialize(bytes)
        .map_err(|_| Rule("GOVERNANCE_ACTION_MALFORMED"))?;
    if encode(&value).ok().as_deref() != Some(bytes) {
        return Err(Rule("GOVERNANCE_ACTION_MALFORMED"));
    }
    Ok(value)
}

fn costs(values: &FeeValues) -> impl Iterator<Item = u64> + '_ {
    [
        values.transaction_overhead,
        values.receipt_metadata_cost,
        values.wire_byte_cost,
        values.read_byte_cost,
        values.write_byte_cost,
    ]
    .into_iter()
    .chain(values.action_costs)
    .chain(values.signature_costs.values().copied())
    .chain(values.validator_proof_costs.values().copied())
    .chain(values.governance_action_costs)
}

/// Checks that need no chain state: enabled class, byte bound, canonical
/// encoding and genesis bounds.
pub(crate) fn validate_proposal(
    candidate: &GovernanceCandidateConfig,
    class: u16,
    data: &[u8],
) -> Result<(), Rule> {
    let limit = candidate
        .class(class)
        .ok_or(Rule("GOVERNANCE_CLASS_NOT_ENABLED"))?;
    if data.len() > limit.max_data_bytes as usize {
        return Err(Rule("GOVERNANCE_ACTION_TOO_LARGE"));
    }
    let bounds = &candidate.parameter_bounds;
    match class {
        CLASS_PARAMETER_CHANGE => match decode::<ParameterChange>(data)? {
            ParameterChange::Fees(values) => {
                if !bounds.gas_price.contains(values.gas_price)
                    || !bounds
                        .account_creation_fee_udrt
                        .contains(values.account_creation_fee_udrt)
                    || !costs(&values).all(|cost| bounds.resource_cost.contains(cost))
                {
                    return Err(Rule("GOVERNANCE_PARAMETER_OUT_OF_BOUNDS"));
                }
            }
            ParameterChange::MinSelfBond(value) => {
                if !bounds.min_self_bond.contains(value) {
                    return Err(Rule("GOVERNANCE_PARAMETER_OUT_OF_BOUNDS"));
                }
            }
            ParameterChange::MaxActive(value) => {
                if !bounds.max_active.contains(value) {
                    return Err(Rule("GOVERNANCE_PARAMETER_OUT_OF_BOUNDS"));
                }
            }
        },
        CLASS_VALIDATOR_REGISTRY => {
            decode::<RegistryChange>(data)?;
        }
        _ => return Err(Rule("GOVERNANCE_CLASS_NOT_ENABLED")),
    }
    Ok(())
}

fn with_fees(base: &FeeProfile, values: &FeeValues, height: u64) -> Result<FeeProfile, Rule> {
    let mut next = base.clone();
    if values
        .signature_costs
        .keys()
        .ne(base.signature_costs.keys())
        || values
            .validator_proof_costs
            .keys()
            .ne(base.validator_proof_costs.keys())
    {
        return Err(Rule("GOVERNANCE_FEE_ALGORITHMS_DIFFER"));
    }
    next.version = base
        .version
        .checked_add(1)
        .ok_or(Rule("GOVERNANCE_FEE_VERSION"))?;
    next.activation_height = height;
    next.gas_price = values.gas_price;
    next.transaction_overhead = values.transaction_overhead;
    next.receipt_metadata_cost = values.receipt_metadata_cost;
    next.wire_byte_cost = values.wire_byte_cost;
    next.read_byte_cost = values.read_byte_cost;
    next.write_byte_cost = values.write_byte_cost;
    next.action_costs = values.action_costs;
    next.signature_costs = values.signature_costs.clone();
    next.validator_proof_costs = values.validator_proof_costs.clone();
    next.account_creation_fee_udrt = values.account_creation_fee_udrt;
    next.validate()
        .map_err(|_| Rule("GOVERNANCE_FEE_PROFILE_INVALID"))?;
    Ok(next)
}

/// The v3 profile in force: the governed one, or the configured candidate's.
pub(crate) fn governance_fee<'a>(
    candidate: &'a GovernanceCandidateConfig,
    parameters: &'a GovernedParameters,
) -> &'a FeeProfileV3 {
    parameters
        .governance_fee
        .as_ref()
        .unwrap_or(&candidate.fee_profile)
}

/// Execute a due proposal on `settlement` and `ordinary`. `Ok(Err(rule))` is a
/// failed execution; the caller discards both copies and refunds.
pub(crate) fn execute(
    proposal: &Proposal,
    settlement: &mut Settlement,
    ordinary: &mut OrdinaryState,
    book: &RecoveryBook,
    candidate: &GovernanceCandidateConfig,
    height: u64,
) -> anyhow::Result<Result<(), Rule>> {
    if let Err(rule) = validate_proposal(candidate, proposal.action_class, &proposal.action_data) {
        return Ok(Err(rule));
    }
    let mut parameters = settlement
        .governance
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Governance state is not staged"))?
        .parameters()
        .clone();
    let rewards = settlement
        .rewards
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Governance requires reward state"))?;
    let mut lifecycle = settlement
        .validators
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Governance requires validator lifecycle"))?;
    let mut lifecycle_changed = false;
    match proposal.action_class {
        CLASS_PARAMETER_CHANGE => match decode::<ParameterChange>(&proposal.action_data) {
            Err(rule) => return Ok(Err(rule)),
            Ok(ParameterChange::Fees(values)) => {
                let base = match with_fees(&ordinary.config.fee_profile, &values, height) {
                    Ok(profile) => profile,
                    Err(rule) => return Ok(Err(rule)),
                };
                let current = governance_fee(candidate, &parameters).clone();
                let v3 = FeeProfileV3 {
                    base: base.clone(),
                    version: current.version.checked_add(1).ok_or_else(|| {
                        anyhow::anyhow!("Governance fee profile version overflow")
                    })?,
                    activation_height: height,
                    max_governance_action_bytes: current.max_governance_action_bytes,
                    governance_action_costs: values.governance_action_costs,
                };
                if v3.validate().is_err() {
                    return Ok(Err(Rule("GOVERNANCE_FEE_PROFILE_INVALID")));
                }
                ordinary.config.fee_profile = base.clone();
                parameters.ordinary_fee = Some(base);
                parameters.governance_fee = Some(v3);
            }
            Ok(ParameterChange::MinSelfBond(value)) => {
                lifecycle.config.min_self_bond = value;
                parameters.min_self_bond = Some(value);
                lifecycle_changed = true;
            }
            Ok(ParameterChange::MaxActive(value)) => {
                let Ok(value) = usize::try_from(value) else {
                    return Ok(Err(Rule("GOVERNANCE_PARAMETER_OUT_OF_BOUNDS")));
                };
                lifecycle.config.max_active = value;
                parameters.max_active = Some(value as u64);
                lifecycle_changed = true;
            }
        },
        CLASS_VALIDATOR_REGISTRY => {
            let operators = &mut lifecycle.config.approved_operators;
            let change = match decode::<RegistryChange>(&proposal.action_data) {
                Ok(change) => change,
                Err(rule) => return Ok(Err(rule)),
            };
            match change {
                RegistryChange::Add {
                    validator_id,
                    owner,
                } => {
                    if operators.contains_key(&validator_id) {
                        return Ok(Err(Rule("GOVERNANCE_OPERATOR_EXISTS")));
                    }
                    // An operator owner must hold a registered account.
                    if book.account_by_address(&owner)?.is_none() {
                        return Ok(Err(Rule("GOVERNANCE_OPERATOR_UNREGISTERED")));
                    }
                    operators.insert(validator_id, owner);
                }
                RegistryChange::Remove { validator_id } => {
                    if operators.remove(&validator_id).is_none() {
                        return Ok(Err(Rule("GOVERNANCE_OPERATOR_UNKNOWN")));
                    }
                    if lifecycle_references(&lifecycle, &validator_id) {
                        return Ok(Err(Rule("GOVERNANCE_OPERATOR_IN_USE")));
                    }
                }
            }
            parameters.approved_operators = Some(lifecycle.config.approved_operators.clone());
            lifecycle_changed = true;
        }
        _ => return Ok(Err(Rule("GOVERNANCE_CLASS_NOT_ENABLED"))),
    }
    if lifecycle_changed {
        // The changed configuration must still describe the current state.
        let valid = lifecycle.config.validate().is_ok()
            && lifecycle.config.max_active <= rewards.config.max_validators
            && lifecycle.validate().is_ok()
            && lifecycle.validate_rewards(&rewards).is_ok()
            && settlement
                .penalties
                .as_ref()
                .is_none_or(|penalties| penalties.validate(&lifecycle).is_ok());
        if !valid {
            return Ok(Err(Rule("GOVERNANCE_PARAMETER_INCONSISTENT")));
        }
        settlement.validators = Some(lifecycle);
    }
    settlement
        .governance
        .as_mut()
        .expect("governance staged")
        .set_parameters(parameters);
    Ok(Ok(()))
}

/// True while any retained lifecycle record names `validator_id`.
fn lifecycle_references(
    lifecycle: &crate::runtime::validator_lifecycle::LifecycleState,
    validator_id: &str,
) -> bool {
    lifecycle.effective.validators.contains_key(validator_id)
        || lifecycle
            .effective
            .positions
            .values()
            .any(|positions| positions.contains_key(validator_id))
        || lifecycle.schedules.values().any(|change| {
            change.view.validators.contains_key(validator_id)
                || change.additions.iter().any(|a| a.validator == validator_id)
                || change.removals.iter().any(|r| r.validator == validator_id)
        })
        || lifecycle
            .unbonding
            .values()
            .any(|entry| entry.validator == validator_id)
        || lifecycle.history.base.validators.contains_key(validator_id)
        || lifecycle
            .history
            .changes
            .values()
            .any(|change| change.identities.contains_key(validator_id))
}

/// Genesis configuration with the governed values in place.
pub(crate) fn effective_lifecycle(
    genesis: &crate::runtime::validator_lifecycle::LifecycleConfig,
    parameters: &GovernedParameters,
) -> anyhow::Result<crate::runtime::validator_lifecycle::LifecycleConfig> {
    let mut config = genesis.clone();
    if let Some(value) = parameters.min_self_bond {
        config.min_self_bond = value;
    }
    if let Some(value) = parameters.max_active {
        config.max_active = usize::try_from(value)?;
    }
    if let Some(operators) = &parameters.approved_operators {
        config.approved_operators = operators.clone();
    }
    Ok(config)
}
pub(crate) fn effective_ordinary(
    genesis: &crate::ordinary_state::OrdinaryConfig,
    parameters: &GovernedParameters,
) -> crate::ordinary_state::OrdinaryConfig {
    let mut config = genesis.clone();
    if let Some(profile) = &parameters.ordinary_fee {
        config.fee_profile = profile.clone();
    }
    config
}

#[cfg(test)]
#[path = "governance_actions_tests.rs"]
mod tests;
