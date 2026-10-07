# Incident runbooks (v1)

Operator procedures for the incident classes of OBS-003 (E04 gap 15; design
in [incident response v1](../architecture/incident-response-v1.md)).

These are preparation documents. They authorize no live halt, restart, key
operation or state change. On-call coverage and the recovery objectives are
approved ([operations objectives](../../../launch/operations/OBJECTIVES.md),
D12-Q02), and the root keys are held in key kits
([key ceremony](../../../launch/custody/KEY_CEREMONY.md)). The following are
unset inputs:
- the named roles (D14-Q03);
- alert checks, thresholds and routing (artifact A22);
- the authenticated incident channel.

Exercise each procedure on disposable local or assigned staging systems
before acceptance.

| Class | Runbook |
| --- | --- |
| Full consensus halt | [halt.md](halt.md) |
| Fork or state divergence | [fork.md](fork.md) |
| Supply mismatch | [supply-mismatch.md](supply-mismatch.md) |
| Key compromise | [key-compromise.md](key-compromise.md) |
| Resource exhaustion | [resource-exhaustion.md](resource-exhaustion.md) |
| Upgrade or handover failure | [upgrade-failure.md](upgrade-failure.md) |
| Restart on a fixed release after a halt | [restart.md](restart.md) |
| Oracle failure | Not applicable. No oracle is in the consensus path (AC-010): the epoch observation is derived from committed blocks, and a submitted one is refused. |

Day-to-day host operations (status, start and stop, staging wipe, moving to
a new release) are in [host.md](host.md); validator recovery (F17) is
[validator-recovery.md](validator-recovery.md).

The procedures assume the native supervisor (`crates/native-supervisor`) as
the service owner. The Python service that `deploy/pqc-engine` once held was
retired in production activation v1, A5.

## Where to look

- **Supervisor report.** One JSON object per line on the supervisor's
  standard output (the journal under systemd). On a stop, the final report
  records `failure` with these fields:
  - `error_class`, `role` and `stage`;
  - `failure_class`, the node's class for a failed preflight;
  - `child_exit`, whose `application_failure_class` comes from the
    application's exit status.

  The engine's own output is discarded, so its panic text is never kept.
  The application's class is the record of a failed block.
- **Metrics files** in the configured metrics directory:
  `dytallix-engine.prom` and `dytallix-app.prom` (metrics v1). A value of
  `dytallix_metrics_written_timestamp_seconds` older than a few intervals
  means that process stopped writing.
- **Status view.** Available when the service configures the loopback HTTP
  adapter. The engine's `/status` returns the latest height, the app hash
  and catching-up. `/block?height=H` returns a header, whose `app_hash`
  commits the state after block H−1. The application's view is
  `/abci_query?path="/status"`: height, `app_hash`, supply, validator sets,
  the emergency, handover and upgrade state. Only the latest height is
  served.
- **Stopped node check.** With the node stopped, run
  `dytallix-state-check --config APPLICATION_CONFIG --genesis NATIVE_GENESIS --db HOME/appdb`.
  It prints the first failure in full ([build and run](../build-and-run.md)).
- **Light blocks.** With the node stopped,
  `dytallix-light-export --home HOME --from A --to B --output FILE` exports
  the node's own signed headers and prints the trusted height and hash.
- **Operator socket.** `dytallix-operator-rpc --home HOME METHOD [--query Q]`
  reads `rpc-operator.sock`. Its methods are `net_info`, `consensus_state`,
  `dump_consensus_state`, `num_unconfirmed_txs`, `status`, `health` and
  `validators`. It submits nothing.

## Application exit classes

| Status | Class | Runbook |
| --- | --- | --- |
| 10 | `configuration` | Check the inputs against the release record; see [upgrade-failure.md](upgrade-failure.md) for a changed configuration |
| 11 | `history` | [fork.md](fork.md) |
| 12 | `supply` | [supply-mismatch.md](supply-mismatch.md) |
| 13 | `replay` | [upgrade-failure.md](upgrade-failure.md) |
| 14 | `release` | [upgrade-failure.md](upgrade-failure.md) |
| 15 | `execution` | [halt.md](halt.md) if every validator stops at the same height, else [fork.md](fork.md) |
| 16 | `storage` | [resource-exhaustion.md](resource-exhaustion.md) first (space and inodes), then [fork.md](fork.md) |
| 17 | `resource` | [resource-exhaustion.md](resource-exhaustion.md) |
| 1 | unclassified | Keep the report and escalate |
| 101 | panic | Keep the report and escalate |

## Rules for every incident

1. **Record first.** Record the start time, the procedure version, the
   supervisor report, both metrics files, and the candidate and
   configuration hashes. Record public identities only.
2. **Keep the evidence.** Keep the application database, the engine data
   and `data/priv_validator_state.json`. To start fresh, move them aside
   instead of deleting them.
3. **One signer.** Never run a second copy of a validator key. Never reset,
   edit or restore an older `priv_validator_state.json`: it is what stops a
   double signature.
4. **No rollback.** The production engine accepts only `start`. Do not use
   the upstream `cometbft rollback` or rewrite the database. Finalized
   history is never reverted.
5. **No restart loop.** A deterministic failure (history, supply, replay,
   release, execution) recurs on every restart, since the engine replays
   the same block. Restart once to confirm it, then stop.
6. **Private material stays private.** It stays off the incident record and
   out of this repository.

Each execution record holds:
- the candidate and configuration hashes;
- public identity references;
- the start and end times and the procedure version;
- commands, without secrets, and their exit codes;
- observations and evidence hashes;
- the operator and the reviewer;
- unresolved deviations.

## Known limits

- **Restart on new code.** A halt that needs new code resumes only through a
  root-signed restart authorization ([restart.md](restart.md)), signed by
  three of the upgrade keys (the kits' upgrade keys under the solo launch
  profile). `dytallix-root-sign sign-control` does not yet take a restart
  artifact; that is an open follow-up.
- **Rejoin.** A node whose database is set aside rejoins by state sync from
  operator-supplied light blocks (state sync v1: the plan's `state_sync` and
  the supervisor's `state_sync.light_blocks`), or by block sync from the
  archive sentry (`block_history: archive`).
- **Penalties.** A validator's first duplicate vote deducts the penalty rate
  from every stake bonded to it and removes the validator for good
  ([penalties v1](../architecture/penalties-v1.md), D09-Q04).
  Light-client-attack evidence is recorded only, and there is no downtime
  penalty. A production build's genesis sets `production_activation`
  ([production activation v1](../architecture/production-activation-v1.md)).
- **Emergency controls.** Freeze and resume are root-signed controls
  (3 of 5 SLH-DSA signatures). They are prepared from the node's status,
  signed offline with three key kits and submitted with the CLI
  ([control signing](../mainnet/control-signing.md)). A control's window is
  34,560 blocks, about two days.
