# Fork or state divergence

Runbook v1 ([index](README.md)). The roles, thresholds and channel are
unset (D14-Q03, D12-Q02).

The engine (CometBFT) finalizes a block once more than two thirds of the
voting power commit it. A fork of finalized history therefore needs more
than a third of the power to be faulty. Most "fork" alarms are one of the
three cases below; identify which before acting.

| Case | What happened | Response |
| --- | --- | --- |
| A. One node diverged | This node's state or app hash differs from the network's; the chain continues | Recover the node |
| B. Conflicting commits | Two valid commits exist for one height | [Full halt](halt.md) |
| C. No progress | Not enough power is online or connected; nothing conflicts | Restore connectivity; see [resource exhaustion](resource-exhaustion.md) |

## Signals

**This node falls behind while the network continues (case A).**
- `dytallix_engine_consensus_latest_block_height` stops while a trusted
  peer's or a public gateway's `/status` keeps rising.
- `dytallix_engine_blocksync_syncing` stays 1.
- The engine refuses the network's next block because its header app hash
  differs from this node's. It then drops the peers that serve it, so the
  node stalls.

**The supervisor stops the service (case A).**
- The application reports `history` (11): a stored record, commitment or
  application hash differs.
- Or the engine exits (an `OWNED_CHILD_EXIT` for the engine) at the startup
  handshake, because the replayed app hash does not match.

**Faulty validators (case B).**
- `dytallix_engine_consensus_byzantine_validators` is above zero.
- Evidence is recorded in blocks (duplicate votes, light client attacks),
  or two headers are signed for one height.

**No progress (case C).**
- The height stops everywhere while `dytallix_engine_consensus_rounds`
  rises.
- `dytallix_engine_consensus_missing_validators_power` is high, and
  `dytallix_engine_p2p_peers` is low.

## A. One node diverged

1. **Record** the supervisor report, the metrics files and the node's last
   height and app hash, from the status view or the metrics.
2. **Stop the node and check it.** Stop the supervisor, then run
   `dytallix-state-check`.
   - `history` or `supply`: the local database is inconsistent with itself.
     Suspect storage corruption, and check the disk and filesystem.
   - `pass`: the database is self-consistent but differs from the network.
     Compare this node's `app_hash` at height H with the network's header
     at H+1, from `/block?height=H+1` on a trusted peer or gateway.
3. **Rule out a determinism fault.** A self-consistent node that computes a
   different app hash, on the release and configuration every validator
   runs, points to nondeterministic execution. Other nodes may diverge the
   same way. Escalate to the release lead before recovering. If more than
   one validator diverges, treat it as case B.
4. **Recover the node.**
   - Move the application database and the engine's data directory aside;
     do not delete them.
   - Copy `data/priv_validator_state.json` back into the new data
     directory. A validator that forgets its last signed step can sign
     twice.
   - Restart, and block sync from a peer that holds every block (an archive
     node). State sync join is not available under the supervisor (see
     [known limits](README.md#known-limits)).
5. **Verify.**
   - The node's height reaches the network's.
   - Its header app hashes match the network's from the rejoin height on.
   - `dytallix-state-check`, run on a later stop, passes.

## B. Conflicting commits

A conflicting commit is a safety failure outside the fault model. There is
no automatic choice of chain.

1. **Record both commits** with their headers, signatures and the peers that
   served them. Export light blocks from each honest node
   (`dytallix-light-export`, with the node stopped).
2. **Follow the [full halt](halt.md) procedure.** Fence every signer, then
   identify the last checkpoint that every honest party accepts.
3. **Identify the faulty validators** from the signatures on both commits
   and from the recorded evidence. Evidence is recorded but not penalized
   while the penalty profile stays off (D09-Q04). Removing faulty power
   therefore needs a governance or operator decision, not an automatic
   penalty.
4. Do not create a replacement genesis or revert finalized history.

## C. No progress

1. Check `dytallix_engine_p2p_peers` and the peer pins in
   `config/pqc_transport.json`. Peers are admitted only by their pinned
   ML-DSA-65 public key.
2. Check host clocks, the network path and each validator's supervisor
   report. A validator stopped by a failure class belongs to its own
   runbook.
3. Consensus resumes on its own once more than two thirds of the power is
   connected. No state change is needed.

## Do not

- Delete a diverged database. It is the evidence.
- Reset or copy `priv_validator_state.json` from another node.
- Use the upstream `cometbft rollback`.
