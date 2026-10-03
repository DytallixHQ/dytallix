# On-chain governance v1 (T6)

Status: approved (P01, 27 September 2026: decisions below); G-S1 to G-S5
done. Engineering task E04. Paths are relative to
`dytallix-fast-launch/node/src/`. Decision IDs refer to
`mainnet/launch/MAINNET_DECISION_REGISTER.json`.

This step puts the approved governance rules on the consensus path and adds
the executors a passed proposal needs. It sets no production numbers:
quorum, approval, veto, deposit, periods, timelock, fees, capacity bounds and
parameter bounds are E05 inputs. Tests use synthetic values.

## Already approved (D11-Q01, D11-Q02, 25 September 2026)

- Electorate: registered accounts with effective bonded DGT at the finalized
  parent block, by stable account ID. Each owner votes its own bond only.
  Liquid, queued and non-effective DGT count zero. No delegation.
- Proposer: a registered account with positive effective bond, plus the
  minimum deposit.
- Snapshot: fixed at the parent block before voting starts; it sets each
  weight and the quorum denominator.
- Tally: abstain counts for quorum only; integer basis points, required
  weight rounded up; veto threshold.
- Deposits: held in accounted DGT escrow and refunded once at every terminal
  outcome. No burn or sweep.
- Timelock: execution at an explicit finalized height; the action digest is
  bound before voting; one execution, a bounded failure state, no retry.
- No cancellation. Exactly one governance action per ordinary-v3
  transaction (tags 13 to 15).

## State before this step

The pieces existed as tested pure planners (`governance_*.rs`,
`runtime/governance_*.rs`). None ran in consensus. Line numbers refer to
that code, which this step replaced.

| ID | Problem | Where |
| --- | --- | --- |
| G1 | Activation always fails: `validate_for_activation` ends with "exact state transitions remain unapproved", and no action class exists. Config open, genesis, v3 decode and dispatch all fail closed. | `runtime/governance_candidate.rs:155`, `consensus_settlement.rs:352, 1675, 3878, 4705` |
| G2 | No executor: a passed proposal reaches its due height and the planner stops the block ("requires an approved execution transition"). Wired as is, that would halt the chain. | `runtime/governance_ordered_admission.rs:247` |
| G3 | Electorate join never matches: account IDs (64 hex) are looked up in lifecycle positions keyed by Bech32m address, so the eligible set is always empty and every proposal and ballot start fails. Tests rekey positions to hex and hide it. | `runtime/governance_ballot.rs:99` |
| G4 | The committed parent loads the complete recovery book on every prepare, process and finalize. | `consensus_settlement.rs:1311` |
| G5 | Unbounded state: one `governance:v1:state` blob (32 MiB) keeps every proposal forever, IDs must be contiguous from 1, each ballot holds a full snapshot copy, and the blob is re-encoded, re-validated and metered as one record per block. At about 1,000 voters the chain halts after roughly 200 proposals, and gas grows with history. | `runtime/governance_state.rs:151, 236, 280` |
| G6 | A block cannot mix automatic transitions (deposit close, ballot start and close, refunds, due actions) with user transactions; the order is unapproved. | `runtime/governance_ordered_admission.rs:238` |
| G7 | No state advance per block, no v3 receipts or receipt windows, no `max_depositors` config field. | — |
| G8 | Every chain parameter is fixed in genesis config. Nothing can change one after launch except an upgrade. | `ConsensusConfig` |

The legacy `runtime/governance.rs` (HTTP routes, `gov:` keys) was never used
by the consensus build, which rejects `gov:` keys; E04 gap 14 removed it.

## Proposed rules

1. **Block order (proposal of 25 September).** At height H: verify the
   committed head at H-1; apply all automatic transitions due at H in
   ascending proposal ID (close deposit stages, refund below-minimum
   proposals, start funded ballots from the H-1 snapshot, close ballots,
   refund rejected ones, execute due actions); then apply signed transactions
   in block order against that staged state; commit everything in one batch.
   A refund at H is spendable at H; a ballot starting at H accepts votes at H.
2. **Execution outcome.** At its due height an action is checked against the
   state at that height. If it is valid, it applies and the proposal becomes
   `Executed`. If not (for example a bound no longer holds), the proposal
   becomes `FailedExecution` with its reason. Either way the deposits are
   refunded in the same block and the block continues. This is the approved
   "bounded failure state"; an action never halts the chain.
