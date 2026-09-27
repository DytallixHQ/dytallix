# Node metrics (E04 gap 7)

Status: approved (P01, 27 September 2026: decisions below). Engineering
task E04, gap 7 of the [E04.1 triage](../mainnet/e04-requirement-triage.md) (OBS-002).
Thresholds, alert routing and escalation belong to D12-Q02 and the runbooks
(gap 15); this design only makes the signals exist.

## Problems

| ID | Problem | Where |
| --- | --- | --- |
| M1 | The engine records nothing: the PQC-only build returns CometBFT's no-op metrics and refuses `instrumentation.prometheus`. | `cometbft/upstream/node/metrics_provider_pqc.go` |
| M2 | CometBFT's metric registry cannot enter the engine: `prometheus/client_golang` brings `net/http`, `crypto/tls` and `crypto/x509`, which the PQC graph check forbids (G35). | `scripts/check_consensus_go_graph.py` |
| M3 | The application has no metrics; `metrics.rs` is an unused stub. | `src/metrics.rs` |

What exists: CometBFT's metric structs use go-kit interfaces, and go-kit's
in-memory `metrics/generic` backend brings no network or TLS package. The
application already measures its startup check, block work and supply in
tests.

## Proposed rules

1. **Recording.** The engine fills CometBFT's metric structs with in-memory
   go-kit metrics. The application keeps its own counters and gauges. Both
   write the Prometheus text format themselves; no metrics library with a
   listener is linked.
2. **Export (decision 1).** Each process writes its metrics to a text file,
   `dytallix-engine.prom` and `dytallix-app.prom`, in an operator-chosen
   directory at an interval, replaced by an atomic rename, for an operator
   agent such as the node_exporter textfile collector to read. No listener
   and no query path are added. Each file carries its write time
   (`dytallix_metrics_written_timestamp_seconds`), so a stopped exporter
   shows as stale.
3. **Metric set (decision 2).** A fixed core set, without per-peer or
   per-account labels (the engine's own validator address and the consensus
   step name are the only labels):
   - engine: height, round and step, block interval, validator count and
     voting power, this validator's missed signatures, peers, mempool size
     and bytes, rejected transactions, block sync and state sync progress;
   - application: block execution and commit time, startup check time,
     supply totals (DRT and DGT by custody bucket), retained window start,
     latest snapshot height and write time, admission queue entries, and
     records pruned.
   Every other CometBFT metric stays a no-op.
4. **Settings.** The directory and interval are operator settings with no
   defaults; without them nothing is written.

## Decisions (P01, approved 27 September 2026)

1. Export: text files in a directory, read by an operator agent; no
   listener or query path.
2. Metric set: the core set above, without per-peer or per-account labels.

## Implementation notes

- **Engine.** `internal/metricsfile` implements go-kit's counter, gauge and
  histogram interfaces over one label-aware in-memory registry; histograms
  are written as summaries (sum and count). `Provider` sets the core fields
  of CometBFT's metric structs and leaves every other field a no-op;
  unlabeled core gauges and counters start at zero. `dytallix-pqc-engine
  start ... --metrics-dir DIR --metrics-interval 15s` writes
  `dytallix-engine.prom` (the interval must be 1 s to 1 h). The Go graph
  check still passes: the registry brings no network or TLS package.
- **Application.** `app_metrics` records block execution time, commit time,
  the startup check time, the supply after each commit (both
  denominations, by custody bucket), the retained window start, block
  records pruned and admission queue entries; the latest snapshot comes
  from the snapshot directory at write time. `consensus_stdio ...
  --metrics-dir DIR --metrics-interval-seconds 15` writes
  `dytallix-app.prom`. A recording failure is logged and never affects the
  committed block.
- **Names.** Engine metrics are `dytallix_engine_{subsystem}_{name}`,
  application metrics `dytallix_app_{name}`; supply amounts are exact
  integers in base units.
- **Tests.** Text format, atomic replacement and the provider's core
  mapping (Go); rendering, file replacement and a committed chain's values
  (Rust). The C5 join (`state-sync-join`) runs node 0 with both files and
  requires positive engine height, peers and validators, application
  height, commits, latest snapshot and DRT total, with write times under a
  minute old.

## Steps

| Step | Content |
| --- | --- |
| M-a | Engine: label-aware in-memory go-kit metrics for the core set, text-format file writer |
| M-b | Application: counters and gauges at the measured points, text-format file writer |
| M-c | Tests: every core metric present and changing on a running chain (the C5 harness), staleness, G35 graph check unchanged |
