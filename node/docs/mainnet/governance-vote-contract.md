# Governance vote storage contract

Status: local development repair. Rick Glenn approved five governance rules on 25 September 2026. Production numeric values, eligibility details and release acceptance remain open.

## Stored votes and tally

A stored vote uses the key `gov:vote:{proposal_id}:{voter}`. Proposal vote
enumeration starts at that exact proposal prefix and stops at the first different
prefix. The trailing colon separates proposal 1 from proposal 10. RocksDB key
order defines enumeration order.

The account cache does not define the voter list. A stored vote must remain
visible when the voter has no cached account and after the database reopens.
The tally uses the weight recorded when the vote was cast. It does not recalculate
that weight from current balances or stake.

Read errors, malformed records and key/record identity mismatches return errors.
The tally must not omit a damaged record and report a partial result as complete.
Vote option totals and the combined total use checked integer addition. Overflow
returns an error. The current proposal stays in its voting state if its tally
fails before the end-block transition. This does not make the entire end-block
loop or governance lifecycle atomic.

## Existing development behavior

The user approved bonded-stake-only voting in the Batch 5 follow-up. Voting power
uses the delegator record's stake amount, including zero. Liquid DGT balances do
not contribute. Validator totals are not added to a delegator's voting weight.
Legacy voting fixtures now provide explicit synthetic stake and consistent total
stake. The original zero-stake test remains unchanged.

This change applies when a new vote records its weight. Existing persisted votes
retain their recorded weights. No live governance-state migration occurred.
The current staking record remains a development implementation; finalized
validator eligibility and a canonical staking snapshot need separate integration.

The current defaults use 6,700 basis points for quorum, 5,000 for approval, and
3,333 for veto. One basis point is 0.01 percent. The runtime rounds a required
weight up. Abstentions count toward quorum but not approval or veto. These
defaults are development values, not approved mainnet parameters. The quorum
denominator still reads current total stake, not an immutable snapshot.

One end-block call tallies a proposal and stores Passed or Rejected. A later
end-block call executes a Passed proposal. This is a two-call lifecycle, not an
enforced timelock. A successful execution stores Executed. Further ordinary
end-block calls do not repeat its parameter change or execution event.

The event fixture uses 70 percent participation against the unchanged 67 percent
development quorum. The fixture checks the existing tally and execution stages.
It does not reduce the quorum or change the runtime execution schedule.

## Remaining mainnet work

The approved E04 rules require an immutable finalized bonded-stake snapshot,
owner-controlled votes, exact threshold arithmetic, refundable deposit escrow,
and an explicit height-based execution timelock. The current runtime has not
implemented the snapshot or timelock. Numeric thresholds, periods, deposit size,
validator eligibility, emergency authority and parameter domains remain open.

Deposit enumeration reads stored records by proposal prefix. It does not use the
account cache. Threshold multiplication uses checked arithmetic across the full
`u128` range. A deposit commits its account debit, depositor record, and proposal
total together. A failed or rejected proposal commits its terminal status and
refunds together. Execution commits the proposal status, governed parameter, and
refund balances together. The production node startup path loads the stored
governance configuration. It rejects damaged configuration and existing
governance data that lacks a configuration record. Such data needs an explicit
migration. Environment settings apply only at first initialization.

Local regressions cover persisted vote weights, duplicate-vote rejection, tally
with an empty account cache, stored parameter restoration, and damaged-record
rejection. They do not qualify a process restart, the full governance lifecycle,
or mainnet operation.
