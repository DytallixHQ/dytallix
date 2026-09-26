# Mainnet artifact gap report

Captured: 2026-09-10T22:42:44.476667+00:00

**Decision: NO GO.** The allocation-policy reference is qualified for its stated scope. No production binary or network is qualified. A Git push would publish source. It would not create a mainnet network.

The register contains **64 artifacts**: 24 required final files and 40 source, release, operator and qualification packages.

| State | Count |
|---|---:|
| HAVE_QUALIFIED | 1 |
| HAVE_UNQUALIFIED | 7 |
| DRAFT | 14 |
| MISSING | 35 |
| BLOCKED | 7 |

At capture, **9/24** exact final filenames exist in the launch root. **0/7** full simulated launches have qualified. Counts measure recorded evidence. They do not measure engineering completion.

## Scope and status

Eight pinned repository snapshots, Batch 1 review of 18 active changed paths, fresh local baseline tests, requirements reconciliation, prior GitHub search records, and authorized read-only observation of one Hetzner host. No deployment, push, canonical genesis, or mainnet launch. Missing means not established within this evidence scope. Batch 2 adds five specification drafts, official consensus candidate source review, three source documentation corrections, repair mapping and offline input validation. No runtime or server changes. Batch 3 implements five source-file changes and records a fresh full local workspace run with three resolved failures and 11 open failures. No deployment or release qualification. Batch 4 implements incremental queue admission and corrects measurement validity. Optimized timing passes; 11 default-profile failures remain. No production qualification. Batch 5 repairs four governance failures and retains one policy-dependent failure. The full default workspace has seven failures. No production qualification. The approved Batch 5 eligibility follow-up completes five local governance repairs. Six default failures remain; no production qualification. Batch 6 implements four approved items as components. Production reward integration remains open; three default failures remain.

Source references use exact commits from `evidence/REPOSITORY_MANIFEST.json`. Local planning files use captured SHA-256 values in the JSON register. Concurrent edits can change these files after capture. The active node still has committed baseline `5e86d1046ae2423b98f125ee8fd09f7d7d837fc7`; Batch 1 binds its uncommitted input to an exact source hash and archive. This input is not a release commit.

- **HAVE_QUALIFIED:** Artifact has complete reviewed acceptance evidence for its declared scope.
- **HAVE_UNQUALIFIED:** Artifact or source exists, but required production acceptance evidence is incomplete.
- **DRAFT:** A planning artifact or unfinished implementation exists.
- **MISSING:** The required deliverable or evidence bundle was not found in the inspected scope. Dependencies may also block its creation.
- **BLOCKED:** A known protocol, implementation or operational prerequisite prevents candidate qualification.

## Batch 10 local transaction admission

[Batch 10 implementation](batch-10/REPORT.md) rejects already processed signed transactions during admission, recheck and proposal selection. Full workspace: **1013 passed, 3 failed, 2 ignored**. New failures: **0**. The six-node network checks durable rejection before and after restart, with zero later transaction inclusions. Nonce and fee-balance admission policy remain unchanged. Complete engine evidence coverage and production penalty rules remain open. Mainnet remains **NO GO**.

## Batch 9 local penalty custody and withdrawals

[Batch 9 implementation](batch-9/REPORT.md) adds local duplicate-vote accounting, H+2 penalty settlement, a separate penalty reserve, and owner withdrawals with recovery checks. Full workspace: **1010 passed, 3 failed, 2 ignored**. New failures: **0**. The six-node network checks ordinary validator changes and withdrawals with zero engine evidence. Synthetic accounting tests do not qualify cryptographic engine faults. Production penalty rules, complete evidence coverage and transaction admission filtering remain open. Mainnet remains **NO GO**.

## Batch 8 local validator lifecycle

[Batch 8 implementation](batch-8/implementation/REPORT.md) implements the seven approved validator lifecycle rules. It adds permissioned registration, exact stake power, H+2 activation, key rotation, owner-specific unbonding and atomic recovery. Full workspace: **986 passed, 3 failed, 2 ignored**. New failures: **0**. The changing-validator local network test passes. Withdrawals remain disabled until evidence and penalty settlement are qualified. Production values remain unset. Mainnet remains **NO GO**.

## Batch 7 local consensus integration

[Batch 7](batch-7/REPORT.md) is complete for its approved local scope. CometBFT v0.40.0 now drives atomic block, issuance and reward commits through a Rust application. The four-validator test passes agreement, quorum loss, restart and recovery checks. Verified signed headers confirm four saved application hashes.

Full workspace: **950 passed, 3 failed, 2 ignored**. Focused adapter checks: **12 passed**. Go tool tests pass. There are **22 additional passes** and **0 new failures**. The three baseline failures remain open. Source SHA-256: `e31e8294e7dad3ed76f56f1db8935ee9f0f76a619b5fb72f11ec77adb8a11f19`.

Mainnet remains **NO GO**. The fixture has a static validator set and synthetic observations. Production operators, custody, validator changes, authenticated observations, peer transport and release qualification remain open. All 34 production gates remain open; zero of seven full launch simulations are qualified. Sections below retain historical results.

## Historical Batch 6 issuance timing follow-up

[The issuance timing follow-up](batch-6/issuance-timing/REPORT.md) implements the approved epoch-to-block rules in the development adapter. It joins controller transitions, observations, all four issuance pools, staking rewards, claims, supply and block records in one atomic write. Full workspace: **928 passed, 3 failed, 2 ignored**. Focused checks: **227 passed**. New failures: **0**.

Epoch length and epoch-zero budget require explicit genesis inputs. Later epochs require a completed-parent-epoch observation. The adapter rejects missing or invalid observations before writes. It schedules only each block's share and retains separate issuance, staking rounding and inactive reserves. No reserve has sweep authority. Legacy issuance methods and the node timer cannot activate this mode.

