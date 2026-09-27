//! Ordinary-v3 governance transactions and the automatic block phase (T6,
//! `docs/architecture/governance-v1.md`).
//!
//! A v3 transaction carries exactly one governance action. Authentication,
//! account authority, expiry and funding are checked before acceptance; a
//! failure there includes nothing and charges nothing. After acceptance every
//! governance rule failure is paid: the fee and nonce apply and the action's
//! effects are discarded. At block start, due transitions run before any
//! transaction in ascending proposal ID (P01, 27 September 2026).
use crate::{
    governance_actions,
    governance_v3_meter::{MeterError, V3GovernanceMeter},
    governance_v3_reservations,
    ordinary_execution::OrdinaryExecutionResult,
    ordinary_fee_settlement::PlanError,
    ordinary_logical as logical,
    ordinary_meter::SharedBlockMeter,
    ordinary_state::OrdinaryState,
    recovery_fees::RecoveryBook,
    runtime::{
        governance_candidate::GovernanceCandidateConfig,
        governance_store::{self as store, Due, GovernanceStore, Outcome, Rule, VoteChoice},
        validator_lifecycle::{LifecycleState, STATE_KEY as LIFECYCLE_STATE_KEY},
    },
    settlement::Settlement,
};
use dytallix_protocol_types::{
    address::AccountAddress,
    ordinary_fees_v3::{self, FeeProfileV3},
    ordinary_v3::{self as v3, Action, SignedOrdinary},
};
use dytallix_runtime_crypto::ordinary_v3::{verify_signed, OrdinaryV3VerificationError};
use std::sync::Arc;

type Result<T> = std::result::Result<T, PlanError>;
const MAX_LOGICAL_BYTES: u32 = 64 * 1024 * 1024;

fn internal(e: impl std::fmt::Display) -> PlanError {
    PlanError::Internal(e.to_string())
}
fn denied(gas: u64, e: impl std::fmt::Display) -> OrdinaryExecutionResult {
    OrdinaryExecutionResult {
        success: false,
        accepted: false,
        gas_used: gas,
        fee: 0,
        error: Some(e.to_string()),
        receipt: None,
        reservation: None,
    }
}
fn choice(choice: v3::VoteChoice) -> VoteChoice {
    match choice {
        v3::VoteChoice::Yes => VoteChoice::Yes,
        v3::VoteChoice::No => VoteChoice::No,
        v3::VoteChoice::NoWithVeto => VoteChoice::NoWithVeto,
        v3::VoteChoice::Abstain => VoteChoice::Abstain,
    }
}

/// The v3 fee profile in force. Governance fee changes replace it.
pub(crate) fn fee_profile<'a>(
    candidate: &'a GovernanceCandidateConfig,
    settlement: &'a Settlement,
) -> Result<&'a FeeProfileV3> {
    let store = settlement
        .governance
        .as_ref()
        .ok_or_else(|| internal("Governance state is not staged"))?;
    Ok(governance_actions::governance_fee(
        candidate,
        store.parameters(),
    ))
}

/// Run the automatic transitions due at `height`, before any transaction:
/// remove finished proposals, close deposit stages, start and close ballots,
/// refund deposits and execute due actions, in ascending proposal ID.
pub(crate) fn begin_block(
    settlement: &mut Settlement,
    ordinary: &mut OrdinaryState,
    book: &RecoveryBook,
    candidate: &GovernanceCandidateConfig,
    height: u64,
) -> anyhow::Result<()> {
    let raw = settlement
        .storage
        .db
        .get(LIFECYCLE_STATE_KEY)?
        .ok_or_else(|| anyhow::anyhow!("Governance requires validator lifecycle state"))?;
    let parent = Arc::new(LifecycleState::decode(&raw)?);
    let dues = settlement
        .governance
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("Governance state is not staged"))?
        .begin_block(height, parent, book)?;
    for due in dues {
        let (id, deposits, outcome) = match due {
            Due::Refund {
                id,
                deposits,
                outcome,
            } => (id, deposits, outcome),
            Due::Execute { proposal } => {
                let mut effects = settlement.clone();
                let mut next = ordinary.clone();
                let outcome = match governance_actions::execute(
                    &proposal,
                    &mut effects,
                    &mut next,
                    book,
                    candidate,
                    height,
                )? {
                    Ok(()) => {
                        *settlement = effects;
                        *ordinary = next;
                        Outcome::Executed
                    }
                    Err(Rule(code)) => Outcome::FailedExecution(code.into()),
                };
                (proposal.id, proposal.deposits.clone(), outcome)
            }
        };
        for (owner, amount) in deposits {
            let account = book
                .accounts
                .find(&hex::encode(owner))?
                .ok_or_else(|| anyhow::anyhow!("Governance depositor is not registered"))?;
            let native = settlement.account(&account.address)?;
            let balance = native
                .balance_of("udgt")
                .checked_add(amount)
                .ok_or_else(|| anyhow::anyhow!("Governance refund overflow"))?;
            native.set_balance("udgt", balance);
        }
        settlement
            .governance
            .as_mut()
            .expect("governance staged")
            .finish(id, outcome)?;
    }
    Ok(())
}