3. **Action classes.** Each class has a fixed executor, a byte bound and a
   canonical encoding. Candidate classes:

   | Class | Effect | Bounds |
   | --- | --- | --- |
   | Parameter change | Sets one governed parameter from the list below, effective at execution height + 1 | Each parameter has a genesis minimum and maximum; values outside fail execution |
   | Validator registry | Adds an approved operator (validator ID and owner), or removes one that has no registered validator | At most 64 operators (existing bound); lifecycle admission rules unchanged |
   | Upgrade approval | Not enabled: upgrades stay root-signed only | — |
   | Treasury spend | Not enabled: POST MAINNET (needs D02-Q02 recipients and a treasury account) | — |

   Governed parameters: the ordinary fee profile, as a new
   profile version (gas price, per-resource costs, account creation fee);
   `min_self_bond`; `max_active`. Not governed in v1: governance's own rules,
   penalty rates (D06 open), evidence and margin limits (they set the
   retention horizon), emission, DGT supply (fixed by `DGT_TOKENOMICS.md`),
   signature and algorithm profiles, recovery template. Those change only by
   upgrade.
4. **Storage (fixes G5).** One small header (next ID, height, held total,
   open list, due index) plus entries per proposal, per voter vote and per
   snapshot owner. Ballots that start at the same height share one snapshot.
   A vote reads one weight and updates a running tally. The header keeps
   counts only, and each due transition is its own keyed entry. The planned
   `max_open_proposals` cap was dropped (P01, 3 October 2026): the 10,000 DGT
   minimum deposit and the block gas limit already bound open proposals and
   the work each block's due transitions cause. IDs stay unique by the
   counter.
5. **Retention.** A terminal proposal (refunded after `Rejected`, `Executed`
   or `FailedExecution`) is removed at the next block start with its votes,
   snapshot entries and escrow record, keeping running totals. Ballot votes
   and snapshots are removed at close. This is the Step 4 rule applied to
   governance. Blocks and events keep the history.
6. **Electorate (fixes G3, G4).** Enumerate lifecycle positions (bounded by
   `max_positions`), resolve each owner with `account_by_address`, and key
   the snapshot by account ID. The committed parent uses the staged recovery
   book; nothing loads the complete book per block.
7. **Activation.** The candidate config lists the enabled classes. With the
   rules above approved, `validate_for_activation` checks the list against
   the implemented executors instead of always failing. Governance runs from
   the genesis activation height; no fallback.

## Steps

| Step | Content | Output change |
| --- | --- | --- |
| G-S1 (done) | Electorate from lifecycle positions resolved to account IDs; no complete-book load (rule 6) | None |
| G-S2 (done) | Per-entry storage, shared snapshots, running tally, due index, terminal removal (rules 4, 5) | State layout |
| G-S3 (done) | Consensus wiring: automatic phase at block start, then signed v3 transactions (rule 1), in CheckTx, prepare, process and finalize; mempool recheck; supply, per-block and complete checks | Governance activates on a configured chain |
| G-S4 (done) | Executors for the approved classes (rules 2, 3): governed parameters entry, effective configuration for fees and lifecycle, operator registry changes | Parameter and registry effects |
| G-S5 (done) | End-to-end tests through the engine | Tests |

**Implementation notes.** Where these differ from the rules above, the
difference is marked.

- Modules. `runtime/governance_store.rs` (entries and automatic
  transitions), `governance_execution.rs` (v3 transactions and the block
  phase), `governance_actions.rs` (the two classes). The block-batch planners,
  the single-record state and the unwired admission and fee modules were
  removed. The v3 meter and reservation modules are kept.
- Keys under `governance:v2:`: `header`, `parameters`,
  `proposal:{id}`, `vote:{id}:{account}`, `snapshot:{height}`,
  `weight:{height}:{account}` and `due:{height}:{id}`. Each proposal has
  exactly one due entry, at the height it next changes; block start reads
  only that height's entries.
- **Differs from rule 4: no open-proposal bound.** A fixed number of open
  slots would let a bonded account block governance by filling them with
  unfunded proposals for the price of fees. Instead every record is removed
  when it finishes, and work per block is bounded by earlier blocks: deposit
  closes by the proposals admitted one deposit period earlier, ballot starts
  by one shared snapshot per height, closes and executions by funded
  deposits. No new parameter.
- **Differs from rule 3: a change takes effect at its execution height**,
  before that block's transactions, not one block later. This follows rule
  1 and avoids re-checking a change a block after it was validated. The
  timelock already tells clients the height in advance.
