# Dytallix mainnet execution plan

Status: NO GO. This plan holds the launch requirements the gates cite: the scope, the stages a candidate passes (the week gates), the seven simulation runs and the evidence rules. The current order of work is the [genesis framework](GENESIS_FRAMEWORK.md#6-order-of-work), and readiness is the [gate list](MAINNET_GATE_MASTER.md). The original plan of 9 September 2026, with its day-by-day schedule, is in this file's history. Updated 10 October 2026 ([launch records](approvals/P01_E05_LAUNCH_RECORDS_2026-10-10.json)).

The objective is one permanent mainnet genesis.

## Entry gate and scope

Required initial scope includes DRT, DGT, staking, delegation, rewards, fees, slashing, governance, treasury controls, wallets, PQC, consensus, recovery, upgrades, and developer interfaces. Do not remove these requirements to meet a date. The selected corrected adaptive emission model is calibrated and integrated; its qualification remains required.

Mainnet v1 launches with the current build only (D07-Q01, P01, 29 September 2026): ordinary transfers and staking, governance, recovery, emergency controls, root-signed upgrades, state sync, the client channel and the local gateway. A contract runtime, bridges, the Airlock, a liquidity bootstrapping pool, wrapped USDC, external oracles, gRPC and treasury spending are POST MAINNET. No mainnet rewards go to an optional service.

## Decisions

Every policy question and required record is in the [decision register](DECISIONS_REQUIRED.md), with the dated P01 approvals in [approvals/](approvals/). Production values and records are in the [E05 intake](E05_INTAKE.md).

## Engineering tasks

| Task | Content | State and record |
|---|---|---|
| E01 | PQC source boundary | Done ([E01 source boundary](../node/docs/mainnet/e01-source-boundary.md)) |
| E02 | Helper ownership under AppArmor | Done ([E02 helper ownership](../node/docs/mainnet/e02-helper-ownership.md)) |
| E03 | Negative-peer runner | Method ready; the runs are on the staging hosts ([E03 method](../node/docs/mainnet/e03-negative-peer-method.md)) |
| E04 | Protocol and economic gaps | All 20 gaps closed ([E04 triage](../node/docs/mainnet/e04-requirement-triage.md)) |
| E05 | Genesis from approved inputs | Built; every value approved, the launch records open ([E05 intake](E05_INTAKE.md), [genesis builder](../node/docs/mainnet/e05-genesis-builder.md)) |
| E06 | Reproducible release | Built; the candidate is frozen after the staging runs, then audited by Hashlock ([release](../release/README.md)) |

**Solo launch profile** (P01, 3 October 2026, [approval](approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json), [trust model](TRUST_MODEL.md)): the founder holds every owner role, except that five people hold the root key kits, one each, since 7 October 2026 ([approval](approvals/P01_E05_ROOT_KEY_HOLDERS_2026-10-07.json)). Where this plan asks for independent engineers, operators, auditors or custodians, the `acceptance_profile` in [LAUNCH_GATES.json](LAUNCH_GATES.json) defines what satisfies it: Hashlock's independent audit of the frozen candidate, a 30-day public review with a DGT bug bounty, a separate review labeled as AI and the founder's sign-off, with the launch labeled unaudited until Hashlock's report is published ([audit](approvals/P01_E06_AUDIT_2026-10-10.json)). An AI assessment is always labeled as AI and never presented as an independent human review.

## Stage gates

The stages a candidate passes, in order; each starts when the one before it is done (task-driven, [simulation plan approval](approvals/P01_T06_SIMULATION_PLAN_2026-10-11.json)).

- Stage 1: one unambiguous specification. A review labeled as AI checks it for a single interpretation, and the public review of the frozen candidate reopens it (solo launch profile). Approval includes quantities, units, authority, limits, and upgrade rules.
- Stage 2: the operator (the founder) recovers required infrastructure failures using written procedures. State integrity and signing safety survive recovery.
- Stage 3: the exact candidate passes consensus, PQC, wallet, supply, staking, rewards, fees, governance, upgrade, recovery, load, and security qualification. No unresolved P0 or absolute launch blocker remains.
- Stage 4: seven valid simulations pass. No shortened local test earns simulation credit.

## Seven simulation runs

The runs happen after the audit's fixes, on the final candidate ([audit](approvals/P01_E06_AUDIT_2026-10-10.json)). The [simulation plan](../node/docs/mainnet/launch-rehearsals.md) sets each run's procedure, the tools and the approved pass criteria. Every run starts from clean, isolated staging infrastructure. Under the solo launch profile, that is the three production hosts before genesis, rebuilt clean for each run and wiped before the production ceremony. Use the frozen production architecture, protocol rules, allocation amounts, validator weights, release artifacts, deployment automation, and operational procedures. Record the exact configuration and all approved rehearsal substitutions.

Use a separate rehearsal chain identity and fresh rehearsal keys. Do not use production signing keys. Record a deterministic mapping from approved allocation recipients to rehearsal accounts. The allowed identity, endpoint, and timestamp substitutions need explicit review. They must not alter consensus, economics, resource limits, or timing rules, except the recorded set of shortened timings in runs 1 to 6, so each lifecycle completes inside its run; run 7 uses the exact production values. Production genesis remains a separate ceremony.

| Run | Required emphasis | Evidence |
|---|---|---|
| 1 | Clean genesis and complete economic startup | Genesis agreement, finalized blocks, wallet transactions, stake, fees, rewards, reconciled supply |
| 2 | Infrastructure loss and restoration | Availability within fault budget, restored RPC/state/validator, preserved supply and stake |
| 3 | Network resilience | Bounded isolated fault scenarios, consensus safety, state agreement, service stability, measured recovery |
| 4 | Production upgrade | Predeclared source-to-target artifacts, migration results, finalized history, balances, lifecycle functions |
| 5 | Operator error | Documented rejection or safe handling of incompatible configuration, artifacts, genesis, snapshots, and signer state |
| 6 | Sustained economic activity | Capacity, saturation, wallet reliability, fees, rewards, supply reconciliation |
| 7 | Final dress rehearsal | T−6h, T−1h, T0, T+15m, T+1h, T+6h, T+24h records and sign-off under the acceptance profile |

Planning assumption: each run includes 24 hours after launch. Seven runs require 168 hours. With one staging fleet (the production hosts, solo launch profile), the runs are sequential: extend the schedule rather than overlap them. Do not reduce the required observation period.

Freeze the permitted upgrade source/target pair before simulation 1. Use the same pair in the upgrade run. If the intended launch artifact changes, document the change and requalify the final artifact. Seven passes spread across incompatible candidate versions do not satisfy the gate.

A launch-blocking failure invalidates the affected evidence. Stop progression, fix the defect, issue a new candidate, document the diff, review the change, and rerun affected tests. Run all seven simulations on the final qualified candidate set before GO.

## Evidence and status rules

Each test record identifies the source revision, release manifest and binary digests, genesis digest, configuration digest, environment, operators, start/end times, command or procedure, result, raw evidence digest, reviewer, and unresolved defects.

Since Day 0 ([DAY_0_STATUS.md](DAY_0_STATUS.md), historical), progress has been recorded in the dated approval records, the decision register and the gate list rather than in daily or weekly status files. Do not create future completion reports in advance.

Readiness percentage means formally accepted gates divided by 35. Read current counts from LAUNCH_GATES.json. MAINNET_GATE_MASTER.md is its generated view. OPEN, PARTIAL and READY FOR ACCEPTANCE do not count as PASS. Gate progress and engineering effort remain separate from formal acceptance.

This plan creates no production deployment, genesis, keys, allocations, release tag or launch approval.