pub(crate) fn execute_signed(
    signed: &SignedOrdinary,
    candidate: &GovernanceCandidateConfig,
    book: &mut RecoveryBook,
    settlement: &mut Settlement,
    height: u64,
    block: &mut SharedBlockMeter,
) -> Result<OrdinaryExecutionResult> {
    execute(signed, candidate, book, settlement, height, block, false)
}

/// CheckTx: authentication, authority and funding against current state. The
/// governance action is checked only when the block executes it.
pub(crate) fn admission_only(
    signed: &SignedOrdinary,
    candidate: &GovernanceCandidateConfig,
    book: &mut RecoveryBook,
    settlement: &mut Settlement,
    height: u64,
    block: &mut SharedBlockMeter,
) -> Result<OrdinaryExecutionResult> {
    execute(signed, candidate, book, settlement, height, block, true)
}

fn execute(
    signed: &SignedOrdinary,
    candidate: &GovernanceCandidateConfig,
    book: &mut RecoveryBook,
    settlement: &mut Settlement,
    height: u64,
    block: &mut SharedBlockMeter,
    admission: bool,
) -> Result<OrdinaryExecutionResult> {
    let profile = fee_profile(candidate, settlement)?.clone();
    let profile = &profile;
    let mut meter = match V3GovernanceMeter::new(profile, signed, height, block) {
        Ok(meter) => meter,
        Err(MeterError::Rejected(e)) => return Ok(denied(0, e)),
        Err(MeterError::BlockCapacity) => return Err(PlanError::BlockCapacity),
        Err(e) => return Err(internal(format!("{e:?}"))),
    };
    macro_rules! pre {
        ($value:expr) => {
            match $value {
                Ok(v) => v,
                Err(e) => {
                    let used = meter
                        .finish()
                        .map_err(|e| internal(format!("{e:?}")))?
                        .used_gas;
                    return Ok(denied(used, e));
                }
            }
        };
    }
    macro_rules! metered {
        ($value:expr) => {
            match $value {
                Ok(()) => {}
                Err(MeterError::Rejected(e)) => {
                    let used = meter
                        .finish()
                        .map_err(|e| internal(format!("{e:?}")))?
                        .used_gas;
                    return Ok(denied(used, e));
                }
                Err(MeterError::BlockCapacity) => return Err(PlanError::BlockCapacity),
                Err(e) => return Err(internal(format!("{e:?}"))),
            }
        };
    }
    metered!(meter.signature());
    let verified = pre!(
        verify_signed(signed, &profile.limits()).map_err(|e| match e {
            OrdinaryV3VerificationError::BackendUnavailable =>
                "Governance signature backend unavailable".to_string(),
            other => other.to_string(),
        })
    );
    let body = verified.body();
    let actor_id = body.domain.account_id;
    let actor_key = hex::encode(actor_id);
    let account = pre!(book
        .accounts
        .find(&actor_key)
        .map_err(internal)?
        .ok_or("Unregistered or uninitialized governance account"));
    let recovery = &account.recovery;
    pre!(if recovery.domain != body.domain {
        Err("Governance signed domain differs from account state")
    } else if recovery.active_key != body.key
        || recovery.active_generation != body.authorization_generation
    {
        Err("Governance key or generation is stale")
    } else if recovery.spending_nonce != body.spending_nonce {
        Err("Governance spending nonce is stale")
    } else if !(height < body.expiry_height
        && body.expiry_height - height <= profile.limits().max_expiry_lifetime)
    {
        Err("Governance request expired or exceeds its lifetime")
    } else if !recovery.outgoing_allowed() {
        Err("Protected account cannot authorize governance")
    } else {
        Ok(())
    });
    let net = pre!(crate::ordinary_authority::network(body.domain.network));
    let address = AccountAddress::from_account_id(net, actor_id).encode();
    if account.address != address {
        return Err(internal("Governance account address differs from its ID"));
    }
    let native = settlement.account(&address).map_err(internal)?.clone();
    if native.nonce != recovery.spending_nonce {
        return Err(internal("Governance native and recovery nonces differ"));
    }
    metered!(meter.read(
        &logical::native_account(&address, &native, MAX_LOGICAL_BYTES)
            .map_err(|e| internal(format!("{e:?}")))?
    ));
    metered!(meter.read(
        &logical::recovery_account(&actor_id, &account, MAX_LOGICAL_BYTES)
            .map_err(|e| internal(format!("{e:?}")))?
    ));
    // The full signed cap and any deposit must be spendable before acceptance.
    let eligible_udrt = settlement
        .ordinary_eligible(&address, "udrt", false)
        .map_err(internal)?;
    pre!(if body.maximum_fee > eligible_udrt {
        Err("Insufficient eligible uDRT for the signed fee cap")
    } else {
        Ok(())
    });
    if let [Action::GovernanceDeposit { amount_udgt, .. }] = body.actions.as_slice() {
        let eligible_udgt = settlement
            .ordinary_eligible(&address, "udgt", false)
            .map_err(internal)?;
        pre!(if *amount_udgt > eligible_udgt {
            Err("Governance deposit exceeds eligible uDGT")
        } else {
            Ok(())
        });
    }
    // One admission queue serves v2 and v3; its context is the ordinary
    // profile, which is this profile's base.
    let reservation = pre!(governance_v3_reservations::reservation_request(
        signed,
        &verified,
        profile,
        dytallix_protocol_types::ordinary_fees::profile_digest(&profile.base).map_err(internal)?,
        height,
    )
    .map_err(|e| e.to_string()));
    let governance = settlement
        .governance
        .as_ref()
        .ok_or_else(|| internal("Governance state is not staged"))?;
    metered!(meter.read(
        &logical::governance_entry(store::HEADER_KEY, governance.header(), MAX_LOGICAL_BYTES)
            .map_err(|e| internal(format!("{e:?}")))?
    ));
    metered!(meter.accept());
    if admission {
        let summary = meter.finish().map_err(|e| internal(format!("{e:?}")))?;
        return Ok(OrdinaryExecutionResult {
            success: true,
            accepted: true,
            gas_used: summary.used_gas,
            fee: 0,
            error: None,
            receipt: None,
            reservation: Some(reservation),
        });
    }
    let mut effects = settlement.clone();
    let mut failure: Option<&'static str> = None;
    match meter.action() {
        Ok(()) => {}
        Err(MeterError::AcceptedOutOfGas) => failure = Some("OUT_OF_GAS"),
        Err(MeterError::BlockCapacity) => return Err(PlanError::BlockCapacity),
        Err(e) => return Err(internal(format!("{e:?}"))),
    }
    if failure.is_none() {
        match apply(
            &body.actions[0],
            candidate,
            actor_id,
            &address,
            &mut effects,
            &mut meter,
        )? {
            Ok(()) => {}
            Err(Rule(code)) => failure = Some(code),
        }
    }
    let summary = meter.finish().map_err(|e| internal(format!("{e:?}")))?;
    if !summary.accepted || summary.used_gas > body.gas_limit {
        return Err(internal("Governance measured work differs from acceptance"));
    }
    let charge = u128::from(summary.used_gas.max(profile.base.minimum_gas))
        .checked_mul(u128::from(profile.base.gas_price))
        .ok_or_else(|| internal("Governance fee overflow"))?;
    if charge > body.maximum_fee {
        return Err(internal("Governance charge exceeds the signed cap"));
    }
    if failure.is_some() {
        effects = settlement.clone();
    }
    effects
        .charge_sponsored(&address, charge)
        .map_err(internal)?;
    let nonce_after = body
        .spending_nonce
        .checked_add(1)
        .ok_or_else(|| internal("Governance nonce exhausted"))?;
    effects.account(&address).map_err(internal)?.nonce = nonce_after;
    book.accounts
        .find_mut(&actor_key)
        .map_err(internal)?
        .ok_or_else(|| internal("Governance actor disappeared"))?
        .recovery
        .spending_nonce = nonce_after;
    *settlement = effects;
    Ok(OrdinaryExecutionResult {
        success: failure.is_none(),
        accepted: true,
        gas_used: summary.used_gas,
        fee: charge,
        error: failure.map(str::to_owned),
        receipt: None,
        reservation: Some(reservation),
    })
}

