# Operations objectives

The operations acceptance specification for mainnet (D12-Q02). P01 approved
it on 3 October 2026 ([operations objectives](../approvals/P01_E05_OPERATIONS_OBJECTIVES_2026-10-03.json)),
and the same day replaced the staffing parts with the solo launch profile
([solo launch](../approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json),
[trust model](../TRUST_MODEL.md)): one operator, the founder, runs the
network. The machine-readable record is [OBJECTIVES.json](OBJECTIVES.json).
These are the targets the recovery and simulation tests (T-suites, T06)
measure. They authorize no host, person or launch.

## Service goals

Published goals, not commitments. Per calendar month, with planned upgrades
excluded:

| Service | Measure | Goal | Budget |
| --- | --- | --- | --- |
| Chain | A block committed in the minute | 99.5% of minutes | About 3.6 hours |
| Client endpoints | At least one endpoint serves state within 3 blocks of the head | 99.0% of minutes | About 7.2 hours |

The independent monitor is a free hosted uptime checker, separate from the
hosts. It checks the public endpoint every few minutes and that the block
height advances, so the per-minute figures are estimates.

## On-call

The founder is paged 24/7 through a free notification service, with no fixed
response time. Severity 1 covers a halted chain, a validator at double-sign
risk, a key compromise and monitoring down.

## Retention

- **Chain history.** Full history on one archive node (the sentry), plus a
  monthly offline export to the founder's own drive. The validator and the
  endpoint keep the approved retained window (E05 `block_history`).
- **Logs.** 90 days, redacted: no secrets or personal data.
- **Metrics.** 13 months.
- **Incident, alert and audit records.** Kept permanently.

## Backups

- The approved daily snapshots (E05 `snapshot_interval_blocks` 17,280, keep 3)
  are copied, encrypted with AES-256, to a second provider.
- Keys live only in the founder's key kits, never in routine backups.
- A validator's signing state is never restored from a backup. A replacement
  signs only after the old host is fenced off.

## Recovery

- **RPO.** Zero committed blocks: a restore never rolls back history. A
  restored node replays committed blocks from its peers or the archive.

| Service | RTO |
| --- | --- |
| Validator | 8 hours: 2 hours to provision plus the approved 6-hour catch-up. The chain pauses meanwhile. |
| Sentry | 4 hours |
| Endpoint service | 4 hours: one endpoint, rebuilt from the published files and state sync |
| Monitoring | 1 hour |

- **Drills.** A restore drill every quarter, recorded with its measured times.
  Before genesis the drills run on the production hosts, which are the
  staging environment until then.

## Still to set

- Alert checks, settings and the chosen free services (artifact A22).
- The hosting provider, the snapshot storage provider and the host records
  (D12-Q03).
- The written recovery procedures: F17 validator recovery is written
  ([validator recovery](../../node/docs/operations/validator-recovery.md));
  F19's design is approved ([disaster recovery v1](../../node/docs/architecture/disaster-recovery-v1.md),
  P01, 7 October 2026: the sentry pushes encrypted copies to a second
  provider, and the validator and the sentry run at different providers);
  its tools and runbook follow.
