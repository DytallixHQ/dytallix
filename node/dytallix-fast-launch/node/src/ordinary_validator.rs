//! Typed ordinary validator transitions. Callers meter each role proof before
//! precheck, check protected principal owners, and commit the returned clone only
//! with the transaction's successful action checkpoint. No balance is changed here.
use crate::runtime::{
    penalty_custody::PenaltyState,
    reward_runtime::RewardState,
    validator_lifecycle::{
        self as lifecycle, LifecycleState, Operation, ValidatorView, VerifiedKeyProof,
    },
};
use anyhow::{anyhow, Context};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use dytallix_protocol_types::ordinary::Action;
use std::collections::BTreeSet;

/// Stable reviewed application rule and fee class. Unknown module errors never
/// enter this enum and remain internal faults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ValidatorApplicationRule {
    LifecycleCapacity,
    UnbondAlreadyReleased,
    UnbondPenaltiesPending,
    UnbondNotMature,
    BondTargetUnavailable,
    UnbondPrincipalUnavailable,
    ValidatorAlreadyRegistered,
    ValidatorTargetUnavailable,
    ValidatorExposureBarred,
    ConsensusKeyAlreadyUsed,
    BondPrincipalZero,
    ValidatorSelfBondMinimum,
    ExitPendingAdditions,
    ValidatorSetEmpty,
}
impl ValidatorApplicationRule {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::LifecycleCapacity => "LIFECYCLE_CAPACITY",
            Self::UnbondAlreadyReleased => "UNBOND_ALREADY_RELEASED",
            Self::UnbondPenaltiesPending => "UNBOND_PENALTIES_PENDING",
            Self::UnbondNotMature => "UNBOND_NOT_MATURE",
            Self::BondTargetUnavailable => "BOND_TARGET_UNAVAILABLE",
            Self::UnbondPrincipalUnavailable => "UNBOND_PRINCIPAL_UNAVAILABLE",
            Self::ValidatorAlreadyRegistered => "VALIDATOR_ALREADY_REGISTERED",
            Self::ValidatorTargetUnavailable => "VALIDATOR_TARGET_UNAVAILABLE",
            Self::ValidatorExposureBarred => "VALIDATOR_EXPOSURE_BARRED",
            Self::ConsensusKeyAlreadyUsed => "CONSENSUS_KEY_ALREADY_USED",
            Self::BondPrincipalZero => "BOND_PRINCIPAL_ZERO",
            Self::ValidatorSelfBondMinimum => "VALIDATOR_SELF_BOND_MINIMUM",
            Self::ExitPendingAdditions => "EXIT_PENDING_ADDITIONS",
            Self::ValidatorSetEmpty => "VALIDATOR_SET_EMPTY",
        }
    }
    pub(crate) fn is_capacity(self) -> bool {
        matches!(self, Self::LifecycleCapacity)
    }
}
impl std::fmt::Display for ValidatorApplicationRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}
use ValidatorApplicationRule::*;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ValidatorError {
    #[error("validator authority rejected: {0}")]
    AuthorityRejected(&'static str),
    #[error("validator application condition: {0}")]
    ApplicationFailure(ValidatorApplicationRule),
    #[error("validator internal fault: {0}")]
    Internal(#[from] anyhow::Error),
}
type Result<T> = std::result::Result<T, ValidatorError>;

/// Constructor is private. A clone preserves exactly the authenticated action.
#[derive(Clone, Debug)]
pub(crate) struct ValidatedValidatorAction {
    action: Action,
    owner: String,
    nonce: u64,
    height: u64,
    chain_id: String,
    proof: Option<VerifiedKeyProof>,
}
#[derive(Clone, Debug)]
pub(crate) struct ValidatorPlan {
    pub(crate) lifecycle: LifecycleState,
    pub(crate) penalties: Option<PenaltyState>,
    pub(crate) withdrawal_amount: Option<u128>,
    /// Includes every delegator whose principal enters unbond custody.
    pub(crate) affected_principal_owners: BTreeSet<String>,
}
fn application(code: ValidatorApplicationRule) -> ValidatorError {
    ValidatorError::ApplicationFailure(code)
}
fn authority(code: &'static str) -> ValidatorError {
    ValidatorError::AuthorityRejected(code)
}
fn projected(state: &LifecycleState, height: u64) -> Result<&ValidatorView> {
    let activation = height
        .checked_add(2)
        .context("Validator activation overflow")?;
    Ok(state
        .schedules
        .range(..=activation)
        .next_back()
        .map(|(_, s)| &s.view)
        .unwrap_or(&state.effective))
}
fn validate_context(state: &LifecycleState, rewards: &RewardState, height: u64) -> Result<()> {
    state.validate_scheduled_capacity()?;
    state.validate_rewards(rewards)?;
    if height == 0 || state.last_height != height || rewards.last_height != height {
        return Err(anyhow!("Validator reward/current height differs").into());
    }
    Ok(())
}

pub(crate) fn precheck(
    state: &LifecycleState,
    rewards: &RewardState,
    owner: &str,
    nonce: u64,
    action: &Action,
    height: u64,
) -> Result<ValidatedValidatorAction> {
    precheck_ordered(state, rewards, owner, nonce, action, height, &[])
}
/// Earlier authenticated registrations can establish this transaction's role.
/// They do not assert that the earlier registration will succeed after acceptance.
pub(crate) fn precheck_ordered(
    state: &LifecycleState,
    rewards: &RewardState,
    owner: &str,
    nonce: u64,
    action: &Action,
    height: u64,
    prior: &[ValidatedValidatorAction],
) -> Result<ValidatedValidatorAction> {
    validate_context(state, rewards, height)?;
    precheck_inner(state, owner, nonce, action, height, prior)
}
/// Queue admission uses the committed snapshot without inventing block-start
/// reward markers. Execution must repeat checks after mandatory block work.
pub(crate) fn precheck_admission_ordered(
    state: &LifecycleState,
    rewards: &RewardState,
    owner: &str,
    nonce: u64,
    action: &Action,
    next_height: u64,
    prior: &[ValidatedValidatorAction],
) -> Result<ValidatedValidatorAction> {
    state.validate_scheduled_capacity()?;
    state.validate_rewards(rewards)?;
    if state.last_height.checked_add(1) != Some(next_height)
        || rewards.last_height != state.last_height
    {
        return Err(anyhow!("Validator admission committed height differs").into());
    }
    precheck_inner(state, owner, nonce, action, next_height, prior)
}
fn precheck_inner(
    state: &LifecycleState,
    owner: &str,
    nonce: u64,
    action: &Action,
    height: u64,
    prior: &[ValidatedValidatorAction],
) -> Result<ValidatedValidatorAction> {
    if owner.is_empty()
        || owner.len() > 256
        || owner.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(authority("INVALID_OWNER"));
    }
    let view = projected(state, height)?;
    let registered_owner = |id: &str| -> Option<&str> {
        view.validators.get(id).map(|v| v.owner.as_str()).or_else(|| {
            prior.iter().rev().find_map(|token| {
                if token.owner == owner && token.nonce == nonce && token.height == height
                    && token.chain_id == state.config.chain_id
                    && matches!(&token.action, Action::ValidatorRegister { validator_id, .. } if validator_id == id)
                { Some(token.owner.as_str()) } else { None }
            })
        })
    };
    let proof = match action {
        Action::ValidatorRegister {
            validator_id,
            consensus_key,
            proof,
            proof_expiry_height,
            amount_udgt,
        } => {
            if state
                .config
                .approved_operators
                .get(validator_id)
                .map(String::as_str)
                != Some(owner)
            {
                return Err(authority("VALIDATOR_OPERATOR_REQUIRED"));
            }
            Some(check_proof(
                state,
                height,
                "register",
                validator_id,
                owner,
                nonce,
                consensus_key,
                proof,
                *proof_expiry_height,
                *amount_udgt,
            )?)
        }
        Action::ValidatorRotateKey {
            validator_id,
            consensus_key,
            proof,
            proof_expiry_height,
        } => {
            if registered_owner(validator_id) != Some(owner) {
                return Err(authority("VALIDATOR_OPERATOR_REQUIRED"));
            }
            Some(check_proof(
                state,
                height,
                "rotate",
                validator_id,
                owner,
                nonce,
                consensus_key,
                proof,
                *proof_expiry_height,
                0,
            )?)
        }
        Action::ValidatorExit { validator_id } => {
            if registered_owner(validator_id) != Some(owner) {
                return Err(authority("VALIDATOR_OPERATOR_REQUIRED"));
            }
            None
        }
        Action::ValidatorWithdraw { unbond_id } => {
            if state.unbonding.get(unbond_id).map(|e| e.owner.as_str()) != Some(owner) {
                return Err(authority("UNBOND_OWNER_REQUIRED"));
            }
            None
        }
        Action::RewardBond { .. } | Action::RewardBeginUnbond { .. } => None,
        _ => return Err(anyhow!("Non-validator action passed to validator adapter").into()),
    };
    Ok(ValidatedValidatorAction {
        action: action.clone(),
        owner: owner.into(),
        nonce,
        height,
        chain_id: state.config.chain_id.clone(),
        proof,
    })
}
/// Recheck the immutable proof material in an accepted historical envelope.
/// The receipt loader supplies the recorded block height and the stable native
/// owner address, and separately authenticates the complete ordinary envelope.
/// This does not apply today's operator registry or current account generation.
/// Paid failures also require every included proof to remain authentic.
pub(crate) fn verify_historical_proofs(
    signed: &dytallix_protocol_types::ordinary::SignedOrdinary,
    config: &lifecycle::LifecycleConfig,
    owner: &str,
    height: u64,
) -> anyhow::Result<()> {
    config.validate()?;
    anyhow::ensure!(
        height > 0 && height < signed.body.expiry_height,
        "Historical ordinary envelope expired at its recorded height"
    );
    anyhow::ensure!(
        signed.body.domain.chain_id == config.chain_id,
        "Historical validator proof chain differs"
    );
    for action in &signed.body.actions {
        let (operation, validator, key, proof, expiry, amount) = match action {
            Action::ValidatorRegister {
                validator_id,
                consensus_key,
                proof,
                proof_expiry_height,
                amount_udgt,
            } => (
                "register",
                validator_id,
                consensus_key,
                proof,
                *proof_expiry_height,
                *amount_udgt,
            ),
            Action::ValidatorRotateKey {
                validator_id,
                consensus_key,
                proof,
                proof_expiry_height,
            } => (
                "rotate",
                validator_id,
                consensus_key,
                proof,
                *proof_expiry_height,
                0,
            ),
            _ => continue,
        };
        anyhow::ensure!(
            key.len() == 1952 && proof.len() == 3309,
            "Historical validator proof must use exact ML-DSA-65 encoding"
        );
        VerifiedKeyProof::verify(
            config,
            height,
            operation,
            validator,
            owner,
            &B64.encode(key),
            signed.body.spending_nonce,
            expiry,
            amount,
            &B64.encode(proof),
        )?;
    }
    Ok(())
}

fn check_proof(
    state: &LifecycleState,
    height: u64,
    operation: &str,
    id: &str,
    owner: &str,
    nonce: u64,
    key: &[u8],
    proof: &[u8],
    expiry: u64,
    amount: u128,
) -> Result<VerifiedKeyProof> {
    if !cfg!(feature = "pqc-fips204") {
        return Err(anyhow!("Validator proof backend unavailable").into());
    }
    if height > expiry || key.len() != 1952 || proof.len() != 3309 {
        return Err(authority("INVALID_VALIDATOR_PROOF"));
    }
    VerifiedKeyProof::verify(
        &state.config,
        height,
        operation,
        id,
        owner,
        &B64.encode(key),
        nonce,
        expiry,
        amount,
        &B64.encode(proof),
    )
    .map_err(|_| authority("INVALID_VALIDATOR_PROOF"))
}

/// Resolve principal owners before fee acceptance, even when the later action
/// fails a state precondition. This method performs no signature verification.
pub(crate) fn affected_principal_owners(
    state: &LifecycleState,
    token: &ValidatedValidatorAction,
) -> Result<BTreeSet<String>> {
    state.validate()?;
    if state.last_height != token.height || state.config.chain_id != token.chain_id {
        return Err(anyhow!("Validator owner projection context differs").into());
    }
    let view = projected(state, token.height)?;
    let mut owners = BTreeSet::from([token.owner.clone()]);
    let exiting = match &token.action {
        Action::ValidatorExit { validator_id } => Some(validator_id),
        Action::RewardBeginUnbond {
            validator_id,
            amount_udgt,
        } => {
            let position = view
                .positions
                .get(&token.owner)
                .and_then(|p| p.get(validator_id))
                .copied()
                .unwrap_or(0);
            if view
                .validators
                .get(validator_id)
                .is_some_and(|v| v.owner == token.owner)
                && *amount_udgt <= position
                && position - *amount_udgt < state.config.min_self_bond
            {
                Some(validator_id)
            } else {
                None
            }
        }
        _ => None,
    };
    if let Some(id) = exiting {
        owners.extend(
            view.positions
                .iter()
                .filter(|(_, p)| p.contains_key(id))
                .map(|(owner, _)| owner.clone()),
        );
    }
    Ok(owners)
}

pub(crate) fn apply(
    state: &LifecycleState,
    penalties: Option<&PenaltyState>,
    rewards: &RewardState,
    token: &ValidatedValidatorAction,
    parent_height: u64,
    parent_seconds: u64,
) -> Result<ValidatorPlan> {
    validate_context(state, rewards, token.height)?;
    if token.chain_id != state.config.chain_id || parent_height.checked_add(1) != Some(token.height)
    {
        return Err(anyhow!("Validator authenticated or parent context differs").into());
    }
    let mut penalty = penalties.cloned();
    if let Some(p) = &penalty {
        p.validate(state)?;
        // Existing oversized state is corruption, never a paid action capacity.
        p.encode()?;
        if p.parent_time.0 != parent_seconds || p.evidence_processed_height != token.height {
            return Err(anyhow!("Validator mandatory evidence or parent time differs").into());
        }
    }
    let mut owners = BTreeSet::from([token.owner.clone()]);
    if let Action::ValidatorWithdraw { unbond_id } = &token.action {
        let p = penalty
            .as_mut()
            .context("Validator withdrawal requires penalty accounting")?;
        let entry = state
            .unbonding
            .get(unbond_id)
            .context("Authenticated unbond disappeared")?;
        if entry.owner != token.owner {
            return Err(anyhow!("Authenticated unbond owner changed").into());
        }
        if p.releases.contains_key(unbond_id) {
            return Err(application(UnbondAlreadyReleased));
        }
        let ids: BTreeSet<_> = p
            .tranches
            .values()
            .filter(|t| t.unbond_id.as_ref() == Some(unbond_id))
            .map(|t| t.id.clone())
            .collect();
        if ids.is_empty() {
            return Err(anyhow!("Unbond principal tranches missing").into());
        }
        if p.incidents
            .values()
            .any(|i| i.settled_height.is_none() && i.allocations.keys().any(|id| ids.contains(id)))
        {
            return Err(application(UnbondPenaltiesPending));
        }
        if !entry.maturity_satisfied(&state.config, parent_height, parent_seconds, true)? {
            return Err(application(UnbondNotMature));
        }
        let amount = p.withdraw(
            &token.owner,
            unbond_id,
            parent_height,
            parent_seconds,
            state,
        )?;
        return Ok(ValidatorPlan {
            lifecycle: state.clone(),
            penalties: penalty,
            withdrawal_amount: Some(amount),
            affected_principal_owners: owners,
        });
    }
    let view = projected(state, token.height)?;
    let activation = token
        .height
        .checked_add(2)
        .context("Activation height overflow")?;
    let additions = state
        .schedules
        .get(&activation)
        .map(|s| s.additions.as_slice())
        .unwrap_or(&[]);
    let operation = match &token.action {
        Action::RewardBond {
            validator_id,
            amount_udgt,
        } => {
            if !view.validators.contains_key(validator_id) {
                return Err(application(BondTargetUnavailable));
            }
            check_exposure(penalty.as_ref(), validator_id)?;
            check_addition(state, view, &token.owner, validator_id, *amount_udgt, false)?;
            Operation::Bond {
                validator: validator_id.clone(),
                amount: *amount_udgt,
            }
        }
        Action::RewardBeginUnbond {
            validator_id,
            amount_udgt,
        } => {
            let identity = view
                .validators
                .get(validator_id)
                .ok_or_else(|| application(UnbondPrincipalUnavailable))?;
            let position = view
                .positions
                .get(&token.owner)
                .and_then(|p| p.get(validator_id))
                .copied()
                .unwrap_or(0);
            let added = additions
                .iter()
                .filter(|a| a.owner == token.owner && a.validator == *validator_id)
                .try_fold(0u128, |sum, a| {
                    sum.checked_add(a.amount)
                        .context("Pending principal overflow")
                })?;
            let available = position
                .checked_sub(added)
                .context("Pending principal exceeds position")?;
            if *amount_udgt == 0 || *amount_udgt > available {
                return Err(application(UnbondPrincipalUnavailable));
            }
            if identity.owner == token.owner && position - *amount_udgt < state.config.min_self_bond
            {
                check_exit(state, view, additions, validator_id, &mut owners)?;
            } else {
                check_removal_capacity(state, 1)?;
            }
            Operation::Unbond {
                validator: validator_id.clone(),
                amount: *amount_udgt,
            }
        }
        Action::ValidatorRegister {
            validator_id,
            consensus_key,
            proof,
            proof_expiry_height,
            amount_udgt,
        } => {
            if state.config.approved_operators.get(validator_id) != Some(&token.owner) {
                return Err(anyhow!("Authenticated validator role changed").into());
            }
            if view.validators.contains_key(validator_id) {
                return Err(application(ValidatorAlreadyRegistered));
            }
            check_exposure(penalty.as_ref(), validator_id)?;
            check_key(state, &B64.encode(consensus_key))?;
            check_addition(state, view, &token.owner, validator_id, *amount_udgt, true)?;
            Operation::Register {
                validator: validator_id.clone(),
                pubkey_base64: B64.encode(consensus_key),
                proof_base64: B64.encode(proof),
                expires_at_height: *proof_expiry_height,
                amount: *amount_udgt,
            }
        }
        Action::ValidatorRotateKey {
            validator_id,
            consensus_key,
            proof,
            proof_expiry_height,
        } => {
            check_current_role(view, validator_id, &token.owner)?;
            check_exposure(penalty.as_ref(), validator_id)?;
            check_key(state, &B64.encode(consensus_key))?;
            Operation::Rotate {
                validator: validator_id.clone(),
                pubkey_base64: B64.encode(consensus_key),
                proof_base64: B64.encode(proof),
                expires_at_height: *proof_expiry_height,
            }
        }
        Action::ValidatorExit { validator_id } => {
            check_current_role(view, validator_id, &token.owner)?;
            check_exit(state, view, additions, validator_id, &mut owners)?;
            Operation::Exit {
                validator: validator_id.clone(),
            }
        }
        _ => return Err(anyhow!("Invalid authenticated validator operation").into()),
    };
    check_future_history_capacity(state, activation)?;
    let mut next = state.clone();
    next.schedule_verified(
        token.height,
        &token.owner,
        token.nonce,
        operation,
        token.proof.as_ref(),
    )
    .map_err(|e| {
        if e.downcast_ref::<lifecycle::TransitionCapacity>().is_some() {
            application(LifecycleCapacity)
        } else {
            ValidatorError::Internal(e)
        }
    })?;
    if let Some(p) = &mut penalty {
        p.sync_lifecycle(state, &next)?;
    }
    Ok(ValidatorPlan {
        lifecycle: next,
        penalties: penalty,
        withdrawal_amount: None,
        affected_principal_owners: owners,
    })
}
fn check_current_role(view: &ValidatorView, id: &str, owner: &str) -> Result<()> {
    match view.validators.get(id) {
        None => Err(application(ValidatorTargetUnavailable)),
        Some(v) if v.owner != owner => Err(anyhow!("Authenticated validator owner changed").into()),
        Some(_) => Ok(()),
    }
}
fn check_exposure(penalty: Option<&PenaltyState>, id: &str) -> Result<()> {
    if penalty.is_some_and(|p| p.first_faults.contains_key(id)) {
        return Err(application(ValidatorExposureBarred));
    }
    Ok(())
}
fn check_key(state: &LifecycleState, key: &str) -> Result<()> {
    let address = lifecycle::consensus_address(key)?;
    for identity in state
        .history
        .values()
        .flat_map(|v| v.validators.values())
        .chain(
            state
                .schedules
                .values()
                .flat_map(|s| s.view.validators.values()),
        )
    {
        if lifecycle::consensus_address(&identity.pubkey_base64)? == address {
            return Err(application(ConsensusKeyAlreadyUsed));
        }
    }
    Ok(())
}
fn check_addition(
    state: &LifecycleState,
    view: &ValidatorView,
    owner: &str,
    id: &str,
    amount: u128,
    register: bool,
) -> Result<()> {
    if amount == 0 {
        return Err(application(BondPrincipalZero));
    }
    if register && amount < state.config.min_self_bond {
        return Err(application(ValidatorSelfBondMinimum));
    }
    if register && view.validators.len() >= state.config.max_active {
        return Err(application(LifecycleCapacity));
    }
    let old = view
        .positions
        .get(owner)
        .and_then(|p| p.get(id))
        .copied()
        .unwrap_or(0);
    old.checked_add(amount)
        .context("Validator principal overflow")?;
    let total =
        view.positions
            .values()
            .flat_map(|p| p.values())
            .try_fold(0u128, |sum, value| {
                sum.checked_add(*value)
                    .context("Validator total principal overflow")
            })?;
    if total
        .checked_add(amount)
        .context("Validator total principal overflow")?
        > lifecycle::MAX_TOTAL_POWER
    {
        return Err(application(LifecycleCapacity));
    }
    let count: usize = view.positions.values().map(|p| p.len()).sum();
    if (old == 0 && count >= state.max_positions)
        || (!state.reserved_owners.contains(owner)
            && state.reserved_owners.len() >= state.max_positions)
        || state
            .schedules
            .get(
                &state
                    .last_height
                    .checked_add(2)
                    .context("Activation overflow")?,
            )
            .is_some_and(|s| s.additions.len() >= 10_000)
    {
        return Err(application(LifecycleCapacity));
    }
    Ok(())
}
fn check_exit(
    state: &LifecycleState,
    view: &ValidatorView,
    additions: &[lifecycle::PendingBond],
    id: &str,
    owners: &mut BTreeSet<String>,
) -> Result<()> {
    if additions.iter().any(|a| a.validator == id) {
        return Err(application(ExitPendingAdditions));
    }
    if view.validators.len() <= 1 {
        return Err(application(ValidatorSetEmpty));
    }
    let affected: Vec<_> = view
        .positions
        .iter()
        .filter(|(_, p)| p.contains_key(id))
        .map(|(o, _)| o.clone())
        .collect();
    check_removal_capacity(state, affected.len())?;
    owners.extend(affected);
    Ok(())
}
fn check_removal_capacity(state: &LifecycleState, count: usize) -> Result<()> {
    let total = state
        .next_unbond_id
        .checked_add(u64::try_from(count).context("Removal count conversion")?)
        .context("Unbond counter overflow")?;
    if total > 10_000 {
        return Err(application(LifecycleCapacity));
    }
    Ok(())
}
fn check_future_history_capacity(state: &LifecycleState, activation: u64) -> Result<()> {
    let future: BTreeSet<_> = state
        .schedules
        .keys()
        .copied()
        .chain([activation])
        .collect();
    if state
        .history
        .len()
        .checked_add(future.len())
        .context("History count overflow")?
        > 10_000
        || (!state.update_history.contains_key(&state.last_height)
            && state.update_history.len() >= 10_000)
    {
        return Err(application(LifecycleCapacity));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::reward_runtime::{RewardConfig, ValidatorStatus};
    use fips204::{
        ml_dsa_65,
        traits::{KeyGen, SerDes, Signer},
    };
    use lifecycle::{LifecycleConfig, ValidatorIdentity};
    use std::collections::BTreeMap;

    fn fixture() -> (LifecycleState, RewardState) {
        let identities = BTreeMap::from([
            (
                "a".into(),
                ValidatorIdentity {
                    owner: "alice".into(),
                    pubkey_base64: B64.encode(vec![1; 1952]),
                },
            ),
            (
                "b".into(),
                ValidatorIdentity {
                    owner: "bob".into(),
                    pubkey_base64: B64.encode(vec![2; 1952]),
                },
            ),
        ]);
        let mut rewards = RewardState::new(
            RewardConfig {
                version: 2,
                activation_height: 1,
                decimals: 6,
                profile: "development".into(),
                chain_id: "ordinary-validator-unit".into(),
                genesis_digest: "ab".repeat(32),
                max_validators: 3,
                max_positions: 20,
            },
            identities
                .keys()
                .map(|id: &String| {
                    (
                        id.clone(),
                        ValidatorStatus {
                            active: true,
                            jailed: false,
                        },
                    )
                })
                .collect(),
            BTreeMap::from([
                ("alice".into(), BTreeMap::from([("a".into(), 100)])),
                ("bob".into(), BTreeMap::from([("b".into(), 100)])),
                ("delegator".into(), BTreeMap::from([("a".into(), 30)])),
            ]),
            BTreeMap::new(),
        )
        .unwrap();
        let mut state = LifecycleState::new(
            LifecycleConfig {
                version: 1,
                profile: lifecycle::PROFILE.into(),
                chain_id: rewards.config.chain_id.clone(),
                approved_operators: BTreeMap::from([
                    ("a".into(), "alice".into()),
                    ("b".into(), "bob".into()),
                    ("c".into(), "carol".into()),
                ]),
                min_self_bond: 10,
                max_active: 3,
                evidence_max_age_blocks: 10,
                evidence_max_age_seconds: 10,
                processing_margin_blocks: 2,
                processing_margin_seconds: 2,
            },
            identities,
            &rewards,
        )
        .unwrap();
        state.advance(1, 0, &mut rewards).unwrap();
        // Explicit zero-budget interval marker; these are adapter tests, not issuance tests.
        rewards.last_height = 1;
        rewards.last_interval_digest = Some([3; 32]);
        rewards.last_interval_input_digest = Some([4; 32]);
        (state, rewards)
    }
    #[test]
    fn cumulative_self_unbond_resolves_every_delegator_before_acceptance() {
        let (state, rewards) = fixture();
        let first = precheck(
            &state,
            &rewards,
            "alice",
            7,
            &Action::RewardBeginUnbond {
                validator_id: "a".into(),
                amount_udgt: 50,
            },
            1,
        )
        .unwrap();
        let next = apply(&state, None, &rewards, &first, 0, 0).unwrap();
        let second = precheck(
            &next.lifecycle,
            &rewards,
            "alice",
            7,
            &Action::RewardBeginUnbond {
                validator_id: "a".into(),
                amount_udgt: 45,
            },
            1,
        )
        .unwrap();
        assert_eq!(
            affected_principal_owners(&next.lifecycle, &second).unwrap(),
            BTreeSet::from(["alice".into(), "delegator".into()])
        );
        let final_plan = apply(&next.lifecycle, None, &rewards, &second, 0, 0).unwrap();
        assert!(!final_plan.lifecycle.schedules[&3]
            .view
            .validators
            .contains_key("a"));
        assert_eq!(
            final_plan.lifecycle.schedules[&3]
                .removals
                .iter()
                .filter(|r| r.owner == "delegator")
                .map(|r| r.amount)
                .sum::<u128>(),
            30
        );
        assert!(state.schedules.is_empty());
    }
    #[test]
    fn wrong_role_rejects_and_authorized_state_failure_is_typed() {
        let (state, rewards) = fixture();
        assert!(matches!(
            precheck(
                &state,
                &rewards,
                "bob",
                1,
                &Action::ValidatorExit {
                    validator_id: "a".into()
                },
                1
            ),
            Err(ValidatorError::AuthorityRejected(_))
        ));
        let action = precheck(
            &state,
            &rewards,
            "alice",
            1,
            &Action::RewardBeginUnbond {
                validator_id: "a".into(),
                amount_udgt: 101,
            },
            1,
        )
        .unwrap();
        assert!(matches!(
            apply(&state, None, &rewards, &action, 0, 0),
            Err(ValidatorError::ApplicationFailure(
                UnbondPrincipalUnavailable
            ))
        ));
        assert!(matches!(
            apply(&state, None, &rewards, &action, 9, 0),
            Err(ValidatorError::Internal(_))
        ));
    }
    #[test]
    fn authenticated_registration_proof_cannot_change_nonce_and_schedules_once() {
        let (state, rewards) = fixture();
        let (pk, sk) = ml_dsa_65::KG::try_keygen_with_rng(&mut rand_core::OsRng).unwrap();
        let key = pk.into_bytes().to_vec();
        let bytes = lifecycle::proof_sign_bytes(
            &state.config.chain_id,
            "register",
            "c",
            "carol",
            &B64.encode(&key),
            7,
            1,
            20,
        )
        .unwrap();
        let proof = sk
            .try_sign_with_rng(&mut rand_core::OsRng, &bytes, b"")
            .unwrap()
            .to_vec();
        let action = Action::ValidatorRegister {
            validator_id: "c".into(),
            consensus_key: key,
            proof,
            proof_expiry_height: 1,
            amount_udgt: 20,
        };
        let token = precheck(&state, &rewards, "carol", 7, &action, 1).unwrap();
        assert!(matches!(
            precheck(&state, &rewards, "carol", 8, &action, 1),
            Err(ValidatorError::AuthorityRejected(_))
        ));
        let plan = apply(&state, None, &rewards, &token, 0, 0).unwrap();
        assert_eq!(
            plan.lifecycle.schedules[&3].view.validators["c"].owner,
            "carol"
        );
        assert_eq!(plan.lifecycle.pending_bond_by_owner("carol").unwrap(), 20);
        // Even a module-local altered action cannot reuse the original proof token.
        let mut altered = token.clone();
        altered.nonce = 8;
        assert!(matches!(
            apply(&state, None, &rewards, &altered, 0, 0),
            Err(ValidatorError::Internal(_))
        ));
    }
    fn genesis_fixture() -> (LifecycleState, RewardState) {
        let (mut state, mut rewards) = fixture();
        state.last_height = 0;
        rewards.last_height = 0;
        rewards.last_interval_digest = None;
        rewards.last_interval_input_digest = None;
        state.validate().unwrap();
        state.validate_rewards(&rewards).unwrap();
        (state, rewards)
    }
    fn penalty_fixture() -> (LifecycleState, RewardState, PenaltyState) {
        let (state, rewards) = genesis_fixture();
        let penalty = PenaltyState::initialize(
            crate::runtime::penalty_custody::PenaltyConfig {
                version: 1,
                profile: crate::runtime::penalty_custody::PROFILE.into(),
                chain_id: state.config.chain_id.clone(),
                penalty_numerator: 1,
                penalty_denominator: 20,
                production_activation: false,
            },
            &state,
            &rewards,
        )
        .unwrap();
        (state, rewards, penalty)
    }
    fn penalty_step(
        state: &mut LifecycleState,
        rewards: &mut RewardState,
        penalty: &mut PenaltyState,
        height: u64,
        parent_seconds: u64,
        finalize: bool,
    ) {
        let before = state.clone();
        state.advance(height, parent_seconds, rewards).unwrap();
        penalty.sync_lifecycle(&before, state).unwrap();
        penalty
            .begin_block(height, (parent_seconds, 0), state)
            .unwrap();
        rewards.last_height = height;
        rewards.last_interval_digest = Some([3; 32]);
        rewards.last_interval_input_digest = Some([4; 32]);
        if finalize {
            penalty.finalize_evidence_batch(state).unwrap();
        }
    }
    fn pending_unbond_fixture() -> (LifecycleState, RewardState, PenaltyState) {
        let (mut state, mut rewards, mut penalty) = penalty_fixture();
        penalty_step(&mut state, &mut rewards, &mut penalty, 1, 0, true);
        let token = precheck(
            &state,
            &rewards,
            "alice",
            7,
            &Action::RewardBeginUnbond {
                validator_id: "a".into(),
                amount_udgt: 20,
            },
            1,
        )
        .unwrap();
        let plan = apply(&state, Some(&penalty), &rewards, &token, 0, 0).unwrap();
        (plan.lifecycle, rewards, plan.penalties.unwrap())
    }
    fn withdrawal_fixture(
        height: u64,
        final_parent_seconds: u64,
    ) -> (LifecycleState, RewardState, PenaltyState, Action) {
        let (mut state, mut rewards, mut penalty) = pending_unbond_fixture();
        for h in 2..=height {
            let seconds = if h == height {
                final_parent_seconds
            } else {
                h - 1
            };
            penalty_step(&mut state, &mut rewards, &mut penalty, h, seconds, true);
        }
        let id = state
            .unbonding
            .values()
            .find(|entry| entry.owner == "alice")
            .unwrap()
            .id
            .clone();
        (
            state,
            rewards,
            penalty,
            Action::ValidatorWithdraw { unbond_id: id },
        )
    }
    #[test]
    fn configured_position_capacity_is_paid_but_overflow_and_corruption_are_internal() {
        let (mut state, mut rewards) = fixture();
        state.max_positions = 3;
        rewards.config.max_positions = 3;
        let token = precheck(
            &state,
            &rewards,
            "alice",
            7,
            &Action::RewardBond {
                validator_id: "b".into(),
                amount_udgt: 1,
            },
            1,
        )
        .unwrap();
        let error = apply(&state, None, &rewards, &token, 0, 0).unwrap_err();
        assert!(
            matches!(error, ValidatorError::ApplicationFailure(rule) if rule == LifecycleCapacity && rule.is_capacity())
        );
        let token = precheck(
            &state,
            &rewards,
            "alice",
            7,
            &Action::RewardBond {
                validator_id: "a".into(),
                amount_udgt: u128::MAX,
            },
            1,
        )
        .unwrap();
        assert!(matches!(
            apply(&state, None, &rewards, &token, 0, 0),
            Err(ValidatorError::Internal(_))
        ));
        state.next_unbond_id = 1;
        assert!(matches!(
            precheck(
                &state,
                &rewards,
                "alice",
                7,
                &Action::RewardBond {
                    validator_id: "a".into(),
                    amount_udgt: 1
                },
                1
            ),
            Err(ValidatorError::Internal(_))
        ));
    }
    #[test]
    fn configured_active_capacity_follows_real_proof_authority() {
        let (mut state, rewards) = fixture();
        state.config.max_active = 2;
        let (pk, sk) = ml_dsa_65::KG::try_keygen_with_rng(&mut rand_core::OsRng).unwrap();
        let key = pk.into_bytes().to_vec();
        let bytes = lifecycle::proof_sign_bytes(
            &state.config.chain_id,
            "register",
            "c",
            "carol",
            &B64.encode(&key),
            7,
            2,
            20,
        )
        .unwrap();
        let proof = sk
            .try_sign_with_rng(&mut rand_core::OsRng, &bytes, b"")
            .unwrap()
            .to_vec();
        let action = Action::ValidatorRegister {
            validator_id: "c".into(),
            consensus_key: key,
            proof,
            proof_expiry_height: 2,
            amount_udgt: 20,
        };
        let token = precheck(&state, &rewards, "carol", 7, &action, 1).unwrap();
        assert!(matches!(
            apply(&state, None, &rewards, &token, 0, 0),
            Err(ValidatorError::ApplicationFailure(LifecycleCapacity))
        ));
        assert!(matches!(
            precheck(&state, &rewards, "alice", 7, &action, 1),
            Err(ValidatorError::AuthorityRejected(_))
        ));
        let mut tampered = action;
        if let Action::ValidatorRegister { proof, .. } = &mut tampered {
            proof[0] ^= 1;
        }
        assert!(matches!(
            precheck(&state, &rewards, "carol", 7, &tampered, 1),
            Err(ValidatorError::AuthorityRejected(_))
        ));
    }
    #[test]
    fn withdrawal_requires_both_strict_parent_margins_and_release_is_single_use() {
        // Last exposure height=2/time=2. Margins end at height=14/time=14.
        let (state, rewards, penalty, action) = withdrawal_fixture(15, 100);
        let token = precheck(&state, &rewards, "alice", 7, &action, 15).unwrap();
        assert!(matches!(
            apply(&state, Some(&penalty), &rewards, &token, 14, 100),
            Err(ValidatorError::ApplicationFailure(UnbondNotMature))
        ));
        let (state, rewards, penalty, action) = withdrawal_fixture(16, 14);
        let token = precheck(&state, &rewards, "alice", 7, &action, 16).unwrap();
        assert!(matches!(
            apply(&state, Some(&penalty), &rewards, &token, 15, 14),
            Err(ValidatorError::ApplicationFailure(UnbondNotMature))
        ));
        let (state, rewards, penalty, action) = withdrawal_fixture(16, 15);
        assert!(matches!(
            precheck(&state, &rewards, "bob", 7, &action, 16),
            Err(ValidatorError::AuthorityRejected(_))
        ));
        let token = precheck(&state, &rewards, "alice", 7, &action, 16).unwrap();
        let plan = apply(&state, Some(&penalty), &rewards, &token, 15, 15).unwrap();
        assert_eq!(plan.withdrawal_amount, Some(20));
        assert_eq!(plan.lifecycle, state);
        assert_eq!(penalty.released_total().unwrap(), 0);
        assert_eq!(
            plan.penalties.as_ref().unwrap().released_total().unwrap(),
            20
        );
        assert!(matches!(
            apply(&state, plan.penalties.as_ref(), &rewards, &token, 15, 15),
            Err(ValidatorError::ApplicationFailure(UnbondAlreadyReleased))
        ));
        let mut incomplete = penalty.clone();
        incomplete.evidence_processed_height -= 1;
        assert!(matches!(
            apply(&state, Some(&incomplete), &rewards, &token, 15, 15),
            Err(ValidatorError::Internal(_))
        ));
        assert!(matches!(
            apply(&state, Some(&penalty), &rewards, &token, 15, 16),
            Err(ValidatorError::Internal(_))
        ));
    }
    #[test]
    fn unsettled_withdrawal_penalty_is_a_distinct_paid_state_condition() {
        let (mut state, mut rewards, mut penalty) = pending_unbond_fixture();
        // Explicit synthetic full-principal ratio makes the existing unbond
        // tranche part of the pending allocation; no production value is set.
        penalty.config.penalty_denominator = 1;
        penalty_step(&mut state, &mut rewards, &mut penalty, 2, 1, false);
        let fact = crate::runtime::penalty_custody::EvidenceFact {
            kind: "duplicate_vote".into(),
            validator_address: lifecycle::consensus_address(
                &state.effective.validators["a"].pubkey_base64,
            )
            .unwrap(),
            height: 1,
            time_seconds: 0,
            time_nanos: 0,
            power: 130,
            total_power: 230,
        };
        let assessed = penalty.assess(&fact, (0, 0), 1, (1, 0), &state).unwrap();
        assert!(assessed.requires_exit);
        let before = state.clone();
        state
            .schedule(
                2,
                "alice",
                7,
                Operation::Exit {
                    validator: "a".into(),
                },
            )
            .unwrap();
        penalty.sync_lifecycle(&before, &state).unwrap();
        penalty.finalize_evidence_batch(&state).unwrap();
        penalty_step(&mut state, &mut rewards, &mut penalty, 3, 2, true);
        let id = state
            .unbonding
            .values()
            .find(|entry| entry.owner == "alice")
            .unwrap()
            .id
            .clone();
        let token = precheck(
            &state,
            &rewards,
            "alice",
            7,
            &Action::ValidatorWithdraw { unbond_id: id },
            3,
        )
        .unwrap();
        assert!(matches!(
            apply(&state, Some(&penalty), &rewards, &token, 2, 2),
            Err(ValidatorError::ApplicationFailure(UnbondPenaltiesPending))
        ));
        assert_eq!(penalty.released_total().unwrap(), 0);
    }
    #[test]
    fn genesis_admission_preserves_real_committed_reward_markers() {
        let (state, rewards) = genesis_fixture();
        let before_state = state.clone();
        let before_rewards = rewards.clone();
        let action = Action::RewardBond {
            validator_id: "a".into(),
            amount_udgt: 1,
        };
        let token =
            precheck_admission_ordered(&state, &rewards, "alice", 7, &action, 1, &[]).unwrap();
        assert_eq!(token.height, 1);
        assert_eq!(state, before_state);
        assert_eq!(rewards, before_rewards);
        assert!(rewards.last_interval_digest.is_none());
        assert!(matches!(
            precheck(&state, &rewards, "alice", 7, &action, 1),
            Err(ValidatorError::Internal(_))
        ));
        assert!(matches!(
            precheck_admission_ordered(&state, &rewards, "alice", 7, &action, 2, &[]),
            Err(ValidatorError::Internal(_))
        ));
    }
}