/// Apply the one governance action to `effects`. A rule failure leaves the
/// caller to discard `effects`; running out of gas is also a rule failure.
fn apply(
    action: &Action,
    candidate: &GovernanceCandidateConfig,
    actor: [u8; 32],
    address: &str,
    effects: &mut Settlement,
    meter: &mut V3GovernanceMeter<'_>,
) -> Result<std::result::Result<(), Rule>> {
    macro_rules! charge {
        ($record:expr, $op:ident) => {
            match meter.$op(&$record.map_err(|e| internal(format!("{e:?}")))?) {
                Ok(()) => {}
                Err(MeterError::AcceptedOutOfGas) => return Ok(Err(Rule("OUT_OF_GAS"))),
                Err(MeterError::BlockCapacity) => return Err(PlanError::BlockCapacity),
                Err(e) => return Err(internal(format!("{e:?}"))),
            }
        };
    }
    macro_rules! rule {
        ($value:expr) => {
            if let Err(rule) = $value {
                return Ok(Err(rule));
            }
        };
    }
    let governance = effects
        .governance
        .as_mut()
        .ok_or_else(|| internal("Governance state is not staged"))?;
    let proposal_id = match action {
        Action::GovernanceProposal {
            proposal_id,
            action_class,
            action_data,
            action_digest,
        } => {
            rule!(governance_actions::validate_proposal(
                candidate,
                *action_class,
                action_data
            ));
            if governance.parent_bond(address).map_err(internal)? == 0 {
                return Ok(Err(Rule("GOVERNANCE_PROPOSER_NOT_BONDED")));
            }
            rule!(governance.propose(
                actor,
                *proposal_id,
                *action_class,
                action_data.clone(),
                *action_digest
            ));
            *proposal_id
        }
        Action::GovernanceDeposit {
            proposal_id,
            amount_udgt,
        } => {
            if let Some(p) = governance.proposal(*proposal_id).map_err(internal)? {
                charge!(
                    logical::governance_entry(
                        &store::proposal_key(*proposal_id),
                        &*p,
                        MAX_LOGICAL_BYTES
                    ),
                    read
                );
            }
            let governance = effects.governance.as_mut().expect("governance staged");
            rule!(governance.deposit(actor, *proposal_id, *amount_udgt));
            let native = effects.account(address).map_err(internal)?;
            let balance = native
                .balance_of("udgt")
                .checked_sub(*amount_udgt)
                .ok_or_else(|| internal("Governance deposit exceeds reserved uDGT"))?;
            native.set_balance("udgt", balance);
            charge!(
                logical::native_account(address, &native.clone(), MAX_LOGICAL_BYTES),
                write
            );
            *proposal_id
        }
        Action::GovernanceVote {
            proposal_id,
            choice: vote,
        } => {
            if let Some(p) = governance.proposal(*proposal_id).map_err(internal)? {
                charge!(
                    logical::governance_entry(
                        &store::proposal_key(*proposal_id),
                        &*p,
                        MAX_LOGICAL_BYTES
                    ),
                    read
                );
                if let store::Phase::Voting {
                    snapshot_height, ..
                } = p.phase
                {
                    if let Some(weight) = governance
                        .weight(snapshot_height, &actor)
                        .map_err(internal)?
                    {
                        charge!(
                            logical::governance_entry(
                                &store::weight_key(snapshot_height, &actor),
                                &weight,
                                MAX_LOGICAL_BYTES
                            ),
                            read
                        );
                    }
                }
            }
            let governance = effects.governance.as_mut().expect("governance staged");
            rule!(governance.cast(actor, *proposal_id, choice(*vote)));
            charge!(
                logical::governance_entry(
                    &store::vote_key(*proposal_id, &actor),
                    &choice(*vote),
                    MAX_LOGICAL_BYTES
                ),
                write
            );
            *proposal_id
        }
        _ => {
            return Err(internal(
                "Ordinary-v3 action escaped its single-action check",
            ))
        }
    };
    let governance = effects.governance.as_ref().expect("governance staged");
    let proposal = governance
        .proposal(proposal_id)
        .map_err(internal)?
        .ok_or_else(|| internal("Governance proposal disappeared"))?;
    charge!(
        logical::governance_entry(
            &store::proposal_key(proposal_id),
            &*proposal,
            MAX_LOGICAL_BYTES
        ),
        write
    );
    charge!(
        logical::governance_entry(store::HEADER_KEY, governance.header(), MAX_LOGICAL_BYTES),
        write
    );
    Ok(Ok(()))
}

/// Open the committed governance state for a block or an admission check.
pub(crate) fn open_store(
    settlement: &Settlement,
    candidate: &GovernanceCandidateConfig,
) -> anyhow::Result<GovernanceStore> {
    GovernanceStore::open(settlement.storage.clone(), candidate.store_rules())
}

// Tested through the engine in `ordinary_consensus_tests.rs` (module `governance`).
