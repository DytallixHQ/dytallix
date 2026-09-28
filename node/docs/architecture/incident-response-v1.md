# Incident response (E04 gap 15)

Engineering task E04, gap 15 of the [E04.1 triage](../mainnet/e04-requirement-triage.md)
(OBS-003). OBS-003 asks for incident procedures for halt, fork, supply
mismatch, key compromise, oracle failure, upgrade failure and resource
exhaustion.

## Problem

A survey of the node's failure behavior found three gaps. Each one keeps
an operator from following a procedure:

1. **No outputs in the service launch.** The native supervisor passes no
   metrics, snapshot or block history flags. So a supervised node writes no
   metrics files (metrics v1), and those files are the procedures' signals.
2. **A stopped application gives no reason.**
   - The release application exits with status 1 and writes nothing. Its
     standard error is the bounded diagnostic channel.
   - The supervisor discards the engine's output. Its failure record classes
     a startup check failure as `OTHER_FAILURE`.
   - No tool runs the startup checks on a stopped node.
   - As a result, a supply mismatch, a history mismatch and a full disk look
     the same.
3. **No production validator key proof.** `ValidatorRegister` and
   `ValidatorRotateKey` need a possession proof from the consensus key. The
   only signer accepts fixture nodes only.

## Decisions (P01, 28 September 2026)

1. **Scope.** The procedures cover every OBS-003 class.
   - Fork, supply mismatch, key compromise and resource exhaustion are new.
   - The full-halt draft becomes a procedure; its authorities stay unset.
   - Upgrade and handover failure are written from the code.
   - Oracle failure is recorded as not applicable: no oracle is in the
     consensus path (AC-010).
2. **Metrics required at release.**
   - The service configuration carries the metrics output, and the release
     binding review reports a configuration without it as missing.
   - Snapshots stay optional.
   - The values are E05 inputs.
3. **Exit class and check tool.**
   - The application exits with a fixed status per failure class and writes
     no text to the diagnostic channel.
   - A separate read-only tool runs the startup checks on a stopped node's
     database and prints the first failure in full.
4. **Key proofs are gap 17**, after gap 16. Until then the key compromise
   procedure says that rotation is blocked.

## R-a: service outputs

The native service configuration gains three fields (strict schema, no
defaults):

| Field | Required | Passed as |
| --- | --- | --- |
| `metrics`: `directory`, `interval_seconds` (1 to 3600) | yes | Application `--metrics-dir`, `--metrics-interval-seconds`; engine `--metrics-dir`, `--metrics-interval Ns` |
| `snapshots`: `directory`, `interval_blocks`, `keep` | no | Application `--snapshot-dir`, `--snapshot-interval`, `--snapshot-keep`; bridge `--snapshot-dir` |
| `block_history`: `window` or `archive` | yes | Application `--block-history` |

Each output directory must meet these conditions:
- It is canonical, owned by the service user and not group or other writable.
- It lies outside `config`, `data`, `abci` and `appdb`, which are the
  protected state tree.
- For snapshots, the mode is exactly 0700 and the directory differs from the
  metrics directory.
- It lies inside a writable root of the rendered unit.

The metrics directory may be group readable, so that an operator agent can
read it.

Two other checks follow the service configuration:
- The mainnet-preparation binding review takes `--service`. It reports
  `service_configuration` as missing unless the file is supplied, and
  `metrics_output` unless the file sets the metrics.
- The E01 inventory lists the service configuration among the required
  production inputs.

## R-b: failure classes

**Classes.** `failure_class` defines eight classes, each with a fixed exit
status:

| Class | configuration | history | supply | replay | release | execution | storage | resource |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Exit status | 10 | 11 | 12 | 13 | 14 | 15 | 16 | 17 |

1 stays unclassified and 101 is a panic.

**Attaching a class.** The class is attached as context that displays the
message it covers, so error text and every downcast are unchanged.
- The first class attached wins.
- A refused resource wins over the attached class: ENOSPC, EDQUOT, ENOMEM,
  EMFILE, ENFILE, EAGAIN, or RocksDB reporting no space.
- An ownership cancellation has no class.

**Classified sites:**
- the configuration, genesis and stored bindings;
- the root receipt, the complete and per-block history checks, and the
  supply checks;
- the emergency, upgrade and handover replays;
- the database open and the commit write;
- the committed-release check and the candidate executable check (the
  latter since R-c).

**Exit status.** The application exits with the class of the last failed
engine-stopping method, which a later successful commit clears, or with the
class of its startup failure. It writes no text.

**Supervisor.** The failure record gains `application_failure_class` (from
the application's exit status) and `failure_class` (from a failed
preflight).

**Check tool.** `dytallix-state-check` runs the startup checks read-only on
a stopped node's database. It prints the first failure in full and exits
with the class's status. The root receipt's authorization and the replays
need the owned root helper, so it lists them as not checked.

## R-c: runbooks

`docs/operations/` holds one runbook per class:
- [halt](../operations/halt.md), the former draft, now with signals and the
  gap 18 stop;
- [fork](../operations/fork.md);
- [supply mismatch](../operations/supply-mismatch.md);
- [key compromise](../operations/key-compromise.md);
- [resource exhaustion](../operations/resource-exhaustion.md);
- [upgrade or handover failure](../operations/upgrade-failure.md).

Oracle failure is recorded as not applicable. The
[index](../operations/README.md) maps each exit class to its runbook, sets
the rules common to every incident and lists the known limits.

Writing the runbooks against the code found three more items (P01,
28 September 2026):

- **Gap 18: no restart on new code after a halt.**
  - A committed block that every validator fails to execute stops the chain,
    because the engine replays it on every restart.
  - Fixed code is a new release, and the application refuses any release
    but the committed one.
  - A handover would change the committed release, but it needs a block.
  - This is next, before gaps 16 and 17.
- **Gap 17, widened to key and operator tooling:**
  - the validator key proof signer;
  - a builder for recovery transactions;
  - an operator socket client.
- **Rejoin, recorded as a limit.** The supervisor refuses state sync, so a
  node whose database is set aside can rejoin only from an archive peer.

## Steps

| Step | Content |
| --- | --- |
| R-a (#292) | Service outputs: metrics (required), snapshots, block history; binding review; E01 required input |
| R-b (#293) | Failure classes: application exit status per class, the class in the supervisor's failure record, a read-only startup check tool |
| R-c | Runbooks for the seven OBS-003 classes; the candidate executable check exits `release`; gaps 17 and 18 recorded |
