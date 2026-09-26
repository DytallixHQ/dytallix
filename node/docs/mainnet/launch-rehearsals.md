# Mainnet launch rehearsals

Status: required before launch. The user requires seven full launch simulations in Week 4.
Authority: `USER_LAUNCH_REQUIREMENTS.txt` in the launch assessment package.
No launch rehearsal has passed yet. Component checks do not qualify a launch.

Use this register with `launch-rehearsals.json`. Keep every required scenario visible.
Record a missing implementation as blocked. Record an available but unexecuted check
as not run. Never count either state as a pass.

## Execution order

1. Run local reference and component checks during implementation.
2. Freeze the protocol rules and integrate consensus, account authorization, and economics.
3. Build the distributed rehearsal harness against the selected release binary.
4. Run isolated launch simulations with synthetic balances and disposable signing keys.
5. Complete seven fresh full launch runs in Week 4 with the exact production architecture and procedures.
6. Complete all operating periods, including the final T-6h through T+24h checkpoints.
7. Reconcile the evidence and obtain the launch decision.

Use distinct rehearsal chain IDs, network identifiers, ports, and storage paths.
Generate disposable keys for each fresh launch. Never copy production secrets into a
rehearsal. Preserve production protocol rules, timing, allocation logic, and workload
structure. Record all differences from the proposed production configuration.

## Required scenario register

Each row is mandatory. Detailed cases, fixtures, and drivers must be added before its
first qualifying run. A plan entry is not an implemented test.

| ID | Scenario | Acceptance evidence | Current dependency | Owner role |
|---|---|---|---|---|
| L01 | Local reference checks | Exact adaptive and address vectors match; module boundaries pass | Runnable in current worktree | Runtime |
| L02 | Local component tests | Adaptive, address, and persistent record suites execute with zero failures | Runnable in current worktree | Runtime |
| L03 | Release baseline | All required workspace, feature, platform, client, crypto vector, and interoperability checks pass on exact candidate; binaries match provenance | Release failures and candidate definition | Release |
| L04 | Genesis ceremony | Independent operators reproduce genesis digest, approved allocations, stake, and initial state; changed inputs fail startup | Approved recipient/custody/vesting records, DRT initial supply, identity, and validator specification | Protocol and operations |
| L05 | Cold network launch | Empty nodes start from the same bundle and agree on first finalized checkpoint; late starts synchronize | Distributed consensus and peer synchronization | Consensus |
| L06 | Client transactions | SDK and wallet submit valid transfers through public interfaces; balances, fees, nonces, and receipts match an independent reference | Client format and account authorization | Client and runtime |
| L07 | Account key lifecycle | Creation, authorized rotation, recovery, and restart preserve account identity and balances; obsolete keys cannot authorize later writes | Atomic account authorization and rotation | Identity |
| L08 | Adaptive economics | Selected corrected adaptive controller, approved 40/30/30 reward allocation, issuance, fees, burns, and supply reconcile at every transition; reference vectors cover boundaries and unavailable observations | D01 specification and qualification, site curve reconciliation, approved parameters, and atomic economic integration | Tokenomics |
| L09 | Validator lifecycle | Delegation, rewards, residuals, unbonding, validator changes, and specified slashing preserve accounting and finality rules | Approved stake lifecycle | Consensus and runtime |
| L10 | Contract and governance lifecycle | Required initial governance and treasury controls pass proposal, vote, outcome execution, authorization, persistence, and upgrade checks; every enabled contract module passes its lifecycle checks | Required initial governance implementation and approved authority/parameter rules; approved optional contract boundary | Runtime |
| L11 | Node outage and rejoin | Planned process and host loss preserve finalized history; restarted nodes catch up within approved recovery target | Distributed harness and recovery target | Operations |
| L12 | Network interruption and restoration | Controlled isolation and delay preserve safety; normal progress resumes under the approved synchrony and stake assumptions | Consensus fault model and isolated network harness | Consensus |
| L13 | Consensus fault-model qualification | Independent review and bounded model tests cover the approved faulty-validator assumptions; no conflicting finalization within the model | Final consensus specification and independent review | Consensus and assurance |
| L14 | Storage recovery | Controlled interruption at commit boundaries, full storage, backup restoration, and snapshot synchronization preserve finalized state | Distributed persistence integration and recovery procedures | Storage and operations |
| L15 | Upgrade and abort | Supported mixed-version period and activation preserve state; pre-activation abort succeeds; post-finality recovery follows the approved protocol | Version compatibility and recovery rules | Release and consensus |
| L16 | Capacity and sustained load | Specified workload meets approved finality and throughput targets; queue, disk, memory, and CPU stay within declared limits | Distributed candidate and launch demand forecast | Performance |
| L17 | Operator response | Monitoring detects injected service failures; operators execute restore, disposable-key rotation, and emergency update procedures within targets | Independent operators, custody, alerts, and runbooks | Operations |
| L18 | Repeated full launch | Seven fresh full launch runs R01-R07 pass in Week 4 using the production architecture, token model, validator model, genesis, deployment, upgrade procedures, and runbooks | L03-L17 integrated qualification and mandatory seven-run harness | QA and operations |
| L19 | Frozen-candidate qualification | Recorded frozen candidate passes all seven full-run operating periods, including R07 checkpoints from T-6h through T+24h; no unapproved configuration shortcuts | R01-R07 evidence and approved workload, finality, recovery, and operating-period criteria | QA |
| L20 | Final launch decision | All mandatory evidence passes; independent mathematical and security reviews close; release and genesis digests agree; named authorities sign | All earlier scenarios and G0-G8 gate evidence | Protocol, security, and operations |