Production numerical parameters, consensus finality, authenticated observations, pause/resume policy, beneficiary custody and release qualification remain open. Mainnet remains **NO GO**. Batch 7 has not started. Source digest: `00a209afbbda2045286b64fa5d8a5237a2f67fbca40b185ba755075a6a89c4b9`.

## Historical Batch 6 claim recovery follow-up — locally checked; production open

[The claim recovery follow-up](batch-6/claim-recovery/REPORT.md) adds two signed-claim failure/reopen/retry tests and prepared adaptive journal writes with five tests. Final focused checks: **199 passed**. Full workspace: **900 passed, 3 failed, 2 ignored**. New failures: **0**. Current baseline failures: **B1-T002, B1-T003, B1-T009**. Original legacy staking source and test remain unchanged.

The journal preparation preserves controller behavior and binary encoding. It adds no minting or issuance integration. The economic mapping decision remains pending. Production finality and epoch-to-block issuance remain undefined or unqualified. Mainnet remains **NO GO**. Batch 6 production qualification is open; Batch 7 has not started. Source digest: `ac6c18e16535f3cb26bdf5229f5f4ebb941227381baba7503c11f727de775c73`.

## Historical Batch 6 integration follow-up

The user approved the three reward implementation recommendations after requesting a small-stake rounding measurement. [The follow-up](batch-6/integration-followup/REPORT.md) records the approval, measurement and development adapter work. Focused checks report **189 passed**. The full workspace reports **893 passed, 3 failed, 2 ignored**, with no new failures. There are 42 additional passes against the prior result. B1-T009 and the two timing failures remain. Original legacy reward source and test are unchanged. Source digest: `2f01f19cd021597da0813486930ff5617c05e4fed4016a1c97d2cd5731e53969`.

The approved rules use each finalized block and its parent-state eligible stake. They assign funded bonded stake to its owner, exclude inactive or jailed validators, retain inactive-period budgets and reject unconverted legacy reward state. Current implementation uses an explicit development fixture adapter. It does not establish consensus finality. Production activation remains disabled. The selected adaptive controller's mapping from epoch issuance to block budgets remains undefined. Mainnet remains **NO GO**; Batch 7 has not started.

## Historical Batch 6 component result

[Batch 6](batch-6/REPORT.md) implements the four [approved items](batch-6/APPROVAL.json): six-decimal native units, exact allocation from supplied stake snapshots, a separate rounding reserve and explicit vesting inputs. Native conversion and genesis parsing use the shared units. The new local reward ledger has no production caller or token-transfer authority. The native importer rejects supplied unsupported lock schedules.

Full workspace: **851 passed, 3 failed, 2 ignored**; no new failures. Three legacy genesis fixtures now pass. B1-T009 and two timing failures remain. Batch 6 is not complete; Batch 7 has not started. Production snapshot, storage and payout integration remain open. Current source digest: `b67fa6ccd5445126deac2f2b16af072a82b553af1a8b9b8c4def44d4ed656c04`. Mainnet remains NO GO.

## Batch 5 eligibility follow-up

[The follow-up](batch-5/eligibility-followup/REPORT.md) implements the user-approved bonded-stake-only rule. All five local governance failures pass. Current workspace: **827 passed, 6 failed, 2 ignored**. Production gates remain open.

## Batch 5 governance implementation

[Batch 5](batch-5/BATCH_5_REPORT.md) repairs four governance failures. One awaits an [eligibility decision](batch-5/GOVERNANCE_DECISION.md). Full default workspace: **825 passed, 7 failed, 2 ignored**. Artifact states and mainnet gates remain unchanged.

## Batch 4 implementation

[Batch 4](batch-4/BATCH_4_REPORT.md) implements queue admission repairs. All five optimized performance tests pass at unchanged limits. The default workspace reports **816 passed, 11 failed, 2 ignored**; two debug timing failures remain open. Artifact states and mainnet gates remain unchanged.

## Batch 3 implementation

[Batch 3](batch-3/BATCH_3_REPORT.md) resolves three local baseline failures. The fresh workspace reports **812 passed, 11 failed, one ignored**. Five source files changed; no new failure appeared. Artifact states and mainnet qualification gates remain unchanged.

## Batch 2 decision package

[Batch 2](batch-2/BATCH_2_REPORT.md) supplies concrete consensus, identity, economic and genesis drafts. The [decision index](batch-2/DECISION_REVIEW.md) lists approval dependencies. Drafts and local tool checks do not close production gates.

## Approved allocation input