- A fee change carries every governed value (`governance_actions::FeeValues`)
  and replaces both the ordinary profile and the v3 profile, each with the
  next version and the execution height as activation height. Limits,
  capacities, minimum gas and algorithm sets stay unchanged.
- A change is applied to a copy of the block state and must still pass the
  lifecycle, reward and penalty checks; otherwise it fails execution and the
  deposits are refunded. For example, a minimum self-bond above an existing
  validator's bond fails. Removing an operator fails while any retained
  lifecycle record names its validator.
- Stored ordinary and lifecycle configurations are compared with the genesis
  configuration plus the governed values (`effective_config`). Two checks
  were narrowed so governed values can change: an unbond's stored
  configuration must match only the evidence limits, and the validator proof
  profile digest excludes operators, `min_self_bond` and `max_active`.
- Fee history keeps a profile only while a retained receipt uses it. An
  earlier profile may remain beside the current one if it has a lower version
  and differs only in governed values; each receipt is checked against its
  own profile. With governance configured, `max_retained_profiles` must be at
  least 2. (Found after the first commit: an ordinary receipt under the old
  profile would have halted the block that executed a fee change, and old
  profiles were never pruned.)
- Governance transactions keep no retained receipt. The result is in the
  block record and app hash, and the nonce prevents replay. The complete
  check accepts them like pruned ordinary receipts.
- Each committed governance transaction is reconciled with its effects
  before it is kept (E04 gap 12, TXN-004), as v2 receipts and charged
  recoveries are:
  - its fee moves from the actor's uDRT to fee custody, and nothing burns;
  - a successful deposit moves its uDGT from the actor to the header's held
    total;
  - the actor's nonce advances by one;
  - no other loaded account, and no reward, validator, penalty, lifecycle or
    evidence state, changes.

  A difference is an internal error: ProcessProposal rejects the block,
  and FinalizeBlock fails closed.
- Only registered (initialized) accounts can sign a v3 transaction; a
  governance participant has already spent.
- At genesis `ballot.max_voters` must be at least the staker bound
  (`max_positions`), so a snapshot never exceeds it.
- The supply check reads only the governance header's held total.

## Tests

In `ordinary_consensus_tests.rs` (module `governance`), through the engine
with real signatures:

- A parameter change runs from submission to execution at the exact
  heights, is in force at its execution height, refunds its deposit once and
  is removed a block later; a restart runs the complete check; the same
  blocks on a second database give the same app hashes.
- Below-minimum and vetoed proposals refund every depositor.
- An inconsistent change fails execution, refunds, and changes nothing.
- A fee change replaces both profiles; a transaction signed for the old one
  is refused and a new one pays the new price.
- A refund at block start is spendable in the same block.
- Rule failures after acceptance (no bond, out of bounds, disabled class,
  wrong ID, vote before the ballot, deposit after close) are charged and
  change no governance state.
- The proposer includes paid governance transactions and drops denied ones.
- A registry addition executes and survives a restart.
- 95 proposals keep at most 7 governance entries.
- The complete check refuses a changed tally, a missing due entry, a vote
  outside its snapshot, a wrong held total, a missing snapshot and an
  unknown key.

Unit tests cover threshold rounding, proposal and header checks, action
bounds and encoding, and unbond maturity after governed changes. Breaking
the electorate lookup, refunds or removal fails the tests.

## Decisions (P01, approved 27 September 2026)

1. Block order: automatic transitions first, then signed transactions
   (rule 1).
2. Action classes: parameter change and validator registry. Upgrades stay
   root-signed only, as today (no governance approval class). Treasury
   spending is POST MAINNET.
3. Governed parameters: new ordinary fee profile versions (gas price,
   resource costs, account creation fee), `min_self_bond` and `max_active`,
   each within genesis bounds. Governance is the fee authority (EC-12).
   Everything else changes only by upgrade.
4. Retention: remove finished proposals with their votes, snapshots and
   escrow records at the next block start after the refund, keeping running
   totals (rule 5).

Rule 2 (failed execution refunds and continues) follows the approved
execution rule.

## Also found

- `mainnet/launch/DECISIONS_REQUIRED.md` marked D11-Q01 and D11-Q02 open
  while the JSON register had them partially approved. Both now record the
  25 and 27 September approvals.
- AC-006 (quadratic voting, vote decay) conflicts with the approved stake
  weighting and is still in the conflict register.
