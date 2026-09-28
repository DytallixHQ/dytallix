# E04.1 requirement and conflict triage (27 September 2026)

Engineering task E04, subtask 1. The 52 remaining requirement rows and 14
tracked conflicts (`decision-register/core-function-alignment/`
`E04_REQUIREMENT_TRIAGE_2026-09-25.csv`, `CONFLICT_REGISTER.csv`) checked
against `main` at 2c76e8e9 (#259). Diagnostic engineering input only; T07 owns
the evidence map and acceptance.

Classes: **DONE**; **PARTIAL**; **GAP** (engineering, size S under a day, M
one to three days, L more); **POLICY** (an open decision); **NOT E04** (E05,
E06, T01–T07, P02, P03); **CLAIM** (a document needs correcting).

## Summary

| Class | Rows | Conflicts |
| --- | --- | --- |
| DONE / resolved | 6 | 2 |
| PARTIAL | 10 | — |
| GAP | 5 | — |
| POLICY | 15 | 5 |
| NOT E04 | 12 | 3 |
| CLAIM | 4 | 4 |

## Engineering gaps, by risk

| # | Gap | Rows | Size |
| --- | --- | --- | --- |
| 1 | **Evidence halts the chain.** With `penalty` unset (required today: the penalty profile refuses vesting locks and production activation), any duplicate-vote evidence fails prepare and finalize. Light-client-attack evidence fails in every configuration (the bridge refuses it; CometBFT then panics in PrepareProposal). One faulty validator can stop the chain. Accept every evidence type without halting; the penalty effect waits for D09. | CONS-002, AC-003, AC-004 | M |
| 2 | **Issuance journal limit halts the chain.** The journal is replayed every block and the block fails once `max_recorded_epochs` is reached. Checkpoint and prune it. | STATE-002 | M |
| 3 | **Restart replays the whole chain.** The verification mark is in memory, so every restart re-checks every block and ML-DSA signature from height 1; block and emission records are never pruned. | STATE-002, SYNC-003 | L |
| 4 | **State sync** (phase C, approved): snapshot export at a height, chunk verification against the root, restore, a complete check and epoch observation that start from the snapshot height, an engine profile that allows it. | STATE-003, SYNC-002, SYNC-003 | L |
| 5 | **Supply check reads every account each block.** Replace the `acct:balances:` scan with running totals. | ECON-004 | M |
| 6 | **Mempool duplicate bypass.** A byte-distinct re-signed copy of a reserved intent is admitted (code 0) and uses mempool and proposal capacity; engine mempool and P2P limits are not pinned. | MEM-002 | S |
| 7 | **No metrics.** Engine metrics are compiled out, the app's metrics are stubs; an exporter must fit the no-`net/http` boundary. | OBS-002 | L |
| 8 | **Clients lag the chain.** The SDK's protocol types lack v3; the CLI sends legacy governance and staking requests; no first-spend path for implicit accounts; no v3 vectors; the SDK does not verify state proofs. | TXN-001, API-003 | M |
| 9 | **Interface inventory.** Six unversioned query paths; balances, stake, rewards and governance readable only as raw keys; no ABCI events. | API-001 | L |
| 10 | RPC controls: method allowlist, pinned limits, error codes. | API-002 | M |
| 11 | Migration and handover tests are `#[ignore]` and not rerun on the phase B layout. | UPG-003 | S–M |
| 12 | Governance v3 per-transaction reconciliation (v2 and recovery have it). | TXN-004 | S |
| 13 | Unit resource limits (MemoryMax, TasksMax, LimitNOFILE); adapter limits are compile-time. | PERF-003 | S |
| 14 | Legacy modules still compiled into the consensus crate but unreachable (`fee_burn`, legacy emission pools, `alerts`, `metrics`, legacy mempool). | — | S–M |
| 15 | Runbooks: fork, supply mismatch, key compromise, resource exhaustion. | OBS-003 | M (docs) |
| 16 | **Keystore in plaintext.** The CLI keystore holds private keys unencrypted (found by gap 9; P01, 28 September 2026). Encrypt at rest: a passphrase-derived key with an AEAD cipher, a versioned file format, migration from version 1. | — | M |

### Progress

| Gaps | Closed by |
| --- | --- |
| 1, 2 | #262 (`docs/architecture/liveness-v1.md`) |
| 3, 4 | #263, #265, #266, #267, #268 (`docs/architecture/state-sync-v1.md`) |
| 5 | #269. Running account totals: `supply:account_totals` (liquid uDGT and uDRT) is written at consensus genesis and updated by each block from the balance records it writes; the per-block supply check reads no other account; the complete check compares the totals with every record. |
| 6 | #270. Duplicate bypass: a reservation request carries its signed envelope's digest, so a re-signed copy of a reserved intent is refused (identity mismatch) in CheckTx, rechecks and proposals; only the same bytes are already reserved. The engine requires the flood mempool with recheck, which the per-head admission queue depends on, and a mempool `max_tx_bytes` no larger than the genesis block; the fixture sets it to the application limit. Mempool and P2P capacity values (size, total bytes, cache, peer rates) stay operator settings until D06-Q02. |
| 7 | #271. Metrics (`docs/architecture/metrics-v1.md`): the core set as Prometheus text files, `dytallix-engine.prom` and `dytallix-app.prom`, at an operator interval, each with its write time; no listener. |
| 8 | Clients (`docs/architecture/clients-v1.md`): K-a (#272) builder safety, exact vendoring with a CI drift check, independent v2 and v3 vectors. K-b: node query `/ordinary/profile_v3` (committed v3 fee profile and next proposal ID); SDK ordinary v3 and governance action data; `dytallix governance` propose, deposit and vote on v3 (#273). K-c1: pinned chain (P01 decisions 4 and 5); one-step send, stake, balance and governance with v1 addresses and first spend; later-height refresh; legacy REST commands under `dytallix legacy` (#274). K-c2: `legacy-network` non-default; SDK `comet-rpc` feature (Comet client with TLS, no legacy REST client); legacy build tested in CI (#275). K-d: state proofs verified to the state root and the application hash (`protocol-types::state_proof`, cross-checked against `jmt` and the engine's committed hash); verified balances; first spends prove funding and no record. |
| 9 | Interfaces (`docs/architecture/interfaces-v1.md`, P01 decisions 28 September 2026): I-a (#277) checked inventory (`interfaces-v1.json`, `scripts/check_interface_inventory.py`, a node test binding query paths); versions on the status and emergency receipt views, metrics files, light-block export, engine ready line, ABCI info (`dytallix-app-v1`), keystore, CLI configuration and pin files and CLI output; keystore written 0600; no ABCI events (decision 3); gap 16 recorded. I-b: typed views for the account summary, validator set, proposals and votes, with SDK reads and `stake status`, `stake validators` and `governance show`. |
| 10 | RPC controls (`docs/architecture/rpc-controls-v1.md`, P01 decisions 28 September 2026): the client socket `rpc.sock` serves an 18-method allowlist and a separate 0600 `rpc-operator.sock` adds diagnostics; search, mempool contents, commit-waiting broadcasts and subscriptions are served nowhere; limits and error semantics documented; the gateway contract (TLS, allowlist, limits no looser than the node's, no client authentication) with values left to D12-Q01; the supervisor refuses a leftover operator socket. |
| 11 | Signed tests (`docs/architecture/signed-tests-v1.md`): M-a, the 21 in-process signed-fixture tests (root genesis, emergency, upgrade and index migration, release handover, history replay) run in CI through `scripts/run_signed_fixture_tests.py` with test-only tools (a snapshot verifier without the owner guard, a disposable fixture signer); four tests updated for phase B (derived observations, the state tree root, verifier order). M-b, the three two-binary process tests, needs an owned Linux harness in the E02 native job. |

## Policy questions (P01)

| Decision | Question | Rows |
| --- | --- | --- |
| D09-Q04, D09-Q05 | Which evidence is penalized (duplicate vote; light-client attack); rates and correlated scaling; jail, tombstone and reinstatement; third-party bonder liability; how penalties apply to vesting-locked stake; whether evidence is only recorded before penalties are qualified. | CONS-002, VAL-002, VAL-004, AC-003 |
| D09-Q03 | Evidence age limits (blocks, seconds) and margins; they also set the unbonding period and retention horizon. | VAL-003, AC-011 |
| D06-Q02 | Block time, timeouts, fault assumptions; state-sync trust source, trust period, snapshot peers, interval and retention; whether operator rollback is allowed. | CONS-001, SYNC-001 |
| D01-Q01, D01-Q02 | Approve observation contract v1 (block bytes, volatility 0) and the controller, or change the controlled variable; `E_min`; confirm no external oracle at launch. | ECON-001, ECON-003, ORC-001, ORC-002, AC-001, AC-010 |
| D03-Q01 | Epoch length. | AC-011 |
| D07-Q01 | Mark LBP, wrapped USDC, the Airlock, bridges and external oracles POST MAINNET. | BRG-001, BRG-002, AC-009 |
| D08-Q02, D08-Q03 | How users get DRT for fees without an LBP. | BRG-001, AC-009 |
| D04-Q01, D06-Q02 | Adopt the implemented mempool rule (arrival order, no replacement or eviction, expiry, release at each head) as normative; capacity values. | MEM-002, MEM-003 |
| D11-Q03, D14-Q02 | Upgrade authority and threshold; minimum notice; validator readiness; client compatibility window. | UPG-002 |
| D12-Q01, D12-Q02 | Public ingress (TLS, authentication, rate limits, methods); alert targets and routing. | API-002, OBS-002, PERF-003 |
| D05-Q02 | All DGT issued at genesis, no later mint. | ECON-004 |
| D11-Q02 (values, E05) | Quorum, approval and veto thresholds, deposit, periods, timelock. | GOV-001, AC-005 |

## Requirement rows

| ID | Class | Finding |
| --- | --- | --- |
| CONS-001 | POLICY | Upstream CometBFT state machine and 2/3 quorum; timeouts are fixture values. D06-Q02. |
| CONS-002 | POLICY + GAP 1 | Duplicate votes only; light-client attack halts the chain. D09-Q04. |
| CONS-003 | DONE | H+2 activation for bonds, faults, rotations, registry. |
| CONS-004 | NOT E04 | T04–T06. |
| TXN-001 | PARTIAL | Node checks done for v2 and v3; client gaps closed by gap 8 (K-a to K-d); acceptance is T07's. |
| TXN-002 | DONE | Shared reservation and meter; fees burned. |
| TXN-003 | DONE | Paid failures, nonce replay protection across restart. |
| TXN-004 | PARTIAL | v3 per-transaction reconciliation missing (GAP 12). |
| VAL-002 | POLICY | One synthetic ratio; production penalties refused; no jail. D09-Q04. |
| VAL-003 | POLICY | Maturity rule done; values unset. D09-Q03, D09-Q05. |
| VAL-004 | POLICY | Penalties refuse vesting locks, so vesting stake cannot withdraw. D09-Q05. |
| ECON-001 | POLICY | Utilization target unchanged. D01. |
| ECON-003 | POLICY | 40/30/30 and payouts done; `E_min` unset. D01-Q01, D03-Q01. |
| ECON-004 | PARTIAL | Conservation checked every block, from running account totals (gap 5 closed); vesting with penalties. |
| GOV-001 | NOT E04 | Rules done; values E05. |
| GOV-002 | DONE | Timelock, bound action, one execution, refunds. |
| GOV-004 | CLAIM | Linear stake weighting; remove quadratic voting and decay claims. |
| STATE-001 | PARTIAL | Commitment frozen (JMT); layout still changes; no key-space spec. |
| STATE-002 | PARTIAL | Windows done; block and emission records unpruned; journal limit halts (GAP 2); restart replays all (GAP 3). |
| STATE-003 | GAP 4 | Snapshots are stubs. |
| STATE-004 | NOT E04 | T05. |
| SYNC-001 | POLICY | Genesis-only start; D06-Q02. |
| SYNC-002 | GAP 4 | Snapshot path missing. |
| SYNC-003 | PARTIAL | Replay checked; restored node would fail the complete check and observation (GAP 3, 4). |
| SYNC-004 | NOT E04 | T05. |
| MEM-001 | DONE | Exact nonce, conflicts refused, reset at head. |
| MEM-002 | PARTIAL | Duplicate bypass closed and mempool rules pinned (gap 6); capacity values open (D06-Q02). |
| MEM-003 | POLICY | Implemented rule not normative. D04-Q01, D06-Q02. |
| MEM-004 | NOT E04 | T05. |
| ORC-001 | POLICY | No oracle; gas price governed; utilization only. D01-Q02. |
| ORC-002 | POLICY | No reporters; D01-Q02. |
| ORC-003 | CLAIM | No outlier slashing; retract the claim. |
| ORC-004 | DONE | Only bounded fee values and validator limits governable. |
| UPG-001 | NOT E04 | E06 provenance. |
| UPG-002 | POLICY | Height activation built; authority open. D11-Q03, D14-Q02. |
| UPG-003 | PARTIAL | In-process signed tests rerun on phase B and in CI (gap 11 M-a); the process tests remain (M-b). |
| UPG-004 | NOT E04 | T05, T06. |
| API-001 | GAP 9 | Gap 9 closed (I-a, I-b): checked inventory `docs/architecture/interfaces-v1.json`; version fields everywhere; typed reads; no ABCI events by decision; acceptance is T07's. |
| API-002 | PARTIAL | Gap 10 closed (R-a): client and operator socket allowlists, pinned limits, error semantics and the gateway contract (`docs/architecture/rpc-controls-v1.md`); public topology and rate values remain D12-Q01; acceptance is T07's. |
| API-003 | GAP 8 | Gap 8 closed (K-a to K-d): SDK and CLI on the consensus chain; SDK header verification later (decision 3); acceptance is T07's. |
| API-004 | NOT E04 | T04, T05. |
| BRG-001 | POLICY | Bridge excluded in code; D07-Q01, D08-Q02. |
| BRG-002 | POLICY | N/A if D07-Q01 excludes bridges. |
| BRG-003 | CLAIM | Add the boundary disclosure to the security model. |
| BRG-004 | NOT E04 | P02. |
| OBS-002 | PARTIAL | Core metrics written as text files (gap 7 closed); thresholds and routing open (D12-Q02). |
| OBS-003 | PARTIAL | Some drafts; runbooks missing (GAP 15). |
| PERF-001 | NOT E04 | T05. |
| PERF-002 | NOT E04 | T05. |
| PERF-003 | POLICY + GAP 13 | Values D06-Q02, D12-Q01. |
| PERF-004 | NOT E04 | T05. |
| ASSUR-003 | CLAIM | See claim fixes. |

## Conflicts

| ID | Class | Finding |
| --- | --- | --- |
| AC-001 | POLICY | D01: approve observation contract v1; drop the claim that issuance controls utilization. |
| AC-002 | RESOLVED | Every fee burned (P01, 27 Sep); the minimum fee resists spam only. |
| AC-003 | POLICY + GAP 1 | D09-Q04, D09-Q05. |
| AC-004 | CLAIM + GAP 1 | CometBFT finality; the papers' checkpoint and LMD-GHOST model applies to nothing. |
| AC-005 | NOT E04 | Engineering part resolved (governance v1); values E05, signers P02. |
| AC-006 | CLAIM | Linear stake weighting; decay and delegation claims remain in docs. |
| AC-007 | CLAIM | Algorithms change only by root-signed upgrade; fix the registry wording. |
| AC-008 | NOT E04 | E05 (D08). |
| AC-009 | POLICY | D07-Q01, D08-Q02. |
| AC-010 | POLICY | D01-Q02, D07-Q01: no oracle at launch. |
| AC-011 | POLICY | Horizon equals maturity; values D03-Q01, D09-Q03, D01-Q01. |
| AC-012 | NOT E04 | E01 closed at source; T01, T02. |
| AC-013 | RESOLVED | Fees move to the burn counter; withheld zero at commit. |
| AC-014 | CLAIM | The three whitepaper PDFs. |

## Claim fixes

- Tokenomics paper: vote decay, delegation and VRF sortition; fee split (now
  every fee burned, no tips); fee floor for validator viability; Oracle
  Medianizer; LBP and USDC floor; MPC Airlock.
- Technical paper: checkpoint finality gadget; "double signing burns 100% of
  stake".
- Foundational paper: checkpoints and LMD-GHOST; governance-mutable
  parameters and algorithm registry; the Airlock.
- `mainnet/docs/docs`: `tokenomics.md` (fees in DGT, decay, delegation);
  `security-model.md` (bridge boundary disclosure; oracle outlier handling);
  `contract-quickstart.md` and `cli-reference.md` (no contract runtime).
- `mainnet/README.md`, `contracts/README.md`: no contract runtime or bridge in
  the consensus build.
- `launch/MAINNET_V1_SPEC.md`: stale (engine, algorithm, governance,
  lifecycle, rewards, fees).
- `launch/DRT_TOKENOMICS.md`, `TOKEN_SUPPLY_MODEL.md`: EC-12 superseded by
  fees v1.
- `launch/MAINNET_DECISION_REGISTER.json`, `DECISIONS_REQUIRED.md`: record the
  26–27 September approvals (creation-fee burn; governance classes, bounds,
  fee authority; fee burn and validator payouts; retention; state root) and
  update D02-Q01, D05-Q01, D11-Q01, D11-Q02.
- SDK `docs/core-concepts.md`: fees are paid in DRT.

## First live CI run (27 September)

The first GitHub Actions run after the billing fix (run 36324670835): Go
engine and SDK passed; the node job stopped at the G35 step (fetch ordering)
and the E01 inventory was stale; E02 native passed every check except
`filter.owned_good_request_under_apparmor` (the helper under the production
unit's `NoNewPrivileges`, syscall filter and AppArmor profile). Fixes and a
diagnostic are in #260; E02 stays open until that check passes.
