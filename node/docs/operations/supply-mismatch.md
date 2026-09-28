# Supply mismatch

Runbook v1 ([index](README.md)). The roles, thresholds and channel are
unset (D14-Q03, D12-Q02).

## What the node checks

The application checks the supply as it builds every block, after every
commit and, completely, at every startup. Any difference fails closed: the
block is not written and the node stops. These are the conservation rules,
in the metric buckets of `dytallix-app.prom`:

**DRT** (`dytallix_app_supply_udrt`):
- `total` = `genesis` + `emitted` − `burned`.
- `liquid` + `withheld_fees` + every `pool_*` = `total`.
- `withheld_fees` is 0 after a commit, since each block burns its fees
  (fees v1).

**DGT** (`dytallix_app_supply_udgt`):
- `liquid` + `pending_bonded` + `staked` + `unbonding` + `penalty_reserve`
  + `governance_escrow` = `issued`.
- `issued` never exceeds 10^15 uDGT (1,000,000,000 DGT).

The running account totals (`supply:account_totals`) must equal the account
records. A block's totals must follow from its own balance changes.

The metrics are recorded from the checked supply after each commit, so one
node's metrics always satisfy these rules. Supply is consensus state, so
every validator's values at the same height must be identical.

## Signals

| Signal | Case |
| --- | --- |
| Every validator stops at the same height with `supply` (12) | 1. A deterministic accounting fault |
| One node stops with `supply` at startup, while the chain continues | 2. Local state inconsistency |
| Monitoring sees different supply values at one height on two validators | 3. State divergence |
| One node's metrics break a rule above | 4. A defect in the node's check |

## 1. Every validator stops

The committed block cannot be executed under the release's supply rules.
Its effects, or the rules, are wrong. The chain is halted.

1. Follow [halt.md](halt.md) from step 1. Fence every signer.
2. On a stopped node, run `dytallix-state-check`.
   - **`pass`** at height H−1: the stored state is sound. The failure is in
     executing block H, and the check tool cannot show its text: the
     exit class is the record. Keep block H: its transactions come from the
     engine's block store or a peer's `/block?height=H`.
   - **`fail`** with class `supply`: block H was written and the check after
     the write failed. The printed error names the rule that failed.
3. Reproduce the failure on a copy of a stopped node's database, never on
   the original. Identify the rule and the transaction.
4. A fix is new code. Restarting the chain on a new release after a halt is
   blocked until gap 18 ([known limits](README.md#known-limits)).

## 2. One node stops at startup

Its stored state no longer satisfies the rules, while the network's does.
Suspect storage corruption or a changed database.

1. Record the supervisor report. Run `dytallix-state-check` for the full
   error.
2. Recover the node as in [fork.md](fork.md), case A: set its database
   aside, keep `priv_validator_state.json`, and block sync.
3. Check the disk, the filesystem and any recent restore before trusting
   the host again.

## 3. Values differ between validators

At one height, supply values that differ mean their states differ. Follow
[fork.md](fork.md): it is case A if one node differs, and case B if the
network splits.

## 4. A node's metrics break a rule

The node's own check passed while an external reconciliation failed. One of
the two is wrong. Keep both results and escalate to the release lead as a
possible defect in the release. Do not change state.

## Do not

- Edit balances, totals or burn counters in the database.
- Restart on a patched binary. The committed release is enforced; see gap
  18.
- Delete the database of a node that failed the check.