The user identified [the live tokenomics page](https://dytallix.com/developers/tokenomics) as the approved mainnet allocation source. The parent task verified the displayed allocation values. This report qualifies the allocation-policy reference only.

| DGT allocation | Share | DGT amount |
|---|---:|---:|
| Ecosystem growth | 30% | 300,000,000 |
| Team and advisors | 20% | 200,000,000 |
| Public sale | 15% | 150,000,000 |
| Private sale | 15% | 150,000,000 |
| Reserve | 20% | 200,000,000 |
| Total | 100% | 1,000,000,000 |

The page gives DRT reward shares of 40% to validators, 30% to stakers and 30% to treasury. Recipient identities, custody proofs, enforceable vesting, initial stake and initial DRT fee funding remain separate requirements. The website issuance curve differs from the selected corrected adaptive controller. Allocation approval does not approve a controller replacement.

Historical assessments and GitHub findings predate this user clarification. Their claims that an approved high-level allocation was absent are superseded. Their recipient, custody, runtime and qualification gaps remain open.

## Source and implementation evidence

| Repository | Inspected commit | Mainnet use |
|---|---|---|
| dytallix-node | `5e86d1046ae2423b98f125ee8fd09f7d7d837fc7` | Required chain runtime; development producer remains unqualified. |
| dytallix-pqc | `f3ed0a30b208cfe9878c62c5474749c75ea62f35` | Cryptographic source; select exact active algorithms and qualification scope. |
| dytallix-sdk | `2ae360498a2d6b0e95136934d313ce2a458af105` | Required wallet, CLI and developer client. |
| dytallix-contracts | `cb2844a1e2dff9e9859a53162f78cd4dbc882bb8` | Reference and possible enabled components; source presence does not establish native enforcement. |
| dytallix-faucet | `33c6aa29b927ff5b8ed1a5e130e0b3e2dd859878` | Keep testnet funding outside canonical mainnet. |
| dytallix-docs | `c2c0357a2ed7b7a821f7b3f08c4e3f6963713da1` | Public protocol and operator documentation. |
| dytallix-explorer | `4a38f3c07c39c272e7664e0995a8cced7530d337` | Minimal explorer surface; define a mainnet consumer requirement before release. |
| .github | `4f97080abca29162b070a23ba0878e15ff58bf4c` | Organization documentation and shared policy. |

The selected fast-node source commits blocks with a local timer. No integrated distributed consensus or finality certificate was established. Mandatory staking, liquid reward claims and governance remain absent or disabled in this selected path. The corrected adaptive controller now drives the explicit development issuance adapter. Production activation remains disabled. Production scope cannot substitute deferred governance or scheduled issuance for the user-required model.

Fresh Batch 1 node tests report **801 passed, 14 failed, 1 ignored**. The 14 failure names match the prior baseline. Supporting checks passed within the scopes in [the test report](batch-1/TEST_BASELINE.md). The active register now specifies seven full launches and all final checkpoints. Historical local run-002 passed two prerequisites and blocked 18; it did not execute a full launch.

## Required final files

Each row has a separate acceptance test, owner role, dependency list and evidence hash in the JSON register. Every owner remains unassigned.

| ID | Required filename | State | Smallest next action |
|---|---|---|---|
| F01 | MAINNET_READINESS_REPORT.md | HAVE_UNQUALIFIED | Batch 7 local implementation, network evidence and full workspace results are recorded. Production qualification remains open. |
| F02 | `MAINNET_V1_SPEC.md` | DRAFT | Resolve the consensus decision and list all permanent protocol fields. |
| F03 | `VALIDATOR_ARCHITECTURE.md` | DRAFT | Select the consensus fault model before assigning validator count and voting power. |
| F04 | `VALIDATOR_OPERATIONS.md` | MISSING | Write the operating procedures against the selected validator implementation. |
| F05 | `VALIDATOR_SECURITY_MODEL.md` | MISSING | Define signer custody, anti-double-signing state and replacement authority. |
| F06 | `DRT_TOKENOMICS.md` | DRAFT | Approve the exact EC decisions and integrate DRT settlement against conservation tests. |
| F07 | `DGT_TOKENOMICS.md` | DRAFT | Obtain approved beneficiary and custody records; resolve remaining issuance and governance rules. |
| F08 | `TOKEN_SUPPLY_MODEL.md` | DRAFT | Review custody and remainder rules, then implement atomic supply reconciliation. |
| F09 | `PQC_ARCHITECTURE.md` | DRAFT | Approve the exact identity and consensus role contract; qualify shared implementations. |
| F10 | `PQC_TEST_REPORT.md` | MISSING | Map each required cryptographic check to one frozen candidate test. |
| F11 | `GENESIS_SPEC.md` | DRAFT | Resolve protocol inputs and fill authenticated beneficiary/operator records before generator implementation. |
| F12 | `GENESIS_CEREMONY.md` | MISSING | Specify input collection, independent generation and operator digest verification. |
| F13 | `GENESIS_VALIDATION.md` | MISSING | Build a validation checklist for the complete selected schema. |
| F14 | `GENESIS_MANIFEST.json` | DRAFT | Replace null fields only from verified candidate and ceremony records. |
| F15 | `VALIDATOR_DEPLOYMENT.md` | MISSING | Document one clean-server deployment path with versioned inputs. |
| F16 | `VALIDATOR_RUNBOOK.md` | MISSING | Write startup, health, halt, incident and maintenance actions for the selected service. |
| F17 | `VALIDATOR_RECOVERY.md` | MISSING | Specify signer fencing and state verification before a replacement starts. |
| F18 | `WALLET_TEST_REPORT.md` | MISSING | Map the user-required wallet lifecycle to exact node and client versions. |
| F19 | `DISASTER_RECOVERY.md` | MISSING | Approve recovery objectives and identify backup contents and trust anchors. |
| F20 | `FAILURE_TEST_REPORT.md` | MISSING | Prepare a requirement matrix for state, server and network failures in isolated staging. |
| F21 | `ECONOMIC_SECURITY_REPORT.md` | MISSING | Translate approved supply and lifecycle rules into reviewable invariant scenarios. |
| F22 | `GOVERNANCE_SECURITY_REPORT.md` | MISSING | Freeze electorate, snapshots, quorum, treasury and upgrade authority boundaries. |
| F23 | `FINAL_MAINNET_SIMULATION_REPORT.md` | MISSING | Implement and qualify the full rehearsal driver after its production prerequisites pass. |
| F24 | `FINAL_GO_NO_GO_REPORT.md` | MISSING | Define the evidence index and required decision signers. |

## Source, release and operational packages

| ID | Artifact | State | Current evidence and gap |
|---|---|---|---|
| A01 | Pinned source inventory | HAVE_UNQUALIFIED | Eight committed snapshots and archive hashes exist. No approved cross-repository mainnet candidate. |
| A02 | Active address and rehearsal changes | DRAFT | Batch 7 adds local CometBFT integration across 22 paths; source remains uncommitted. Production activation, validator lifecycle and release qualification remain open. |
| A03 | Production build inputs | HAVE_UNQUALIFIED | Rust 1.88.0, Cargo locks and node CI exist. Native dependencies and runner environment lack an immutable complete build specification. |
| A04 | Local node binary | HAVE_UNQUALIFIED | Pinned Go engine/tools and Rust application build locally. The passing network executable is retained by hash. No qualified Linux release, production image or independent reproducibility evidence. |
| A05 | Approved allocation policy | HAVE_QUALIFIED | User identifies the live tokenomics page as approved allocations. DGT: fixed 1 billion; ecosystem 30%, team/advisors 20%, public sale 15%, private sale 15%, reserve 20%. DRT rewards: validators 40%, stakers 30%, treasury 30%. No gap remains for this allocation-policy-reference scope. A12 tracks executable recipients, custody, vesting and DRT genesis funding. The page issuance curve does not replace the selected adaptive controller. |
| A06 | Consensus and peer runtime | BLOCKED | CometBFT v0.40.0 and a Rust application pass local ML-DSA-65 validator agreement and recovery tests. Production consensus policy, validator changes, operators, custody and classical peer-transport boundary remain open. |
| A07 | Complete DRT/DGT economic runtime | BLOCKED | Approved epoch timing, rewards and signed claims now settle atomically from locally finalized engine blocks. Production observations, calibration, allocation settlement, vesting and complete economic lifecycle remain unqualified. |
| A08 | PQC, address and account authorization runtime | BLOCKED | Existing signature source and address vectors plus Batch 2 identity/algorithm decision packet exist. Node/SDK formats and labels differ. Persistent authorization and role-specific production policy remain incomplete. |
| A09 | Full initial governance and upgrade authority | BLOCKED | Dormant governance source exists; selected startup rejects activation. Required initial governance, electorate and durable authority are not integrated. |
| A10 | Actual validator operator and power register | DRAFT | A local schema exists with zero registered actual operators and zero approved validators. Operator acceptance, control groups, public keys and voting powers are absent from this record. |
| A11 | Key custody and signing-safety evidence | MISSING | Existing-key startup checks and custody planning text exist. No candidate-bound operator custody attestations or recovery/fencing evidence. |
| A12 | Executable allocations and vesting schedule | DRAFT | Approved allocation shares and six-decimal amounts, explicit vesting-input requirements and the Batch 6 input checker exist. Beneficiaries, custody, schedule values, initial stake, DRT funding, canonical generation and runtime lock enforcement remain missing or unqualified. |
| A13 | Production infrastructure inventory | DRAFT | Read-only SSH verified one Hetzner host and six service observations. The node uses PM2 as root. Provider-wide inventory, region, account boundary, named owners, fault domains, custody, backups and monitoring acceptance remain incomplete. Existing host is not mainnet-qualified. |
| A14 | Canonical genesis bytes and state vectors | MISSING | Development parser and testnet examples exist; GENESIS_MANIFEST explicitly is not importable. No final mainnet genesis bytes, complete schema or operator-agreed genesis digest. |
| A15 | Mainnet candidate tags and release manifest | MISSING | Node package is 0.1.0; GitHub search found no node tag or published release. SDK v0.1.0 exists independently. No v1.0.0-rc1, controlled rc2 upgrade candidate or frozen final candidate artifact set. |
| A16 | Production node container | MISSING | Faucet Dockerfile exists; no node Dockerfile or node image identified. A faucet image does not supply the requested production node container. |
| A17 | Checksums, signatures and release provenance | MISSING | Source snapshot hashes exist; SDK release workflow has an attestation pattern. No production-node checksum set, publisher signatures or builder provenance. |
| A18 | Clean-server deployment automation | HAVE_UNQUALIFIED | systemd and PM2 templates plus a path preflight script exist. No complete repeated clean-server validator deployment on the production topology. |
| A19 | Wallet, CLI and SDK release | BLOCKED | Rust SDK and CLI source plus release workflow exist. Required key encryption, persistence/recovery, final transaction status and mainnet compatibility remain unqualified. |
| A20 | RPC and developer interface contract | HAVE_UNQUALIFIED | HTTP, REST-style and WebSocket interfaces and documentation exist. No frozen mainnet API contract, finality semantics or gRPC scope decision. |
| A21 | Production RPC service and network configuration | MISSING | Historical testnet endpoint statements and configuration templates exist. No current mainnet RPC host, DNS/TLS, peer/sentry configuration, capacity or failover evidence. |
| A22 | Monitoring, alerts and on-call package | BLOCKED | Node metrics and alert source exist. Validator/oracle inputs include placeholders; no collection, dashboard, delivery, acknowledgement or escalation proof. |
| A23 | Backup, snapshot and restore package | MISSING | Local startup consistency checks and logical controller snapshots exist. No authenticated full-node snapshot format, off-host backup manifest, retention or completed restore evidence. |
| A24 | Permanent-history upgrade package | BLOCKED | Startup rejects incompatible policy; legacy relaunch guide requires a reset. No accepted migration, activation, compatibility or rollback procedure that preserves canonical history. |
| A25 | SBOM and dependency vulnerability report | MISSING | Node CI generates a dependency inventory command; lockfiles are retained. No candidate-linked software bill of materials or reviewed dependency report was supplied. |
| A26 | Complete candidate test qualification bundle | HAVE_UNQUALIFIED | Batch 7 workspace: 950 passed, 3 failed, 2 ignored; 0 new failures. Twelve adapter tests and local network checks pass. Three baseline failures and production qualification remain open. |
| A27 | Independent assurance and P0 closure | MISSING | Phase 0 static assessments and decision records exist. No independent protocol, cryptographic, economic and security acceptance set for the final candidate. |
| A28 | Full seven-run harness and scenario specification | DRAFT | Active register now requires R01-R07, all user-specified full scenarios and final checkpoints. L01-L02 prerequisites remain separate from required L03-L20 scenarios and R01-R07 full runs. No full distributed rehearsal driver or qualified candidate. R07 spans 30 hours; week-four placement and overlap remain undecided. |
| A29 | Full simulation run 1: CLEAN GENESIS | MISSING | No successful full production-equivalent run established. Local component checks are recorded separately. Run evidence must follow the exact corresponding SIMULATION DAY section in USER_LAUNCH_REQUIREMENTS.txt. Run 7 must retain T-minus 6 hours, T-minus 1 hour, T-zero, T-plus 15 minutes, 1 hour, 6 hours and 24 hours checks. |
| A30 | Full simulation run 2: INFRASTRUCTURE FAILURE | MISSING | No successful full production-equivalent run established. Local component checks are recorded separately. Run evidence must follow the exact corresponding SIMULATION DAY section in USER_LAUNCH_REQUIREMENTS.txt. Run 7 must retain T-minus 6 hours, T-minus 1 hour, T-zero, T-plus 15 minutes, 1 hour, 6 hours and 24 hours checks. |
| A31 | Full simulation run 3: ADVERSARIAL NETWORK | MISSING | No successful full production-equivalent run established. Local component checks are recorded separately. Run evidence must follow the exact corresponding SIMULATION DAY section in USER_LAUNCH_REQUIREMENTS.txt. Run 7 must retain T-minus 6 hours, T-minus 1 hour, T-zero, T-plus 15 minutes, 1 hour, 6 hours and 24 hours checks. |
| A32 | Full simulation run 4: PRODUCTION UPGRADE | MISSING | No successful full production-equivalent run established. Local component checks are recorded separately. Run evidence must follow the exact corresponding SIMULATION DAY section in USER_LAUNCH_REQUIREMENTS.txt. Run 7 must retain T-minus 6 hours, T-minus 1 hour, T-zero, T-plus 15 minutes, 1 hour, 6 hours and 24 hours checks. |
| A33 | Full simulation run 5: OPERATOR ERROR | MISSING | No successful full production-equivalent run established. Local component checks are recorded separately. Run evidence must follow the exact corresponding SIMULATION DAY section in USER_LAUNCH_REQUIREMENTS.txt. Run 7 must retain T-minus 6 hours, T-minus 1 hour, T-zero, T-plus 15 minutes, 1 hour, 6 hours and 24 hours checks. |
| A34 | Full simulation run 6: PRODUCTION LOAD AND ECONOMIC ACTIVITY | MISSING | No successful full production-equivalent run established. Local component checks are recorded separately. Run evidence must follow the exact corresponding SIMULATION DAY section in USER_LAUNCH_REQUIREMENTS.txt. Run 7 must retain T-minus 6 hours, T-minus 1 hour, T-zero, T-plus 15 minutes, 1 hour, 6 hours and 24 hours checks. |
| A35 | Full simulation run 7: FINAL MAINNET DRESS REHEARSAL | MISSING | No successful full production-equivalent run established. Local component checks are recorded separately. Run evidence must follow the exact corresponding SIMULATION DAY section in USER_LAUNCH_REQUIREMENTS.txt. Run 7 must retain T-minus 6 hours, T-minus 1 hour, T-zero, T-plus 15 minutes, 1 hour, 6 hours and 24 hours checks. |
| A36 | Deployment and publication acceptance package | MISSING | Historical testnet records and a fresh one-host observation exist. Running binary digest and checkout revision were captured separately. No attested source-to-running-binary mapping, approved mainnet deployment, operator acknowledgement or public endpoint acceptance. |
| A37 | Daily status and weekly readiness reports | DRAFT | Batch 7 local implementation and current test evidence are recorded. Production qualification remains open. No full simulated launch is qualified. |
| A38 | Independent binary reproducibility report | MISSING | Pinned Rust and locks plus one local selected binary build exist. No two independent clean builders have established identical production binary digests. |
| A39 | Production capacity and PQC benchmark report | MISSING | Component performance tests exist; prior baseline includes performance-related failures. No production-equivalent sustained/peak throughput, propagation, block/mempool capacity or hardware envelope. |
| A40 | Frozen production configuration bundle | MISSING | Runtime defaults, deployment templates and protocol drafts exist. No candidate-bound bundle fixes chain identity, features, protocol parameters, adaptive inputs, gas/fee settings, peer settings and service roles. |

## Acquisition and acceptance plan

All owner roles below remain unassigned. The JSON register gives the exact path, revision or captured digest for each evidence item.

### A01 — Pinned source inventory

**Next action:** Select the required repositories and reconcile active changes before a release freeze.
**Owner role:** Release engineer. **Dependencies:** None.
**Acceptance:** Reviewed manifest binds repository, commit, source digest, features, target and license set.
**Destination:** Versioned launch repository and release assets.

### A02 — Active address and rehearsal changes

**Next action:** Continue implementation against the approved rules and reconcile source with verified upstream history.
**Owner role:** Protocol and release engineer. **Dependencies:** A01.
**Acceptance:** Reviewed source revision, clean status and tests tied to that revision.
**Destination:** Source repository.

### A03 — Production build inputs

**Next action:** Record target, features and native packages in a pinned build environment.
**Owner role:** Release engineer. **Dependencies:** A01.
**Acceptance:** Complete compiler, linker, native dependency and build-image digests reproduce the binary.
**Destination:** Source repository and release manifest.

### A04 — Local node binary

**Next action:** Build the frozen production candidate for the approved target after required repairs and protocol decisions.
**Owner role:** Release engineer. **Dependencies:** A03, A06, A07, A08, A09.
**Acceptance:** Candidate binary matches independent clean builds and passes target-platform checks.
**Destination:** Signed release assets; install on validator and RPC hosts.

### A05 — Approved allocation policy

**Next action:** Preserve the qualified policy reference and use its exact shares as inputs to A12.
**Owner role:** Protocol economist. **Dependencies:** None.
**Acceptance:** Dated source extraction, explicit user approval and reconciled bucket totals establish the allocation-policy reference only. No binary, genesis execution or network qualification is implied.
**Destination:** Versioned allocation record and public tokenomics documentation.

### A06 — Consensus and peer runtime

**Next action:** Resolve the candidate acceptance criteria, then implement and qualify the engine/application boundary.
**Owner role:** Consensus lead. **Dependencies:** A01.
**Acceptance:** Reviewed protocol and candidate demonstrate common finality, deterministic replay, synchronization and validator transitions under the approved fault model.
**Destination:** Node source, release binary and public protocol specification.

### A07 — Complete DRT/DGT economic runtime

**Next action:** Freeze initial DRT funding and exact custody transitions against the approved allocation policy.
**Owner role:** Runtime and economics lead. **Dependencies:** A05, A06.
**Acceptance:** Full economic lifecycle reconciles every base unit across mint, fee, reward, burn, stake, penalty, vesting, restart and upgrade.
**Destination:** Node source and release binary.

### A08 — PQC, address and account authorization runtime

**Next action:** Approve identity, role algorithms, signed envelope, migration and recovery rules; implement shared codecs and authorization.
**Owner role:** Cryptography and protocol lead. **Dependencies:** A02.
**Acceptance:** Known-answer and cross-implementation vectors plus complete account/key authorization, serialization and migration checks on exact artifacts.
**Destination:** Node/SDK source and versioned public vector bundle.

### A09 — Full initial governance and upgrade authority

**Next action:** Specify electorate, snapshot timing, proposal actions and execution limits.
**Owner role:** Governance lead. **Dependencies:** A06, A07.
**Acceptance:** Approved proposal-to-execution lifecycle, treasury bounds and persistent upgrade authorization pass on candidate.
**Destination:** Node source and public governance specification.

### A10 — Actual validator operator and power register

**Next action:** Identify intended operators and collect non-sensitive acceptance and ownership proof references.
**Owner role:** Validator operations lead. **Dependencies:** A06.
**Acceptance:** Approved public identities, keys, powers, fault domains and signed genesis acknowledgements for the selected set.
**Destination:** Public validator summary; restricted operator detail store.

### A11 — Key custody and signing-safety evidence

**Next action:** Define public evidence fields and select custody owners; obtain attestations through protected operator records.
**Owner role:** Custody lead. **Dependencies:** A08, A10.
**Acceptance:** Validated public key ownership, storage controls, independent access, anti-double-signing persistence and recovery drill.
**Destination:** Protected operator store; public fingerprints and opaque proof references only.

### A12 — Executable allocations and vesting schedule

**Next action:** Complete the input records and approvals. Generate canonical bytes only after the engine and application schema are frozen.
**Owner role:** Economics and custody lead. **Dependencies:** A05, A08, A10, A11.
**Acceptance:** Base-unit allocations sum exactly; ownership proofs, vesting enforcement and initial fee funding pass with independent approval.
**Destination:** Public allocation schedule and genesis; restricted custody evidence.

### A13 — Production infrastructure inventory

**Next action:** Complete provider-wide records and named operator assignments. Design and accept the mainnet topology against the selected consensus model.
**Owner role:** SRE lead. **Dependencies:** A06, A10.
**Acceptance:** Approved inventory matches installed nodes, signer/control groups, network policy, capacity and failure-domain analysis.
**Destination:** Protected operator store; public topology summary.

### A14 — Canonical genesis bytes and state vectors

**Next action:** Implement the approved genesis schema and deterministic generator after protocol and allocation freeze.
**Owner role:** Protocol and release engineer. **Dependencies:** F11, A12, A10.
**Acceptance:** Independent generators match exact bytes, initialized state root, supply and validator-set commitments.
**Destination:** Immutable release assets and public network repository.

### A15 — Mainnet candidate tags and release manifest

**Next action:** Define a release manifest schema that pins all source, binary, image, genesis and client versions.
**Owner role:** Release engineer. **Dependencies:** A01, A03, A06, A07, A08, A09.
**Acceptance:** Immutable candidate tags and signed manifest resolve to exact tested artifacts and approved feature scope.
**Destination:** Source tags and release registry.

### A16 — Production node container

**Next action:** Create a node container build definition using the pinned production build inputs.
**Owner role:** Release engineer. **Dependencies:** A03, A04.
**Acceptance:** Published node image by digest matches candidate provenance and passes node identity, runtime restriction and health checks.
**Destination:** OCI container registry.

### A17 — Checksums, signatures and release provenance

**Next action:** Define the release signing authority and the manifest verification format.
**Owner role:** Release and custody lead. **Dependencies:** A11, A15, A16.
**Acceptance:** All candidate assets have verified checksums, trusted publisher signatures and source/build provenance.
**Destination:** Public release registry; private signing material remains in protected custody.

### A18 — Clean-server deployment automation

**Next action:** Select one service path and encode the missing OS, firewall, genesis, peer, logs and backup steps.
**Owner role:** SRE lead. **Dependencies:** A13, A15, A16, A17.
**Acceptance:** Independent operators repeatedly deploy verified artifacts from clean hosts and join the approved staging set.
**Destination:** Source repository; environment-specific protected operator configuration.

### A19 — Wallet, CLI and SDK release

**Next action:** Define supported wallet surfaces and reconcile SDK/node network, fee, address and receipt contracts.
**Owner role:** Wallet and SDK lead. **Dependencies:** A08, A15.
**Acceptance:** Supported platform packages pass the complete user-required wallet and developer lifecycle against the candidate.
**Destination:** Signed release registry; package registry if selected; public install docs.

### A20 — RPC and developer interface contract

**Next action:** Write the versioned supported method and error contract; explicitly decide gRPC scope.
**Owner role:** API and SDK lead. **Dependencies:** A06, A07, A08, A09.
**Acceptance:** Client compatibility tests cover every supported interface, complete writes and finality/error behavior.
**Destination:** Source repository and public developer documentation.

### A21 — Production RPC service and network configuration

**Next action:** Map required endpoint roles to the verified inventory and define capacity and failover targets.
**Owner role:** SRE lead. **Dependencies:** A13, A18, A20.
**Acceptance:** Configured services match candidate identity and survive required failover, load and availability checks.
**Destination:** Protected operator configuration; public endpoint catalog.

### A22 — Monitoring, alerts and on-call package

**Next action:** List each required signal and bind it to a real candidate measurement and on-call owner.
**Owner role:** SRE lead. **Dependencies:** A06, A07, A09, A13.
**Acceptance:** Controlled faults produce correct metrics, alerts, delivery, acknowledgement, escalation and recovery events.
**Destination:** Versioned monitoring configuration; restricted delivery routes and on-call records.

### A23 — Backup, snapshot and restore package

**Next action:** Define consistent backup contents, trust anchors, retention and recovery objectives.
**Owner role:** Storage and SRE lead. **Dependencies:** A06, A11, A13, A14.
**Acceptance:** Verified restore recovers finalized state and supply, preserves signer safety, and meets approved recovery limits.
**Destination:** Protected backup store; public format and hash verification instructions.

### A24 — Permanent-history upgrade package

**Next action:** Specify upgrade authority, activation boundary and migration manifest for RC1 to RC2.
**Owner role:** Protocol and release lead. **Dependencies:** A06, A09, A23.
**Acceptance:** Normal, late and failed-node upgrade cases preserve ledger invariants and permitted recovery boundaries.
**Destination:** Source migration scripts, release assets and protected execution records.

### A25 — SBOM and dependency vulnerability report

**Next action:** Generate a complete candidate SBOM including native libraries, and review dependency results.
**Owner role:** Release and security lead. **Dependencies:** A03, A15, A16.
**Acceptance:** Retained SBOM and dependency report match artifact digests; relevant findings have owner and disposition.
**Destination:** Release registry; restricted advisory details by controlled reference.

### A26 — Complete candidate test qualification bundle

**Next action:** Address reward conservation and genesis consistency; resolve dependent units and remainder rules before permanent mainnet changes.
**Owner role:** QA lead. **Dependencies:** A04, A06, A07, A08, A09, A19.
**Acceptance:** All mandatory test jobs pass with exact command, revision, environment, result, logs and independent review; exclusions are explicit.
**Destination:** Versioned evidence index and retained release/CI artifacts.

### A27 — Independent assurance and P0 closure

**Next action:** Assign independent reviewers and produce a candidate-linked scope and finding register.
**Owner role:** Independent assurance lead. **Dependencies:** F02, F08, F09, A15, A25, A26.
**Acceptance:** All critical findings close with verified evidence; unresolved ambiguities are zero for mandatory systems.
**Destination:** Protected review evidence with publishable final summaries.

### A28 — Full seven-run harness and scenario specification

**Next action:** Implement the seven-run driver and evidence bundle after production prerequisites pass. Resolve the schedule without removing checkpoints.
**Owner role:** Release QA lead. **Dependencies:** A15, A18, A22, A23, A24, A26, A27, A38, A39, A40.
**Acceptance:** Harness runs full candidate architecture from ceremony through economic operation, recovery and incident checks; output includes source/artifact/genesis hashes and reviewer records.
**Destination:** Source repository and qualification evidence store.

### A29 — Full simulation run 1: CLEAN GENESIS

**Next action:** Execute this full scenario only after the frozen candidate and rehearsal prerequisites pass.
**Owner role:** Release QA and operations lead. **Dependencies:** A28.
**Acceptance:** Record exact source, binaries, container, genesis, topology, custody references, scenario steps, timing, state/supply roots, observations, defects and independent decision. Every mandatory check passes.
**Destination:** Protected qualification evidence store; public redacted run summary.

### A30 — Full simulation run 2: INFRASTRUCTURE FAILURE

**Next action:** Execute this full scenario only after the frozen candidate and rehearsal prerequisites pass.
**Owner role:** Release QA and operations lead. **Dependencies:** A28, A29.
**Acceptance:** Record exact source, binaries, container, genesis, topology, custody references, scenario steps, timing, state/supply roots, observations, defects and independent decision. Every mandatory check passes.
**Destination:** Protected qualification evidence store; public redacted run summary.

### A31 — Full simulation run 3: ADVERSARIAL NETWORK

**Next action:** Execute this full scenario only after the frozen candidate and rehearsal prerequisites pass.
**Owner role:** Release QA and operations lead. **Dependencies:** A28, A30.
**Acceptance:** Record exact source, binaries, container, genesis, topology, custody references, scenario steps, timing, state/supply roots, observations, defects and independent decision. Every mandatory check passes.
**Destination:** Protected qualification evidence store; public redacted run summary.

### A32 — Full simulation run 4: PRODUCTION UPGRADE

**Next action:** Execute this full scenario only after the frozen candidate and rehearsal prerequisites pass.
**Owner role:** Release QA and operations lead. **Dependencies:** A28, A31.
**Acceptance:** Record exact source, binaries, container, genesis, topology, custody references, scenario steps, timing, state/supply roots, observations, defects and independent decision. Every mandatory check passes.
**Destination:** Protected qualification evidence store; public redacted run summary.

### A33 — Full simulation run 5: OPERATOR ERROR

**Next action:** Execute this full scenario only after the frozen candidate and rehearsal prerequisites pass.
**Owner role:** Release QA and operations lead. **Dependencies:** A28, A32.
**Acceptance:** Record exact source, binaries, container, genesis, topology, custody references, scenario steps, timing, state/supply roots, observations, defects and independent decision. Every mandatory check passes.
**Destination:** Protected qualification evidence store; public redacted run summary.

### A34 — Full simulation run 6: PRODUCTION LOAD AND ECONOMIC ACTIVITY

**Next action:** Execute this full scenario only after the frozen candidate and rehearsal prerequisites pass.
**Owner role:** Release QA and operations lead. **Dependencies:** A28, A33.
**Acceptance:** Record exact source, binaries, container, genesis, topology, custody references, scenario steps, timing, state/supply roots, observations, defects and independent decision. Every mandatory check passes.
**Destination:** Protected qualification evidence store; public redacted run summary.

### A35 — Full simulation run 7: FINAL MAINNET DRESS REHEARSAL

**Next action:** Execute this full scenario only after the frozen candidate and rehearsal prerequisites pass.
**Owner role:** Release QA and operations lead. **Dependencies:** A28, A34.
**Acceptance:** Record exact source, binaries, container, genesis, topology, custody references, scenario steps, timing, state/supply roots, observations, defects and independent decision. Every mandatory check passes.
**Destination:** Protected qualification evidence store; public redacted run summary.

### A36 — Deployment and publication acceptance package

**Next action:** Define candidate-bound installation verification and publication acceptance records before launch.
**Owner role:** Release lead and named launch authority. **Dependencies:** F14, F15, F16, F17, F19, A17, A19, A20, A21, A22, A23, A24, A35.
**Acceptance:** After GO and authorized launch, operators attest installed digests and common genesis/finality. Public docs resolve to exact releases and supported interfaces.
**Destination:** Release registry, public network/docs and protected operator acceptance store.

### A37 — Daily status and weekly readiness reports

**Next action:** Continue implementation with exact approval, source and test evidence.
**Owner role:** Technical program manager. **Dependencies:** None.
**Acceptance:** Each day and week records completed work, changes, tests, defects, open blockers, readiness and progression decision without treating filenames as closure.
**Destination:** Versioned launch repository.

### A38 — Independent binary reproducibility report

**Next action:** Run the frozen candidate build on two independent clean build environments after the build inputs are fixed.
**Owner role:** Independent release reviewer. **Dependencies:** A03, A04, A15.
**Acceptance:** Record build environments, input digests, commands and byte-identical outputs. Resolve nondeterminism before acceptance.
**Destination:** Release evidence registry.

### A39 — Production capacity and PQC benchmark report

**Next action:** Define measurable load and latency limits for the selected topology before week-three qualification.
**Owner role:** Performance QA and SRE lead. **Dependencies:** A06, A08, A13, A19, A21, A26.
**Acceptance:** Measure signing/verification, payload sizes, block propagation, throughput, queue growth, CPU, memory and degradation. Approve safe sustained/peak limits and hardware requirements.
**Destination:** Release evidence registry and public conservative capacity guidance.

### A40 — Frozen production configuration bundle

**Next action:** Export the approved settings into a typed versioned bundle; retain protected values through opaque references.
**Owner role:** Protocol and release lead. **Dependencies:** F02, F06, F07, F09, A10, A12, A13, A15.
**Acceptance:** Validate every setting against the frozen specifications. Reject development defaults, incompatible versions and omitted required fields. Simulations use this exact bundle.
**Destination:** Public protocol configuration and protected per-host operator store.

## Release destinations

| Destination | Contents |
|---|---|
| Source repositories | Reviewed code, protocol specifications, build/deployment definitions, test drivers and public runbooks. |
| Release registry | Immutable source references, target binaries, checksums, signatures, build provenance, SBOM, genesis bytes and release manifests. |
| Container registry | Production node image with an immutable digest. |
| Protected operator store | Custody evidence, infrastructure details, environment configuration, backup artifacts and restricted operational records. Private keys and recovery material never enter public reports. |
| Public documentation | Approved token policy, network identity, genesis hash, validator summary, endpoints, supported client versions and verified operator instructions. |

Creating a filename does not close its gate. Publishing an artifact does not prove installation. Installation does not prove consensus, supply integrity or launch acceptance.

## Ordered dependency plan

1. **Complete the evidence baseline.** Batch 1 source review, fresh tests, allocation reconciliation and seven-run planning are complete. Complete the remaining Phase 0 operational and independent-review records. Assign owners and independent reviewers. Do not start discretionary features.
2. **Freeze protocol and economic decisions.** Select consensus and PQC interfaces. Fix validator lifecycle, account authorization, supply, adaptive inputs, rewards, fees and full initial governance. Define permanent genesis fields and governed upgrade boundaries. Two independent engineers must agree on the rules.
3. **Collect external records in parallel.** Identify actual operators, control groups, recipient custody, vesting and provider inventory. Generate public summaries from validated records. Software agents can prepare schemas. They cannot invent operators, attest custody or approve beneficiaries.
4. **Complete and qualify the required runtime.** Integrate consensus, staking, reward payments, adaptive issuance, governance, identity and deterministic genesis. Resolve recorded test failures. Qualify wallet and SDK compatibility. Keep development startup restrictions until acceptance.
5. **Build the release and operating system around it.** Pin production build inputs. Produce independently matching binaries, node image, SBOM, checksums and signatures. Automate clean hosts. Connect RPC, monitoring, backup and permanent-history upgrade procedures. Preserve all evidence.
6. **Close week-three qualification.** Execute the complete economic lifecycle, approved capacity tests, controlled fault tests, independent reviews and RC1-to-RC2 upgrade rehearsal. Freeze one final candidate after every mandatory gate passes.
7. **Run all seven week-four simulations.** Use fresh isolated launch state, the frozen candidate, production-equivalent topology, approved token/validator model and complete runbooks. Record each scenario separately. A local prerequisite run does not count. Any blocking change requires a new candidate and relevant requalification.
8. **Make the pre-launch decision.** Assemble the 24 final files and their evidence references. Obtain the named authorities' decision against exact candidate and genesis hashes. Only then perform the authorized single canonical mainnet ceremony, installation and post-launch acceptance. No database-reset upgrade or second economic genesis is acceptable.

The four-week target is conditional on these gates. The inspected state does not support a reliable completion-date estimate. Consensus implementation, independent operators and review availability determine the critical path. Week four cannot begin as production-equivalent qualification while those prerequisites remain open.

**Inventory at capture:** 64 artifacts; 4/24 exact final filenames present; 1 qualified; 0/7 full simulations qualified. The one qualified item is the allocation-policy reference only. No production binary or network is qualified. Refresh this register after candidate changes or new accepted evidence.
