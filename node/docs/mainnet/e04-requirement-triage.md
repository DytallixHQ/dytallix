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
| CLAIM | 4, all fixed | 4, all fixed |

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
| 15 | Runbooks: fork, supply mismatch, key compromise, resource exhaustion (P01, 28 September 2026: every OBS-003 class, with halt, upgrade failure and oracle failure). Found by the survey: the service launch cannot enable metrics or snapshots, and a stopped application gives no reason. | OBS-003 | M–L |
| 16 | **Keystore in plaintext.** The CLI keystore holds private keys unencrypted (found by gap 9; P01, 28 September 2026). Encrypt at rest: a passphrase-derived key with an AEAD cipher, a versioned file format, migration from version 1. | — | M |
| 17 | **Key and operator tooling** (found by gap 15; P01, 28 September 2026: one gap, after gap 16). (a) No production validator key proof signer: `ValidatorRegister` and `ValidatorRotateKey` need a possession proof from the consensus key, and the only signer (`dytallix-comet-proof`) accepts the local fixture nodes only; a signer for the operator's ML-DSA-65 key that signs only well-formed proofs for the configured chain. (b) No client builds recovery transactions (rotate, start, finalize, cancel, resume): a CLI and SDK builder. (c) No client reaches the operator socket (`net_info`, `consensus_state`): an operator socket client. | KEY-004 | M–L |
| 18 | **No restart on new code after a halt** (found by gap 15; P01, 28 September 2026: next, before gaps 16 and 17). A committed block that every validator fails to execute stops the chain: the engine replays it on every restart. Fixed code is a new release, the application refuses any release but the committed one, and a handover needs a block. Design first, with its policy questions: for example, a root-signed restart authorization binding the checkpoint, its application hash and the new release, accepted only at startup, never rewriting history. | OBS-003, KEY-004 | L |
| 19 | **Classical TLS at the client edges** (P01, 28 September 2026: no classical public-key cryptography anywhere in the stack). The SDK and CLI reach remote nodes over HTTPS through reqwest's default TLS, the platform stack (OpenSSL, Security.framework or SChannel), with classical key exchange and certificate signatures; `rustls` and `ring` sit in the SDK lockfile. The contracts toolkit's CosmWasm bridge pulls k256, ecdsa and ed25519-zebra. The public gateway contract (RPC controls v1) terminates TLS. Remove classical TLS from every SDK and CLI feature, with nodes reached through a post-quantum transport or channel; design the public browser gateway, since browsers have no post-quantum certificates. | API-002 | L |
| 20 | **Classical code in the source tree** (found closing gap 19; P01, 29 September 2026). The engine fork keeps upstream classical code that the PQC-only build excludes but the default build compiles: Ed25519, secp256k1, secp256k1eth and BLS keys, SecretConnection, the remote signer and libp2p (88 files). The testnet faucet sat in `mainnet/` behind HTTPS. Operator host access assumed SSH. Decisions: delete the fork's classical code and make PQC-only the one build; move the faucet out of `mainnet/`; operator host access is console-only (E05); E03 runs only on hosts dedicated to Dytallix. | — | L |

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
| 11 | Signed tests (`docs/architecture/signed-tests-v1.md`): M-a, the 21 in-process signed-fixture tests (root genesis, emergency, upgrade and index migration, release handover, history replay) run in CI through `scripts/run_signed_fixture_tests.py` with test-only tools (a snapshot verifier without the owner guard, a disposable fixture signer); four tests updated for phase B (derived observations, the state tree root, verifier order). M-b (P01 decision 28 September 2026): the three process tests run in CI on two test builds of the application (feature `test-snapshot-verifier`, which the production bridge refuses) through the owner launcher `release-runtime-owned-launch`, under no_new_privs and the production system call deny list; the hardened launch stays with E02 and T03. |
| 12 | Governance reconciliation (`docs/architecture/governance-v1.md`): each committed v3 governance transaction is reconciled with its effects, as v2 receipts and charged recoveries are. The checks: the fee reaches fee custody, a deposit reaches the held total, the actor's nonce advances by one, and nothing else changes (other accounts, burns, staking, validator, penalty, lifecycle and evidence state). A difference is an internal error: ProcessProposal rejects the block and FinalizeBlock fails closed. |
| 13 | Resource limits (P01 decisions 28 September 2026). Unit: the renderer requires `resources` (MemoryMax, TasksMax, LimitNOFILE) with no defaults, and fixes MemorySwapMax=0 and LimitCORE=0; the E02 native job checks they take effect and that a child inherits them. Adapter: its compiled limits become ceilings that operator flags (passed from the supervisor's `adapter_limits`) can only lower. The values stay with D06-Q02 and D12-Q01. |
| 14 | Legacy code (P01 decision 28 September 2026: two steps). L-a removed code that no configuration reaches: `alerts`, the old `metrics` server, the legacy `mempool`, `production_control`, the legacy `runtime/governance.rs` and dead-man-switch module, the legacy emission engine and pools, the development block adapter (`block_settlement`) with its dead helpers, the `signature-policy` crate and ten unused dependencies (28 fewer locked packages; the consensus graph has 111), with the tests of that code. L-b removed the legacy `Signed` transaction path: the wire kind, its admission, proposal and block handling, the accepted-receipt records and their history checks, both legacy executors, `fee_burn`, `signed_transaction`, the legacy `types`, the legacy settlement operations, the `dytallix-gas` crate, the storage crate's block, transaction, receipt and oracle stores, `SignatureAlgorithm`, the `lifecycle_fixture` and `txhash` tools and the E01 `legacy_signed_transaction` route (the consensus graph has 110 packages). Its lifecycle and penalty tests run on ordinary v2 (`ordinary_test_support.rs`); tests of the legacy path alone went with it. It found a liveness fault, fixed separately: without the penalty profile a validator withdrawal failed the block (now a paid `VALIDATOR_WITHDRAWAL_DISABLED` failure, liveness v1 L3). L-c (P01 decision 28 September 2026: required at release): the production application configuration carries the recovery and ordinary profiles, the only user-transaction paths. It is recorded in the E01 production configuration requirements, and the mainnet-preparation binding review (now in CI) reports a configuration without both as missing; local and test profiles may omit them. Gap 14 closed. |
| 15 | #292, #293 and R-c (`docs/architecture/incident-response-v1.md`). R-a: the native service configuration sets the metrics output (required; the release binding review reports `service_configuration` and `metrics_output` as missing), optional snapshots and the block history mode. R-b: fixed failure classes with exit statuses 10 to 17, repeated in the supervisor's failure record, and `dytallix-state-check`, the read-only startup check of a stopped node. R-c: runbooks in `docs/operations/` for halt, fork, supply mismatch, key compromise, resource exhaustion and upgrade or handover failure; oracle failure is not applicable (AC-010); a refused candidate executable exits `release`. The runbooks record gaps 17 and 18, and that a node can rejoin only from an archive peer, since the supervisor refuses state sync. Gap 15 closed. |
| 18 | #295, #296, #297 (`docs/architecture/restart-v1.md`, P01 decisions 28 September 2026: a restart authorization, signed by the handover authority, release only, resume open). S-a (#295): the contract and `release_handover::restart` (artifact, validation against the committed checkpoint, root-helper signature verification, state transition, replay). S-b (#296): the application takes `--restart-authorization`, selects the restart at preflight and startup, runs block H on the target, stores the switch and receipt, replays and pins it; an in-process and a two-build process test. S-c (#297): the supervisor's pinned `restart_authorization` and restart preflight; formatted files accepted; `dytallix-state-check --restart-target` builds the unsigned authorization and the bytes to sign; the restart runbook (`docs/operations/restart.md`). Gap 18 closed. |
| 16 | #298. Keystore v2 (`docs/architecture/keystore-v2.md`, P01 decisions 28 September 2026): each private key is encrypted with AES-256-GCM under an Argon2id key (64 MiB, 3 passes, stored with the salt; weaker files refused), with the entry's metadata as associated data and a passphrase check; listing needs no passphrase; passphrases come from the terminal without echo or an owner-only file (`DYTALLIX_KEYSTORE_PASSPHRASE_FILE`); version 1 files are refused for signing until `dytallix wallet migrate`; the CLI's direct file readers now go through the keystore; `wallet export` writes a new owner-only file. Gap 16 closed. |
| 17 | `docs/architecture/key-tooling-v1.md`. T-a: `dytallix-validator-key` generates ML-DSA-65 validator keys with a fresh signing state and signs only the register or rotate proofs it builds for the engine genesis chain (golden vector asserted by the node). T-b: `dytallix-operator-rpc`, read-only diagnostics over the operator socket. Both in the PQC-only production build and the G35 Go graph check (#299). T-c1 (#300): the typed recovery view `/recovery/account/{account_id}` (P01 29 September 2026: typed view, all nine actions, offline multi-party signing). T-c2 (#301): `dytallix_sdk::recovery` (build every action from the view with its signers, sign, assemble with checks, sponsor within the fee bounds) and `dytallix recovery` query, prepare, sign, assemble, sponsor, submit and receipt. Gap 17 closed. |
| 19 | #302, #303, #304, #305, #306 and C-e (`docs/architecture/client-channel-v1.md`, P01 decisions 29 September 2026: a post-quantum channel to endpoints whose full ML-DSA-65 keys clients pin; a local browser companion, with no public HTTPS; AES-256-GCM records in the channel and the peer transport; the legacy testnet commands and the contracts toolkit's CosmWasm bridge removed). C-a: the version 1 protocol and the sans-IO `dytallix-client-channel` crate (ML-KEM-768, ML-DSA-65, HKDF-SHA-256, AES-256-GCM), whose fixed-seed vector Go reproduces with CIRCL; the CosmWasm bridge removed from `mainnet/contracts` (#302). C-b: the endpoint. The HTTP adapter's client channel listener (`--channel-listen`, `--channel-network`; 64 connections, 4 per address, lowered by flags) forwards to the client socket under the loopback bounds. The endpoint signs with its own owner-only seed (`config/client_channel_seed.bin`, refused if equal to the peer seed). `dytallix-channel-key` generates the seed and writes the endpoint pin. The supervisor's `adapter_channel` pins it and, before readiness, completes a channel `GET /status` with its key. The TLS gateway contract becomes the public endpoint contract (#303). C-c1: the SDK vendors the channel crate. `dytallix_sdk::transport` reaches a loopback node over plain HTTP (hyper, no TLS) or a remote one over the channel. The CLI's `--endpoint` takes an endpoint pin file, and `chain.json` version 2 holds the endpoint key. The default CLI and the Comet and local SDK graphs contain no TLS crate or reqwest, and `dytallix-ordinary-local` builds again (#304). C-c2: the legacy testnet client is removed: SDK `network`, CLI `legacy-network` and their commands, `config.json`, the testnet examples, and reqwest with every TLS crate. `check_no_classical.py` refuses a classical or TLS crate in any of the five mainnet Rust lockfiles, and CI runs it (#305). C-d: `dytallix gateway`, the loopback browser companion. It relays its own pages' JSON-RPC to the pinned chain over the channel, and serves a wallet bundle pinned by manifest digest. It refuses a foreign Host, Origin or cross-site fetch and answers no CORS preflight (#306). C-e: peer transport wire version 2. Records are sealed with AES-256-GCM, and the suite names the cipher, so version 1 peers cannot agree on keys. The E03 probe's copy of the transport must be refreshed outside the repository. Gap 19 closed. |
| 20 | #308, #309, #311 and F-c (P01 decisions 29 September 2026). F-a (#308): the testnet faucet moved to `testnet/faucet`. E05 operator host access is console-only. E03's method packet, outside the repository, was refreshed to peer wire version 2, and its runner refuses hosts shared with products. F-b (#309): the engine fork's classical code deleted (`consensus/cometbft/PQC_BUILD_BOUNDARY.md`). The 94 files the production tags excluded are gone: classical keys, `lp2p`, SecretConnection, the remote signer and its server, and the HTTP RPC, TLS, gRPC, Prometheus and SQL paths. So are the tests that used classical keys and the fixture's legacy profile. The 32 tag-required files are unconditional, so PQC-only is the one build; the graph check requires the removed packages to stay absent. Test hygiene (#311): CI vets and tests every upstream package (`consensus/cometbft/UPSTREAM_TESTS.md`). The module sums are tidied, tests of removed features deleted, and the classical-key tests ported to ML-DSA-65. The 43 reactor tests that connect switches are skipped until the p2p test helpers have an authenticated PQC upgrade. F-c: the inert `dytallix_pqc_only` and `dytallix_pqc_ipc` tags are gone from CI, the build and boundary tools, the Go sources and the docs; the checks that compared the constant build profile are removed; CI vets and tests the fork once. The graph check refuses a fork source file with a Dytallix build constraint, and the boundary checker refuses an engine built with any tag. Gap 20 closed. |

## Policy questions (P01)

Decided 29 September 2026 (`launch/approvals/P01_E04_POLICY_2026-09-29.json`):
D07-Q01 launch scope (current build; the rest POST MAINNET), D05-Q02 (all DGT
at genesis; the runtime mint path is removed and the binding review requires
the full total), D01-Q02 (observation contract v1, no external oracle) and
the D08-Q02 bootstrap policy (liquid genesis DRT to named accounts).

Decided 30 September 2026 (`launch/approvals/P01_E04_PENALTIES_2026-09-30.json`,
`docs/architecture/penalties-v1.md`): double-signing penalized with removal
(D09-Q04), withdrawals from genesis (D09-Q05), vesting-locked stake
penalized like unlocked stake, and a permanent penalty escrow. The launch
configuration now requires the lifecycle and penalty profiles.

Decided 30 September 2026 (`launch/approvals/P01_E04_OPERATIONS_2026-09-30.json`):
the implemented mempool rule is normative (D06-Q02, `docs/architecture/mempool-v1.md`);
upgrades need a separate 3-of-5 custodian group, distinct from the emergency
custodians, with a fixed minimum notice (D11-Q03); validators are private
behind sentries (D12-Q01). The remaining questions are E05 values and
records, and E06 release acceptance.

E05 values, first set (30 September 2026,
`launch/approvals/P01_E05_VALUES_1_2026-09-30.json`): about 5-second blocks;
14-day evidence limits with 1-hour margins (D09-Q03); a 5% double-sign
penalty (D09-Q04); the governance thresholds, periods and deposit (D11-Q02).

E05 values, second set (30 September 2026,
`launch/approvals/P01_E05_VALUES_2_2026-09-30.json`): 16 active validators
within 4 to 32 and a 100,000 DGT self-bond within 10,000 to 1,000,000 DGT
(D09-Q01); 7-day recovery template windows with 1-day submissions (D10-Q02);
7-day upgrade notice (D11-Q03); daily epochs (D03-Q01) and snapshots keeping
three (D06-Q02); issuance of 1,000 DRT a block as base and ceiling, 500 floor,
50% target (D01-Q01); a 1 DRT transfer governed between 0.1 and 10 DRT and a
10 DRT account creation fee within 1 to 100 DRT (D04-Q01); a 1,000,000 DRT
bootstrap (D08-Q02).

| Decision | Question | Rows |
| --- | --- | --- |
| D09-Q05 | Parameter migration. | VAL-004 |
| D06-Q02 | Fault assumptions; state-sync trust source, trust period and snapshot peers; whether operator rollback is allowed. | CONS-001, SYNC-001 |
| D01-Q01 | The controller's gains, integral limits, sample window and shock threshold. | ECON-001, ECON-003, ORC-001, ORC-002, AC-001, AC-010 |
| D03-Q01 | The sample window, with D01-Q01. | AC-011 |
| D08-Q03 (records, E05) | The DRT bootstrap recipient rows. | BRG-001, AC-009 |
| D06-Q02 (values, E05) | Mempool, queue and peer capacity values. | MEM-002 |
| D11-Q03, D14-Q02 (records and values, E05, E06) | Upgrade custodians; the production upgrade policy (schema 2); client compatibility window. | UPG-002 |
| D12-Q01, D12-Q02 (values, E05) | Endpoint addresses and keys, rate limits; counts and hosts; alert targets and routing. | API-002, OBS-002, PERF-003 |

## Requirement rows

| ID | Class | Finding |
| --- | --- | --- |
| CONS-001 | POLICY | Upstream CometBFT state machine and 2/3 quorum; timeouts are fixture values. D06-Q02. |
| CONS-002 | DONE | Gap 1 closed; a first duplicate vote is penalized with removal and light-client attacks are recorded only (penalties v1). |
| CONS-003 | DONE | H+2 activation for bonds, faults, rotations, registry. |
| CONS-004 | NOT E04 | T04–T06. |
| TXN-001 | PARTIAL | Node checks done for v2 and v3; client gaps closed by gap 8 (K-a to K-d); acceptance is T07's. |
| TXN-002 | DONE | Shared reservation and meter; fees burned. |
| TXN-003 | DONE | Paid failures, nonce replay protection across restart. |
| TXN-004 | PARTIAL | Every transaction kind is reconciled per transaction (v2 receipts, charged recoveries, v3 governance since gap 12). Candidate-bound conservation evidence and acceptance belong to T07. |
| VAL-002 | POLICY | Penalty model approved (penalties v1); the rate is an E05 value (D09-Q04). |
| VAL-003 | POLICY | Maturity rule done; values unset. D09-Q03, D09-Q05. |
| VAL-004 | DONE | Vesting-locked stake is penalized like unlocked stake and can withdraw (penalties v1). |
| ECON-001 | POLICY | Utilization target unchanged. D01. |
| ECON-003 | POLICY | 40/30/30 and payouts done; `E_min` unset. D01-Q01, D03-Q01. |
| ECON-004 | PARTIAL | Conservation checked every block, from running account totals (gap 5 closed); vesting with penalties. |
| GOV-001 | NOT E04 | Rules done; values E05. |
| GOV-002 | DONE | Timelock, bound action, one execution, refunds. |
| GOV-004 | DONE | Claim fixed: linear stake weighting; the docs and whitepaper errata drop quadratic voting and decay. |
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
| MEM-003 | DONE | The implemented rule is normative (`docs/architecture/mempool-v1.md`). |
| MEM-004 | NOT E04 | T05. |
| ORC-001 | DONE | No oracle; gas price governed; utilization only (D01-Q02 approved). |
| ORC-002 | DONE | No reporters (D01-Q02 approved). |
| ORC-003 | DONE | Claim fixed: no outlier slashing (security model, errata). |
| ORC-004 | DONE | Only bounded fee values and validator limits governable. |
| UPG-001 | NOT E04 | E06 provenance. |
| UPG-002 | POLICY | Height activation built; a separate 3-of-5 upgrade custodian group and a fixed minimum notice approved; custodians, notice length and the production upgrade policy are E05 and E06. |
| UPG-003 | PARTIAL | Gap 11 closed (M-a, M-b): the signed in-process and process tests run in CI. A baseline from an earlier release, the hardened launch and acceptance belong to T03, T06 and T07. |
| UPG-004 | NOT E04 | T05, T06. |
| API-001 | GAP 9 | Gap 9 closed (I-a, I-b): checked inventory `docs/architecture/interfaces-v1.json`; version fields everywhere; typed reads; no ABCI events by decision; acceptance is T07's. |
| API-002 | PARTIAL | Gap 10 closed (R-a): client and operator socket allowlists, pinned limits, error semantics and the gateway contract (`docs/architecture/rpc-controls-v1.md`), whose TLS gap 19 replaces with the client channel; public topology and rate values remain D12-Q01; acceptance is T07's. |
| API-003 | GAP 8 | Gap 8 closed (K-a to K-d): SDK and CLI on the consensus chain; SDK header verification later (decision 3); acceptance is T07's. |
| API-004 | NOT E04 | T04, T05. |
| BRG-001 | DONE | Bridge excluded in code and POST MAINNET (D07-Q01); fees bootstrapped by genesis DRT (D08-Q02 policy). |
| BRG-002 | DONE | N/A: bridges are POST MAINNET (D07-Q01). |
| BRG-003 | DONE | Claim fixed: the security model's bridge boundary section. |
| BRG-004 | NOT E04 | P02. |
| OBS-002 | PARTIAL | Core metrics written as text files (gap 7 closed); thresholds and routing open (D12-Q02). |
| OBS-003 | PARTIAL | Runbooks for every class (gap 15 closed, `docs/operations/`), including restart on a fixed release after a halt (gap 18 closed); roles, thresholds, custodians and channel open (D12-Q02, D14-Q03, E05). |
| PERF-001 | NOT E04 | T05. |
| PERF-002 | NOT E04 | T05. |
| PERF-003 | POLICY | Gap 13 closed: unit limits are required render inputs, swap and core dumps are off, and adapter limits can be lowered at run time. Values are D06-Q02 and D12-Q01. |
| PERF-004 | NOT E04 | T05. |
| ASSUR-003 | DONE | See claim fixes. |

## Conflicts

| ID | Class | Finding |
| --- | --- | --- |
| AC-001 | POLICY | Observation contract v1 approved (D01-Q02); controller values remain (D01-Q01). |
| AC-002 | RESOLVED | Every fee burned (P01, 27 Sep); the minimum fee resists spam only. |
| AC-003 | RESOLVED | Penalties v1 (D09-Q04, D09-Q05); the rate is an E05 value. |
| AC-004 | RESOLVED | CometBFT finality; gap 1 closed, and the errata correct the papers' checkpoint and LMD-GHOST model. |
| AC-005 | NOT E04 | Engineering part resolved (governance v1); values E05, signers P02. |
| AC-006 | RESOLVED | Linear stake weighting; the decay and delegation claims are corrected. |
| AC-007 | RESOLVED | Algorithms change only by root-signed upgrade; the registry wording is corrected. |
| AC-008 | NOT E04 | E05 (D08). |
| AC-009 | RESOLVED | LBP, wrapped USDC, the Airlock and bridges are POST MAINNET (D07-Q01); genesis DRT bootstraps fees (D08-Q02). |
| AC-010 | RESOLVED | No oracle at launch (D01-Q02, D07-Q01). |
| AC-011 | POLICY | Horizon equals maturity; values D03-Q01, D09-Q03, D01-Q01. |
| AC-012 | NOT E04 | E01 closed at source; T01, T02. |
| AC-013 | RESOLVED | Fees move to the burn counter; withheld zero at commit. |
| AC-014 | RESOLVED | The PDFs are unchanged; `mainnet/docs/docs/whitepapers.md` lists their errata. |

## Claim fixes

All done in the E04 claim-fix batch (29 September 2026). The whitepaper
PDFs are unchanged; `mainnet/docs/docs/whitepapers.md` lists their errata,
and `mainnet/docs/public-surface.json` now pins the corrected statements.

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
- Also corrected: the other `mainnet/docs/docs` pages that repeated a claim;
  `launch/PQC_ARCHITECTURE.md` (peer transport v2, no SecretConnection or
  remote signer); `docs/architecture/modular-node.md` (storage helpers and the
  gas crate removed).

## E04 exit (30 September 2026)

All 20 engineering gaps are closed, and every E04 policy question has its
rule decided. What remains is E05 values and records, and E06 release
acceptance; decision IDs refer to `launch/MAINNET_DECISION_REGISTER.json`.

- **E05 values:** fee prices, resource costs, limits and the creation fee
  (D04-Q01, D04-Q02, D10-Q02); the issuance controller and epoch length
  (D01-Q01, D03-Q01); the DRT bootstrap amount (D08-Q02); the penalty rate,
  evidence limits and margins (D09-Q03, D09-Q04); `min_self_bond`,
  `max_active` and governed-parameter bounds (D09-Q01, D11-Q03); governance
  thresholds, deposit, periods and timelock (D11-Q02); block time,
  timeouts, capacity and state-sync trust inputs (D06-Q02); the upgrade
  notice length and emergency timing windows (D11-Q03).
- **E05 records:** beneficiaries, vesting and the DRT bootstrap rows (D08-Q01,
  D08-Q03); the treasury recipient (D02-Q02); the initial validators and
  operators (D09-Q02); signing custodians, including the emergency and
  upgrade groups (D10-Q03, D11-Q03); topology, hosts and service objectives
  (D12-Q01 to D12-Q03); chain identity and the genesis digest (D13-Q01,
  D13-Q02); the production account and signing allowlists (D10-Q01).
- **E05 and E06 engineering:** the production upgrade policy (schema 2) with
  its threshold and notice; production activation of the local
  qualification profiles; the exact engine release (D06-Q01).
- **E06 and T:** release targets and acceptance (D14-Q01 to D14-Q03); the
  independent protocol review (P02).

## First live CI run (27 September)

The first GitHub Actions run after the billing fix (run 36324670835): Go
engine and SDK passed; the node job stopped at the G35 step (fetch ordering)
and the E01 inventory was stale; E02 native passed every check except
`filter.owned_good_request_under_apparmor` (the helper under the production
unit's `NoNewPrivileges`, syscall filter and AppArmor profile). Fixes and a
diagnostic are in #260; E02 stays open until that check passes.
