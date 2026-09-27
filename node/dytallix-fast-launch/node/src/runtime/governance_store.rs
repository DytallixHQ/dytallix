//! Governance state stored per entry (T6, `docs/architecture/governance-v1.md`).
//!
//! A small header, one record per proposal, one vote entry per voter and
//! ballot, bond-weight entries per snapshot height, and a due index by height.
//! Automatic transitions run at block start in ascending proposal ID; the
//! caller settles their DGT refunds and action effects. A finished proposal is
//! removed at the next block start, with its votes and unused snapshot, so
//! retained state covers open proposals only. Running totals keep proposal IDs
//! unique and the held deposit total exact.
use super::governance_candidate::{BallotRules, DepositRules};
use super::validator_lifecycle::LifecycleState;
use crate::recovery_fees::RecoveryBook;
use crate::storage::state::Storage;
use anyhow::{ensure, Context, Result};
use bincode::Options;
use rocksdb::{Direction, IteratorMode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub const VERSION: u16 = 2;
pub const PREFIX: &str = "governance:v2:";
pub const HEADER_KEY: &str = "governance:v2:header";
const PROPOSAL: &str = "governance:v2:proposal:";
const VOTE: &str = "governance:v2:vote:";
const WEIGHT: &str = "governance:v2:weight:";
const SNAPSHOT: &str = "governance:v2:snapshot:";
const DUE: &str = "governance:v2:due:";
pub const PARAMETERS_KEY: &str = "governance:v2:parameters";
const MAX_RECORD_BYTES: u64 = 8 * 1024 * 1024;
const DGT_SUPPLY: u128 = dytallix_protocol_types::units::DGT_TOTAL_BASE_UNITS;
pub type Owner = [u8; 32];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoteChoice {
    Yes,
    No,
    NoWithVeto,
    Abstain,
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let bytes = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .serialize(value)?;
    ensure!(
        bytes.len() as u64 <= MAX_RECORD_BYTES,
        "Governance record exceeds size limit"
    );
    Ok(bytes)
}
fn decode<T: Serialize + DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let value: T = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(MAX_RECORD_BYTES)
        .reject_trailing_bytes()
        .deserialize(bytes)?;
    ensure!(encode(&value)? == bytes, "Noncanonical governance record");
    Ok(value)
}
fn owner_hex(owner: &Owner) -> String {
    hex::encode(owner)
}
pub(crate) fn proposal_key(id: u64) -> String {
    format!("{PROPOSAL}{id:020}")
}
fn vote_prefix(id: u64) -> String {
    format!("{VOTE}{id:020}:")
}
pub(crate) fn vote_key(id: u64, owner: &Owner) -> String {
    format!("{}{}", vote_prefix(id), owner_hex(owner))
}
fn weight_prefix(height: u64) -> String {
    format!("{WEIGHT}{height:020}:")
}
pub(crate) fn weight_key(height: u64, owner: &Owner) -> String {
    format!("{}{}", weight_prefix(height), owner_hex(owner))
}
fn snapshot_key(height: u64) -> String {
    format!("{SNAPSHOT}{height:020}")
}
fn due_prefix(height: u64) -> String {
    format!("{DUE}{height:020}:")
}
fn due_key(height: u64, id: u64) -> String {
    format!("{}{id:020}", due_prefix(height))
}
fn owner_suffix(key: &[u8], prefix: &str) -> Result<Owner> {
    let text = std::str::from_utf8(&key[prefix.len()..]).context("Governance key is not UTF-8")?;
    ensure!(text.len() == 64, "Governance owner key length differs");
    hex::decode(text)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Governance owner key length differs"))
}
fn id_suffix(text: &str) -> Result<u64> {
    ensure!(
        text.len() == 20 && text.bytes().all(|b| b.is_ascii_digit()),
        "Governance numeric key differs"
    );
    Ok(text.parse()?)
}
/// Committed keys under `prefix`, in key order.
fn scan(storage: &Storage, prefix: &str) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let mut found = Vec::new();
    for item in storage
        .db
        .iterator(IteratorMode::From(prefix.as_bytes(), Direction::Forward))
    {
        let (key, value) = item?;
        if !key.starts_with(prefix.as_bytes()) {
            break;
        }
        found.push((key.to_vec(), value.to_vec()));
    }
    Ok(found)
}

