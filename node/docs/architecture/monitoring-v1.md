# Monitoring and alerts v1 (G27, G28)

Engineering task E05, launch gates G27 (monitoring) and G28 (alerting).
[Node metrics](metrics-v1.md) made the signals exist. This design collects
them, adds the host's own, and turns them into alerts that reach a person.
Targets and severities are the
[operations objectives](../../../launch/operations/OBJECTIVES.md)'.

## Problem

- **Nothing collects the metrics.** The node writes `dytallix-app.prom` and
  `dytallix-engine.prom` every 15 seconds, but no host runs a collector.
  Inbound traffic is dropped and the Prometheus listener is refused, so
  nothing can scrape the hosts either.
- **Measurements are missing.** Gate G27's list (Day 9) needs these, which
  nothing records today:
  - transaction throughput and failures, gas, fees and rewards;
  - governance and slashing events, and signature verification failures;
  - PQC signing and verification time, and node restarts;
  - the host's CPU, memory, disk and network.
- **Nothing alerts.** No alert condition is evaluated, no one is paged, and a
  silent monitor looks the same as a healthy chain.
- **The independent monitor can't see a halt.** The endpoint's status page
  answers 200 with the last height even when the chain has stopped.

## Decisions (P01, 9 October 2026)

[Approval](../../../launch/approvals/P01_E05_MONITORING_2026-10-09.json):

1. **Push from each host.** A monitor job on each host, run every minute by
   a timer like the sentry's backup job, reads the node's metric files and
   the host's own counters, evaluates the alert rules, and pushes a
   heartbeat and any alert outbound with the host's `curl`. Nothing listens
   and no host is added. This is a deliberate exception to "nothing else
   runs on the host" ([host setup v1](host-setup-v1.md)).
2. **A halt shows on the status page.** The endpoint's status page answers
   503 when the newest block is older than an approved age, so any uptime
   checker detects a halt or a stuck endpoint from the HTTP status alone.
3. **The founder plus one backup.** The founder is the primary on-call. A
   severity 1 alert not acknowledged within 15 minutes also goes to one
   backup person, a root key holder named outside this repository.
4. **Vendor-neutral delivery.** Heartbeats and alerts go to webhook URLs held
   in each host's sealed settings. The founder chooses the free services and
   creates their accounts later (artifact A22).

## Rule

### The monitor job

- **Where.** `dytallix-monitor.timer` and `dytallix-monitor.service` on every
  host, in the host bundle. They run `python3 -I -B monitor.py` from the
  release as their own unprivileged user. It needs read access to
  `/var/lib/dytallix/metrics` (already world-readable), `/proc` and the
  node's unit state.
- **Reads, each minute:**
  - both metric files and their write times;
  - CPU (`/proc/stat`), memory (`/proc/meminfo`), the data disk's free space
    and its growth (`statvfs`), and network bytes (`/proc/net/dev`);
  - `dytallix-node`'s state and restart count (`systemctl show`);
  - on the sentry, the backup job's last upload.
- **State.** `/var/lib/dytallix-monitor` keeps the previous sample for rates,
  each alert's state (firing or resolved, and since when), and the metric
  history (see Retention).
- **Sends**, outbound only, through the host's `curl`. TLS stays in `curl`
  and out of every release binary, as the backup job's upload already does
  (G35):
  - a heartbeat every run to the host's heartbeat URL. The service alerts
    when it stops, which covers a host down, a monitor down and failed
    collection;
  - each alert on a change of state (firing or resolved) to the alert URL,
    as JSON: severity, condition, host, value, threshold and a runbook link.
- **Escalation.** The alerting service holds acknowledgements. Where it can
  escalate, it does so after 15 minutes. Otherwise the job sends a
  severity 1 alert still firing after 15 minutes to the backup URL too.
- **Failed delivery.** The job retries on its next run and counts failures;
  the heartbeat carries the count. A service that stops receiving
  heartbeats alerts on its own.

### The status page

The endpoint's `/status` answers 503, with the same JSON body, when the
engine's newest block time is more than `status_max_head_age_seconds` old:
60 seconds (P01, 10 October 2026,
[approval](../../../launch/approvals/P01_E05_STATUS_HEAD_AGE_2026-10-10.json)),
about 12 missed blocks. An unreadable block time also answers 503, and a
block time ahead of the endpoint's clock counts as fresh. The independent
uptime checker polls it each minute. That one check measures the chain's
and the endpoint's service targets from outside the hosts.

