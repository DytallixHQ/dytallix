use crate::{runtime::staking::StakingModule, state::State, storage::state::Storage};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::{fs, path::PathBuf};

/// Governance configuration with sensible defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceConfig {
    pub min_deposit: u128,      // 1000 DGT in micro units
    pub deposit_period: u64,    // blocks
    pub voting_period: u64,     // blocks
    pub gas_limit: u64,         // Current gas limit parameter
    pub max_gas_per_block: u64, // Consensus parameter for max gas per block
    pub quorum: u128,           // Minimum participation required (in basis points)
    pub threshold: u128, // Minimum yes votes for proposal to pass (in basis points, e.g., 5000 = 50%)
    pub veto_threshold: u128, // Minimum no_with_veto votes to veto proposal (in basis points)
}

impl Default for GovernanceConfig {
    fn default() -> Self {
        Self {
            min_deposit: 1_000_000_000,    // 1000 DGT (assuming 6 decimal places)
            deposit_period: 300,           // 300 blocks for deposit period
            voting_period: 300,            // 300 blocks for voting period
            gas_limit: 2000, // Testnet-friendly: 2000 gas * 1000 = 2M udgt = 2 DGT fee (enough for intrinsic gas)
            max_gas_per_block: 10_000_000, // Default max gas per block
            quorum: 6700,    // 67.00% quorum required (in basis points)
            threshold: 5000, // 50% threshold for passing (in basis points)
            veto_threshold: 3333, // 33.33% veto threshold (in basis points)
        }
    }
}

fn basis_points_ceil(value: u128, basis_points: u128) -> Result<u128, String> {
    if basis_points > 10_000 {
        return Err("Governance basis points exceed 10000".to_string());
    }
    let whole = (value / 10_000)
        .checked_mul(basis_points)
        .ok_or("Governance threshold product exceeds u128")?;
    let remainder = (value % 10_000)
        .checked_mul(basis_points)
        .ok_or("Governance threshold product exceeds u128")?;
    whole
        .checked_add(remainder / 10_000)
        .and_then(|value| value.checked_add(u128::from(remainder % 10_000 != 0)))
        .ok_or_else(|| "Governance threshold exceeds u128".to_string())
}

/// Proposal data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: u64,
    pub title: String,
    pub description: String,
    pub proposal_type: ProposalType,
    pub status: ProposalStatus,
    pub total_deposit: u128,
    pub submit_height: u64,
    pub deposit_end_height: u64,
    pub voting_start_height: u64,
    pub voting_end_height: u64,
    pub tally: Option<TallyResult>,
}

/// Types of proposals supported
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProposalType {
    ParameterChange { key: String, value: String },
}

/// Proposal status transitions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProposalStatus {
    DepositPeriod,
    VotingPeriod,
    Passed,
    Rejected,
    Failed, // For expired deposits without reaching minimum
    Executed,
    FailedExecution, // For proposals that passed but failed to execute
}

/// Vote on a proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub proposal_id: u64,
    pub voter: String,
    pub option: VoteOption,
    pub weight: u128, // Voting power recorded when the vote was cast
}

/// Deposit on a proposal (for tracking individual deposits)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deposit {
    pub proposal_id: u64,
    pub depositor: String,
    pub amount: u128,
    pub denom: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VoteOption {
    Yes,
    No,
    NoWithVeto,
    Abstain,
}

/// Tally result for a proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TallyResult {
    pub yes: u128,
    pub no: u128,
    pub no_with_veto: u128,
    pub abstain: u128,
    pub total_voting_power: u128,
}

/// Governance events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GovernanceEvent {
    ProposalSubmitted {
        id: u64,
    },
    Deposit {
        id: u64,
        amount: u128,
    },
    VotingStarted {
        id: u64,
    },
    VoteCast {
        id: u64,
        voter: String,
    },
    ProposalPassed {
        id: u64,
        yes: u128,
        no: u128,
        abstain: u128,
    },
    ProposalRejected {
        id: u64,
        reason: Option<String>,
    },
    ProposalExecuted {
        id: u64,
    },
    ExecutionFailed {
        id: u64,
        error: String,
    },
    ParameterChanged {
        key: String,
        old_value: String,
        new_value: String,
    },
    DepositRefunded {
        proposal_id: u64,
        depositor: String,
        amount: u128,
    },
}

pub struct GovernanceModule {
    storage: Arc<Storage>,
    state: Arc<Mutex<State>>,
    staking: Arc<Mutex<StakingModule>>,
    config: GovernanceConfig,
    events: Vec<GovernanceEvent>,
}

impl GovernanceModule {
    /// Open the persisted governance configuration for the node process.
    /// Environment values apply only when governance is first initialized.
    pub fn open(
        storage: Arc<Storage>,
        state: Arc<Mutex<State>>,
        staking: Arc<Mutex<StakingModule>>,
        enable_governance: bool,
    ) -> Result<Self, String> {
        let stored = storage
            .db
            .get("gov:config")
            .map_err(|e| format!("Failed to read governance config: {e}"))?;
        let config = if let Some(bytes) = stored {
            let config: GovernanceConfig = bincode::deserialize(&bytes)
                .map_err(|e| format!("Failed to deserialize governance config: {e}"))?;
            Self::validate_config(&config)?;
            config
        } else if enable_governance {
            let prefix = b"gov:";
            if let Some(entry) = storage
                .db
                .iterator(rocksdb::IteratorMode::From(
                    prefix,
                    rocksdb::Direction::Forward,
                ))
                .next()
            {
                let (key, _) =
                    entry.map_err(|e| format!("Failed to inspect governance state: {e}"))?;
                if key.starts_with(prefix) {
                    return Err("Existing governance state has no configuration; an explicit migration is required".to_string());
                }
            }
            let mut config = GovernanceConfig::default();
            Self::read_initial_env_overrides(&mut config)?;
            Self::validate_config(&config)?;
            storage
                .db
                .put(
                    "gov:config",
                    bincode::serialize(&config)
                        .map_err(|e| format!("Failed to serialize governance config: {e}"))?,
                )
                .map_err(|e| format!("Failed to initialize governance config: {e}"))?;
            config
        } else {
            GovernanceConfig::default()
        };
        Ok(Self {
            storage,
            state,
            staking,
            config,
            events: Vec::new(),
        })
    }

    fn validate_config(config: &GovernanceConfig) -> Result<(), String> {
        if config.min_deposit == 0
            || config.deposit_period == 0
            || config.voting_period == 0
            || config.gas_limit == 0
            || config.max_gas_per_block == 0
            || config.quorum > 10_000
            || config.threshold > 10_000
            || config.veto_threshold > 10_000
        {
            return Err("Stored governance configuration is invalid".to_string());
        }
        Ok(())
    }

    fn read_initial_env_overrides(config: &mut GovernanceConfig) -> Result<(), String> {
        fn read<T: std::str::FromStr>(name: &str) -> Result<Option<T>, String> {
            match std::env::var(name) {
                Ok(raw) => raw
                    .parse::<T>()
                    .map(Some)
                    .map_err(|_| format!("Invalid initial governance setting: {name}")),
                Err(std::env::VarError::NotPresent) => Ok(None),
                Err(_) => Err(format!("Invalid initial governance setting: {name}")),
            }
        }
        if let Some(value) = read("DYT_GOV_MIN_DEPOSIT")? {
            config.min_deposit = value;
        }
        if let Some(value) = read("DYT_GOV_DEPOSIT_PERIOD")? {
            config.deposit_period = value;
        }
        if let Some(value) = read("DYT_GOV_VOTING_PERIOD")? {
            config.voting_period = value;
        }
        if let Some(value) = read("DYT_GOV_QUORUM_BPS")? {
            config.quorum = value;
        }
        if let Some(value) = read("DYT_GOV_THRESHOLD_BPS")? {
            config.threshold = value;
        }
        if let Some(value) = read("DYT_GOV_VETO_BPS")? {
            config.veto_threshold = value;
        }
        Ok(())
    }

