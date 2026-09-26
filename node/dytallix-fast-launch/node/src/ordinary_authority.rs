//! Ordinary-v2 authority preparation for the explicit combined local profile.
//!
//! This module does not execute transactions, charge fees, persist grants, or
//! change either nonce. The metered runtime separately checks funding, module
//! rules, and atomic publication. Existing recovery-only profiles
//! do not call these new compatibility-mirror checks.
use crate::recovery_fees::{RecoveryAccount, RecoveryBook};
use anyhow::{ensure, Context, Result};
use dytallix_protocol_types::{
    address::{AccountAddress, AddressNetwork},
    ordinary::{self as wire, Action, Limits, OrdinaryTransaction, SignedOrdinary},
};
use dytallix_runtime_crypto::ordinary::{verify_signed, VerifiedOrdinary};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Future committed discretionary permission. This does not represent a mandatory
/// liability, validator schedule, penalty, reward accrual, or vesting obligation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DiscretionaryGrant {
    pub version: u16,
    pub owner: [u8; 32],
    pub beneficiary: [u8; 32],
    pub owner_generation: u64,
    pub period_blocks: u64,
    pub last_active_height: u64,
}
pub(crate) type Grants = BTreeMap<String, DiscretionaryGrant>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RegistryRequirement {
    Validator {
        validator_id: String,
        actor: [u8; 32],
    },
    ValidatorProof {
        validator_id: String,
        actor: [u8; 32],
    },
    Unbond {
        unbond_id: String,
        owner: [u8; 32],
    },
    RewardEntitlement {
        owner: [u8; 32],
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActionOwners {
    pub index: u16,
    /// Includes custody ownership where the amount is not yet liquid.
    pub debit_owners: BTreeSet<[u8; 32]>,
}
/// Non-authority module preconditions checked only after fee acceptance under OF02.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DeferredApplicationCheck {
    DmsClaimMaturity {
        action_index: u16,
        deadline_height: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NoncePreparation {
    pub actor: [u8; 32],
    pub generation: u64,
    pub before: u64,
    pub after: u64,
}
/// An authenticated, current-authority assessment. It is not an execution permit.
/// The prospective changes are read-only planning data. A future executor must
/// integrate the approved fee contract and recheck current authority before a commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AuthorityAssessment {
    pub transaction_id: [u8; 32],
    pub envelope_hash: [u8; 32],
    pub actor: [u8; 32],
    pub fee_payer: [u8; 32],
    pub action_owners: Vec<ActionOwners>,
    pub prospective_nonce: NoncePreparation,
    pub prospective_grants: Grants,
    pub registry_requirements: Vec<RegistryRequirement>,
    pub deferred_application_checks: Vec<DeferredApplicationCheck>,
}
impl AuthorityAssessment {
    pub(crate) fn is_executable(&self) -> bool {
        false
    }
    pub(crate) fn require_paid_execution(&self) -> Result<()> {
        anyhow::bail!(
            "Authority assessment cannot execute; use the signed combined runtime entrypoint"
        )
    }
}
fn account<'a>(book: &'a RecoveryBook, id: &[u8; 32]) -> Result<&'a RecoveryAccount> {
    book.accounts
        .get(&hex::encode(id))
        .context("Unregistered ordinary account reference")
}
fn network(code: u8) -> Result<AddressNetwork> {
    Ok(match code {
        1 => AddressNetwork::Mainnet,
        2 => AddressNetwork::Testnet,
        3 => AddressNetwork::Development,
        _ => anyhow::bail!("Unknown ordinary account network"),
    })
}
/// Check explicit combined-profile state. Never synthesize or repair a mirror.
/// Native nonce map keys are native account addresses, not origin or active keys.
pub(crate) fn validate_nonce_mirrors(
    book: &RecoveryBook,
    native_nonces: &BTreeMap<String, u64>,
) -> Result<()> {
    book.validate()?;
    for state in book.accounts.values() {
        let domain = &state.recovery.domain;
        let address = AccountAddress::decode(network(domain.network)?, &state.address)?;
        ensure!(
            address.account_id() == &domain.account_id,
            "Ordinary account address mapping differs from stable ID"
        );
        ensure!(
            native_nonces.get(&state.address) == Some(&state.recovery.spending_nonce),
            "Ordinary native nonce mirror missing or unequal"
        );
    }
    Ok(())
}
/// Stale grants remain recorded but cannot be used. Their owners must register
/// new grants. Do not delete them, copy a current generation, or resume them.
pub(crate) fn validate_grants(book: &RecoveryBook, grants: &Grants) -> Result<()> {
    ensure!(
        grants.len() <= book.accounts.len(),
        "Discretionary grant count exceeds owner capacity"
    );
    for (key, grant) in grants {
        ensure!(
            grant.version == 1 && *key == hex::encode(grant.owner),
            "Invalid discretionary grant record key/version"
        );
        let owner = account(book, &grant.owner)?;
        account(book, &grant.beneficiary)?;
        ensure!(
            grant.owner != grant.beneficiary && grant.period_blocks > 0,
            "Invalid discretionary beneficiary or period"
        );
        ensure!(
            grant.owner_generation <= owner.recovery.active_generation,
            "Discretionary grant contains a future generation"
        );
        ensure!(
            grant.last_active_height <= book.last_height,
            "Discretionary grant activity is in the future"
        );
        grant
            .last_active_height
            .checked_add(grant.period_blocks)
            .context("Discretionary grant deadline overflow")?;
    }
    Ok(())
}
fn allowed_owner(book: &RecoveryBook, owner: &[u8; 32]) -> Result<()> {
    ensure!(
        account(book, owner)?.recovery.outgoing_allowed(),
        "Protected account cannot authorize a discretionary action or debit"
    );
    Ok(())
}
pub(crate) fn check_signed(
    book: &RecoveryBook,
    native_nonces: &BTreeMap<String, u64>,
    grants: &Grants,
    height: u64,
    signed: &SignedOrdinary,
    limits: &Limits,
) -> Result<AuthorityAssessment> {
    let verified = verify_signed(signed, limits)?;
    check_verified(book, native_nonces, grants, height, &verified, limits)
}
pub(crate) fn check_verified(
    book: &RecoveryBook,
    native_nonces: &BTreeMap<String, u64>,
    grants: &Grants,
    height: u64,
    verified: &VerifiedOrdinary,
    limits: &Limits,
) -> Result<AuthorityAssessment> {
    let mut assessment =
        prepare_body(book, native_nonces, grants, height, verified.body(), limits)?;
    assessment.transaction_id = verified.transaction_id();
    assessment.envelope_hash = verified.envelope_hash();
    Ok(assessment)
}
/// OF02 preacceptance check. Ownership, generation and protection remain strict.
/// Only inactivity maturity moves to a typed post-acceptance application check.
pub(crate) fn check_verified_preacceptance(
    book: &RecoveryBook,
    native_nonces: &BTreeMap<String, u64>,
    grants: &Grants,
    height: u64,
    verified: &VerifiedOrdinary,
    limits: &Limits,
) -> Result<AuthorityAssessment> {
    let mut assessment = prepare_body_phase(
        book,
        native_nonces,
        grants,
        height,
        verified.body(),
        limits,
        true,
    )?;
    assessment.transaction_id = verified.transaction_id();
    assessment.envelope_hash = verified.envelope_hash();
    Ok(assessment)
}
// Private model preparation has no authentication claim. Only check_verified and
// check_signed expose an assessment outside this module.
fn prepare_body(
    book: &RecoveryBook,
    native_nonces: &BTreeMap<String, u64>,
    grants: &Grants,
    height: u64,
    body: &OrdinaryTransaction,
    limits: &Limits,
) -> Result<AuthorityAssessment> {
    prepare_body_phase(book, native_nonces, grants, height, body, limits, false)
}
fn prepare_body_phase(
    book: &RecoveryBook,
    native_nonces: &BTreeMap<String, u64>,
    grants: &Grants,
    height: u64,
    body: &OrdinaryTransaction,
    limits: &Limits,
    defer_maturity: bool,
) -> Result<AuthorityAssessment> {
    limits.validate()?;
    wire::signing_bytes(body, limits)?;
    validate_nonce_mirrors(book, native_nonces)?;
    validate_grants(book, grants)?;
    ensure!(
        book.last_height == height,
        "Ordinary authority requires current block-start recovery state"
    );
    let actor_id = body.domain.account_id;
    let actor = account(book, &actor_id)?;
    ensure!(
        actor.recovery.domain == body.domain,
        "Ordinary signed domain differs from account state"
    );
    ensure!(
        actor.recovery.active_key == body.key
            && actor.recovery.active_generation == body.authorization_generation,
        "Ordinary key or generation is stale"
    );
    ensure!(
        actor.recovery.spending_nonce == body.spending_nonce,
        "Ordinary spending nonce is stale"
    );
    let next_nonce = body
        .spending_nonce
        .checked_add(1)
        .context("Ordinary spending nonce exhausted")?;
    ensure!(
        height < body.expiry_height && body.expiry_height - height <= limits.max_expiry_lifetime,
        "Ordinary submission expired or exceeds configured lifetime"
    );
    allowed_owner(book, &actor_id)?; // Actor is also the only ordinary fee payer.
    let mut staged_grants = grants.clone();
    let mut owners = Vec::with_capacity(body.actions.len());
    let mut requirements = Vec::new();
    let mut deferred_application_checks = Vec::new();
    for (index, action) in body.actions.iter().enumerate() {
        let mut debit_owners = BTreeSet::from([actor_id]);
        match action {
            Action::Send {
                recipient, amount, ..
            } => {
                ensure!(*amount > 0, "Ordinary transfer amount must be positive");
                account(book, recipient)?; // Protected recipients may receive funds.
            }
            Action::Data { .. } => {}
            Action::DmsRegister {
                beneficiary,
                period_blocks,
            } => {
                account(book, beneficiary)?;
                ensure!(
                    *beneficiary != actor_id && *period_blocks > 0,
                    "Invalid discretionary grant registration"
                );
                height
                    .checked_add(*period_blocks)
                    .context("Discretionary grant deadline overflow")?;
                staged_grants.insert(
                    hex::encode(actor_id),
                    DiscretionaryGrant {
                        version: 1,
                        owner: actor_id,
                        beneficiary: *beneficiary,
                        owner_generation: actor.recovery.active_generation,
                        period_blocks: *period_blocks,
                        last_active_height: height,
                    },
                );
            }
            Action::DmsPing => {
                let grant = staged_grants
                    .get_mut(&hex::encode(actor_id))
                    .context("Discretionary grant not registered")?;
                ensure!(
                    grant.owner_generation == actor.recovery.active_generation,
                    "Stale grant requires owner registration, not ping"
                );
                height
                    .checked_add(grant.period_blocks)
                    .context("Discretionary grant deadline overflow")?;
                grant.last_active_height = height;
            }
            Action::DmsClaim {
                owner,
                expected_grant_generation,
            } => {
                allowed_owner(book, owner)?;
                debit_owners.insert(*owner);
                let owner_state = account(book, owner)?;
                let grant = staged_grants
                    .get(&hex::encode(owner))
                    .context("Discretionary grant not registered")?;
                ensure!(
                    grant.beneficiary == actor_id && *owner != actor_id,
                    "Actor is not the registered distinct beneficiary"
                );
                ensure!(
                    *expected_grant_generation == grant.owner_generation
                        && grant.owner_generation == owner_state.recovery.active_generation,
                    "Discretionary grant generation is stale"
                );
                let deadline = grant
                    .last_active_height
                    .checked_add(grant.period_blocks)
                    .context("Discretionary grant deadline overflow")?;
                if defer_maturity {
                    deferred_application_checks.push(DeferredApplicationCheck::DmsClaimMaturity {
                        action_index: u16::try_from(index)
                            .context("Ordinary action index exceeds u16")?,
                        deadline_height: deadline,
                    });
                } else {
                    ensure!(height >= deadline, "Discretionary grant is not mature");
                }
                // Preserve existing grant semantics after a claim. This preparation
                // does not consume balances or invent a new one-time grant policy.
            }
            Action::RewardBond {
                validator_id,
                amount_udgt,
            }
            | Action::RewardBeginUnbond {
                validator_id,
                amount_udgt,
            } => {
                ensure!(*amount_udgt > 0, "Ordinary stake amount must be positive");
                requirements.push(RegistryRequirement::Validator {
                    validator_id: validator_id.clone(),
                    actor: actor_id,
                });
            }
            Action::RewardClaim => {
                requirements.push(RegistryRequirement::RewardEntitlement { owner: actor_id })
            }
            Action::ValidatorRegister {
                validator_id,
                amount_udgt,
                ..
            } => {
                ensure!(*amount_udgt > 0, "Validator bond amount must be positive");
                requirements.push(RegistryRequirement::Validator {
                    validator_id: validator_id.clone(),
                    actor: actor_id,
                });
                requirements.push(RegistryRequirement::ValidatorProof {
                    validator_id: validator_id.clone(),
                    actor: actor_id,
                });
            }
            Action::ValidatorRotateKey { validator_id, .. } => {
                requirements.push(RegistryRequirement::Validator {
                    validator_id: validator_id.clone(),
                    actor: actor_id,
                });
                requirements.push(RegistryRequirement::ValidatorProof {
                    validator_id: validator_id.clone(),
                    actor: actor_id,
                });
            }
            Action::ValidatorExit { validator_id } => {
                requirements.push(RegistryRequirement::Validator {
                    validator_id: validator_id.clone(),
                    actor: actor_id,
                })
            }
            Action::ValidatorWithdraw { unbond_id } => {
                requirements.push(RegistryRequirement::Unbond {
                    unbond_id: unbond_id.clone(),
                    owner: actor_id,
                })
            }
        }
        // Applies to every action, including Data, self-payments, same-owner
        // claims, voluntary withdrawals and future fee settlement. No system flag.
        for owner in &debit_owners {
            allowed_owner(book, owner)?;
        }
        owners.push(ActionOwners {
            index: u16::try_from(index).context("Ordinary action index exceeds u16")?,
            debit_owners,
        });
    }
    validate_grants(book, &staged_grants)?;
    Ok(AuthorityAssessment {
        transaction_id: wire::transaction_id(body, limits)?,
        envelope_hash: [0; 32],
        actor: actor_id,
        fee_payer: actor_id,
        action_owners: owners,
        prospective_nonce: NoncePreparation {
            actor: actor_id,
            generation: actor.recovery.active_generation,
            before: body.spending_nonce,
            after: next_nonce,
        },
        prospective_grants: staged_grants,
        registry_requirements: requirements,
        deferred_application_checks,
    })
}

#[cfg(test)]
#[path = "ordinary_authority_tests.rs"]
mod tests;