/// Integer basis-point threshold, rounded up (approved D11-Q02 arithmetic).
pub fn required_weight(total: u128, bps: u16) -> Result<u128> {
    ensure!(bps <= 10_000, "Governance basis points exceed 10000");
    let bps = u128::from(bps);
    let whole = (total / 10_000)
        .checked_mul(bps)
        .context("Governance required weight overflow")?;
    let remainder = (total % 10_000) * bps;
    whole
        .checked_add(remainder / 10_000)
        .and_then(|value| value.checked_add(u128::from(remainder % 10_000 != 0)))
        .context("Governance required weight overflow")
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tally {
    pub yes: u128,
    pub no: u128,
    pub no_with_veto: u128,
    pub abstain: u128,
    pub votes: u32,
}
impl Tally {
    fn add(&mut self, choice: VoteChoice, weight: u128) -> Result<()> {
        let bucket = match choice {
            VoteChoice::Yes => &mut self.yes,
            VoteChoice::No => &mut self.no,
            VoteChoice::NoWithVeto => &mut self.no_with_veto,
            VoteChoice::Abstain => &mut self.abstain,
        };
        *bucket = bucket
            .checked_add(weight)
            .context("Governance tally overflow")?;
        self.votes = self
            .votes
            .checked_add(1)
            .context("Governance vote count overflow")?;
        Ok(())
    }
    fn turnout(&self) -> Result<u128> {
        self.decisive()?
            .checked_add(self.abstain)
            .context("Governance turnout overflow")
    }
    fn decisive(&self) -> Result<u128> {
        self.yes
            .checked_add(self.no)
            .and_then(|sum| sum.checked_add(self.no_with_veto))
            .context("Governance decisive weight overflow")
    }
    /// Quorum counts abstentions; approval and veto use decisive weight only.
    pub fn passes(&self, total_weight: u128, rules: &BallotRules) -> Result<bool> {
        let decisive = self.decisive()?;
        Ok(
            self.turnout()? >= required_weight(total_weight, rules.quorum_bps)?
                && decisive > 0
                && self.no_with_veto < required_weight(decisive, rules.veto_bps)?
                && self.yes >= required_weight(decisive, rules.approval_bps)?,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    BelowMinimum,
    Rejected,
    Executed,
    FailedExecution(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// Deposits are accepted in `[admitted_height, close_height)`.
    Collecting {
        close_height: u64,
    },
    /// Votes are accepted in `[snapshot_height + 1, end_height]`.
    Voting {
        snapshot_height: u64,
        end_height: u64,
        tally: Tally,
    },
    Passed {
        execute_at: u64,
        tally: Tally,
    },
    /// Refunded at `height` and removed at the next block start.
    Finished {
        outcome: Outcome,
        height: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub id: u64,
    pub proposer: Owner,
    pub action_class: u16,
    pub action_data: Vec<u8>,
    pub action_digest: [u8; 32],
    pub admitted_height: u64,
    pub deposits: BTreeMap<Owner, u128>,
    pub deposited: u128,
    pub phase: Phase,
}
impl Proposal {
    /// Deposits still held in escrow.
    pub fn held(&self) -> u128 {
        match self.phase {
            Phase::Finished { .. } => 0,
            _ => self.deposited,
        }
    }
    /// The one height at which this proposal next changes automatically.
    pub fn due_height(&self) -> Result<u64> {
        match &self.phase {
            Phase::Collecting { close_height } => Ok(*close_height),
            Phase::Voting { end_height, .. } => end_height
                .checked_add(1)
                .context("Governance close height overflow"),
            Phase::Passed { execute_at, .. } => Ok(*execute_at),
            Phase::Finished { height, .. } => height
                .checked_add(1)
                .context("Governance removal height overflow"),
        }
    }
    fn validate(&self, deposit: &DepositRules, ballot: &BallotRules) -> Result<()> {
        ensure!(
            self.id > 0 && self.admitted_height > 0,
            "Invalid governance proposal identity"
        );
        ensure!(
            self.action_data.len() <= deposit.max_action_bytes as usize
                && dytallix_protocol_types::ordinary_v3::governance_action_digest(
                    self.action_class,
                    &self.action_data
                )? == self.action_digest,
            "Governance proposal action differs from its digest or bound"
        );
        ensure!(
            self.deposits.len() <= deposit.max_depositors as usize
                && self.deposits.values().all(|amount| *amount > 0),
            "Governance deposits outside bounds"
        );
        let sum = self
            .deposits
            .values()
            .try_fold(0u128, |sum, amount| sum.checked_add(*amount))
            .context("Governance deposit sum overflow")?;
        ensure!(
            sum == self.deposited && sum <= DGT_SUPPLY,
            "Governance deposit total differs"
        );
        let close = self
            .admitted_height
            .checked_add(deposit.deposit_period_blocks)
            .context("Governance deposit close overflow")?;
        match &self.phase {
            Phase::Collecting { close_height } => {
                ensure!(*close_height == close, "Governance deposit close differs")
            }
            Phase::Voting {
                snapshot_height,
                end_height,
                tally,
            } => {
                ensure!(
                    snapshot_height.checked_add(1) == Some(close)
                        && close.checked_add(ballot.voting_period_blocks) == Some(*end_height)
                        && self.deposited >= deposit.minimum_deposit_udgt,
                    "Governance voting window differs"
                );
                tally.turnout()?;
            }
            Phase::Passed { execute_at, tally } => {
                let closed = close
                    .checked_add(ballot.voting_period_blocks)
                    .and_then(|h| h.checked_add(1))
                    .context("Governance close height overflow")?;
                ensure!(
                    closed.checked_add(ballot.timelock_blocks) == Some(*execute_at)
                        && self.deposited >= deposit.minimum_deposit_udgt,
                    "Governance timelock differs"
                );
                tally.turnout()?;
            }
            Phase::Finished { height, .. } => {
                ensure!(
                    *height >= close,
                    "Governance proposal finished before its close"
                )
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub version: u16,
    pub chain_id: String,
    pub genesis_digest: [u8; 32],
    pub height: u64,
    pub next_proposal_id: u64,
    /// Stored proposals, including finished ones awaiting removal.
    pub proposals: u64,
    /// Deposits held in escrow by unfinished proposals.
    pub held_udgt: u128,
}
impl Header {
    pub fn genesis(chain_id: String, genesis_digest: [u8; 32]) -> Result<Self> {
        let header = Self {
            version: VERSION,
            chain_id,
            genesis_digest,
            height: 0,
            next_proposal_id: 1,
            proposals: 0,
            held_udgt: 0,
        };
        header.validate()?;
        Ok(header)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == VERSION,
            "Unsupported governance state version"
        );
        ensure!(
            !self.chain_id.is_empty()
                && self.chain_id.len() <= 128
                && !self.chain_id.chars().any(char::is_control),
            "Invalid governance state chain ID"
        );
        ensure!(
            self.genesis_digest != [0; 32],
            "Governance state genesis digest is absent"
        );
        ensure!(
            self.next_proposal_id > 0 && self.proposals < self.next_proposal_id,
            "Governance proposal counters differ"
        );
        ensure!(
            self.held_udgt <= DGT_SUPPLY,
            "Governance held deposits exceed DGT supply"
        );
        Ok(())
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        encode(self)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let header: Self = decode(bytes)?;
        header.validate()?;
        Ok(header)
    }
    pub fn read(storage: &Storage) -> Result<Option<Self>> {
        storage
            .db
            .get(HEADER_KEY)?
            .map(|bytes| Self::decode(&bytes))
            .transpose()
    }
}

/// Current values of the parameters governance changed, replacing their
/// genesis values (P01, 27 September 2026). Absent fields keep genesis.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedParameters {
    pub ordinary_fee: Option<dytallix_protocol_types::ordinary_fees::FeeProfile>,
    pub governance_fee: Option<dytallix_protocol_types::ordinary_fees_v3::FeeProfileV3>,
    pub min_self_bond: Option<u128>,
    pub max_active: Option<u64>,
    pub approved_operators: Option<BTreeMap<String, String>>,
}
impl GovernedParameters {
    pub fn read(storage: &Storage) -> Result<Self> {
        storage
            .db
            .get(PARAMETERS_KEY)?
            .map(|bytes| decode(&bytes))
            .transpose()
            .map(Option::unwrap_or_default)
    }
}

/// Bond weights fixed at one finalized height, shared by every ballot that
/// starts in the following block. Removed when its last ballot closes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotMeta {
    pub total_weight: u128,
    pub owners: u32,
    pub ballots: u32,
}

/// A due automatic transition. The caller settles every monetary and action
/// effect before calling `finish`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Due {
    /// Refund `deposits` (below minimum at close, or rejected ballot).
    Refund {
        id: u64,
        deposits: BTreeMap<Owner, u128>,
        outcome: Outcome,
    },
    /// Run the approved action, then refund with its outcome.
    Execute { proposal: Arc<Proposal> },
}

/// The configured rules a store applies. Values are E05 inputs.
#[derive(Clone, Debug)]
pub struct StoreRules {
    pub deposit: DepositRules,
    pub ballot: BallotRules,
}

/// Staged changes over the committed governance entries.
#[derive(Clone)]
pub(crate) struct GovernanceStore {
    storage: Arc<Storage>,
    rules: Arc<StoreRules>,
    base: Header,
    header: Header,
    proposals: BTreeMap<u64, Option<Arc<Proposal>>>,
    votes: BTreeMap<(u64, Owner), VoteChoice>,
    cleared_votes: BTreeSet<u64>,
    snapshot: Option<(u64, Arc<BTreeMap<Owner, u128>>)>,
    snapshots: BTreeMap<u64, Option<SnapshotMeta>>,
    due: BTreeMap<(u64, u64), bool>,
    base_parameters: GovernedParameters,
    parameters: GovernedParameters,
    /// Effective bonds at the finalized parent, by native address.
    parent: Option<Arc<LifecycleState>>,
}
impl GovernanceStore {
    /// Open the committed state. Automatic transitions are not applied.
    pub(crate) fn open(storage: Arc<Storage>, rules: StoreRules) -> Result<Self> {
        let header = Header::read(&storage)?.context("Governance state missing")?;
        let parameters = GovernedParameters::read(&storage)?;
        Ok(Self {
            storage,
            rules: Arc::new(rules),
            base: header.clone(),
            header,
            proposals: BTreeMap::new(),
            votes: BTreeMap::new(),
            cleared_votes: BTreeSet::new(),
            snapshot: None,
            snapshots: BTreeMap::new(),
            due: BTreeMap::new(),
            base_parameters: parameters.clone(),
            parameters,
            parent: None,
        })
    }
    pub(crate) fn header(&self) -> &Header {
        &self.header
    }
    pub(crate) fn parameters(&self) -> &GovernedParameters {
        &self.parameters
    }
    pub(crate) fn set_parameters(&mut self, parameters: GovernedParameters) {
        self.parameters = parameters;
    }
    pub(crate) fn proposal(&self, id: u64) -> Result<Option<Arc<Proposal>>> {
        if let Some(staged) = self.proposals.get(&id) {
            return Ok(staged.clone());
        }
        let Some(bytes) = self.storage.db.get(proposal_key(id))? else {
            return Ok(None);
        };
        let proposal: Proposal = decode(&bytes)?;
        ensure!(proposal.id == id, "Governance proposal key differs");
        proposal.validate(&self.rules.deposit, &self.rules.ballot)?;
        Ok(Some(Arc::new(proposal)))
    }
    pub(crate) fn vote(&self, id: u64, owner: &Owner) -> Result<Option<VoteChoice>> {
        if let Some(choice) = self.votes.get(&(id, *owner)) {
            return Ok(Some(*choice));
        }
        if self.cleared_votes.contains(&id) {
            return Ok(None);
        }
        self.storage
            .db
            .get(vote_key(id, owner))?
            .map(|bytes| decode(&bytes))
            .transpose()
    }
    pub(crate) fn weight(&self, snapshot_height: u64, owner: &Owner) -> Result<Option<u128>> {
        if let Some((height, weights)) = &self.snapshot {
            if *height == snapshot_height {
                return Ok(weights.get(owner).copied());
            }
        }
        if matches!(self.snapshots.get(&snapshot_height), Some(None)) {
            return Ok(None);
        }
        self.storage
            .db
            .get(weight_key(snapshot_height, owner))?
            .map(|bytes| decode(&bytes))
            .transpose()
    }
    pub(crate) fn snapshot_meta(&self, height: u64) -> Result<Option<SnapshotMeta>> {
        if let Some(staged) = self.snapshots.get(&height) {
            return Ok(staged.clone());
        }
        self.storage
            .db
            .get(snapshot_key(height))?
            .map(|bytes| decode(&bytes))
            .transpose()
    }
    /// The proposer's effective bond at the finalized parent.
    pub(crate) fn parent_bond(&self, address: &str) -> Result<u128> {
        let parent = self
            .parent
            .as_ref()
            .context("Governance finalized parent is not attached")?;
        parent
            .effective
            .positions
            .get(address)
            .map_or(Ok(0), |positions| {
                positions
                    .values()
                    .try_fold(0u128, |sum, amount| sum.checked_add(*amount))
                    .context("Governance bond sum overflow")
            })
    }
    fn put_proposal(&mut self, proposal: Proposal) -> Result<()> {
        proposal.validate(&self.rules.deposit, &self.rules.ballot)?;
        self.proposals.insert(proposal.id, Some(Arc::new(proposal)));
        Ok(())
    }
    fn set_due(&mut self, height: u64, id: u64, present: bool) {
        self.due.insert((height, id), present);
    }
    fn due_at(&self, height: u64) -> Result<Vec<u64>> {
        let prefix = due_prefix(height);
        let mut ids = BTreeSet::new();
        for (key, _) in scan(&self.storage, &prefix)? {
            let text = std::str::from_utf8(&key[prefix.len()..])?;
            ids.insert(id_suffix(text)?);
        }
        for ((due_height, id), present) in &self.due {
            if *due_height == height {
                if *present {
                    ids.insert(*id);
                } else {
                    ids.remove(id);
                }
            }
        }
        Ok(ids.into_iter().collect())
    }

    /// Start block `height`: attach the finalized parent and return the due
    /// transitions in ascending proposal ID. Removals, deposit closes that
    /// start a ballot, and ballot closes that pass are applied here; refunds
    /// and actions are returned for the caller to settle and `finish`.
    pub(crate) fn begin_block(
        &mut self,
        height: u64,
        parent: Arc<LifecycleState>,
        book: &RecoveryBook,
    ) -> Result<Vec<Due>> {
        ensure!(
            self.header.height.checked_add(1) == Some(height)
                && parent.last_height == self.header.height,
            "Governance block height differs from its finalized parent"
        );
        // Every earlier due transition was applied: the first due key is at
        // or after this height.
        if let Some((key, _)) = self
            .storage
            .db
            .iterator(IteratorMode::From(DUE.as_bytes(), Direction::Forward))
            .next()
            .transpose()?
        {
            if key.starts_with(DUE.as_bytes()) {
                let text = std::str::from_utf8(&key[DUE.len()..DUE.len() + 20])?;
                ensure!(
                    id_suffix(text)? >= height,
                    "Governance transition was skipped"
                );
            }
        }
        self.parent = Some(parent);
        self.header.height = height;
        let mut settle = Vec::new();
        for id in self.due_at(height)? {
            self.set_due(height, id, false);
            let proposal = self
                .proposal(id)?
                .context("Due governance proposal is absent")?;
            ensure!(
                proposal.due_height()? == height,
                "Governance due index differs"
            );
            match &proposal.phase {
                Phase::Finished { .. } => {
                    self.proposals.insert(id, None);
                    self.header.proposals -= 1;
                }
                Phase::Collecting { .. } => {
                    if proposal.deposited < self.rules.deposit.minimum_deposit_udgt {
                        settle.push(Due::Refund {
                            id,
                            deposits: proposal.deposits.clone(),
                            outcome: Outcome::BelowMinimum,
                        });
                        continue;
                    }
                    let snapshot_height = height - 1;
                    self.start_snapshot(snapshot_height, book)?;
                    let mut next = (*proposal).clone();
                    next.phase = Phase::Voting {
                        snapshot_height,
                        end_height: height
                            .checked_add(self.rules.ballot.voting_period_blocks)
                            .context("Governance voting end overflow")?,
                        tally: Tally::default(),
                    };
                    self.set_due(next.due_height()?, id, true);
                    self.put_proposal(next)?;
                }
                Phase::Voting {
                    snapshot_height,
                    tally,
                    ..
                } => {
                    let meta = self
                        .snapshot_meta(*snapshot_height)?
                        .context("Governance ballot snapshot is absent")?;
                    let passes = tally.passes(meta.total_weight, &self.rules.ballot)?;
                    self.release_snapshot(*snapshot_height, meta)?;
                    self.cleared_votes.insert(id);
                    self.votes.retain(|(vote_id, _), _| *vote_id != id);
                    if passes {
                        let mut next = (*proposal).clone();
                        next.phase = Phase::Passed {
                            execute_at: height
                                .checked_add(self.rules.ballot.timelock_blocks)
                                .context("Governance timelock overflow")?,
                            tally: tally.clone(),
                        };
                        self.set_due(next.due_height()?, id, true);
                        self.put_proposal(next)?;
                    } else {
                        settle.push(Due::Refund {
                            id,
                            deposits: proposal.deposits.clone(),
                            outcome: Outcome::Rejected,
                        });
                    }
                }
                Phase::Passed { .. } => settle.push(Due::Execute { proposal }),
            }
        }
        Ok(settle)
    }
    /// Record a due proposal's outcome after its refunds (and action) were
    /// settled. The record is removed at the next block start.
    pub(crate) fn finish(&mut self, id: u64, outcome: Outcome) -> Result<()> {
        let proposal = self
            .proposal(id)?
            .context("Finished governance proposal is absent")?;
        ensure!(
            !matches!(proposal.phase, Phase::Finished { .. })
                && proposal.due_height()? == self.header.height,
            "Governance proposal is not due"
        );
        self.header.held_udgt = self
            .header
            .held_udgt
            .checked_sub(proposal.deposited)
            .context("Governance held deposit underflow")?;
        let mut next = (*proposal).clone();
        next.phase = Phase::Finished {
            outcome,
            height: self.header.height,
        };
        self.set_due(next.due_height()?, id, true);
        self.put_proposal(next)
    }
    fn start_snapshot(&mut self, height: u64, book: &RecoveryBook) -> Result<()> {
        if let Some((staged, _)) = &self.snapshot {
            ensure!(*staged == height, "Governance snapshot height differs");
        } else {
            let parent = self.parent.as_ref().context("Governance parent missing")?;
            let mut weights = BTreeMap::new();
            // Registered owners with an effective bond at the parent, by
            // stable account ID. Positions are keyed by native address.
            for (address, positions) in &parent.effective.positions {
                let Some(account) = book.account_by_address(address)? else {
                    continue;
                };
                let weight = positions
                    .values()
                    .try_fold(0u128, |sum, amount| sum.checked_add(*amount))
                    .context("Governance bond sum overflow")?;
                if weight > 0 {
                    weights.insert(account.recovery.domain.account_id, weight);
                }
            }
            ensure!(
                weights.len() <= self.rules.ballot.max_voters as usize,
                "Governance electorate exceeds its configured bound"
            );
            self.snapshot = Some((height, Arc::new(weights)));
        }
        let weights = &self.snapshot.as_ref().expect("snapshot staged").1;
        let mut meta = self.snapshot_meta(height)?.unwrap_or(SnapshotMeta {
            total_weight: weights
                .values()
                .try_fold(0u128, |sum, w| sum.checked_add(*w))
                .context("Governance snapshot weight overflow")?,
            owners: u32::try_from(weights.len())?,
            ballots: 0,
        });
        meta.ballots = meta
            .ballots
            .checked_add(1)
            .context("Governance snapshot overflow")?;
        self.snapshots.insert(height, Some(meta));
        Ok(())
    }
    fn release_snapshot(&mut self, height: u64, mut meta: SnapshotMeta) -> Result<()> {
        meta.ballots = meta
            .ballots
            .checked_sub(1)
            .context("Governance snapshot reference underflow")?;
        self.snapshots
            .insert(height, if meta.ballots == 0 { None } else { Some(meta) });
        Ok(())
    }

    /// Admit a proposal from `proposer` at the current height.
    pub(crate) fn propose(
        &mut self,
        proposer: Owner,
        proposal_id: u64,
        action_class: u16,
        action_data: Vec<u8>,
        action_digest: [u8; 32],
    ) -> std::result::Result<(), Rule> {
        if proposal_id != self.header.next_proposal_id {
            return Err(Rule("GOVERNANCE_PROPOSAL_ID"));
        }
        let height = self.header.height;
        let proposal = Proposal {
            id: proposal_id,
            proposer,
            action_class,
            action_data,
            action_digest,
            admitted_height: height,
            deposits: BTreeMap::new(),
            deposited: 0,
            phase: Phase::Collecting {
                close_height: height
                    .checked_add(self.rules.deposit.deposit_period_blocks)
                    .ok_or(Rule("GOVERNANCE_HEIGHT"))?,
            },
        };
        let due = proposal
            .due_height()
            .map_err(|_| Rule("GOVERNANCE_HEIGHT"))?;
        self.put_proposal(proposal)
            .map_err(|_| Rule("GOVERNANCE_ACTION"))?;
        self.set_due(due, proposal_id, true);
        self.header.next_proposal_id += 1;
        self.header.proposals += 1;
        Ok(())
    }
    /// Add `amount` from `owner` to a collecting proposal's escrow. The
    /// caller debits the owner's DGT in the same transaction.
    pub(crate) fn deposit(
        &mut self,
        owner: Owner,
        proposal_id: u64,
        amount: u128,
    ) -> std::result::Result<(), Rule> {
        let height = self.header.height;
        let proposal = self
            .proposal(proposal_id)
            .map_err(|_| Rule("GOVERNANCE_STATE"))?
            .ok_or(Rule("GOVERNANCE_PROPOSAL_UNKNOWN"))?;
        match proposal.phase {
            Phase::Collecting { close_height } if height < close_height => {}
            _ => return Err(Rule("GOVERNANCE_DEPOSIT_CLOSED")),
        }
        if amount == 0 {
            return Err(Rule("GOVERNANCE_DEPOSIT_ZERO"));
        }
        let mut next = (*proposal).clone();
        let entry = next.deposits.entry(owner).or_insert(0);
        *entry = entry
            .checked_add(amount)
            .ok_or(Rule("GOVERNANCE_DEPOSIT_OVERFLOW"))?;
        if next.deposits.len() > self.rules.deposit.max_depositors as usize {
            return Err(Rule("GOVERNANCE_DEPOSITOR_CAPACITY"));
        }
        next.deposited = next
            .deposited
            .checked_add(amount)
            .filter(|total| *total <= DGT_SUPPLY)
            .ok_or(Rule("GOVERNANCE_DEPOSIT_OVERFLOW"))?;
        let held = self
            .header
            .held_udgt
            .checked_add(amount)
            .filter(|total| *total <= DGT_SUPPLY)
            .ok_or(Rule("GOVERNANCE_DEPOSIT_OVERFLOW"))?;
        self.put_proposal(next)
            .map_err(|_| Rule("GOVERNANCE_STATE"))?;
        self.header.held_udgt = held;
        Ok(())
    }
    /// Record `owner`'s vote with its snapshot weight.
    pub(crate) fn cast(
        &mut self,
        owner: Owner,
        proposal_id: u64,
        choice: VoteChoice,
    ) -> std::result::Result<(), Rule> {
        let height = self.header.height;
        let proposal = self
            .proposal(proposal_id)
            .map_err(|_| Rule("GOVERNANCE_STATE"))?
            .ok_or(Rule("GOVERNANCE_PROPOSAL_UNKNOWN"))?;
        let Phase::Voting {
            snapshot_height,
            end_height,
            tally,
        } = &proposal.phase
        else {
            return Err(Rule("GOVERNANCE_VOTING_CLOSED"));
        };
        if height > *end_height {
            return Err(Rule("GOVERNANCE_VOTING_CLOSED"));
        }
        let weight = self
            .weight(*snapshot_height, &owner)
            .map_err(|_| Rule("GOVERNANCE_STATE"))?
            .ok_or(Rule("GOVERNANCE_VOTER_NOT_ELIGIBLE"))?;
        if self
            .vote(proposal_id, &owner)
            .map_err(|_| Rule("GOVERNANCE_STATE"))?
            .is_some()
        {
            return Err(Rule("GOVERNANCE_ALREADY_VOTED"));
        }
        let mut tally = tally.clone();
        tally
            .add(choice, weight)
            .map_err(|_| Rule("GOVERNANCE_STATE"))?;
        let mut next = (*proposal).clone();
        next.phase = Phase::Voting {
            snapshot_height: *snapshot_height,
            end_height: *end_height,
            tally,
        };
        self.put_proposal(next)
            .map_err(|_| Rule("GOVERNANCE_STATE"))?;
        self.votes.insert((proposal_id, owner), choice);
        Ok(())
    }

    /// The header and every entry this block changed; deletions separately.
    pub(crate) fn changes(&self) -> Result<(BTreeMap<Vec<u8>, Vec<u8>>, BTreeSet<Vec<u8>>)> {
        let mut writes = BTreeMap::new();
        let mut deletes = BTreeSet::new();
        if self.header != self.base {
            writes.insert(HEADER_KEY.as_bytes().to_vec(), self.header.encode()?);
        }
        if self.parameters != self.base_parameters {
            writes.insert(
                PARAMETERS_KEY.as_bytes().to_vec(),
                encode(&self.parameters)?,
            );
        }
        for (id, proposal) in &self.proposals {
            let key = proposal_key(*id).into_bytes();
            match proposal {
                Some(p) => {
                    writes.insert(key, encode(&**p)?);
                }
                None => {
                    deletes.insert(key);
                }
            }
        }
        for ((id, owner), choice) in &self.votes {
            writes.insert(vote_key(*id, owner).into_bytes(), encode(choice)?);
        }
        for id in &self.cleared_votes {
            for (key, _) in scan(&self.storage, &vote_prefix(*id))? {
                deletes.insert(key);
            }
        }
        if let Some((height, weights)) = &self.snapshot {
            for (owner, weight) in weights.iter() {
                writes.insert(weight_key(*height, owner).into_bytes(), encode(weight)?);
            }
        }
        for (height, meta) in &self.snapshots {
            let key = snapshot_key(*height).into_bytes();
            match meta {
                Some(meta) => {
                    writes.insert(key, encode(meta)?);
                }
                None => {
                    ensure!(
                        self.snapshot.as_ref().map(|(h, _)| h) != Some(height),
                        "Governance snapshot released in its first block"
                    );
                    deletes.insert(key);
                    for (key, _) in scan(&self.storage, &weight_prefix(*height))? {
                        deletes.insert(key);
                    }
                }
            }
        }
        for ((height, id), present) in &self.due {
            let key = due_key(*height, *id).into_bytes();
            if *present {
                writes.insert(key, vec![1]);
            } else {
                deletes.insert(key);
            }
        }
        ensure!(
            deletes.iter().all(|key| !writes.contains_key(key)),
            "Governance entry both written and removed"
        );
        Ok((writes, deletes))
    }

    /// Per-block check of the header against the changed proposals.
    pub(crate) fn validate_block(&self) -> Result<()> {
        self.header.validate()?;
        ensure!(
            self.header.chain_id == self.base.chain_id
                && self.header.genesis_digest == self.base.genesis_digest
                && self.header.height == self.base.height + 1
                && self.header.next_proposal_id >= self.base.next_proposal_id,
            "Governance header identity or height differs"
        );
        let mut held_delta: i128 = 0;
        let mut count_delta: i64 = 0;
        for (id, staged) in &self.proposals {
            let before = self
                .storage
                .db
                .get(proposal_key(*id))?
                .map(|bytes| decode::<Proposal>(&bytes))
                .transpose()?;
            if before.is_none() {
                ensure!(
                    *id >= self.base.next_proposal_id && *id < self.header.next_proposal_id,
                    "Governance proposal ID reused"
                );
            }
            held_delta += staged.as_ref().map_or(0, |p| p.held() as i128)
                - before.as_ref().map_or(0, |p| p.held() as i128);
            count_delta += i64::from(staged.is_some()) - i64::from(before.is_some());
            if let Some(proposal) = staged {
                proposal.validate(&self.rules.deposit, &self.rules.ballot)?;
                ensure!(
                    self.due.get(&(proposal.due_height()?, *id)) == Some(&true)
                        || self
                            .storage
                            .db
                            .get(due_key(proposal.due_height()?, *id))?
                            .is_some(),
                    "Governance proposal has no due entry"
                );
            }
        }
        ensure!(
            self.header.held_udgt as i128 - self.base.held_udgt as i128 == held_delta
                && self.header.proposals as i64 - self.base.proposals as i64 == count_delta,
            "Governance running totals differ from changed proposals"
        );
        Ok(())
    }
}

/// A governance rule failure after acceptance; the fee is charged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rule(pub &'static str);

/// Write the genesis header. Nothing else exists at genesis.
pub(crate) fn genesis_writes(
    chain_id: &str,
    genesis_digest: [u8; 32],
) -> Result<BTreeMap<Vec<u8>, Vec<u8>>> {
    let header = Header::genesis(chain_id.to_owned(), genesis_digest)?;
    Ok(BTreeMap::from([(
        HEADER_KEY.as_bytes().to_vec(),
        header.encode()?,
    )]))
}

/// Complete check of every committed governance entry (startup and tests).
pub(crate) fn validate_complete(storage: &Storage, rules: &StoreRules) -> Result<Header> {
    let header = Header::read(storage)?.context("Governance state missing")?;
    let mut proposals = BTreeMap::new();
    for (key, value) in scan(storage, PROPOSAL)? {
        let text = std::str::from_utf8(&key[PROPOSAL.len()..])?;
        let id = id_suffix(text)?;
        let proposal: Proposal = decode(&value)?;
        ensure!(proposal.id == id, "Governance proposal key differs");
        proposal.validate(&rules.deposit, &rules.ballot)?;
        ensure!(
            id < header.next_proposal_id,
            "Governance proposal ID ahead of counter"
        );
        proposals.insert(id, proposal);
    }
    let mut due = BTreeSet::new();
    for (key, _) in scan(storage, DUE)? {
        let text = std::str::from_utf8(&key[DUE.len()..])?;
        ensure!(
            text.len() == 41 && &text[20..21] == ":",
            "Governance due key differs"
        );
        let (height, id) = (id_suffix(&text[..20])?, id_suffix(&text[21..])?);
        ensure!(height > header.height, "Governance transition was skipped");
        ensure!(due.insert(id), "Governance proposal has two due entries");
        let proposal = proposals
            .get(&id)
            .context("Governance due entry has no proposal")?;
        ensure!(
            proposal.due_height()? == height,
            "Governance due index differs"
        );
    }
    ensure!(
        due.len() == proposals.len(),
        "Governance proposal has no due entry"
    );
    let held = proposals
        .values()
        .try_fold(0u128, |sum, p| sum.checked_add(p.held()))
        .context("Governance held sum overflow")?;
    ensure!(
        held == header.held_udgt && proposals.len() as u64 == header.proposals,
        "Governance running totals differ from stored proposals"
    );
    let mut snapshots = BTreeMap::new();
    for (key, value) in scan(storage, SNAPSHOT)? {
        let height = id_suffix(std::str::from_utf8(&key[SNAPSHOT.len()..])?)?;
        snapshots.insert(height, decode::<SnapshotMeta>(&value)?);
    }
    let mut weights: BTreeMap<u64, BTreeMap<Owner, u128>> = BTreeMap::new();
    for (key, value) in scan(storage, WEIGHT)? {
        let text = std::str::from_utf8(&key[WEIGHT.len()..])?;
        ensure!(
            text.len() == 85 && &text[20..21] == ":",
            "Governance weight key differs"
        );
        let height = id_suffix(&text[..20])?;
        let owner = owner_suffix(&key, &weight_prefix(height))?;
        let weight: u128 = decode(&value)?;
        ensure!(weight > 0, "Zero governance weight");
        weights.entry(height).or_default().insert(owner, weight);
    }
    let mut ballots: BTreeMap<u64, u32> = BTreeMap::new();
    let mut tallies = BTreeMap::new();
    for proposal in proposals.values() {
        if let Phase::Voting {
            snapshot_height,
            tally,
            ..
        } = &proposal.phase
        {
            *ballots.entry(*snapshot_height).or_default() += 1;
            tallies.insert(
                proposal.id,
                (*snapshot_height, tally.clone(), Tally::default()),
            );
        }
    }
    ensure!(
        snapshots.keys().eq(ballots.keys()),
        "Governance snapshots differ from open ballots"
    );
    for (height, meta) in &snapshots {
        let owners = weights.get(height).cloned().unwrap_or_default();
        let total = owners
            .values()
            .try_fold(0u128, |sum, w| sum.checked_add(*w))
            .context("Governance snapshot sum overflow")?;
        ensure!(
            meta.ballots == ballots[height]
                && meta.total_weight == total
                && meta.owners as usize == owners.len()
                && owners.len() <= rules.ballot.max_voters as usize,
            "Governance snapshot differs from its weights"
        );
    }
    ensure!(
        weights.keys().all(|height| snapshots.contains_key(height)),
        "Governance weights without a snapshot"
    );
    for (key, value) in scan(storage, VOTE)? {
        let text = std::str::from_utf8(&key[VOTE.len()..])?;
        ensure!(
            text.len() == 85 && &text[20..21] == ":",
            "Governance vote key differs"
        );
        let id = id_suffix(&text[..20])?;
        let owner = owner_suffix(&key, &vote_prefix(id))?;
        let choice: VoteChoice = decode(&value)?;
        let (height, _, counted) = tallies
            .get_mut(&id)
            .context("Governance vote for a closed or absent ballot")?;
        let weight = weights
            .get(height)
            .and_then(|w| w.get(&owner))
            .context("Governance vote outside its snapshot")?;
        counted.add(choice, *weight)?;
    }
    for (_, stored, counted) in tallies.values() {
        ensure!(stored == counted, "Governance tally differs from its votes");
    }
    for item in storage
        .db
        .iterator(IteratorMode::From(b"governance:", Direction::Forward))
    {
        let (key, _) = item?;
        if !key.starts_with(b"governance:") {
            break;
        }
        ensure!(
            key.as_ref() == HEADER_KEY.as_bytes()
                || key.as_ref() == PARAMETERS_KEY.as_bytes()
                || [PROPOSAL, VOTE, WEIGHT, SNAPSHOT, DUE]
                    .iter()
                    .any(|prefix| key.starts_with(prefix.as_bytes())),
            "Unsupported governance record"
        );
    }
    GovernedParameters::read(storage)?;
    Ok(header)
}

#[cfg(test)]
#[path = "governance_store_tests.rs"]
mod tests;