    pub fn new(
        storage: Arc<Storage>,
        state: Arc<Mutex<State>>,
        staking: Arc<Mutex<StakingModule>>,
    ) -> Self {
        let config = GovernanceConfig::default();
        Self {
            storage,
            state,
            staking,
            config,
            events: Vec::new(),
        }
    }

    pub fn new_with_config(
        storage: Arc<Storage>,
        state: Arc<Mutex<State>>,
        staking: Arc<Mutex<StakingModule>>,
        config: GovernanceConfig,
    ) -> Self {
        Self {
            storage,
            state,
            staking,
            config,
            events: Vec::new(),
        }
    }

    /// Submit a new proposal
    pub fn submit_proposal(
        &mut self,
        height: u64,
        title: String,
        description: String,
        proposal_type: ProposalType,
    ) -> Result<u64, String> {
        let last_id = self
            .storage
            .db
            .get("gov:last_proposal_id")
            .map_err(|e| format!("Failed to read proposal ID: {e}"))?
            .map(|bytes| {
                bincode::deserialize::<u64>(&bytes)
                    .map_err(|e| format!("Failed to deserialize proposal ID: {e}"))
            })
            .transpose()?
            .unwrap_or(0);
        let proposal_id = last_id.checked_add(1).ok_or("Proposal ID overflow")?;
        let deposit_end_height = height
            .checked_add(self.config.deposit_period)
            .ok_or("Deposit end height overflow")?;
        let voting_end_height = deposit_end_height
            .checked_add(self.config.voting_period)
            .ok_or("Voting end height overflow")?;

        let proposal = Proposal {
            id: proposal_id,
            title,
            description,
            proposal_type,
            status: ProposalStatus::DepositPeriod,
            total_deposit: 0,
            submit_height: height,
            deposit_end_height,
            voting_start_height: deposit_end_height,
            voting_end_height,
            tally: None,
        };

        let mut batch = rocksdb::WriteBatch::default();
        batch.put(
            "gov:last_proposal_id",
            bincode::serialize(&proposal_id)
                .map_err(|e| format!("Failed to serialize proposal ID: {e}"))?,
        );
        batch.put(
            format!("gov:proposal:{proposal_id}"),
            bincode::serialize(&proposal)
                .map_err(|e| format!("Failed to serialize proposal: {e}"))?,
        );
        self.storage
            .db
            .write(batch)
            .map_err(|e| format!("Failed to commit proposal: {e}"))?;
        self.emit_event(GovernanceEvent::ProposalSubmitted { id: proposal_id });
        let _ = self.write_governance_evidence();
        Ok(proposal_id)
    }

    /// Deposit DGT tokens on a proposal
    pub fn deposit(
        &mut self,
        height: u64,
        depositor: &str,
        proposal_id: u64,
        amount: u128,
        denom: &str,
    ) -> Result<(), String> {
        if denom != "udgt" {
            return Err("Only DGT (udgt) deposits are allowed".to_string());
        }
        if amount == 0 {
            return Err("Deposit amount must be positive".to_string());
        }

        let mut proposal = self
            ._get_proposal(proposal_id)?
            .ok_or("Proposal not found")?;

        // Check if we're in deposit period
        if proposal.status != ProposalStatus::DepositPeriod || height > proposal.deposit_end_height
        {
            return Err("Proposal is not in deposit period".to_string());
        }
        proposal.total_deposit = proposal
            .total_deposit
            .checked_add(amount)
            .ok_or("Proposal deposit total overflow")?;

        let deposit_key = format!("gov:deposit:{proposal_id}:{depositor}");
        let prior = self
            .storage
            .db
            .get(&deposit_key)
            .map_err(|e| format!("Failed to read prior deposit: {e}"))?
            .map(|bytes| {
                bincode::deserialize::<Deposit>(&bytes)
                    .map_err(|e| format!("Failed to deserialize prior deposit: {e}"))
            })
            .transpose()?;
        if let Some(existing) = &prior {
            if existing.proposal_id != proposal_id
                || existing.depositor != depositor
                || existing.denom != denom
            {
                return Err("Stored deposit identity does not match its key".to_string());
            }
        }
        let deposit = Deposit {
            proposal_id,
            depositor: depositor.to_string(),
            amount: prior
                .map(|existing| existing.amount)
                .unwrap_or(0)
                .checked_add(amount)
                .ok_or("Depositor total overflow")?,
            denom: denom.to_string(),
        };

        // Check if min deposit reached - transition to voting period
        let start_voting = proposal.total_deposit >= self.config.min_deposit;
        if start_voting {
            proposal.status = ProposalStatus::VotingPeriod;
            // When transitioning early, start voting immediately and set end relative to now
            proposal.voting_start_height = height;
            proposal.voting_end_height = height
                .checked_add(self.config.voting_period)
                .ok_or("Voting end height overflow")?;
        }

        // Commit balance, deposit record, and proposal together. Update the
        // account cache and emit events only after the database commit succeeds.
        let mut state = self.state.lock().unwrap();
        let mut account = state.get_account(depositor);
        account.sub_balance(denom, amount)?;
        let mut batch = rocksdb::WriteBatch::default();
        batch.put(
            format!("acct:balances:{depositor}"),
            bincode::serialize(&account.balances)
                .map_err(|e| format!("Failed to serialize deposit balance: {e}"))?,
        );
        batch.put(
            deposit_key,
            bincode::serialize(&deposit)
                .map_err(|e| format!("Failed to serialize deposit: {e}"))?,
        );
        batch.put(
            format!("gov:proposal:{proposal_id}"),
            bincode::serialize(&proposal)
                .map_err(|e| format!("Failed to serialize proposal: {e}"))?,
        );
        self.storage
            .db
            .write(batch)
            .map_err(|e| format!("Failed to commit deposit: {e}"))?;
        state.accounts.insert(depositor.to_string(), account);
        drop(state);
        if start_voting {
            self.emit_event(GovernanceEvent::VotingStarted { id: proposal_id });
        }
        self.emit_event(GovernanceEvent::Deposit {
            id: proposal_id,
            amount,
        });
        let _ = self.write_governance_evidence();
        Ok(())
    }

    /// Vote on a proposal
    pub fn vote(
        &mut self,
        height: u64,
        voter: &str,
        proposal_id: u64,
        option: VoteOption,
    ) -> Result<(), String> {
        let proposal = self
            ._get_proposal(proposal_id)?
            .ok_or("Proposal not found")?;

        // Check if we're in voting period
        if proposal.status != ProposalStatus::VotingPeriod {
            return Err("Proposal is not in voting period".to_string());
        }

        if height < proposal.voting_start_height || height > proposal.voting_end_height {
            return Err("Not in voting period".to_string());
        }

        // Check if voter already voted
        if self.has_voted(proposal_id, voter)? {
            return Err("Voter has already voted on this proposal".to_string());
        }

        // Get voter's voting power from staking (delegations + validator self-stake)
        let weight = self.voting_power(voter)?;

        let vote = Vote {
            proposal_id,
            voter: voter.to_string(),
            option,
            weight,
        };

        self.store_vote(&vote)?;
        self.emit_event(GovernanceEvent::VoteCast {
            id: proposal_id,
            voter: voter.to_string(),
        });
        let _ = self.write_governance_evidence();
        Ok(())
    }

