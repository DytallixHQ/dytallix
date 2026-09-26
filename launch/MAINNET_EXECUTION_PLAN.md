> Decision status: use [the current decision register](DECISIONS_REQUIRED.md). This technical draft contains historical proposals and implementation statements. The current register supersedes conflicting status statements. Original text is preserved in [the prior records](decision-register/evidence/prior-launch-records/).

# Dytallix mainnet execution plan

Status: NO GO. Planning baseline: 9 September 2026, America/Phoenix.

The objective is one permanent mainnet genesis. The four-week period is a target. It is not evidence that the remaining work fits that period. Batch 7 integrates CometBFT v0.40.0 for local qualification. Production consensus, economic policy and operator assignments remain open. See [Batch 7](batch-7/REPORT.md).

## Entry gate and scope

Complete the Phase 0 assessment before discretionary feature work. The current source assessment covers eight pinned repositories. Batch 1 inspected one existing Hetzner host. Provider-wide inventory, approved mainnet topology, named operators and custody records remain incomplete. See MAINNET_READINESS_REPORT.md.

Required initial scope includes DRT, DGT, staking, delegation, rewards, fees, slashing, governance, treasury controls, wallets, PQC, consensus, recovery, upgrades, and developer interfaces. Do not remove these requirements to meet a date. Keep the selected corrected adaptive emission model. Its integration and qualification remain required.

Propose POST MAINNET status for optional product integrations, experimental aggregation, AI modules, bridges, and nonessential UI changes. Confirm whether any external input is necessary for adaptive emission before deferral. Do not allocate mainnet rewards to an undefined optional service.

## Batch 2 decision package

[Batch 2](batch-2/BATCH_2_REPORT.md) supplies consensus, identity, economic and genesis proposals plus the 14-case repair plan. It does not freeze the protocol or qualify a release. Resolve [the decision index](batch-2/DECISION_REVIEW.md) before dependent runtime integration.

## Engineering schedule

Day numbers below identify engineering work packages. They do not certify elapsed days or completed work. Each of weeks 1–3 has five planned engineering days and two days reserved for correction, review, and gate closure. Do not use week 4 for unfinished engineering.

| Week/day | Work package | Required output and closure | Owner role |
|---|---|---|---|
| 1/1 | Define mainnet v1 | Resolve DECISIONS_REQUIRED.md. Freeze MAINNET_V1_SPEC.md only after source mapping and two independent engineer reviews. | Protocol lead |
| 1/2 | Validator and economic security | Approve consensus, voting weights, operator control, admission, exit, penalties, custody, and fault domains. Complete validator architecture, operations, and security model. | Consensus lead |
| 1/3 | Token model | Apply the approved page allocation shares. Complete DRT/DGT rules, recipient custody, vesting, fees, burns, rewards, and treasury authority. Prove integer custody reconciliation. | Economics lead |
| 1/4 | Cryptographic model | Freeze algorithm identifiers, encoding, authorization, address identity, replay rules, custody, and migration. Qualify known-answer and interoperability vectors. | Cryptography lead |
| 1/5 | Genesis model | Complete deterministic generator, allocation ledger, validator map, ceremony, validation, and manifest. Validate explicit genesis input at startup. | Release lead |
| 2/6 | Reproducible release | Produce an immutable source set, RC1, binaries, node container, checksums, dependency locks, SBOM, dependency report, and build manifest. Compare independent clean builds. | Release lead |
| 2/7 | Deployment | Deploy approved validator and RPC architecture from clean staging servers. Complete deployment, runbook, and recovery documents. | SRE lead |
| 2/8 | Wallet | Qualify creation, encryption, persistence, recovery, signing, network selection, fees, nonce handling, receipts, reconnect, and upgrade compatibility. | Wallet lead |
| 2/9 | Observability | Connect consensus and accounting measurements. Verify alert delivery, acknowledgement, escalation, and independent monitoring of the monitoring service. | SRE lead |
| 2/10 | Backup and recovery | Restore state and validator operation under approved recovery objectives. Verify supply, finalized history, and signer exclusivity after each recovery. | Storage/SRE leads |
| 3/11 | Economic lifecycle | Qualify complete signed transactions, stake changes, rewards, fees, penalties, governance, treasury controls, restart, and full supply reconciliation. | Runtime/QA leads |
| 3/12 | PQC capacity | Measure representative sustained and peak load in isolated staging. Set safe limits from observed saturation and recovery. | Performance lead |
| 3/13 | Economic and governance review | Review concentration, parameter bounds, rounding, custody, and authority. Record invariant checks and independent review. | Economics/security leads |
| 3/14 | Upgrade and security qualification | Qualify a controlled RC1-to-RC2 upgrade. Review affected code and deployment controls. Close all critical findings. | Release/security leads |
| 3/15 | Candidate freeze | Freeze source, binaries, images, parameters, genesis template, automation, documentation, monitoring, and runbooks. All required test suites must pass. | Release lead |

