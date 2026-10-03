# Operations objectives

The operations acceptance specification for mainnet (D12-Q02), approved by P01
on 3 October 2026 ([approval](../approvals/P01_E05_OPERATIONS_OBJECTIVES_2026-10-03.json)).
The machine-readable record is [OBJECTIVES.json](OBJECTIVES.json). These are
the targets the recovery and simulation tests (T-suites, T06) measure. They
authorize no host, person or launch.

## Service targets

Per calendar month, measured by the independent monitor, with planned upgrades
excluded:

| Service | Measure | Target | Budget |
| --- | --- | --- | --- |
| Chain | A block committed in the minute | 99.9% of minutes | About 43 minutes |
| Client endpoints | At least one endpoint serves state within 3 blocks of the head | 99.5% of minutes | About 3.6 hours |

## On-call

24/7 coverage, with at least two people in the rotation and an escalation
contact.

| Severity | Examples | Response |
| --- | --- | --- |
| 1 | Chain halted, a validator at double-sign risk, a key compromise, monitoring down | Paged 24/7: acknowledged within 15 minutes, a responder at a console within 30 minutes |
| 2 | Set with the alert routes (A22) | Within 4 hours |
| 3 | Set with the alert routes (A22) | The next business day |

## Retention

- **Chain history.** Full history on at least two archive nodes, run by
  different operators on different providers, plus a monthly offline export.
  Validators, sentries and endpoints keep the approved retained window
  (E05 `block_history`).
- **Logs.** 90 days, redacted: no secrets or personal data.
- **Metrics.** 13 months.
- **Incident, alert and audit records.** Kept permanently.

## Backups

- The approved daily snapshots (E05 `snapshot_interval_blocks` 17,280, keep 3)
  are copied, encrypted with AES-256, to a second provider.
- Keys move only through custody, never in routine backups.
- A validator's signing state is never restored from a backup. A replacement
  signs only after the old host is fenced off.

## Recovery

- **RPO.** Zero committed blocks: a restore never rolls back history. A
  restored node replays committed blocks from its peers or the archives.

| Service | RTO |
| --- | --- |
| Validator | 8 hours: 2 hours to provision plus the approved 6-hour catch-up |
| Sentry | 4 hours |
| Endpoint service | 15 minutes, by failover to another endpoint |
| Monitoring | 1 hour |

- **Drills.** A restore drill every quarter on staging, recorded with its
  measured times.

## Still to set

- Alert signals, thresholds and routes (artifact A22).
- Named on-call people, contacts and the escalation contact (D14-Q03).
- Archive node operators and providers, and the backup provider (D12-Q03).
- The written recovery procedures (F17 validator recovery, F19 disaster
  recovery).