    /// Process end of block - handle period transitions and execution
    pub fn end_block(&mut self, height: u64) -> Result<(), String> {
        let proposal_ids = self.get_all_proposal_ids()?;

        for proposal_id in proposal_ids {
            if let Some(mut proposal) = self._get_proposal(proposal_id)? {
                match proposal.status {
                    ProposalStatus::DepositPeriod => {
                        // Updated: if min deposit has been reached (at any time), transition to voting now.
                        // Deposits after deposit_end_height are rejected in `deposit`, so this is safe and fixes timing races.
                        if proposal.total_deposit >= self.config.min_deposit {
                            proposal.status = ProposalStatus::VotingPeriod;
                            proposal.voting_start_height = height;
                            proposal.voting_end_height = height
                                .checked_add(self.config.voting_period)
                                .ok_or("Voting end height overflow")?;
                            self.store_proposal(&proposal)?;
                            self.emit_event(GovernanceEvent::VotingStarted { id: proposal_id });
                            let _ = self.write_governance_evidence();
                            continue;
                        }

                        if height > proposal.deposit_end_height {
                            // Deposit period ended without reaching min deposit
                            proposal.status = ProposalStatus::Failed;
                            self.refund_terminal_proposal(&proposal)?;
                            self.emit_event(GovernanceEvent::ProposalRejected {
                                id: proposal_id,
                                reason: Some("Insufficient deposits - proposal failed".to_string()),
                            });
                            let _ = self.write_governance_evidence();
                        }
                    }
                    ProposalStatus::VotingPeriod => {
                        if height > proposal.voting_end_height {
                            // Voting period ended - tally votes
                            let tally = self.tally(proposal_id)?;
                            proposal.tally = Some(tally.clone());

                            // Use enhanced tally logic to determine if proposal passes
                            if self.proposal_passes(&tally)? {
                                proposal.status = ProposalStatus::Passed;
                                self.emit_event(GovernanceEvent::ProposalPassed {
                                    id: proposal_id,
                                    yes: tally.yes,
                                    no: tally.no,
                                    abstain: tally.abstain,
                                });
                                let _ = self.write_governance_evidence();
                            } else {
                                proposal.status = ProposalStatus::Rejected;
                                self.refund_terminal_proposal(&proposal)?;
                                let reason = if tally.total_voting_power
                                    < basis_points_ceil(
                                        self.get_total_staking_power()?,
                                        self.config.quorum,
                                    )? {
                                    "Quorum not met"
                                } else if tally.no_with_veto
                                    >= basis_points_ceil(
                                        tally
                                            .yes
                                            .checked_add(tally.no)
                                            .and_then(|weight| {
                                                weight.checked_add(tally.no_with_veto)
                                            })
                                            .ok_or("Participating voting power exceeds u128")?,
                                        self.config.veto_threshold,
                                    )?
                                {
                                    "Proposal vetoed"
                                } else {
                                    "Threshold not met"
                                };
                                self.emit_event(GovernanceEvent::ProposalRejected {
                                    id: proposal_id,
                                    reason: Some(reason.to_string()),
                                });
                                let _ = self.write_governance_evidence();
                            }
                            if proposal.status == ProposalStatus::Passed {
                                self.store_proposal(&proposal)?;
                            }
                        }
                    }
                    ProposalStatus::Passed => {
                        // A recorded FailedExecution is a completed outcome.
                        // Storage or deposit errors leave the proposal Passed
                        // and must stop this end-block call.
                        if let Err(error) = self.execute(proposal_id) {
                            let current = self
                                ._get_proposal(proposal_id)?
                                .ok_or("Proposal disappeared during execution")?;
                            if current.status != ProposalStatus::FailedExecution {
                                return Err(error);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Tally votes for a proposal
    pub fn tally(&self, proposal_id: u64) -> Result<TallyResult, String> {
        let votes = self._get_proposal_votes(proposal_id)?;

        let mut yes = 0u128;
        let mut no = 0u128;
        let mut no_with_veto = 0u128;
        let mut abstain = 0u128;

        for vote in votes {
            let bucket = match vote.option {
                VoteOption::Yes => &mut yes,
                VoteOption::No => &mut no,
                VoteOption::NoWithVeto => &mut no_with_veto,
                VoteOption::Abstain => &mut abstain,
            };
            *bucket = bucket
                .checked_add(vote.weight)
                .ok_or("Vote option total exceeds u128")?;
        }

        let total_voting_power = yes
            .checked_add(no)
            .and_then(|total| total.checked_add(no_with_veto))
            .and_then(|total| total.checked_add(abstain))
            .ok_or("Total voting power exceeds u128")?;

        Ok(TallyResult {
            yes,
            no,
            no_with_veto,
            abstain,
            total_voting_power,
        })
    }

    /// Check if a proposal passes based on governance parameters
    pub fn proposal_passes(&self, tally: &TallyResult) -> Result<bool, String> {
        // Get total staking power for quorum calculation
        let total_staking_power = self.get_total_staking_power()?;

        // Check quorum: minimum participation required
        let quorum_required = basis_points_ceil(total_staking_power, self.config.quorum)?;
        if tally.total_voting_power < quorum_required {
            return Ok(false); // Quorum not met
        }

        // Check veto threshold: if no_with_veto >= veto_threshold, proposal is vetoed
        let participating_votes = tally
            .yes
            .checked_add(tally.no)
            .and_then(|total| total.checked_add(tally.no_with_veto))
            .ok_or("Participating voting power exceeds u128")?;
        let veto_threshold = basis_points_ceil(participating_votes, self.config.veto_threshold)?;
        if tally.no_with_veto >= veto_threshold {
            return Ok(false); // Proposal vetoed
        }

        // Check threshold: yes votes must be >= threshold of participating votes (excluding abstain)
        if participating_votes == 0 {
            return Ok(false); // No participating votes
        }

        let threshold_required = basis_points_ceil(participating_votes, self.config.threshold)?;
        Ok(tally.yes >= threshold_required)
    }

    /// Get voting power for a specific address (derived from delegations and validator self-bond)
    pub fn voting_power(&self, address: &str) -> Result<u128, String> {
        let staking = self.staking.lock().unwrap();

        // Liquid balances do not contribute voting power. Each delegator's
        // recorded bonded stake is counted once; validator totals are not added.
        Ok(staking.load_delegator_record(address).stake_amount)
    }

    /// Get total voting power across all eligible stakers
    pub fn total_voting_power(&self) -> Result<u128, String> {
        let staking = self.staking.lock().unwrap();
        Ok(staking.total_stake)
    }

    /// Get active set voting power (currently same as total for MVP)
    pub fn active_set_voting_power(&self) -> Result<u128, String> {
        // For MVP, active set is same as total staking power
        // In future this would filter to only active validators
        self.total_voting_power()
    }

    /// Get total staking power for quorum calculation (updated to use staking module)
    fn get_total_staking_power(&self) -> Result<u128, String> {
        // Use the new total_voting_power function
        let total_power = self.total_voting_power()?;

        // Minimum total power to avoid division by zero
        Ok(total_power.max(1))
    }

    /// Execute a passed proposal
    pub fn execute(&mut self, proposal_id: u64) -> Result<(), String> {
        let mut proposal = self
            ._get_proposal(proposal_id)?
            .ok_or("Proposal not found")?;

        if proposal.status != ProposalStatus::Passed {
            return Err("Proposal has not passed".to_string());
        }

        let (key, value) = match &proposal.proposal_type {
            ProposalType::ParameterChange { key, value } => (key.clone(), value.clone()),
        };
        let planned = self.plan_parameter_change(&key, &value);
        proposal.status = if planned.is_ok() {
            ProposalStatus::Executed
        } else {
            ProposalStatus::FailedExecution
        };

        let deposits = self.get_proposal_deposits(proposal_id)?;
        let deposit_total = deposits.iter().try_fold(0u128, |sum, deposit| {
            if deposit.denom != "udgt" || deposit.amount == 0 {
                return Err("Stored governance deposit is invalid".to_string());
            }
            sum.checked_add(deposit.amount)
                .ok_or_else(|| "Stored governance deposit total overflow".to_string())
        })?;
        if deposit_total != proposal.total_deposit {
            return Err("Stored governance deposits do not match proposal total".to_string());
        }

        let mut state = self.state.lock().unwrap();
        let mut batch = rocksdb::WriteBatch::default();
        let mut credited = Vec::with_capacity(deposits.len());
        for deposit in &deposits {
            let mut account = state.get_account(&deposit.depositor);
            let balance = account
                .balance_of(&deposit.denom)
                .checked_add(deposit.amount)
                .ok_or("Governance deposit refund overflow")?;
            account.set_balance(&deposit.denom, balance);
            batch.put(
                format!("acct:balances:{}", deposit.depositor),
                bincode::serialize(&account.balances)
                    .map_err(|e| format!("Failed to serialize refund balance: {e}"))?,
            );
            credited.push((deposit.depositor.clone(), account));
        }
        batch.put(
            format!("gov:proposal:{proposal_id}"),
            bincode::serialize(&proposal)
                .map_err(|e| format!("Failed to serialize executed proposal: {e}"))?,
        );
        if let Ok((next_config, _)) = &planned {
            batch.put(
                "gov:config",
                bincode::serialize(next_config)
                    .map_err(|e| format!("Failed to serialize governance config: {e}"))?,
            );
        }
        self.storage
            .db
            .write(batch)
            .map_err(|e| format!("Failed to commit governance execution: {e}"))?;
        for (depositor, account) in credited {
            state.accounts.insert(depositor, account);
        }
        drop(state);

        match planned {
            Ok((next_config, old_value)) => {
                self.config = next_config;
                self.emit_event(GovernanceEvent::ParameterChanged {
                    key,
                    old_value,
                    new_value: value,
                });
                self.emit_event(GovernanceEvent::ProposalExecuted { id: proposal_id });
                let _ = self.write_governance_evidence();
                Ok(())
            }
            Err(error) => {
                self.emit_event(GovernanceEvent::ExecutionFailed {
                    id: proposal_id,
                    error: error.clone(),
                });
                let _ = self.write_governance_evidence();
                Err(error)
            }
        }
    }

    fn plan_parameter_change(
        &self,
        key: &str,
        value: &str,
    ) -> Result<(GovernanceConfig, String), String> {
        let old_value = self.get_parameter_value(key)?;
        let mut next_config = self.config.clone();
        match key {
            "gas_limit" => {
                let gas_limit: u64 = value
                    .parse()
                    .map_err(|_| "Invalid gas_limit value: must be a valid u64".to_string())?;

                // Validation: gas limit should be reasonable (between 1K and 100M)
                if !(1_000..=100_000_000).contains(&gas_limit) {
                    return Err("gas_limit must be between 1,000 and 100,000,000".to_string());
                }

                next_config.gas_limit = gas_limit;
            }
            "consensus.max_gas_per_block" => {
                let max_gas_per_block: u64 = value.parse().map_err(|_| {
                    "Invalid consensus.max_gas_per_block value: must be a valid u64".to_string()
                })?;

                // Validation: max gas per block should be reasonable (between 1M and 1B)
                if !(1_000_000..=1_000_000_000).contains(&max_gas_per_block) {
                    return Err(
                        "consensus.max_gas_per_block must be between 1,000,000 and 1,000,000,000"
                            .to_string(),
                    );
                }

                next_config.max_gas_per_block = max_gas_per_block;
            }
            "staking_reward_rate" => {
                return Err("Legacy staking reward-rate mutation is retired".into())
            }
            _ => {
                return Err(format!(
                    "Parameter '{}' is not governable. Allowed parameters: {:?}",
                    key,
                    self.get_governable_parameters()
                ))
            }
        }
        Ok((next_config, old_value))
    }

    /// Apply a validated parameter directly for local administration and tests.
    fn apply_parameter_change(&mut self, key: &str, value: &str) -> Result<(), String> {
        let (next_config, old_value) = self.plan_parameter_change(key, value)?;
        let data = bincode::serialize(&next_config)
            .map_err(|e| format!("Failed to serialize governance config: {e}"))?;
        self.storage
            .db
            .put("gov:config", data)
            .map_err(|e| format!("Failed to store config: {e}"))?;
        self.config = next_config;
        self.emit_event(GovernanceEvent::ParameterChanged {
            key: key.to_string(),
            old_value,
            new_value: value.to_string(),
        });
        let _ = self.write_governance_evidence();
        Ok(())
    }

    fn get_parameter_value(&self, key: &str) -> Result<String, String> {
        match key {
            "gas_limit" => Ok(self.config.gas_limit.to_string()),
            "consensus.max_gas_per_block" => Ok(self.config.max_gas_per_block.to_string()),
            "staking_reward_rate" => {
                let staking = self.staking.lock().unwrap();
                let bps = staking.get_reward_rate_bps();
                // Convert back to decimal fraction string
                let frac = (bps as f64) / 10_000.0;
                Ok(format!("{frac:.4}"))
            }
            _ => Err(format!("Unknown parameter: {key}")),
        }
    }

    pub fn get_governable_parameters(&self) -> Vec<String> {
        vec![
            "gas_limit".to_string(),
            "consensus.max_gas_per_block".to_string(),
        ]
    }

    /// Get current value of a governance parameter
    pub fn get_config(&self) -> &GovernanceConfig {
        &self.config
    }

    /// Get events
    pub fn get_events(&self) -> &[GovernanceEvent] {
        &self.events
    }

    /// Clear events (should be called after processing)
    pub fn clear_events(&mut self) {
        self.events.clear();
    }

    /// Public method to get proposal (exposed for RPC)
    pub fn get_proposal(&self, proposal_id: u64) -> Result<Option<Proposal>, String> {
        self._get_proposal(proposal_id)
    }

    /// Get all proposals (for API endpoint)
    pub fn get_all_proposals(&self) -> Result<Vec<Proposal>, String> {
        let mut proposals = Vec::new();
        for id in self.get_all_proposal_ids()? {
            proposals.push(
                self._get_proposal(id)?
                    .ok_or_else(|| format!("Proposal {id} disappeared during read"))?,
            );
        }
        Ok(proposals)
    }

    /// Get votes for a proposal (exposed for RPC)
    pub fn get_proposal_votes(&self, proposal_id: u64) -> Result<Vec<Vote>, String> {
        self._get_proposal_votes(proposal_id)
    }

    // Storage helper methods

    fn store_proposal(&self, proposal: &Proposal) -> Result<(), String> {
        let key = format!("gov:proposal:{}", proposal.id);
        let data = bincode::serialize(proposal)
            .map_err(|e| format!("Failed to serialize proposal: {e}"))?;
        self.storage
            .db
            .put(key, data)
            .map_err(|e| format!("Failed to store proposal: {e}"))?;
        Ok(())
    }

    fn _get_proposal(&self, proposal_id: u64) -> Result<Option<Proposal>, String> {
        let key = format!("gov:proposal:{proposal_id}");
        match self.storage.db.get(key) {
            Ok(Some(data)) => {
                let proposal = bincode::deserialize::<Proposal>(&data)
                    .map_err(|e| format!("Failed to deserialize proposal: {e}"))?;
                if proposal.id != proposal_id {
                    return Err("Stored proposal ID does not match its key".to_string());
                }
                Ok(Some(proposal))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(format!("Failed to get proposal: {e}")),
        }
    }

    fn store_vote(&self, vote: &Vote) -> Result<(), String> {
        let key = format!("gov:vote:{}:{}", vote.proposal_id, vote.voter);
        let data =
            bincode::serialize(vote).map_err(|e| format!("Failed to serialize vote: {e}"))?;
        self.storage
            .db
            .put(key, data)
            .map_err(|e| format!("Failed to store vote: {e}"))?;
        Ok(())
    }

    fn has_voted(&self, proposal_id: u64, voter: &str) -> Result<bool, String> {
        let key = format!("gov:vote:{proposal_id}:{voter}");
        Ok(self
            .storage
            .db
            .get(key)
            .map_err(|e| format!("Failed to check vote: {e}"))?
            .is_some())
    }

    fn _get_proposal_votes(&self, proposal_id: u64) -> Result<Vec<Vote>, String> {
        let prefix = format!("gov:vote:{proposal_id}:");
        let mut votes = Vec::new();
        // Stored votes define the tally. Account-cache contents are unrelated to
        // voter eligibility and are empty after restart until accounts are read.
        for entry in self.storage.db.iterator(rocksdb::IteratorMode::From(
            prefix.as_bytes(),
            rocksdb::Direction::Forward,
        )) {
            let (key, data) = entry.map_err(|e| format!("Failed to read proposal votes: {e}"))?;
            if !key.starts_with(prefix.as_bytes()) {
                break;
            }
            let vote: Vote = bincode::deserialize(&data)
                .map_err(|e| format!("Failed to deserialize proposal vote: {e}"))?;
            let expected_key = format!("{prefix}{}", vote.voter);
            if vote.proposal_id != proposal_id || key.as_ref() != expected_key.as_bytes() {
                return Err("Stored vote identity does not match its key".to_string());
            }
            votes.push(vote);
        }

        Ok(votes)
    }

    fn get_all_proposal_ids(&self) -> Result<Vec<u64>, String> {
        let last_id = self
            .storage
            .db
            .get("gov:last_proposal_id")
            .map_err(|e| format!("Failed to read proposal ID counter: {e}"))?
            .map(|bytes| {
                bincode::deserialize::<u64>(&bytes)
                    .map_err(|e| format!("Failed to deserialize proposal ID counter: {e}"))
            })
            .transpose()?
            .unwrap_or(0);
        let prefix = b"gov:proposal:";
        let mut ids = Vec::new();
        for entry in self.storage.db.iterator(rocksdb::IteratorMode::From(
            prefix,
            rocksdb::Direction::Forward,
        )) {
            let (key, _) = entry.map_err(|e| format!("Failed to enumerate proposals: {e}"))?;
            if !key.starts_with(prefix) {
                break;
            }
            let suffix = std::str::from_utf8(&key[prefix.len()..])
                .map_err(|_| "Stored proposal key is not UTF-8")?;
            let id = suffix
                .parse::<u64>()
                .map_err(|_| "Stored proposal key has invalid ID")?;
            ids.push(id);
        }
        ids.sort_unstable();
        let count = u64::try_from(ids.len()).map_err(|_| "Too many proposals")?;
        if count != last_id
            || ids.iter().enumerate().any(|(index, id)| {
                u64::try_from(index).ok().and_then(|n| n.checked_add(1)) != Some(*id)
            })
        {
            return Err("Proposal ID counter and stored proposals differ".to_string());
        }
        Ok(ids)
    }

    fn store_config(&self) -> Result<(), String> {
        let data = bincode::serialize(&self.config)
            .map_err(|e| format!("Failed to serialize config: {e}"))?;
        self.storage
            .db
            .put("gov:config", data)
            .map_err(|e| format!("Failed to store config: {e}"))?;
        Ok(())
    }

    fn emit_event(&mut self, event: GovernanceEvent) {
        // Keep in memory
        self.events.push(event.clone());
        // Append to evidence log (best-effort)
        let _ = Self::append_event_to_evidence_log(&event);
    }

    // Deposit storage and retrieval functions

    fn store_deposit(&self, deposit: &Deposit) -> Result<(), String> {
        let key = format!("gov:deposit:{}:{}", deposit.proposal_id, deposit.depositor);
        let data =
            bincode::serialize(deposit).map_err(|e| format!("Failed to serialize deposit: {e}"))?;
        self.storage
            .db
            .put(key, data)
            .map_err(|e| format!("Failed to store deposit: {e}"))?;
        Ok(())
    }

    fn get_proposal_deposits(&self, proposal_id: u64) -> Result<Vec<Deposit>, String> {
        let prefix = format!("gov:deposit:{proposal_id}:");
        let mut deposits = Vec::new();
        for entry in self.storage.db.iterator(rocksdb::IteratorMode::From(
            prefix.as_bytes(),
            rocksdb::Direction::Forward,
        )) {
            let (key, data) =
                entry.map_err(|e| format!("Failed to read proposal deposits: {e}"))?;
            if !key.starts_with(prefix.as_bytes()) {
                break;
            }
            let deposit: Deposit = bincode::deserialize(&data)
                .map_err(|e| format!("Failed to deserialize proposal deposit: {e}"))?;
            let expected_key = format!("{prefix}{}", deposit.depositor);
            if deposit.proposal_id != proposal_id || key.as_ref() != expected_key.as_bytes() {
                return Err("Stored deposit identity does not match its key".to_string());
            }
            deposits.push(deposit);
        }
        Ok(deposits)
    }

    /// Commit a terminal status and its complete deposit refund in one batch.
    fn refund_terminal_proposal(&mut self, proposal: &Proposal) -> Result<(), String> {
        if !matches!(
            proposal.status,
            ProposalStatus::Rejected | ProposalStatus::Failed
        ) {
            return Err("Deposit refund requires a terminal proposal".to_string());
        }
        let stored = self
            ._get_proposal(proposal.id)?
            .ok_or("Proposal disappeared before refund")?;
        if !matches!(
            stored.status,
            ProposalStatus::DepositPeriod | ProposalStatus::VotingPeriod
        ) {
            return Err("Proposal deposits are already settled".to_string());
        }
        let deposits = self.get_proposal_deposits(proposal.id)?;
        let mut total = 0u128;
        let mut state = self.state.lock().unwrap();
        let mut credited = Vec::with_capacity(deposits.len());
        let mut batch = rocksdb::WriteBatch::default();
        for deposit in &deposits {
            if deposit.denom != "udgt" || deposit.amount == 0 {
                return Err("Stored governance deposit is invalid".to_string());
            }
            total = total
                .checked_add(deposit.amount)
                .ok_or("Stored governance deposit total overflow")?;
            let mut account = state.get_account(&deposit.depositor);
            let balance = account
                .balance_of("udgt")
                .checked_add(deposit.amount)
                .ok_or("Governance deposit refund overflow")?;
            account.set_balance("udgt", balance);
            batch.put(
                format!("acct:balances:{}", deposit.depositor),
                bincode::serialize(&account.balances)
                    .map_err(|e| format!("Failed to serialize refund balance: {e}"))?,
            );
            credited.push((deposit.depositor.clone(), account));
        }
        if total != proposal.total_deposit || total != stored.total_deposit {
            return Err("Stored governance deposits do not match proposal total".to_string());
        }
        batch.put(
            format!("gov:proposal:{}", proposal.id),
            bincode::serialize(proposal)
                .map_err(|e| format!("Failed to serialize terminal proposal: {e}"))?,
        );
        self.storage
            .db
            .write(batch)
            .map_err(|e| format!("Failed to commit governance refund: {e}"))?;
        for (depositor, account) in credited {
            state.accounts.insert(depositor, account);
        }
        drop(state);
        for deposit in deposits {
            self.emit_event(GovernanceEvent::DepositRefunded {
                proposal_id: proposal.id,
                depositor: deposit.depositor,
                amount: deposit.amount,
            });
        }
        Ok(())
    }
}

impl GovernanceModule {
    fn evidence_dir() -> PathBuf {
        PathBuf::from("launch-evidence/governance")
    }

    fn ensure_evidence_dir() -> std::io::Result<()> {
        fs::create_dir_all(Self::evidence_dir())
    }

    fn append_event_to_evidence_log(event: &GovernanceEvent) -> std::io::Result<()> {
        Self::ensure_evidence_dir()?;
        let log_path = Self::evidence_dir().join("execution.log");
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let line = format!("{ts} {event:?}\n");
        use std::io::Write;
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)?;
        f.write_all(line.as_bytes())
    }

    fn write_json_file(path: PathBuf, value: &serde_json::Value) -> std::io::Result<()> {
        let s = serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string());
        fs::write(path, s)
    }

    fn write_governance_evidence(&self) -> std::io::Result<()> {
        Self::ensure_evidence_dir()?;

        // Proposals snapshot
        let mut proposals_json = serde_json::json!({"proposals": []});
        if let Ok(list) = self.get_all_proposals() {
            let mut arr = Vec::new();
            for p in list {
                let tally = self.tally(p.id).ok();
                arr.push(serde_json::json!({
                    "id": p.id,
                    "title": p.title,
                    "description": p.description,
                    "type": match &p.proposal_type { ProposalType::ParameterChange { key, value } => serde_json::json!({"parameter_change": {"key": key, "value": value}}) },
                    "status": format!("{:?}", p.status),
                    "total_deposit": p.total_deposit.to_string(),
                    "submit_height": p.submit_height,
                    "deposit_end_height": p.deposit_end_height,
                    "voting_start_height": p.voting_start_height,
                    "voting_end_height": p.voting_end_height,
                    "tally": tally.map(|t| serde_json::json!({
                        "yes": t.yes.to_string(),
                        "no": t.no.to_string(),
                        "no_with_veto": t.no_with_veto.to_string(),
                        "abstain": t.abstain.to_string(),
                        "total_voting_power": t.total_voting_power.to_string(),
                    })),
                }));
            }
            proposals_json["proposals"] = serde_json::Value::Array(arr);
        }
        let _ = Self::write_json_file(Self::evidence_dir().join("proposal.json"), &proposals_json);

        // Votes snapshot
        let mut votes_obj = serde_json::Map::new();
        if let Ok(ids) = self.get_all_proposal_ids() {
            for pid in ids {
                if let Ok(votes) = self.get_proposal_votes(pid) {
                    let mut vlist = Vec::new();
                    for v in votes {
                        vlist.push(serde_json::json!({
                            "voter": v.voter,
                            "option": format!("{:?}", v.option),
                            "weight": v.weight.to_string(),
                        }));
                    }
                    votes_obj.insert(pid.to_string(), serde_json::Value::Array(vlist));
                }
            }
        }
        let _ = Self::write_json_file(
            Self::evidence_dir().join("votes.json"),
            &serde_json::Value::Object(votes_obj),
        );

        // Final params snapshot
        let params_json = serde_json::json!({
            "gas_limit": self.config.gas_limit,
            "consensus.max_gas_per_block": self.config.max_gas_per_block,
            "quorum_bps": self.config.quorum,
            "threshold_bps": self.config.threshold,
            "veto_threshold_bps": self.config.veto_threshold,
        });
        let _ = Self::write_json_file(Self::evidence_dir().join("final_params.json"), &params_json);

        Ok(())
    }
}

/// Gas accounting constants for governance operations
pub const GAS_SUBMIT_PROPOSAL: u64 = 50_000;
pub const GAS_DEPOSIT: u64 = 30_000;
pub const GAS_VOTE: u64 = 20_000;
pub const GAS_TALLY: u64 = 10_000;

#[cfg(test)]
#[path = "../../tests/support/stake_fixture.rs"]
mod stake_fixture;

#[cfg(test)]
mod tests {
    use super::stake_fixture::StakeFixture;
    use super::*;
    use crate::storage::state::Storage;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    fn setup_test_governance() -> (GovernanceModule, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let storage = Arc::new(Storage::open(temp_dir.path().to_path_buf()).unwrap()); // use open instead of new
        let state = Arc::new(Mutex::new(State::new(storage.clone())));

        // Provide minimal staking module instance using real constructor (no Default implementation available)
        let staking_storage = storage.clone();
        let staking = Arc::new(Mutex::new(StakingModule::new(staking_storage)));

        let governance = GovernanceModule::new(storage, state, staking);
        (governance, temp_dir)
    }

    #[test]
    fn test_submit_proposal() {
        let (mut governance, _temp_dir) = setup_test_governance();

        let proposal_id = governance
            .submit_proposal(
                100,
                "Test Proposal".to_string(),
                "Test Description".to_string(),
                ProposalType::ParameterChange {
                    key: "gas_limit".to_string(),
                    value: "500000".to_string(),
                },
            )
            .unwrap();

        assert_eq!(proposal_id, 1);

        let proposal = governance.get_proposal(proposal_id).unwrap().unwrap();
        assert_eq!(proposal.title, "Test Proposal");
        assert_eq!(proposal.status, ProposalStatus::DepositPeriod);
        assert_eq!(proposal.submit_height, 100);
    }

    #[test]
    fn failed_proposal_creation_does_not_consume_id() {
        let (mut governance, _temp_dir) = setup_test_governance();
        let proposal_type = ProposalType::ParameterChange {
            key: "gas_limit".to_string(),
            value: "500000".to_string(),
        };
        assert!(governance
            .submit_proposal(
                u64::MAX,
                "bad".into(),
                "overflow".into(),
                proposal_type.clone()
            )
            .is_err());
        assert!(governance
            .storage
            .db
            .get("gov:last_proposal_id")
            .unwrap()
            .is_none());
        assert!(governance.get_proposal(1).unwrap().is_none());

        let id = governance
            .submit_proposal(100, "valid".into(), "after failure".into(), proposal_type)
            .unwrap();
        assert_eq!(id, 1);
        assert!(governance.get_proposal(id).unwrap().is_some());
    }

    #[test]
    fn malformed_or_missing_proposal_index_stops_end_block() {
        let (mut governance, _temp_dir) = setup_test_governance();
        governance
            .storage
            .db
            .put("gov:last_proposal_id", b"corrupt")
            .unwrap();
        assert!(governance.end_block(100).is_err());
        assert!(governance.get_all_proposals().is_err());

        governance
            .storage
            .db
            .put("gov:last_proposal_id", bincode::serialize(&1u64).unwrap())
            .unwrap();
        assert!(governance.end_block(100).is_err());
        assert!(governance.get_all_proposals().is_err());
    }

    #[test]
    fn test_deposit_transitions_to_voting() {
        let (mut governance, _temp_dir) = setup_test_governance();

        // Setup account with DGT balance
        {
            let mut state = governance.state.lock().unwrap();
            let mut account = state.get_account("depositor1");
            account.add_balance("udgt", 2_000_000_000); // 2000 DGT
            state.accounts.insert("depositor1".to_string(), account);
        }

        let proposal_id = governance
            .submit_proposal(
                100,
                "Test Proposal".to_string(),
                "Test Description".to_string(),
                ProposalType::ParameterChange {
                    key: "gas_limit".to_string(),
                    value: "500000".to_string(),
                },
            )
            .unwrap();

        // Deposit enough to meet minimum
        governance
            .deposit(150, "depositor1", proposal_id, 1_000_000_000, "udgt")
            .unwrap();

        let proposal = governance.get_proposal(proposal_id).unwrap().unwrap();
        assert_eq!(proposal.status, ProposalStatus::VotingPeriod);
        assert_eq!(proposal.total_deposit, 1_000_000_000);
        // Ensure voting window starts at deposit height when threshold is met early
        assert_eq!(proposal.voting_start_height, 150);
        assert_eq!(
            proposal.voting_end_height,
            150 + governance.config.voting_period
        );
    }

    #[test]
    fn repeated_deposits_keep_full_refund_record() {
        let (mut governance, _temp_dir) = setup_test_governance();
        {
            let mut state = governance.state.lock().unwrap();
            let mut account = state.get_account("depositor1");
            account.add_balance("udgt", 100);
            state.accounts.insert("depositor1".to_string(), account);
        }
        let id = governance
            .submit_proposal(
                100,
                "Repeated deposits".to_string(),
                "Check refund accounting".to_string(),
                ProposalType::ParameterChange {
                    key: "gas_limit".to_string(),
                    value: "500000".to_string(),
                },
            )
            .unwrap();
        governance
            .deposit(101, "depositor1", id, 20, "udgt")
            .unwrap();
        governance
            .deposit(102, "depositor1", id, 30, "udgt")
            .unwrap();

        assert_eq!(
            governance.get_proposal(id).unwrap().unwrap().total_deposit,
            50
        );
        let deposits = governance.get_proposal_deposits(id).unwrap();
        assert_eq!(deposits.len(), 1);
        assert_eq!(deposits[0].amount, 50);
        assert_eq!(
            governance
                .state
                .lock()
                .unwrap()
                .get_account("depositor1")
                .balance_of("udgt"),
            50
        );
        assert_eq!(
            governance.storage.get_balances_db("depositor1").get("udgt"),
            Some(&50)
        );
    }

    #[test]
    fn deposit_overflow_keeps_account_and_record_unchanged() {
        let (mut governance, _temp_dir) = setup_test_governance();
        {
            let mut state = governance.state.lock().unwrap();
            let mut account = state.get_account("depositor1");
            account.add_balance("udgt", 10);
            state.accounts.insert("depositor1".to_string(), account);
        }
        let id = governance
            .submit_proposal(
                100,
                "Overflow".to_string(),
                "Check deposit arithmetic".to_string(),
                ProposalType::ParameterChange {
                    key: "gas_limit".to_string(),
                    value: "500000".to_string(),
                },
            )
            .unwrap();
        let mut proposal = governance.get_proposal(id).unwrap().unwrap();
        proposal.total_deposit = u128::MAX;
        governance.store_proposal(&proposal).unwrap();
        assert!(governance
            .deposit(101, "depositor1", id, 1, "udgt")
            .is_err());
        assert_eq!(
            governance
                .state
                .lock()
                .unwrap()
                .get_account("depositor1")
                .balance_of("udgt"),
            10
        );
        assert!(governance.get_proposal_deposits(id).unwrap().is_empty());
    }

    fn passed_deposited_proposal(governance: &mut GovernanceModule, key: &str, value: &str) -> u64 {
        {
            let mut state = governance.state.lock().unwrap();
            let mut account = state.get_account("depositor1");
            account.add_balance("udgt", 2_000_000_000);
            state.accounts.insert("depositor1".to_string(), account);
        }
        let id = governance
            .submit_proposal(
                100,
                "Execution".into(),
                "Check atomic refund".into(),
                ProposalType::ParameterChange {
                    key: key.into(),
                    value: value.into(),
                },
            )
            .unwrap();
        governance
            .deposit(101, "depositor1", id, 1_000_000_000, "udgt")
            .unwrap();
        let mut proposal = governance.get_proposal(id).unwrap().unwrap();
        proposal.status = ProposalStatus::Passed;
        governance.store_proposal(&proposal).unwrap();
        id
    }

    #[test]
    fn execution_commits_parameter_status_and_refund_once() {
        let (mut governance, _temp_dir) = setup_test_governance();
        let id = passed_deposited_proposal(&mut governance, "gas_limit", "500000");
        governance.execute(id).unwrap();
        assert_eq!(governance.config.gas_limit, 500000);
        assert_eq!(
            governance.get_proposal(id).unwrap().unwrap().status,
            ProposalStatus::Executed
        );
        assert_eq!(
            governance.storage.get_balances_db("depositor1").get("udgt"),
            Some(&2_000_000_000)
        );
        governance.state.lock().unwrap().accounts.clear();
        assert_eq!(
            governance
                .state
                .lock()
                .unwrap()
                .get_account("depositor1")
                .balance_of("udgt"),
            2_000_000_000
        );
        assert!(governance.execute(id).is_err());
        assert_eq!(
            governance.storage.get_balances_db("depositor1").get("udgt"),
            Some(&2_000_000_000)
        );
    }

    #[test]
    fn failed_execution_commits_refund_and_failure_status() {
        let (mut governance, _temp_dir) = setup_test_governance();
        let initial = governance.config.gas_limit;
        let id = passed_deposited_proposal(&mut governance, "gas_limit", "1");
        assert!(governance.execute(id).is_err());
        assert_eq!(governance.config.gas_limit, initial);
        assert_eq!(
            governance.get_proposal(id).unwrap().unwrap().status,
            ProposalStatus::FailedExecution
        );
        assert_eq!(
            governance.storage.get_balances_db("depositor1").get("udgt"),
            Some(&2_000_000_000)
        );
    }

    #[test]
    fn corrupt_deposit_stops_execution_before_mutation() {
        let (mut governance, _temp_dir) = setup_test_governance();
        let initial = governance.config.gas_limit;
        let id = passed_deposited_proposal(&mut governance, "gas_limit", "500000");
        governance
            .storage
            .db
            .put(format!("gov:deposit:{id}:depositor1"), b"corrupt")
            .unwrap();
        assert!(governance.execute(id).is_err());
        assert_eq!(governance.config.gas_limit, initial);
        assert_eq!(
            governance.get_proposal(id).unwrap().unwrap().status,
            ProposalStatus::Passed
        );
        assert!(governance.end_block(500).is_err());
        assert_eq!(
            governance.storage.get_balances_db("depositor1").get("udgt"),
            Some(&1_000_000_000)
        );
    }

    #[test]
    fn stored_deposits_do_not_depend_on_account_cache() {
        let (governance, _temp_dir) = setup_test_governance();
        let deposit = Deposit {
            proposal_id: 1,
            depositor: "owner".to_string(),
            amount: 17,
            denom: "udgt".to_string(),
        };
        governance.store_deposit(&deposit).unwrap();
        governance
            .store_deposit(&Deposit {
                proposal_id: 10,
                depositor: "other".to_string(),
                amount: 23,
                denom: "udgt".to_string(),
            })
            .unwrap();
        governance.state.lock().unwrap().accounts.clear();

        let found = governance.get_proposal_deposits(1).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].depositor, "owner");
        assert_eq!(found[0].amount, 17);

        governance
            .storage
            .db
            .put("gov:deposit:1:corrupt", b"not-a-deposit")
            .unwrap();
        assert!(governance.get_proposal_deposits(1).is_err());
    }

    #[test]
    fn test_vote_with_dgt_weight() {
        let (mut governance, _temp_dir) = setup_test_governance();

        // Setup account with DGT balance
        {
            let mut state = governance.state.lock().unwrap();
            let mut account = state.get_account("voter1");
            account.add_balance("udgt", 500_000_000); // 500 DGT
            state.accounts.insert("voter1".to_string(), account);
        }

        {
            let mut staking = governance.staking.lock().unwrap();
            staking.seed_delegator_stake("voter1", 500_000_000);
            staking.seed_total_stake(500_000_000);
        }

        let proposal_id = governance
            .submit_proposal(
                100,
                "Test Proposal".to_string(),
                "Test Description".to_string(),
                ProposalType::ParameterChange {
                    key: "gas_limit".to_string(),
                    value: "500000".to_string(),
                },
            )
            .unwrap();

        // Manually transition to voting period for test and set a valid window
        {
            let mut proposal = governance.get_proposal(proposal_id).unwrap().unwrap();
            proposal.status = ProposalStatus::VotingPeriod;
            proposal.voting_start_height = 150;
            proposal.voting_end_height = 150 + governance.config.voting_period;
            governance.store_proposal(&proposal).unwrap();
        }

        governance
            .vote(200, "voter1", proposal_id, VoteOption::Yes)
            .unwrap();

        let tally = governance.tally(proposal_id).unwrap();
        assert_eq!(tally.yes, 500_000_000);
        assert_eq!(tally.no, 0);
        assert_eq!(tally.no_with_veto, 0);
        assert_eq!(tally.abstain, 0);
    }

    #[test]
    fn bonded_voting_power_excludes_liquid_and_counts_stake_once() {
        let (governance, _temp_dir) = setup_test_governance();
        {
            let mut staking = governance.staking.lock().unwrap();
            staking.seed_delegator_stake("voter", 29);
            staking.seed_total_stake(29);
        }
        for liquid in [0, 1_000_000, u128::MAX] {
            let mut state = governance.state.lock().unwrap();
            let mut account = state.get_account("voter");
            account.set_balance("udgt", liquid);
            state.accounts.insert("voter".into(), account);
            drop(state);
            assert_eq!(governance.voting_power("voter").unwrap(), 29);
            assert_eq!(governance.total_voting_power().unwrap(), 29);
        }
        {
            let mut staking = governance.staking.lock().unwrap();
            staking.seed_delegator_stake("voter", 0);
            staking.seed_total_stake(0);
        }
        assert_eq!(governance.voting_power("voter").unwrap(), 0);
        assert_eq!(governance.total_voting_power().unwrap(), 0);
        assert_eq!(governance.voting_power("absent").unwrap(), 0);
        assert!(!governance
            .state
            .lock()
            .unwrap()
            .accounts
            .contains_key("absent"));
    }

    #[test]
    fn governance_threshold_arithmetic_handles_full_u128_range() {
        assert_eq!(basis_points_ceil(u128::MAX, 10_000).unwrap(), u128::MAX);
        assert_eq!(
            basis_points_ceil(u128::MAX, 5_000).unwrap(),
            u128::MAX / 2 + 1
        );
        assert_eq!(basis_points_ceil(3, 6700).unwrap(), 3);
        assert!(basis_points_ceil(1, 10_001).is_err());

        let (governance, _temp_dir) = setup_test_governance();
        governance
            .staking
            .lock()
            .unwrap()
            .seed_total_stake(u128::MAX);
        let tally = TallyResult {
            yes: u128::MAX,
            no: 0,
            no_with_veto: 0,
            abstain: 0,
            total_voting_power: u128::MAX,
        };
        assert!(governance.proposal_passes(&tally).unwrap());
        let invalid = TallyResult { no: 1, ..tally };
        assert!(governance.proposal_passes(&invalid).is_err());
    }

    #[test]
    fn exact_quorum_and_veto_exclude_abstentions_from_veto_denominator() {
        let (governance, _temp_dir) = setup_test_governance();
        governance.staking.lock().unwrap().seed_total_stake(3);
        let below_quorum = TallyResult {
            yes: 2,
            no: 0,
            no_with_veto: 0,
            abstain: 0,
            total_voting_power: 2,
        };
        assert!(!governance.proposal_passes(&below_quorum).unwrap());

        governance.staking.lock().unwrap().seed_total_stake(111);
        let veto_with_abstentions = TallyResult {
            yes: 6,
            no: 0,
            no_with_veto: 5,
            abstain: 100,
            total_voting_power: 111,
        };
        assert!(!governance.proposal_passes(&veto_with_abstentions).unwrap());
    }

    #[test]
    fn failed_and_rejected_proposals_refund_deposits_once() {
        for rejected in [false, true] {
            let (mut governance, _temp_dir) = setup_test_governance();
            governance
                .state
                .lock()
                .unwrap()
                .set_balance("depositor", "udgt", 2_000_000_000);
            let id = governance
                .submit_proposal(
                    100,
                    "Refund".into(),
                    "Terminal refund".into(),
                    ProposalType::ParameterChange {
                        key: "gas_limit".into(),
                        value: "500000".into(),
                    },
                )
                .unwrap();
            let amount = if rejected { 1_000_000_000 } else { 100 };
            governance
                .deposit(101, "depositor", id, amount, "udgt")
                .unwrap();
            governance
                .end_block(if rejected { 402 } else { 401 })
                .unwrap();
            assert_eq!(
                governance.get_proposal(id).unwrap().unwrap().status,
                if rejected {
                    ProposalStatus::Rejected
                } else {
                    ProposalStatus::Failed
                }
            );
            assert_eq!(
                governance.storage.get_balances_db("depositor").get("udgt"),
                Some(&2_000_000_000)
            );
            governance.end_block(500).unwrap();
            assert_eq!(
                governance.storage.get_balances_db("depositor").get("udgt"),
                Some(&2_000_000_000)
            );
        }
    }

    #[test]
    fn test_no_with_veto_vote() {
        let (mut governance, _temp_dir) = setup_test_governance();

        // Setup account with DGT balance
        {
            let mut state = governance.state.lock().unwrap();
            let mut account = state.get_account("voter1");
            account.add_balance("udgt", 500_000_000); // 500 DGT
            state.accounts.insert("voter1".to_string(), account);
        }

        {
            let mut staking = governance.staking.lock().unwrap();
            staking.seed_delegator_stake("voter1", 500_000_000);
            staking.seed_total_stake(500_000_000);
        }

        let proposal_id = governance
            .submit_proposal(
                100,
                "Test Proposal".to_string(),
                "Test Description".to_string(),
                ProposalType::ParameterChange {
                    key: "consensus.max_gas_per_block".to_string(),
                    value: "15000000".to_string(),
                },
            )
            .unwrap();

        // Manually transition to voting period for test and set a valid window
        {
            let mut proposal = governance.get_proposal(proposal_id).unwrap().unwrap();
            proposal.status = ProposalStatus::VotingPeriod;
            proposal.voting_start_height = 150;
            proposal.voting_end_height = 150 + governance.config.voting_period;
            governance.store_proposal(&proposal).unwrap();
        }

        governance
            .vote(200, "voter1", proposal_id, VoteOption::NoWithVeto)
            .unwrap();

        let tally = governance.tally(proposal_id).unwrap();
        assert_eq!(tally.yes, 0);
        assert_eq!(tally.no, 0);
        assert_eq!(tally.no_with_veto, 500_000_000);
        assert_eq!(tally.abstain, 0);
    }

    #[test]
    fn test_parameter_change_execution() {
        let (mut governance, _temp_dir) = setup_test_governance();

        // Test gas_limit parameter change
        governance
            .apply_parameter_change("gas_limit", "100000")
            .unwrap();
        assert_eq!(governance.config.gas_limit, 100000);

        // Test consensus.max_gas_per_block parameter change
        governance
            .apply_parameter_change("consensus.max_gas_per_block", "20000000")
            .unwrap();
        assert_eq!(governance.config.max_gas_per_block, 20000000);

        // Historical rate remains readable, but governance cannot change it.
        let original_rate = governance.staking.lock().unwrap().get_reward_rate_bps();
        let before: Vec<_> = governance
            .storage
            .db
            .iterator(rocksdb::IteratorMode::Start)
            .map(|entry| entry.unwrap())
            .collect();
        let error = governance
            .apply_parameter_change("staking_reward_rate", "0.10")
            .unwrap_err();
        assert!(error.contains("retired"));
        assert_eq!(
            governance.staking.lock().unwrap().get_reward_rate_bps(),
            original_rate
        );
        let after: Vec<_> = governance
            .storage
            .db
            .iterator(rocksdb::IteratorMode::Start)
            .map(|entry| entry.unwrap())
            .collect();
        assert_eq!(after, before);

        // Test invalid parameter
        assert!(governance
            .apply_parameter_change("invalid_param", "123")
            .is_err());
    }

    #[test]
    fn production_open_restores_committed_governance_config() {
        let (mut governance, _temp_dir) = setup_test_governance();
        governance
            .apply_parameter_change("gas_limit", "500000")
            .unwrap();
        let restored = GovernanceModule::open(
            governance.storage.clone(),
            governance.state.clone(),
            governance.staking.clone(),
            false,
        )
        .unwrap();
        assert_eq!(restored.config.gas_limit, 500000);
        assert_eq!(restored.config.quorum, governance.config.quorum);
    }

    #[test]
    fn production_open_initializes_empty_governance_store_once() {
        let (governance, _temp_dir) = setup_test_governance();
        let opened = GovernanceModule::open(
            governance.storage.clone(),
            governance.state.clone(),
            governance.staking.clone(),
            true,
        )
        .unwrap();
        let saved = governance.storage.db.get("gov:config").unwrap().unwrap();
        let decoded: GovernanceConfig = bincode::deserialize(&saved).unwrap();
        assert_eq!(decoded.min_deposit, opened.config.min_deposit);
        assert_eq!(decoded.quorum, opened.config.quorum);
    }

    #[test]
    fn production_open_rejects_corrupt_or_unmigrated_governance_state() {
        let (mut governance, _temp_dir) = setup_test_governance();
        governance.storage.db.put("gov:config", b"corrupt").unwrap();
        assert!(GovernanceModule::open(
            governance.storage.clone(),
            governance.state.clone(),
            governance.staking.clone(),
            true,
        )
        .is_err());
        governance.storage.db.delete("gov:config").unwrap();
        governance
            .submit_proposal(
                10,
                "Existing".into(),
                "Needs explicit migration".into(),
                ProposalType::ParameterChange {
                    key: "gas_limit".into(),
                    value: "500000".into(),
                },
            )
            .unwrap();
        assert!(GovernanceModule::open(
            governance.storage.clone(),
            governance.state.clone(),
            governance.staking.clone(),
            true,
        )
        .is_err());
    }
}