L13 specifies assurance requirements only. It does not provide an exploit driver.
Simulation cannot prove safety outside the stated model or economic market stability.
Continue the partial Codex Security coverage and independent review as separate work.

## Seven required full launch runs

Weeks 1 through 3 cover engineering, protocol freeze, and release qualification.
Week 4 consists of these seven runs. Start each run from clean infrastructure and fresh
genesis. Use the exact production architecture, token model, validator model, genesis
process, deployment, upgrade procedures, and runbooks. Use isolated rehearsal identities
and disposable keys. Record each substitution. Unapproved functional shortcuts invalidate
qualification. All required initial features remain in scope, including governance,
treasury controls, staking, delegation, undelegation, slashing, rewards, and both tokens.

The JSON `full_launch_runs` register retains every simulation instruction from the user
brief. Its scenario mappings identify the focused checks. Each run also includes the
common initial scope. L01 and L02 are local prerequisites. They are not full launch runs.
L18 requires seven successful runs. L19 checks the frozen candidate and operating periods.
L20 records the final decision. No full run has passed.

| Run | Context | Focused scenario mapping |
|---|---|---|
| R01 | Clean Genesis | L04, L05, L06, L08, L09, L17 |
| R02 | Infrastructure Failure | L11, L12, L14, L17 |
| R03 | Adversarial Network | L06, L12, L13, L16 |
| R04 | Production Upgrade | L10, L15 |
| R05 | Operator Error | L03, L04, L05, L07, L11, L14, L15, L17 |
| R06 | Production Load And Economic Activity | L06, L07, L08, L09, L10, L16 |
| R07 | Final Mainnet Dress Rehearsal | L03, L04, L05, L06, L07, L08, L09, L10, L11, L12, L13, L14, L15, L16, L17, L19, L20 |

R01 covers the complete clean launch and initial economic lifecycle. R02 covers validator,
RPC, node, snapshot, peer, and connectivity restoration. R03 specifies isolated fault and
invalid-input coverage. It does not implement an exploit driver. R04 covers the production
upgrade and post-upgrade state, governance, wallet, and token checks. R05 covers wrong
configuration, binaries, networks, genesis, keys, snapshots, fees, peers, and versions.
R06 covers sustained production load, economic activity, governance, and resource limits.
R07 repeats the exact production launch procedure with these required checkpoints:

