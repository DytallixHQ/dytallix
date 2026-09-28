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

## Steps

| Step | Content |
| --- | --- |
| R-a | Service outputs: metrics (required), snapshots, block history; binding review; E01 required input |
| R-b | Failure classes: application exit status per class, the class in the supervisor's failure record, a read-only startup check tool |
| R-c | Procedures for the seven OBS-003 classes, written against R-a and R-b |