Built in M2: the adapter's `--status-max-head-age-seconds`, the
supervisor's `adapter_status_max_head_age_seconds` (required with the
status page), and the host files' value from `E05_VALUES.json`.

### Measurements (G27, Day 9)

| Measurement | Source |
| --- | --- |
| Block height, production, time; consensus rounds | Engine: height, block interval, rounds; app: height |
| Validator participation, missed blocks, voting power, availability | Engine: validators, missing validators, missed blocks, last signed height |
| Peer count | Engine: `p2p_peers` |
| Mempool size | Engine: mempool size and bytes |
| Transaction throughput and failure rate, gas, fees | App, new: transactions by result, gas used, fees burned |
| Reward distribution, DRT issuance, DGT supply, burns | App: supply buckets; new: rewards paid |
| Delegation activity, slashing events, governance events | App, new: staking, penalty and governance event counters |
| Signature verification failures; PQC signing and verification time | App and engine, new: counters and timings |
| Node restarts | Monitor: `systemctl` restart count |
| CPU, memory, disk, disk growth, network traffic | Monitor: `/proc`, `statvfs` |
| RPC latency and error rate | Monitor on the endpoint: a local timed status request; uptime checker from outside |

### Alerts (G28, Day 9)

Every threshold here is a proposal (E05 values, approved in step M4).

| Alert | Rule (proposed) | Severity | Where |
| --- | --- | --- | --- |
| Consensus halt | App height unchanged for 60 s; status page 503 | 1 | All hosts; uptime checker |
| Validator outage | The validator's heartbeat missing for 3 minutes | 1 | Alerting service |
| Abnormal missed blocks | 5 or more missed in 10 minutes | 2 | Validator |
| Disk exhaustion | Data disk under 15% free (2), under 5% (1) | 2, 1 | All hosts |
| RPC outage | Status page not 200 for 3 minutes; local status request failing | 2 | Uptime checker; endpoint |
| Supply invariant violation | DRT total differs from genesis plus emitted minus burned; DGT issued changes | 1 | Validator, sentry |
| Unexpected mint or burn | Emitted beyond the issuance schedule; burned without fees | 1 | Validator, sentry |
| Abnormal staking event | Staked DGT changes by more than 5% in an hour | 2 | Validator |
| Abnormal governance event | Any governance proposal or execution event | 3 | Validator |
| Repeated signature verification failures | 10 or more in 10 minutes | 2 | All hosts |
| Node restart loop | 3 or more restarts in 15 minutes | 1 | All hosts |
| Stale metrics | A metric file older than 60 s | 2 | All hosts |
| Stale backup | No upload for 26 hours | 2 | Sentry |
| Monitoring down | Any host's heartbeat missing for 3 minutes | 1 | Alerting service |

A divergence between the validator and the sentry stops the sentry, which
the halt and stale-metric alerts catch on the sentry.

### Retention

- **Metrics, 13 months.** Each host's job appends one compact line per
  minute to a daily file and compresses the previous day's. It keeps 13
  months, about 30 MB per host.
- **Logs, 90 days.** journald already keeps 90 days.
- **Alert records, permanently.** Alert records are kept by the alerting
  service, plus the founder's monthly export.
- **Proposed for step M4:** each host also uploads its daily file, encrypted,
  to the backup store, so the founder can draw dashboards on their own
  machine from the history. The hosts are console-only.

## Steps

| Step | Content | Output change |
| --- | --- | --- |
| M1 | This design and the decisions | None |
| M2 | The status page's 503 on a stale head; `status_max_head_age` | Status page |
| M3 | The missing application and engine measurements | New metrics |
| M4 | The monitor job, its timer, sealed webhook settings, alert rules and history; the threshold values | Host bundle |
| M5 | Staging drill in CI: a stand-in alert receiver; a halt, a restart loop, a full disk and a stopped monitor each alert and resolve | Evidence |
| M6 | On the production hosts: the services chosen, each alert exercised, delivery, acknowledgement and escalation recorded (T-phase) | Acceptance evidence |

## What does not change

The node's metric files and their format, the firewall (inbound stays
closed), and every release binary's freedom from TLS (G35).