| Checkpoint | Required checks |
|---|---|
| T MINUS 6 HOURS | Verify binaries. Verify checksums. Verify release commit. Verify genesis. Verify all genesis allocations. Verify DRT supply. Verify DGT supply. Verify validators. Verify validator keys. Verify infrastructure. Verify networking. Verify DNS. Verify RPC. Verify monitoring. Verify alerts. Verify backups. Verify recovery procedures. |
| T MINUS 1 HOUR | Confirm validator coordination. Confirm genesis hash. Confirm binary hash. Confirm peers. Confirm system clocks. Confirm validator keys. Confirm configuration. Confirm token allocations. Confirm network parameters. |
| T ZERO | Start validators. Confirm consensus. Confirm block production. Verify expected validator participation. |
| T PLUS 15 MINUTES | Verify RPC. Verify wallet connectivity. Execute canonical PQC transaction. Execute DRT transaction. Execute DGT transaction. Create delegation. Verify fee accounting. Verify finalization. |
| T PLUS 1 HOUR | Verify state consistency across validators. Verify wallets. Verify staking. Verify rewards. Verify token supply. Verify monitoring. Verify RPC infrastructure. |
| T PLUS 6 HOURS | Review telemetry. Review logs. Review validator participation. Review token accounting. Review transaction statistics. Review security alerts. Review staking activity. |
| T PLUS 24 HOURS | Perform full supply reconciliation. Perform full validator review. Perform final state consistency review. |

Complete `FINAL_MAINNET_SIMULATION_REPORT.md` after the final reconciliation.
Use actual elapsed time for R07. Resolve calendar placement before scheduling its
30-hour T-6h through T+24h span. Do not compress these checkpoints to fit one date.
Define the other full operating periods before execution. The brief specifies no
30-day mandatory soak. The earlier three-run and 30-day proposals are superseded.

## Approved allocations and open economic decisions

The user identified the tokenomics page as the approved allocation source.
DGT totals 1,000,000,000 tokens. The shares are ecosystem growth 30%, team and advisors
20%, public sale 15%, private sale 15%, and reserve 20%. DRT reward shares are validators
40%, stakers 30%, and treasury 30%. These replace conflicting testnet allocations.
Recipient accounts, custody, vesting, locks, initial stake, and initial DRT remain open.

Keep D01: the selected corrected adaptive controller. The site's piecewise-linear
emission curve differs from D01. Allocation approval does not replace the controller.
Resolve this discrepancy before protocol freeze. These document changes do not change
runtime values or activate the controller.

## Unapproved quantitative proposals

Four independent operators, three failure domains, two restore/upgrade exercises,
capacity at twice forecast peak, and 99.9% normal-load finality remain proposals.
They are not approved launch thresholds. Define transaction mix, payload sizes,
stake distribution, hardware, network conditions, peak forecast, finality deadline,
recovery deadline, and acceptable data loss before qualification. Approve operator
and performance criteria. Zero conflicting finalized checkpoints and zero unexplained
state or accounting differences remain mandatory integrity conditions.

## Evidence for every run

Record the scenario ID, run ID, start and end time, status, owner, and reviewer.
Record repository commits, working changes, toolchain, dependency locks, build flags,
artifact digests, protocol version, genesis digest, configuration digest, and random seed.
Record node identities, operator ownership, stake weights, hardware, failure domains,
workload, injected service failures, and timing. Store public identities only.

Keep per-node finalized checkpoints and state roots, supply reconciliations, client
receipts, resource metrics, event logs, expected results, actual results, and assertions.
Attach an independent reference result where applicable. Hash the evidence files.
Do not infer success from process exit alone for a distributed rehearsal.

Stop a run on conflicting finality or unexplained accounting divergence. Preserve
the evidence. Resolve the cause, then rerun affected scenarios and their dependencies.
Changes to consensus, economics, cryptography, or persistence invalidate affected
candidate results and restart the affected qualification window. Record the decision.
Never use a testnet database reset as a mainnet recovery procedure.

## Current runner

`scripts/run_launch_rehearsals.py` runs local prerequisites only. It does not launch
a network. It records blocked distributed scenarios in the same report. Its process
exit code describes selected local checks, not mainnet readiness. The report always
sets `mainnet_ready` to false. This tool cannot grant launch approval.

From the node repository, use Python 3.11 or later:

```sh
python3 scripts/run_launch_rehearsals.py --profile components --output /absolute/path/outside/repository/new-run
```

Use `--profile preflight` for reference and module checks only. Use a new output
directory per run. Cargo runs with the dependency lock and offline mode. Missing
cached dependencies cause a failed prerequisite. The runner hashes tracked and
nonignored untracked files before and after execution. Source changes invalidate
the run. Ignored build caches and external system libraries are outside that digest.

A local prerequisite run is evidence for its recorded working tree. It is not release evidence.
The address patch remains unfinished. The SDK still needs the same address format.
L03-L20 need implementation, decisions, operator work, or independent approval.