All owner roles are unassigned to named people. Agent assessments do not satisfy independent engineer, operator, security auditor, or custody sign-off requirements.

## Week gates

- Week 1: one unambiguous specification. Two independent engineers reach the same interpretation. Approval includes quantities, units, authority, limits, and upgrade rules.
- Week 2: operators recover required infrastructure failures using written procedures. State integrity and signing safety survive recovery.
- Week 3: the exact candidate passes consensus, PQC, wallet, supply, staking, rewards, fees, governance, upgrade, recovery, load, and security qualification. No unresolved P0 or absolute launch blocker remains.
- Week 4: seven valid simulations pass. No shortened local test earns simulation credit.

## Seven simulation runs

Every run starts from clean, isolated staging infrastructure. Use the frozen production architecture, protocol rules, allocation amounts, validator weights, release artifacts, deployment automation, and operational procedures. Record the exact configuration and all approved rehearsal substitutions.

Use a separate rehearsal chain identity and fresh rehearsal keys. Do not use production signing keys. Record a deterministic mapping from approved allocation recipients to rehearsal accounts. The allowed identity, endpoint, and timestamp substitutions need explicit review. They must not alter consensus, economics, resource limits, or timing rules. Production genesis remains a separate ceremony.

| Run | Required emphasis | Evidence |
|---|---|---|
| 1 | Clean genesis and complete economic startup | Genesis agreement, finalized blocks, wallet transactions, stake, fees, rewards, reconciled supply |
| 2 | Infrastructure loss and restoration | Availability within fault budget, restored RPC/state/validator, preserved supply and stake |
| 3 | Network resilience | Bounded isolated fault scenarios, consensus safety, state agreement, service stability, measured recovery |
| 4 | Production upgrade | Predeclared source-to-target artifacts, migration results, finalized history, balances, lifecycle functions |
| 5 | Operator error | Documented rejection or safe handling of incompatible configuration, artifacts, genesis, snapshots, and signer state |
| 6 | Sustained economic activity | Capacity, saturation, wallet reliability, fees, rewards, supply reconciliation |
| 7 | Final dress rehearsal | T−6h, T−1h, T0, T+15m, T+1h, T+6h, T+24h records and independent sign-off |

Planning assumption: each run includes 24 hours after launch. Seven runs require 168 hours. The final six-hour preflight must overlap run 6 on a separate clean staging fleet if week 4 remains exactly seven days. Staff and approve that overlap before scheduling. Otherwise extend the schedule. Do not reduce the required observation period.

Freeze the permitted upgrade source/target pair before simulation 1. Use the same pair in the upgrade run. If the intended launch artifact changes, document the change and requalify the final artifact. Seven passes spread across incompatible candidate versions do not satisfy the gate.

A launch-blocking failure invalidates the affected evidence. Stop progression, fix the defect, issue a new candidate, document the diff, review the change, and rerun affected tests. Run all seven simulations on the final qualified candidate set before GO.

## Evidence and status rules

Each test record identifies the source revision, binary/image digest, genesis digest, configuration digest, environment, operators, start/end times, command or procedure, result, raw evidence digest, reviewer, and unresolved defects.

At each actual day end, produce DAY_X_STATUS.md with the fields in USER_LAUNCH_REQUIREMENTS.txt. At each actual week end, produce WEEK_X_MAINNET_REPORT.md. Do not create future completion reports in advance. See DAY_0_STATUS.md for this assessment session.

Readiness percentage means formally accepted gates divided by 35. Read current counts from LAUNCH_GATES.json. MAINNET_GATE_MASTER.md is its generated view. OPEN, PARTIAL and READY FOR ACCEPTANCE do not count as PASS. Gate progress and engineering effort remain separate from formal acceptance.

No production deployment, genesis, keys, allocations, release tag, or launch approval was created by this plan. No recurring automation is scheduled.
