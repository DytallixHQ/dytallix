//! Ordinary-v2 authority preparation for the explicit combined local profile.
//!
//! This module does not execute transactions, charge fees, persist grants, or
//! change either nonce. The metered runtime separately checks funding, module
//! rules, and atomic publication. Existing recovery-only profiles
//! do not call these new compatibility-mirror checks.
use crate::recovery_fees::{RecoveryAccount, RecoveryBook, MAX_ACCOUNTS};
use anyhow::{ensure, Context, Result};
use dytallix_protocol_types::{
    address::{AccountAddress, AddressNetwork, OriginKeyAlgorithm},
    ordinary::{self as wire, Action, Limits, OrdinaryTransaction, SignedOrdinary},
    recovery::{RecoveryConfig, RecoveryState},
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
/// The recovery records one ordinary transaction may use: the book, plus the
/// actor's prospective record on its first spend (B1c). Authority checks read
/// the prospective record; they never store it.
#[derive(Clone, Copy)]
pub(crate) struct Accounts<'a> {
    book: &'a RecoveryBook,
    initial: Option<&'a RecoveryAccount>,
}
impl<'a> Accounts<'a> {
    pub(crate) fn with_initial(
        book: &'a RecoveryBook,
        initial: Option<&'a RecoveryAccount>,
    ) -> Self {
        Self { book, initial }
    }
    fn get(self, id: &[u8; 32]) -> Option<&'a RecoveryAccount> {
        self.initial
            .filter(|a| a.recovery.domain.account_id == *id)
            .or_else(|| self.book.accounts.get(&hex::encode(id)))
    }
}
impl<'a> From<&'a RecoveryBook> for Accounts<'a> {
    fn from(book: &'a RecoveryBook) -> Self {
        Self {
            book,
            initial: None,
        }
    }
}
fn account<'a>(accounts: Accounts<'a>, id: &[u8; 32]) -> Result<&'a RecoveryAccount> {
    accounts
        .get(id)
        .context("Unregistered ordinary account reference")
}
pub(crate) fn network(code: u8) -> Result<AddressNetwork> {
    Ok(match code {
        1 => AddressNetwork::Mainnet,
        2 => AddressNetwork::Testnet,
        3 => AddressNetwork::Development,
        _ => anyhow::bail!("Unknown ordinary account network"),
    })
}
pub(crate) fn origin_algorithm(value: &str) -> Result<OriginKeyAlgorithm> {
    match value {
        "mldsa65" => Ok(OriginKeyAlgorithm::MlDsa65),
        "mldsa87" => Ok(OriginKeyAlgorithm::MlDsa87),
        _ => anyhow::bail!("Unsupported exact ordinary origin algorithm"),
    }
}
/// The network every account in the book shares (`RecoveryBook::validate`
/// rejects mixed chain domains).
pub(crate) fn chain_network(book: &RecoveryBook) -> Result<AddressNetwork> {
    network(
        book.accounts
            .values()
            .next()
            .context("Recovery book has no chain domain")?
            .recovery
            .domain
            .network,
    )
}
/// The actor's recovery record for its first spend (B1c), or None when it
/// already has one. An account created by a transfer has no key on chain: its
/// first transaction must be signed by the origin key its ID derives from,
/// under the chain's domain, at generation and nonce zero. The record takes
/// the chain account template. Nothing is stored here.
pub(crate) fn prospective_actor(
    book: &RecoveryBook,
    template: &RecoveryConfig,
    body: &OrdinaryTransaction,
) -> Result<Option<RecoveryAccount>> {
    let id = body.domain.account_id;
    if book.accounts.contains_key(&hex::encode(id)) {
        return Ok(None);
    }
    let chain = &book
        .accounts
        .values()
        .next()
        .context("Recovery book has no chain domain")?
        .recovery
        .domain;
    ensure!(
        body.domain.network == chain.network
            && body.domain.chain_id == chain.chain_id
            && body.domain.genesis_digest == chain.genesis_digest,
        "Ordinary signed domain differs from the chain domain"
    );
    ensure!(
        body.authorization_generation == 0 && body.spending_nonce == 0,
        "Uninitialized account requires generation and nonce zero"
    );
    ensure!(
        book.accounts.len() < MAX_ACCOUNTS,
        "Ordinary account capacity exhausted"
    );
    let address = AccountAddress::from_origin_key(
        network(body.domain.network)?,
        &body.domain.chain_id,
        origin_algorithm(&body.key.algorithm)?,
        &body.key.public_key,
    )?;
    ensure!(
        address.account_id() == &id,
        "Ordinary key is not the account's origin key"
    );
    // Also requires the key's algorithm and length to be in the template.
    let recovery = RecoveryState::new(
        body.domain.clone(),
        template.clone(),
        body.key.clone(),
        book.last_height,
    )
    .map_err(|e| anyhow::anyhow!("Account template rejects the origin key: {e}"))?;
    Ok(Some(RecoveryAccount {
        address: address.encode(),
        recovery,
        sponsor_nonce: 0,
    }))
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
/// Accounts an ordinary transaction reads or changes: the actor, each `Send`
/// recipient, each `DmsRegister` beneficiary and each `DmsClaim` owner.
/// Validator and reward actions debit or credit only the actor.
pub(crate) fn touched_accounts(body: &OrdinaryTransaction) -> BTreeSet<[u8; 32]> {
    let mut touched = BTreeSet::from([body.domain.account_id]);
    for action in &body.actions {
        match action {
            Action::Send { recipient, .. } => {
                touched.insert(*recipient);
            }
            Action::DmsRegister { beneficiary, .. } => {
                touched.insert(*beneficiary);
            }
            Action::DmsClaim { owner, .. } => {
                touched.insert(*owner);
            }
            _ => {}
        }
    }
    touched
}
/// `validate_nonce_mirrors` restricted to the given accounts. Unregistered IDs
/// are skipped so that the authority check, not this invariant, rejects them.
/// The whole book is validated at block start and by the complete check.
pub(crate) fn validate_nonce_mirrors_for<'a>(
    accounts: impl Into<Accounts<'a>>,
    native_nonces: &BTreeMap<String, u64>,
    ids: &BTreeSet<[u8; 32]>,
) -> Result<()> {
    let accounts = accounts.into();
    for id in ids {
        let Some(state) = accounts.get(id) else {
            continue;
        };
        state.recovery.validate()?;
        let domain = &state.recovery.domain;
        ensure!(
            domain.account_id == *id && state.recovery.last_height == accounts.book.last_height,
            "Recovery account key or height differs from book"
        );
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
        validate_grant(book.into(), key, grant)?;
    }
    Ok(())
}
/// `validate_grants` restricted to the grants owned by the given accounts. The
/// count bound follows from each key being a distinct registered owner.
pub(crate) fn validate_grants_for<'a>(
    accounts: impl Into<Accounts<'a>>,
    grants: &Grants,
    owners: &BTreeSet<[u8; 32]>,
) -> Result<()> {
    let accounts = accounts.into();
    for owner in owners {
        let key = hex::encode(owner);
        if let Some(grant) = grants.get(&key) {
            validate_grant(accounts, &key, grant)?;
        }
    }
    Ok(())
}
fn validate_grant(accounts: Accounts<'_>, key: &str, grant: &DiscretionaryGrant) -> Result<()> {
    ensure!(
        grant.version == 1 && key == hex::encode(grant.owner),
        "Invalid discretionary grant record key/version"
    );
    let owner = account(accounts, &grant.owner)?;
    account(accounts, &grant.beneficiary)?;
    ensure!(
        grant.owner != grant.beneficiary && grant.period_blocks > 0,
        "Invalid discretionary beneficiary or period"
    );
    ensure!(
        grant.owner_generation <= owner.recovery.active_generation,
        "Discretionary grant contains a future generation"
    );
    ensure!(
        grant.last_active_height <= accounts.book.last_height,
        "Discretionary grant activity is in the future"
    );
    grant
        .last_active_height
        .checked_add(grant.period_blocks)
        .context("Discretionary grant deadline overflow")?;
    Ok(())
}
fn allowed_owner(accounts: Accounts<'_>, owner: &[u8; 32]) -> Result<()> {
    ensure!(
        account(accounts, owner)?.recovery.outgoing_allowed(),
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
/// `accounts` may carry the actor's prospective first-spend record.
pub(crate) fn check_verified_preacceptance<'a>(
    accounts: impl Into<Accounts<'a>>,
    native_nonces: &BTreeMap<String, u64>,
    grants: &Grants,
    height: u64,
    verified: &VerifiedOrdinary,
    limits: &Limits,
) -> Result<AuthorityAssessment> {
    let mut assessment = prepare_body_phase(
        accounts.into(),
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
fn prepare_body<'a>(
    accounts: impl Into<Accounts<'a>>,
    native_nonces: &BTreeMap<String, u64>,
    grants: &Grants,
    height: u64,
    body: &OrdinaryTransaction,
    limits: &Limits,
) -> Result<AuthorityAssessment> {
    prepare_body_phase(
        accounts.into(),
        native_nonces,
        grants,
        height,
        body,
        limits,
        false,
    )
}
fn prepare_body_phase(
    accounts: Accounts<'_>,
    native_nonces: &BTreeMap<String, u64>,
    grants: &Grants,
    height: u64,
    body: &OrdinaryTransaction,
    limits: &Limits,
    defer_maturity: bool,
) -> Result<AuthorityAssessment> {
    limits.validate()?;
    wire::signing_bytes(body, limits)?;
    let touched = touched_accounts(body);
    validate_nonce_mirrors_for(accounts, native_nonces, &touched)?;
    validate_grants_for(accounts, grants, &touched)?;
    ensure!(
        accounts.book.last_height == height,
        "Ordinary authority requires current block-start recovery state"
    );
    let actor_id = body.domain.account_id;
    let actor = account(accounts, &actor_id)?;
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
    allowed_owner(accounts, &actor_id)?; // Actor is also the only ordinary fee payer.
    let mut staged_grants = grants.clone();
    let mut owners = Vec::with_capacity(body.actions.len());
    let mut requirements = Vec::new();
    let mut deferred_application_checks = Vec::new();
    for (index, action) in body.actions.iter().enumerate() {
        let mut debit_owners = BTreeSet::from([actor_id]);
        match action {
            Action::Send { amount, .. } => {
                // Any account ID may receive (B1c): a transfer to one with no
                // account creates it and burns the account creation fee.
                // Protected accounts may receive funds.
                ensure!(*amount > 0, "Ordinary transfer amount must be positive");
            }
            Action::Data { .. } => {}
            Action::DmsRegister {
                beneficiary,
                period_blocks,
            } => {
                account(accounts, beneficiary)?;
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
                allowed_owner(accounts, owner)?;
                debit_owners.insert(*owner);
                let owner_state = account(accounts, owner)?;
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
            allowed_owner(accounts, owner)?;
        }
        owners.push(ActionOwners {
            index: u16::try_from(index).context("Ordinary action index exceeds u16")?,
            debit_owners,
        });
    }
    validate_grants_for(accounts, &staged_grants, &touched)?;
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
