# Mainnet launch simulations

Status: planned; no run has passed. Seven full launch simulations (R01 to R07) must pass on the final candidate before genesis (T06). Component checks do not qualify a launch. The decisions are in the [simulation plan approval](../../../launch/approvals/P01_T06_SIMULATION_PLAN_2026-10-11.json); the requirements are the launch brief's, kept verbatim in [launch-rehearsals.json](launch-rehearsals.json) (`full_launch_runs`), and the run definitions in the [execution plan](../../../launch/MAINNET_EXECUTION_PLAN.md#seven-simulation-runs).

## Where and when

- **Where.** The two production hosts (the validator, and the sentry, which also serves clients; [two seats](../../../launch/approvals/P01_E05_TWO_SEATS_2026-10-11.json)), before genesis, under the solo launch profile. Every tool is first built and proven in the CI staging network ([host install workflow](../../../.github/workflows/host-install.yml)).
- **When.** Task-driven: after Hashlock's audit fixes, on the final candidate, one run after another ([audit approval](../../../launch/approvals/P01_E06_AUDIT_2026-10-10.json), [order of work](../../../launch/GENESIS_FRAMEWORK.md#6-order-of-work)). The staging runs before the freeze (E03, the T suites, the capacity measurements, the kit drills) come first and find defects early.
- **Length.** Each run operates for 24 hours after its genesis; R07 spans T−6h to T+24h. No checkpoint is compressed.
- **Before the runs.** During staging, a [statistical campaign](#statistical-campaign-staging) of about 15 to 21 days of host time measures attacks, faults, performance and endurance with confidence.

## Every run

1. **Clean hosts.** Each host is rebuilt from the provider's Ubuntu 24.04 image and installed from its sealed rehearsal bundle (`install.sh`).
2. **Rehearsal identity.** A fresh rehearsal chain ID per run (never one naming mainnet), fresh throwaway node keys, and a rehearsal root authority, three of five, from throwaway kits; never production keys.
3. **Spendable rehearsal genesis.** The production allocation (buckets, treasury, holder grants, validator self-bond, DRT bootstrap) mapped deterministically to rehearsal accounts whose keys the harness holds, plus wallet accounts for the load. The mapping is recorded.
4. **Values.** Every production value, except R01 to R06's recorded timing set (below).
5. **Services.** The real alerting and uptime services, the backup bucket, the staging agent on each host, and, for R03 and R06, the load machines.
6. **Evidence.** Recorded as in [Evidence](#evidence) and hashed.

### Shortened timings (R01 to R06 only)

So that each lifecycle completes inside a 24-hour run, these production values are shortened and listed, with their production values, in every run record ([approval](../../../launch/approvals/P01_T06_REHEARSAL_TIMINGS_2026-10-12.json), P01, 12 October 2026). The set is the `rehearsal` profile in [TIMINGS.json](../../../launch/simulations/TIMINGS.json); `staging_chain.py --timings rehearsal` builds a chain with it, and the CI install job does so on every change.

| Value | Production | Rehearsal |
|---|---|---|
| Upgrade and handover notice (also the kit replacement notice) | 120,960 blocks | 720 blocks (1 hour) |
| Governance deposit and voting periods | 120,960 blocks each | 720 blocks each |
| Governance timelock | 34,560 blocks | 360 blocks |
| Evidence age and margin (unbond maturity) | 241,920 + 720 blocks; 14 days + 1 hour | 2,880 + 120 blocks; 4 hours + 10 minutes |
| Snapshot interval | 17,280 blocks | 720 blocks |
| State sync trust period | 168 hours | 1 hour |

The tools derive and check the rest before building: the trust period stays below the evidence age; a restore trusts a copy for the evidence age less a fourteenth (about 3 hours 43 minutes), longer than the newest backup copy can be (a snapshot interval plus the hourly backup and its 15-minute delay, about 2 hours 15 minutes); block and second ages agree at 5-second blocks; and unbonding (about 4 hours 10 minutes), a full governance cycle (2 hours 30 minutes) and the notice fit in the run. A restore's trust counts from the copy's header, so in R01 to R06 a lost host must be restored within about 88 minutes of the loss in the worst case (production: about 12 days); R02's restore runs inside that, and its 4-hour rebuild target is checked at production timings (the CI restore drill and the campaign's heavy faults).

R07 uses the exact production values.

### The staging agent

The production hosts are console-only. While they serve as staging, each runs a small agent:

- It polls the backup bucket over the host's `curl`, outbound only, for command files.
- It runs a command only if the founder's ML-DSA-65 staging command key signed it and the command is on a fixed allowlist: the run steps below, such as start, stop, fault injection and status capture. Key custody, encryption, store layout and the allowlist are approved ([staging agent approval](../../../launch/approvals/P01_T06_STAGING_AGENT_2026-10-14.json)).
- It uploads each result encrypted, like the metric history. TLS stays transport only.
- It is never part of a release, and the wipe before the production install removes it.

The design, its threat model and the choices still open for P01 are in [staging agent v1](../architecture/staging-agent-v1.md).

### The load machines

Four to eight small rented machines, outside the hosts' providers, each with its own address: Vultr's smallest instances across regions, rented by the hour for R03, R06 and the campaign's flood runs only and destroyed after each use ([simulation inputs approval](../../../launch/approvals/P01_T06_SIMULATION_INPUTS_2026-10-14.json)). They reach the chain only through the sentry's public client channel, within its production limits (16 connections, 2 per address), and hold only rehearsal keys.

## Pass criteria

Approved thresholds (D14-Q02):

| Area | Criterion |
|---|---|
| **Integrity** (every run; a failure stops the run) | Zero conflicting finalized blocks. The same app hash on every node at every checkpoint. Supply reconciles exactly: DRT genesis plus emitted minus burned, DGT unchanged, and the account totals equal the supply counters. No committed block lost in any recovery. |
| **Liveness** (outside injected faults) | Median block interval at most 6 s; 99th percentile at most 10 s. |
| **Inclusion** | At half capacity, 99% of valid transactions committed within 2 blocks. At full capacity, no valid transaction lost: each is committed or refused at admission. |
| **Recovery** | Blocks again within 5 minutes of a validator process restart; the sentry caught up within 2 minutes of a network heal; a destroyed sentry rebuilt within 4 hours; a lost validator host within 8 hours. Every injected fault raises its alert and resolves after recovery. |
| **Load** (R06) | Full capacity, about 180 basic Sends a block, for 1 hour; half capacity for 6 hours; across 1,000 wallets. |

A missed threshold fails the run. A failure is fixed in a new candidate, with a documented diff, and the affected runs are rerun on it; all seven must pass on the final candidate.

## One validator

The launch has one validator, so some of the brief's network scenarios mean something specific:

- **Stopping the validator halts the chain.** Every host raises a halt alert. A restart resumes it with no procedure, since the double-sign startup check is 0 while there is one validator. The unit does not restart itself, so a crashed validator needs the console (or the agent, in staging).
- **A validator partition does not halt it.** Holding all the power, the validator keeps committing alone. The sentry stalls, and the status page answers 503 after 60 seconds. On reconnect it redials within 60 seconds and catches up.
- **Latency and packet loss** change propagation and how fresh the sentry's view for clients is, not consensus.
- **A second copy of the validator key** is the catastrophic case. A duplicate vote removes the validator for good and ends the chain, and an isolated copy would finalize its own fork. Nothing automatic stops it: today only the fence-first rule ([validator recovery](../operations/validator-recovery.md)) and the pinned peer addresses stand in the way. R05 checks that a second copy at another address cannot reach the network, and the residual risk is recorded.

## Statistical campaign (staging)

The seven runs are pass/fail qualification. Before them, during staging and before the freeze, the production hosts run a longer campaign that measures behavior with statistical confidence ([simulation plan approval](../../../launch/approvals/P01_T06_SIMULATION_PLAN_2026-10-11.json)). It uses rehearsal identities, the tools below, and its results go to Hashlock with the frozen candidate.

| Part | Rule | Host time |
|---|---|---|
| **Attacks on core functions** | Each attack type is attempted at least 3,000 times with zero successes. By the rule of three, that bounds its success rate below 1 in 1,000 at 95% confidence. Connection- and network-level attacks are repeated about 30 times each. | About 2 to 4 days in all |
| **Faults and recovery** | Each quick fault (process kill, peer drop, partition, latency, sentry restart) about 30 times; each heavy fault (host rebuild, snapshot restore, validator recovery) 3 to 5 times; recovery times recorded as distributions | About 3 to 5 days in all |
| **Performance** | Block intervals, inclusion delay and response times: a day gives about 17,280 blocks, enough for the median, p95 and p99; p99.9 comes from the whole endurance run | Within the endurance run |
| **Endurance** | At least 15 days continuous at the exact production values, with background load and periodic half-capacity periods: full unbonding (14 days), governance deposit and voting (7 + 7 days), a release handover after its 7-day notice, at least 15 epochs and snapshots, and state and memory growth | At least 15 days; the attack and fault campaigns interleave inside it |

The attack types cover every core function:

- **Transactions:** invalid, wrong-key and truncated ML-DSA-65 signatures; malformed, non-canonical and oversized envelopes; the wrong chain or network; expired transactions; stale, future and conflicting nonces; duplicate bytes and re-signed copies of a pending intent; fee caps below the minimum; spending more than the eligible balance; too many actions.
- **Staking:** bonds below the minimum or to an unknown validator; unbonding more than is bonded; early withdrawal; repeated reward claims; validator registration with a bad possession proof or a reused key.
- **Governance:** proposals below the deposit; votes without a bond, repeated or after the period; replayed execution; parameter changes outside the genesis bounds.
- **Root controls:** too few signatures; keys from the wrong epoch; a replayed sequence; an expired window; non-canonical encoding; a kit replacement while frozen.
- **Recovery:** replayed and wrong-key recovery transactions.
- **Endpoint and RPC:** oversized bodies, header and connection floods beyond the limits, slow clients, invalid handshakes, a wrong pin, methods outside the allowlist.
- **Network:** latency, packet loss, partitions and peer instability; wrong-key and malformed peers come from the E03 runner.

Any successful attack is a defect, fixed before the freeze. Simulation cannot cover events rarer than about once a day with confidence; the audit and the public review cover those. Each attack listed above is one type: at least 3,000 attempts per type for transactions, staking, governance, root controls and recovery, and about 30 per endpoint, RPC and network type, each with zero successes ([simulation inputs approval](../../../launch/approvals/P01_T06_SIMULATION_INPUTS_2026-10-14.json)).

## The runs

Each run starts from [Every run](#every-run) and operates for its full period. The brief's full list of required actions is in [launch-rehearsals.json](launch-rehearsals.json).

| Run | What it does | Tools it needs |
|---|---|---|
| **R01 Clean genesis** | The full launch with rehearsal identities: genesis built and verified, bundles installed, validator and sentry started; wallets connect, accounts created and funded, DGT bonded to the validator, DRT and DGT transfers, fees burned, rewards accrued and claimed; supply reconciled at each checkpoint | T1, T3, T6, T7 |
| **R02 Infrastructure failure** | Validator process killed and restarted (the chain halts and resumes); the sentry stopped and restored (clients cut off, then back); peer connections dropped and redialed; sentry destroyed and restored from its snapshot; network cut and restored; the validator host lost and recovered by the fence-first procedure. Each fault alerts and resolves, and continuity, state, supply, staking, rewards and wallets are checked after each | T2, T5, T6, T9 |
| **R03 Adversarial network** | Latency, packet loss and a validator partition at the host; malformed and non-canonical transactions, invalid ML-DSA-65 signatures, bad nonces, duplicates and re-signed copies, oversized requests, invalid serialization, request and transaction floods from the load machines; peer instability. Consensus, state, signature checks, the queue, RPC stability, recovery time and accounting are measured | T2, T3, T4, T5, T6 |
| **R04 Production upgrade** | Normal operation with transactions, staking and rewards; then a real release handover and state upgrade from RC1 to RC2: prepared, signed three of five with the rehearsal kits, admitted, staged on every host, activated at its height, switched; consensus resumes, and state, balances, staking, rewards, governance, RPC, wallets, transactions and supply are checked before and after | T1, T3, T6, T8 |
| **R05 Operator error** | Wrong configuration, binary, network, genesis and genesis hash; a misconfigured and an offline validator; the sentry lost; an old snapshot restore; a second copy of the validator key; wrong peers, network settings, fees and environment; incompatible versions. Each must be refused, or stay harmless, with the documented response | T2, T6 |
| **R06 Load and economic activity** | 1,000 wallets with fresh ML-DSA-65 keys; funding, DRT and DGT transfers, bonding, unbonding to maturity, reward claims, governance proposals, deposits and votes; sustained load and the load thresholds; block, queue, RPC and signature-check limits pushed; malformed traffic mixed in. Safe and peak throughput, saturation, host utilization, wallet reliability and fees are measured | T1, T3, T4, T6 |
| **R07 Final dress rehearsal** | The exact production launch procedure with production values and the checkpoints below | T1, T2, T3, T6, T7 |

R07's checkpoints, from the brief:

| Checkpoint | Checks |
|---|---|
| T−6h | Binaries, checksums and release commit; genesis and every allocation; DRT and DGT supply; validators and keys; infrastructure, networking and addresses; RPC, monitoring, alerts, backups and recovery procedures |
| T−1h | Genesis hash, binary hash, peers, clocks, keys, configuration, allocations and network parameters |
| T0 | Start; consensus and block production; validator participation |
| T+15m | RPC, wallet connection, a PQC transaction, a DRT and a DGT transaction, a delegation, fee accounting, finality |
| T+1h | State consistency across nodes; wallets, staking, rewards, supply, monitoring, RPC |
| T+6h | Telemetry, logs, participation, accounting, transaction statistics, alerts, staking activity |
| T+24h | Full supply reconciliation, validator review, final state consistency review |

`FINAL_MAINNET_SIMULATION_REPORT.md` is written after R07's final reconciliation.

## Tools to build (A3)

Each is proven in the CI staging network before it touches a real host.

| Tool | Purpose | Builds on |
|---|---|---|
| **T1** Spendable rehearsal genesis | Real keys for every allocation row and the wallet fleet; per-run chain ID; the recorded mapping. **Spendable accounts built** (`staging_chain.py`, proven by the CI `wallet-drill`); **the timing profiles built** (`--timings`, [TIMINGS.json](../../../launch/simulations/TIMINGS.json); the CI install job runs the rehearsal profile) | `genesis_rehearsal_records.py`, `staging_chain.py`, `dytallix wallet create` |
| **T2** Staging agent and real-host harness | Signed, allowlisted commands on console-only hosts; the existing drills' assertions against real addresses. **Agent built** ([staging agent v1](../architecture/staging-agent-v1.md): `staging-agent/agent.py`, `staging_command.py`, proven by the CI `agent-drill`); its choices are approved ([approval](../../../launch/approvals/P01_T06_STAGING_AGENT_2026-10-14.json)) and its fault commands are T5's; the real-host harness follows | `staging_network.py` (`run`, `restore-drill`, `kit-drill`, `monitor-drill`), `staging_host.py`, the host scripts |
| **T3** Load generator and wallet fleet | Persistent workers that sign locally, track nonces and generate the transaction mix through the client channel. **Built** (`load.py`, proven by the CI `load` step and a T6 reconcile against its ledger): a fleet made with `dytallix wallet create`, funded through distributors (25 paying 40 each for R06) and primed online before the window; a paced, configurable mix (DRT and DGT sends, account creation, bond, unbond, claim) signed offline with the release CLI at locally tracked nonces and batched through `dytallix gateway serve`; governance propose, deposit and (once the deposit period closes) vote online, charged by the liquid DRT fall; the operators' and stakers' rewards swept back to the funder about hourly, so the genesis allocation stays the production one. Measures inclusion delay, throughput, block intervals, response times and lost transactions; digest-bound evidence that `load.py verify` re-evaluates. CI runs 1 transaction a second for 2 minutes from one address; full capacity needs the load machines | SDK `CometClient`, ordinary v2 and v3 `prepare` and `sign`, `submit_sync`, `wait_for_receipt` |
| **T4** Adversarial generator | Invalid signatures, bad encodings, bad nonces, duplicates, oversized requests, floods. **Built (lean scope)** (`adversary.py`, proven by the CI `adversary-drill` step): through the client channel an invalid ML-DSA-65 signature, a decode failure, a stale nonce and an oversized request are each refused by the node at admission (CheckTx code greater than zero, nothing committed) while a valid control transaction is admitted and committed; digest-bound evidence that `adversary.py verify` re-evaluates from the file. A transport or uncertain outcome never counts as a refusal, and the positive control guards against a channel that refuses everything. Duplicates, re-signed copies and request/transaction floods follow with the load machines (R03) | `CometClient::relay`, the client channel crate, the admission and consensus test cases |
| **T5** Fault injector | Latency, loss and partition (`tc netem`, an nftables table), peer drops, process kills, host loss, each with recovery timing. **Built (lean scope)** (`faults.py`, proven by the CI `fault-drill` step driven through the signed agent): latency and loss (`tc netem`), a p2p-scoped partition (a self-expiring `inet dytallix_fault` nftables timeout set, so a console-only host cannot be locked out) and a process kill each take hold on the sentry while the validator keeps producing, and the sentry catches back up; digest-bound evidence that `faults.py verify` re-evaluates. Each recovery is judged against the approved **120-second** recovery target (P01, 13 October 2026; `launch/simulations/TIMINGS.json` `fault_recovery_target_seconds`, approval `launch/approvals/P01_E05_FAULT_RECOVERY_2026-10-13.json`), so a scenario passes when the sentry catches back up within it. This is the transient-fault acceptance, stricter than and distinct from the six-hour per-host readiness catch-up budget (E05 `catch_up_millis`) the supervisor uses for a node rejoining from far behind. Peer drops and host loss follow (they need a VM restart path); the fault commands self-expire, which is what keeps a console-only host safe once the agent is installed on hel1/fsn1 | `staging_network.py` network setup, the monitor drill's fire-and-resolve pattern |
| **T6** Reconciliation report | At a height: every node's app hash and supply buckets compared; genesis plus emitted minus burned; fees against burns; tracked accounts summed against the totals; `dytallix-state-check` on a stopped copy. **Built** (`reconcile.py`, proven by the CI `reconcile` step after the wallet drill): PASS, FAIL or INCONCLUSIVE, only PASS counts; digest-bound evidence that `reconcile.py verify` re-evaluates. Its hosts are pluggable; on the real hosts it waits for T2's transport | `restore-drill`'s comparison, the monitor's supply rules, the status view, `dytallix-state-check` |
| **T7** Checkpoint recorder | Runs each checkpoint's checks and writes digest-bound evidence. **Built** (`checkpoints.py`, proven by the CI `checkpoint` step after the after-load reconciliation): each checkpoint's status follows from its checks (a FAIL stops the run, an INCONCLUSIVE never passes), the reconciliation (T6) and load (T3) checks bind that tool's evidence by digest and re-run its `verify`, and a check with no source yet is INCONCLUSIVE and names the tool it waits for; `checkpoints.py verify` re-evaluates from the file, reads genesis time from the engine genesis, and refuses a compressed or early checkpoint and any schedule that is not R07's as the dress rehearsal. R07's schedule is the brief's. R01 to R06's are approved (P01, 14 October 2026, [approval](../../../launch/approvals/P01_T06_SIMULATION_INPUTS_2026-10-14.json)) and still to be built into the recorder (its schedules are fixed offsets from genesis; the event checkpoints follow events, so this is a tool change, not only data): one T−1h checkpoint before genesis with both R07's T−6h and T−1h checks, R07's T0 to T+24h after it, and a reconciliation after each fault recovery, upgrade switch and load window | `scripts/run_launch_rehearsals.py`, the drills' JSON evidence, the monitor history |
| **T8** Upgrade drill | Builds RC2's bundles for an existing chain and runs the handover and upgrade end to end on the hosts | the kit drill's control flow, `stage.sh` and `switch.sh`, the signed handover process tests |
| **T9** Validator recovery drill | Fence, set aside the consensus WAL, reinstall, block-sync, time against the target | [validator recovery](../operations/validator-recovery.md), the restore drill |

The capacity measurements (T05) use T3 and T6, and the statistical campaign uses T2 to T6 with a campaign driver that schedules and counts the attempts.

## Scenario register

`launch-rehearsals.json` keeps the twenty mandatory scenarios (L01 to L20) that the runs cover and the brief's verbatim instructions. L01 and L02 are local prerequisites (`scripts/run_launch_rehearsals.py`); L18 is the seven runs; L19 the frozen candidate's operating periods; L20 the launch decision. Under the solo launch profile, "independent operators" and "independent reviewers" mean what the gate data's `acceptance_profile` says.

## Evidence

Each run records: the run ID, start and end, status and the founder's sign-off; the source revision, the release manifest and binary digests, the genesis and configuration digests, the rehearsal chain ID and the shortened timing set; the node identities (public only), the workload, the injected faults and their timing; per-node finalized heights, app hashes and supply reconciliations; client receipts, resource metrics, alerts and logs; expected and actual results and every assertion. The evidence files are hashed. Success is never inferred from a process exiting.

A run stops on conflicting finality or an unexplained accounting difference: the evidence is preserved, the cause fixed, and the affected runs rerun. A change to consensus, economics, cryptography or persistence invalidates the affected results. A testnet-style database reset is never a mainnet recovery procedure.

## Local prerequisites

`scripts/run_launch_rehearsals.py` runs the local prerequisites only (L01, L02) and records the distributed scenarios as blocked; its report always sets `mainnet_ready` to false.

```sh
python3 scripts/run_launch_rehearsals.py --profile components --output /absolute/path/outside/repository/new-run
```

Use `--profile preflight` for the reference and module checks only, and a new output directory per run. Cargo runs locked and offline. The runner hashes tracked and non-ignored untracked files before and after; a source change invalidates the run.
