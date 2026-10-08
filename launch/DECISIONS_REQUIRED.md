# Dytallix decision and record register

Gate readiness has one source: [LAUNCH_GATES.json](LAUNCH_GATES.json), rendered as [the master list](MAINNET_GATE_MASTER.md). This document tracks policy questions and required records. It does not grant launch authority.

Links into `decision-register/`, `batch-*/`, `evidence/` and `snapshots/` name launch evidence records held outside this repository; they do not resolve here.

The current register contains no OPEN policy questions, 13 PARTIALLY_APPROVED policy questions, 13 APPROVED policy questions and eight OPEN required records. APPROVED means P01 approved the policy rule the question asks for; production values, named owners, reviewer acceptance and gate qualification remain separate. Explicit approval records identify the accepted portions. Unset production values, assignees and reviewers remain unset.

Current evidence includes the emergency controls and upgrade execution package (historical record decision-register/emergency-upgrade-execution/REPORT.md, not in this repository). The master credits implementation and qualification within each report's stated scope. Production acceptance remains incomplete.

Each recorded approval scope below retains its original implementation context. Read current implementation progress and remaining work in the linked gates. Exact question fields, approvals and supersession records remain in [MAINNET_DECISION_REGISTER.json](MAINNET_DECISION_REGISTER.json).

## Design and policy approvals, 26 September – 5 October 2026

P01 approved these engineering designs. Each document records the options and the decisions; the register lists them under `design_approvals`.

- [account model v2](../node/docs/architecture/account-model-v2.md) (26 September): implicit account creation by `Send`, with an account creation fee that is burned. The fee amount and account template values are unset.
- [fees v1](../node/docs/architecture/fees-v1.md) (27 September): every transaction fee is burned in the block that charges it (D05-Q01); the validator share of issuance is divided every block by voting power (D02-Q01).
- [governance v1](../node/docs/architecture/governance-v1.md) (27 September): block order, action classes, governed parameters and fee authority, and finished-proposal retention (D09-Q01, D11-Q02, D11-Q03).
- [validator lifecycle retention](../node/docs/architecture/validator-lifecycle-retention.md) (27 September): retention horizon from the existing evidence limits plus margins, removal of withdrawn unbonds and settled incidents, permanent consensus-key no-reuse, and staker-slot release.
- [state root v2](../node/docs/architecture/state-root-v2.md) (27 September): the `jmt` tree if it passes G35, proof queries at launch, and state sync before launch.
- [29 September policy approvals](approvals/P01_E04_POLICY_2026-09-29.json) (29 September): launch scope (D07-Q01), all DGT at genesis (D05-Q02), observation contract v1 (D01-Q02) and the genesis DRT bootstrap policy (D08-Q02).
- [penalties v1](../node/docs/architecture/penalties-v1.md), with the [30 September penalty approvals](approvals/P01_E04_PENALTIES_2026-09-30.json) (30 September): double-signing penalized with removal (D09-Q04), withdrawals from genesis (D09-Q05), vesting-locked stake penalized like unlocked stake, and a permanent penalty escrow.
- [30 September operations approvals](approvals/P01_E04_OPERATIONS_2026-09-30.json) (30 September): the implemented mempool rule is normative ([mempool v1](../node/docs/architecture/mempool-v1.md), D06-Q02), a separate 3-of-5 upgrade custodian group with a fixed minimum notice (D11-Q03), and private validators behind sentries (D12-Q01).
- [Upgrade notice scope](approvals/P01_E05_UPGRADE_NOTICE_2026-10-01.json) (1 October): the 120,960-block notice applies to release handovers as well as state-migration upgrades (D11-Q03).
- [Root controls](approvals/P01_E05_ROOT_CONTROLS_2026-10-01.json) (1 October): a production node refuses a configuration without the emergency freeze, upgrade and release handover controls at schema 2 (D07-Q01, D11-Q03).
- [Fee range placement](approvals/P01_E05_FEE_RANGE_2026-10-02.json) (2 October): the basic transfer's governed range is the genesis bound `reference_send_fee_udrt`; governed fee changes and the genesis fee profile must price a reference basic Send inside it (D04-Q01, D11-Q03).
- [Supervisor production mode](approvals/P01_E05_SUPERVISOR_2026-10-01.json) (1 October): the engine, application, bridge and HTTP adapter are observed once at startup and then held by kernel limits, and readiness waits for catch-up up to a per-host budget whose value is unset (D12-Q01, D12-Q02).
- [Network identity](approvals/P01_E05_CHAIN_IDENTITY_2026-10-03.json) (3 October): chain ID `dytallix-mainnet-1`, display name Dytallix, and a genesis time set at the final freeze at 14:00:00 UTC on a weekday, at least 72 hours after the final build (D13-Q01; [identity](genesis/IDENTITY.json)).
- [Operations objectives](approvals/P01_E05_OPERATIONS_OBJECTIVES_2026-10-03.json) (3 October): monthly targets of 99.9% (chain) and 99.5% (endpoints), 24/7 paging, archive and log retention, RPO zero committed blocks and RTOs per role (D12-Q02; [operations objectives](operations/OBJECTIVES.md)).
- [Solo launch profile](approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json) (3 October): the founder launches alone. Every root key in five key kits; one validator on three hosts; a 30-day public review with a DGT bug bounty and a labeled AI review instead of an independent human audit, with the launch labeled unaudited; best-effort operations; one founder account per token bucket; staging on the production hosts; reproducible builds on a clean CI runner and a local container ([trust model](TRUST_MODEL.md)).
- [Engineering close-out](approvals/P01_E05_ENGINEERING_CLOSE_2026-10-03.json) (3 October): sentries and endpoints can never sign and validators may start before registration; a plain-HTTP status page with no cryptography for the uptime checker; `client_compatibility_window` and `max_open_proposals` dropped; fault assumptions recorded; the CLI is the launch wallet.
- [E06 release](approvals/P01_E06_RELEASE_2026-10-05.json) (5 October): Linux x86_64, the wallet also from source; a published builder image, no node container; GitHub Releases on DytallixHQ/dytallix after the repository move; the root-signed genesis binds the first release, with no separate release key ([release](../release/README.md)).
- [E06 release libraries](approvals/P01_E06_STATIC_RELEASE_2026-10-05.json) (5 October): every release binary is static, with glibc and libstdc++ linked in, so the release manifest lists no runtime libraries and host updates never change what the node runs ([release manifest](../release/README.md#release-manifest)).
- [E06 repository](approvals/P01_E06_REPOSITORY_2026-10-05.json) (5 October): MIT OR Apache-2.0; DCO sign-off for outside contributions, no CLA; the moved history uses the founder's GitHub noreply address; afterwards mainnet/ in exocognosis/dytallix becomes a pointer. Done on 5 October ([the move](../release/MOVE.md)).
- [Control signing](approvals/P01_E05_CONTROL_SIGNING_2026-10-06.json) (6 October): every root control window and anchor age is at most 34,560 blocks (two days), so the founder can reach three key kits and sign offline; the CLI prepares, dry-runs and submits controls from the founder's laptop (D11-Q03).
- [Host setup](approvals/P01_E05_HOSTS_2026-10-06.json) (6 October): Ubuntu 24.04 LTS; each host's public IPv4 on its own interface, no NAT; node keys made offline and delivered in digest-checked host bundles, sealed under a passphrase; the service account and layout ([host setup v1](../node/docs/architecture/host-setup-v1.md), D12-Q01).
- [Host values](approvals/P01_E05_HOST_VALUES_2026-10-06.json) (6 October): unit limits, ports, bounds and timeouts for the host setup generator ([SETUP_VALUES.json](hosts/SETUP_VALUES.json), D12-Q01).
- [Root helper bound](approvals/P01_E05_HOST_HELPER_BOUND_2026-10-07.json) (7 October): the root helper observation's file bounds are 16 MiB, equal to the helper bound; the approved 32 MiB made every node refuse to start (D12-Q01).
- [Supervisor root preflight](approvals/P01_E05_SUPERVISOR_ROOT_2026-10-07.json) (7 October): option D. The production supervisor never runs the root helper (only the application may, E02); it selects the release from the pinned configuration, genesis and root records and checks the catalog, and the application verifies the root genesis signatures, controls and any restart authorization before it opens state ([supervisor root preflight](../node/docs/architecture/supervisor-root-preflight.md), D12-Q01).
- [Restore](approvals/P01_E05_RESTORE_2026-10-08.json) (8 October): a restore accepts a copy up to 13 days old (its header trusted for the evidence age less one day, 312 hours; peer state sync keeps 168 hours), and restore.sh refuses on the validator unless the operator adds --accept-history-loss after the incident decision (D12-Q02) ([disaster recovery v1](../node/docs/architecture/disaster-recovery-v1.md#restore)).
- [Kit replacement](approvals/P01_E05_KIT_REPLACEMENT_2026-10-08.json) (8 October): one control, signed by three of the five current upgrade keys, replaces one seat's upgrade, freeze and resume keys; it takes effect after the handover notice of 120,960 blocks (seven days), and the five seats and threshold of three stay fixed (D10-Q03, D11-Q03) ([root kit replacement v1](../node/docs/architecture/root-kit-replacement-v1.md)).
- [Root key holders](approvals/P01_E05_ROOT_KEY_HOLDERS_2026-10-07.json) (7 October): five people hold the five root key kits, one each, so no one and no two can act; each makes their own kit offline. Each kit is on one encrypted fob with no paper copy and no backup, and a kit replacement control signed by three kits is built before launch. Each holder receives 20,000 DGT at genesis from the Team and advisors bucket, on a separate wallet fob. The kits control the chain only; the bucket and treasury account keys move into the founder's own custody. Supersedes the solo profile's kit and bucket-key custody and, for the kits, the paper copy (D10-Q03, D11-Q03, D08-Q01) ([key ceremony](custody/KEY_CEREMONY.md)).
- [Disaster recovery restore](approvals/P01_E05_DISASTER_RECOVERY_RESTORE_2026-10-07.json) (7 October): a rebuilt host restores from the newest off-host copy on its own, before its first start: the backup code typed from paper, the application's restore checks, and the engine's stores set at the copy's height from its light blocks, verified by the light client; it then catches up from the validator. The sentry's engine writes the light blocks for each snapshot height (H to H+2), so every copy is self-contained (D12-Q02) ([disaster recovery v1](../node/docs/architecture/disaster-recovery-v1.md#restore)).
- [Disaster recovery](approvals/P01_E05_DISASTER_RECOVERY_2026-10-07.json) (7 October): the sentry pushes its daily snapshot (state and every block record), encrypted under a paper backup code, to object storage at a second provider with a write-only key sealed in its bundle, and the founder downloads the latest copy monthly (D12-Q02); the validator and the sentry run at different hosting providers (D12-Q03) ([disaster recovery v1](../node/docs/architecture/disaster-recovery-v1.md)).
- [Host sealing](approvals/P01_E05_HOST_SEALING_2026-10-06.json) (6 October): each host's node keys sealed by `dytallix-root-sign seal` with AES-256-GCM under the host's own 256-bit seal code, printed once as a checked paper line; each code written twice and kept with two different kits' papers (D12-Q01).
- [Host release switch and validator fencing](approvals/P01_E05_HOST_RECOVERY_2026-10-06.json) (6 October): a host stages a new release beside the running one from its new bundle and switches the unit when the old release stops at activation, keeping the previous release until the next stage (D12-Q01); validator recovery (F17) deletes or reinstalls the old validator server in the provider console before the replacement is installed (D12-Q02).
- [Validator restart under the solo launch profile](approvals/P01_E05_SOLO_VALIDATOR_RESTART_2026-10-06.json) (6 October): `double_sign_check_height` is 0 on every role while the chain has one validator, replacing 10 on validators, because a lone validator's own signatures fill the last blocks and the check would stop every restart for good (D06-Q02).

## D01 — Adaptive issuance

Gate references: G14, G19.

**Recorded approval scope and historical implementation context:** Corrected adaptive controller selected. Atomic epoch issuance and explicit observation inputs authorized for local implementation.

### D01-Q01 — PARTIALLY_APPROVED

Which production controller version, gains, bounds, initial state and initial command apply?

Approved portion (P01, 30 September 2026): base and ceiling 1,000 DRT a block, 17,280,000 DRT per 17,280-block epoch (about 6.31 billion DRT a year); floor 500 DRT a block; a 50% utilization target; the first epoch's command equals the base ([E05 values, second set](approvals/P01_E05_VALUES_2_2026-09-30.json)).

Calibration (P01, 30 September 2026): a one-day linear response. The window is one epoch; the proportional gain is 17,280,000,000,000 in both regimes, and the integral and derivative gains are 0. Issuance falls linearly from 1,000 DRT a block at 50% utilization to 500 at full blocks ([E05 values, third set](approvals/P01_E05_VALUES_3_2026-09-30.json)).

Controller constants (P01, 2 October 2026): integral limits 0 and 0, shock and volatility thresholds 1,000,000 ppm, as the calibration requires ([E05 values, fourth set](approvals/P01_E05_VALUES_4_2026-10-02.json)).

Remaining input: a rescale of the gain if the capacity tests (T05) show blocks cannot fill completely.

Required output: Production controller configuration.

Proposed owner role: Protocol and economics lead. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [node/docs/mainnet/specification-decisions.md](../node/docs/mainnet/specification-decisions.md), batch-6/issuance-timing/APPROVAL.json (historical record, not in this repository).

### D01-Q02 — APPROVED

Which observations, sources, authentication, aggregation and missing-input or resume rules apply?

Approved (observation contract v1, P01, 29 September 2026): every validator derives the epoch observation from committed blocks. Utilization is the epoch's transaction bytes, excluding its own observation, over `epoch_blocks × max_block_bytes`, in parts per million; volatility is 0. CheckTx refuses submitted observations, the proposer inserts the derived one and execution rejects a mismatch. No external oracle is used at launch.

Recorded approval: [29 September policy approvals](approvals/P01_E04_POLICY_2026-09-29.json); contract in [adaptive emission v1](../node/docs/mainnet/adaptive-emission-v1.md).

Required output: Observation contract.

Proposed owner role: Protocol and economics lead. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [node/docs/mainnet/specification-decisions.md](../node/docs/mainnet/specification-decisions.md), batch-6/issuance-timing/APPROVAL.json (historical record, not in this repository).

## D02 — DRT reward allocation

Gate references: G14, G16, G17, G19, G23.

**Recorded approval scope and historical implementation context:** 40/30/30 validator/staker/treasury split. Six decimals. Checked floor allocation by eligible stake. Sole recipient gets the budget. Staking pool pays owners with no commission. Separate rounding and inactive reserves have no automatic recipient or sweep authority. LR01 authorizes local retirement of the identified development timer and direct legacy staking/emission mutations. Preserve explicit RewardState adapters, shared planning, historical reads and staking ownership compatibility records. Deployment, migration and state deletion are not authorized. The legacy rounding defect remains historical diagnostic evidence; retirement is not an arithmetic repair.

### D02-Q01 — APPROVED

What eligibility and allocation rules apply to the separate validator reward pool?

Approved (validator option A, P01, 27 September 2026): each block's validator budget is divided by voting power among the validators in the effective set and credited to each operator's owner, claimed with the existing `RewardClaim`. A validator outside the set earns nothing for that block; the rounding remainder stays in the pool. Issuance values remain E05 inputs.

Recorded approval: [fees v1](../node/docs/architecture/fees-v1.md).

Required output: Validator reward specification.

Proposed owner role: Reward accounting and treasury leads. Named assignment and reviewer remain as recorded in the structured register.

Evidence: evidence/TOKENOMICS_APPROVED_SOURCE.json, batch-6/APPROVAL.json, batch-6/integration-followup/APPROVAL.json, batch-6/issuance-timing/APPROVAL.json (historical records, not in this repository).

### D02-Q02 — OPEN

Who receives treasury rewards, and which custody records authorize receipt?

**Solo launch** ([P01, 3 October 2026](approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json)): the treasury account is the founder's, publicly listed, with its key in the founder's key kits.

Required output: Treasury reward configuration.

Proposed owner role: Reward accounting and treasury leads. Named assignment and reviewer remain as recorded in the structured register.

Preparation: D02-Q02 packet (historical record decision-register/parallel-tracks-20260912/track-4/records/D02-Q02.json, not in this repository). Acceptance is still open.

Evidence: evidence/TOKENOMICS_APPROVED_SOURCE.json, batch-6/APPROVAL.json, batch-6/integration-followup/APPROVAL.json, batch-6/issuance-timing/APPROVAL.json, decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json, decision-register/ordinary-client-compatibility/RECORD_STATUS.md (historical records, not in this repository).

## D03 — Interval timing

Gate references: G14, G19.

**Recorded approval scope and historical implementation context:** One finalized block per reward interval, using parent finalized state. Explicit N finalized blocks per issuance epoch. Even per-bucket scheduling gives extra units to first blocks. Separate split reserve. Batch 8 makes stake and membership changes effective at H+2.

### D03-Q01 — APPROVED

What production epoch length N and observation sampling windows apply?

Approved portion (P01, 30 September 2026): 17,280 blocks, one day at 5-second blocks, with one observation per epoch under observation contract v1 ([E05 values, second set](approvals/P01_E05_VALUES_2_2026-09-30.json)).

Sample window (P01, 30 September 2026): one epoch ([E05 values, third set](approvals/P01_E05_VALUES_3_2026-09-30.json)).

Required output: Production issuance timing matrix.

Proposed owner role: Protocol lead. Named assignment and reviewer remain as recorded in the structured register.

Evidence: batch-6/integration-followup/APPROVAL.json, batch-6/issuance-timing/APPROVAL.json, batch-8/implementation/APPROVAL.json (historical records, not in this repository).

## D04 — Fees

Gate references: G10, G11, G12, G14, G20.

**Recorded approval scope and historical implementation context:** Recovery RF01 through RF06 and ordinary actor-paid udrt/no-tips authority remain selected. OF01 through OF05 select ordinary fee contract format 1, canonical profile binding, deterministic metering, zero-charge preacceptance rejection, measured accepted application failure, accepted out-of-gas, shared reservations, atomic cap release and nonce outcomes, and undistributed udrt custody. Production values and actual runtime or durable integration remain separate. Reserve metadata gas before acceptance and count it once. Initial vesting/custody eligibility remains a zero-charge funding condition; a later breached reservation guarantee is an internal fault. Dms authority checks precede acceptance while maturity is a typed post-acceptance state condition. Shared aggregate limits supplement recovery-specific ceilings; mandatory expiry retains its own budget. The later fee burn (D05-Q01, P01, 27 September 2026) supersedes the undistributed udrt custody. The current result records local OF-W04 and OF-W05 runtime and durable integration checks within its reported scope. This closes neither the parent production work nor OF-W06 client, production parameter, custody, genesis, operator or release qualification. The current result records the locally checked OF-W06-CLIENT slice: explicit ordinary-v2 SDK, wallet CLI, committed node client views, genesis identity compatibility and documented local pipe-adapter acceptance. This is not an actual CometBFT engine or live RPC result. Full OF-W06 remains open for hosted wallet UI, live engine/RPC and light-client assurance, production values/roles/records and release qualification.

### D04-Q01 — PARTIALLY_APPROVED

Which fee denomination, metering, formula, prices, bounds and tips apply?

Account creation fee: a field of the signed ordinary fee profile, committed through `fee_profile_digest` ([account model v2](../node/docs/architecture/account-model-v2.md), 26 September 2026). Governance sets new fee profile versions within genesis bounds ([governance v1](../node/docs/architecture/governance-v1.md), 27 September 2026).

Fee levels (P01, 30 September 2026): a basic transfer costs 1 DRT, and governance can move it only between 0.1 and 10 DRT; account creation costs 10 DRT, governed between 1 and 100 DRT ([E05 values, second set](approvals/P01_E05_VALUES_2_2026-09-30.json)).

Fee values (P01, 30 September 2026): floor pricing with free reads. Gas price 10 times minimum gas 100,000 is the 1 DRT floor, which a basic Send (about 47,600 gas) pays. Overhead 10,000, receipt metadata 1,000, wire 2 and write 1 per byte, read 0, ML-DSA-65 signature and validator proof 20,000, every action 5,000; gas price bounds 1 to 100 and cost bounds 0 to 100,000 ([E05 values, third set](approvals/P01_E05_VALUES_3_2026-09-30.json)). Capacity and limit values are proposed in [genesis/PROPOSALS.json](genesis/PROPOSALS.json) for the T05 measurements.

Fee range placement (P01, 2 October 2026): the 0.1 to 10 DRT range is the genesis bound `parameter_bounds.reference_send_fee_udrt` (100,000 to 10,000,000 uDRT). The node refuses a fee change, and a genesis fee profile, that prices the reference basic Send outside it: one ML-DSA-65 signature and one Send between existing accounts, at 5,565 wire, 5,536 read and 486 write bytes measured from a real Send ([fee range approval](approvals/P01_E05_FEE_RANGE_2026-10-02.json)).

Required output: Fee parameter specification.

Proposed owner role: Fee accounting lead. Named assignment and reviewer remain as recorded in the structured register.

Recorded approval: decision-register/ordinary-fee-implementation/APPROVAL.json (historical record, not in this repository).

Evidence: batch-2/ECONOMIC_DECISIONS.json, batch-10/REPORT.md, decision-register/recovery-fee-storage/PROPOSAL.json, decision-register/recovery-fee-storage/AUTHORIZATION.json, decision-register/recovery-fee-storage/CONTRACT.md, decision-register/recovery-fee-storage/FEE_REVIEW.md, decision-register/recovery-fee-storage/STORAGE_MAP.md, decision-register/recovery-fee-storage/WORK_ITEMS.json, decision-register/recovery-fee-storage/ACCEPTANCE.json, decision-register/recovery-fee-implementation/APPROVAL.json, decision-register/recovery-fee-implementation/SCOPE.json, decision-register/recovery-fee-implementation/REPORT.md, decision-register/recovery-fee-implementation/TEST_RESULTS.json, decision-register/ordinary-signing-implementation/APPROVAL.json, decision-register/ordinary-fee-implementation/APPROVAL.json, decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.md, decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.json, decision-register/ordinary-fee-implementation/SCOPE.json, decision-register/ordinary-fee-implementation/WORK_ITEMS.json, decision-register/ordinary-fee-implementation/README.md, decision-register/ordinary-fee-implementation/DECISION_STATUS.md, decision-register/ordinary-fee-implementation/REPORT.md, decision-register/ordinary-fee-implementation/TEST_RESULTS.json, decision-register/ordinary-runtime-integration/APPROVAL.json, decision-register/ordinary-runtime-integration/WORK_ITEMS.json, decision-register/ordinary-runtime-integration/README.md, decision-register/ordinary-runtime-integration/DECISION_STATUS.md, decision-register/ordinary-runtime-integration/REPORT.md, decision-register/ordinary-runtime-integration/TEST_RESULTS.json, decision-register/ordinary-client-compatibility/APPROVAL.json, decision-register/ordinary-client-compatibility/SCOPE.json, decision-register/ordinary-client-compatibility/WORK_ITEMS.json, decision-register/ordinary-client-compatibility/README.md, decision-register/ordinary-client-compatibility/DECISION_STATUS.md, decision-register/ordinary-client-compatibility/REPORT.md, decision-register/ordinary-client-compatibility/TEST_RESULTS.json (historical records, not in this repository).

### D04-Q02 — PARTIALLY_APPROVED

How do reservation, failures, refunds, minimum charges and recipient credits work?

Fee destination: fees are burned in the block that charges them, with no recipient records (D05-Q01, [fees v1](../node/docs/architecture/fees-v1.md)). Remaining inputs: production profile and receipt retention parameters.

Required output: Fee settlement specification.

Proposed owner role: Fee accounting lead. Named assignment and reviewer remain as recorded in the structured register.

Recorded approval: decision-register/ordinary-fee-implementation/APPROVAL.json (historical record, not in this repository).

Evidence: batch-2/ECONOMIC_DECISIONS.json, batch-10/REPORT.md, decision-register/recovery-fee-storage/PROPOSAL.json, decision-register/recovery-fee-storage/AUTHORIZATION.json, decision-register/recovery-fee-storage/CONTRACT.md, decision-register/recovery-fee-storage/FEE_REVIEW.md, decision-register/recovery-fee-storage/STORAGE_MAP.md, decision-register/recovery-fee-storage/WORK_ITEMS.json, decision-register/recovery-fee-storage/ACCEPTANCE.json, decision-register/recovery-fee-implementation/APPROVAL.json, decision-register/recovery-fee-implementation/SCOPE.json, decision-register/recovery-fee-implementation/REPORT.md, decision-register/recovery-fee-implementation/TEST_RESULTS.json, decision-register/ordinary-signing-implementation/APPROVAL.json, decision-register/ordinary-fee-implementation/APPROVAL.json, decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.md, decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.json, decision-register/ordinary-fee-implementation/SCOPE.json, decision-register/ordinary-fee-implementation/WORK_ITEMS.json, decision-register/ordinary-fee-implementation/README.md, decision-register/ordinary-fee-implementation/DECISION_STATUS.md, decision-register/ordinary-fee-implementation/REPORT.md, decision-register/ordinary-fee-implementation/TEST_RESULTS.json, decision-register/ordinary-runtime-integration/APPROVAL.json, decision-register/ordinary-runtime-integration/WORK_ITEMS.json, decision-register/ordinary-runtime-integration/README.md, decision-register/ordinary-runtime-integration/DECISION_STATUS.md, decision-register/ordinary-runtime-integration/REPORT.md, decision-register/ordinary-runtime-integration/TEST_RESULTS.json, decision-register/ordinary-client-compatibility/APPROVAL.json, decision-register/ordinary-client-compatibility/SCOPE.json, decision-register/ordinary-client-compatibility/WORK_ITEMS.json, decision-register/ordinary-client-compatibility/README.md, decision-register/ordinary-client-compatibility/DECISION_STATUS.md, decision-register/ordinary-client-compatibility/REPORT.md, decision-register/ordinary-client-compatibility/TEST_RESULTS.json (historical records, not in this repository).

## D05 — Burning and supply authority

Gate references: G05, G12, G13, G14, G15, G20, G23.

**Recorded approval scope and historical implementation context:** Fixed one-billion DGT total and category shares approved. RF05 and OF05 selected undistributed udrt custody with no recipient, burn or sweep operation; the D05-Q01 fee burn (P01, 27 September 2026) supersedes both. Fee delta must be reconciled separately from successful action balance changes. DGT issuance authority (D05-Q02) remains open.

### D05-Q01 — APPROVED

Which DRT fee components burn, and what custody debit and supply change occur on success or failure?

Approved (fee option A, P01, 27 September 2026): every transaction fee, ordinary, governance and sponsored recovery, is burned. Fees collect in the withheld counter while a block runs and move to `supply:drt_burned` at its end, so the payer's uDRT falls and the burned total rises by each charge, on success, accepted failure and out-of-gas alike. The withheld counter is zero at every commit, and supply is genesis plus emitted minus burned. The account creation fee is burned (P01, 26 September 2026). A bandwidth-only burn was not offered: the approved meter has one combined gas charge.

Recorded approvals: [fees v1](../node/docs/architecture/fees-v1.md), [account model v2](../node/docs/architecture/account-model-v2.md).

Required output: DRT burn specification.

Proposed owner role: Economics and supply accounting leads. Named assignment and reviewer remain as recorded in the structured register.

Evidence: evidence/TOKENOMICS_APPROVED_SOURCE.json, batch-2/ECONOMIC_DECISIONS.json, decision-register/recovery-fee-implementation/APPROVAL.json (historical records, not in this repository).

### D05-Q02 — APPROVED

Will all DGT issue at genesis, with no later mint authority and no initial DGT burn?

Approved (P01, 29 September 2026): genesis issues the whole 1,000,000,000 DGT and nothing mints DGT later; the runtime mint path is removed, and the binding review reports `full_dgt_issuance` as missing otherwise. No DGT is burned. Penalized DGT goes to a penalty escrow until D09 sets its destination. Changing the total needs an upgrade.

Recorded approval: [29 September policy approvals](approvals/P01_E04_POLICY_2026-09-29.json).

Required output: DGT issuance and authority specification.

Proposed owner role: Economics and supply accounting leads. Named assignment and reviewer remain as recorded in the structured register.

Evidence: evidence/TOKENOMICS_APPROVED_SOURCE.json, batch-2/ECONOMIC_DECISIONS.json, decision-register/recovery-fee-implementation/APPROVAL.json (historical records, not in this repository).

## D06 — Consensus and validation

Gate references: G01, G02, G09, G25, G26, G30, G35.

**Recorded approval scope and historical implementation context:** CometBFT v0.40.0 local integration and qualification authorized. D06-Q01 partial approval (historical record decision-register/core-function-alignment/E01_PRODUCTION_PROFILE_APPROVAL_2026-09-25.json, not in this repository) selects the post-quantum role and pinned-peer policy. The exact production engine commit, addresses and acceptance remain unset.

### D06-Q01 — PARTIALLY_APPROVED

Which exact engine version, cryptographic roles and transport authentication profile will production freeze?

Required output: Production consensus profile.

Proposed owner role: Protocol lead with cryptography and network reviewers. Named assignment and reviewer remain as recorded in the structured register.

Evidence: batch-7/APPROVAL.json, batch-9/REPORT.md, batch-10/REPORT.md (historical records, not in this repository).

Approved portion: ML-KEM-768 and ML-DSA-65 authenticated pinned peers; ML-DSA-65 validators; distinct role keys; exact approved IP endpoints; no classical or plaintext fallback. The exact engine commit, production chain ID, addresses, keys, custody and candidate-bound review remain open.

Peer records (P01, 30 September 2026): one IP per node, with static 1:1 NAT for public sentries, and a published pin plan keeping each node within 64 pins ([production activation approval](approvals/P01_E05_ACTIVATION_2026-09-30.json), design [production activation v1](../node/docs/architecture/production-activation-v1.md)). Replaced (P01, 6 October 2026, [host setup](approvals/P01_E05_HOSTS_2026-10-06.json)): each host's public IPv4 is on its own interface, with no NAT.

### D06-Q02 — PARTIALLY_APPROVED

What timing, capacity, fault assumptions, synchronization and history requirements apply?

Approved portion (P01, 30 September 2026): the implemented mempool rule is normative ([mempool v1](../node/docs/architecture/mempool-v1.md)): arrival order, one pending transaction per account nonce, fee caps reserved against the payer's balance, no replacement or eviction when full, expiry, and release at each committed block.

Recorded approval: [30 September operations approvals](approvals/P01_E04_OPERATIONS_2026-09-30.json).

Timing (P01, 30 September 2026): about 5-second blocks, `timeout_commit` 4 s with the upstream propose (3 s), prevote (1 s) and precommit (1 s) timeouts ([E05 values, first set](approvals/P01_E05_VALUES_1_2026-09-30.json)).

Snapshots (P01, 30 September 2026): every 17,280 blocks (daily), keeping three ([E05 values, second set](approvals/P01_E05_VALUES_2_2026-09-30.json)).

Operating values (P01, 2 October 2026, [E05 values, fourth set](approvals/P01_E05_VALUES_4_2026-10-02.json)):
- **Capacity:** about 180 basic Sends per 5 s block (1 MiB blocks, 1,000,000 transaction bytes, 200 signature checks). T05 tests these values and can return one for a new approval.
- **Transaction format, state and retention limits:** at most 10,000 concurrent staking owners and a 20-minute transaction expiry.
- **Engine timing:** 500 ms deltas, and an empty block every 5 s.
- **Double-sign startup check:** 10 blocks on validators.
- **Networking, mempool and state sync:** peers up to the 64-pin bound, a 5 s PQC handshake, and a 168h state-sync trust period.
- **Pinned peers:** redialed for as long as the engine runs, at most 60 s apart ([pinned redial approval](approvals/P01_E05_PINNED_REDIAL_2026-10-02.json)).

Fault assumptions ([P01, 3 October 2026](approvals/P01_E05_ENGINEERING_CLOSE_2026-10-03.json)): less than one third of voting power is faulty at any time; with one validator at launch, no fault is tolerated.

Remaining inputs: measured mempool size and bytes, send and receive rates and the admission queue (T05), and the state-sync trust source and snapshot peers (records).

Required output: Consensus operating specification.

Proposed owner role: Protocol lead with cryptography and network reviewers. Named assignment and reviewer remain as recorded in the structured register.

Evidence: batch-7/APPROVAL.json, batch-9/REPORT.md, batch-10/REPORT.md (historical records, not in this repository).

## D07 — Required launch scope

Gate references: G31, G32, G33.

**Recorded approval scope and historical implementation context:** The launch brief requires the initial lifecycle, governance, wallet, PQC, RPC, monitoring, recovery and upgrade features. One permanent mainnet is required. These requirements are not optional by default. LR01 authorizes local retirement of the identified development timer and direct legacy staking/emission mutations. Preserve explicit RewardState adapters, shared planning, historical reads and staking ownership compatibility records. Deployment, migration and state deletion are not authorized. The legacy rounding defect remains historical diagnostic evidence; retirement is not an arithmetic repair.

### D07-Q01 — APPROVED

What exact enabled-module and interface matrix satisfies the required launch scope?

Approved (current build only, P01, 29 September 2026): mainnet v1 launches with ordinary v2 transfers and staking, ordinary v3 governance, recovery, emergency controls, root-signed upgrades, state sync, the client channel and the local gateway. A contract runtime, bridges, the Airlock, a liquidity bootstrapping pool, wrapped USDC, external oracles, gRPC and treasury spending are POST MAINNET.

Recorded approval: [29 September policy approvals](approvals/P01_E04_POLICY_2026-09-29.json).

Enforcement (P01, 1 October 2026): a production node refuses a configuration without recovery, ordinary and governance (production activation A1) or without the emergency freeze, upgrade and release handover controls at schema 2 (A4, [root controls approval](approvals/P01_E05_ROOT_CONTROLS_2026-10-01.json)).

Required output: Launch feature matrix.

Proposed owner role: Product and protocol leads. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [USER_LAUNCH_REQUIREMENTS.txt](USER_LAUNCH_REQUIREMENTS.txt), batch-1/REQUIREMENTS_RECONCILIATION.md (historical record, not in this repository).

## D08 — Genesis allocations and vesting

Gate references: G04, G05, G12, G13, G14, G15, G16, G17, G18, G20.

**Recorded approval scope and historical implementation context:** One billion DGT; shares 30/20/15/15/20. Six decimals give 10^15 base units. Explicit beneficiary vesting inputs required. Locked DGT may stake only when its schedule permits; locks survive bonding and unbonding.

### D08-Q01 — OPEN

Which beneficiaries, recipients, custody approvals, exact vesting schedules and initial delegations apply?

**Solo launch** ([P01, 3 October 2026](approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json)): one genesis account per bucket (Ecosystem growth, Team and advisors, Public sale, Private sale, Reserve), all the founder's, publicly listed with their purpose; tokens leave only by visible on-chain transfers; the validator self-bond comes from the Ecosystem bucket. A DGT bug bounty for the public review is paid from the Ecosystem bucket (tiers set later).

Required output: Signed allocation and vesting input set.

Proposed owner role: Genesis coordinator with custody and economics reviewers. Named assignment and reviewer remain as recorded in the structured register.

Preparation: D08-Q01 packet (historical record decision-register/parallel-tracks-20260912/track-4/records/D08-Q01.json, not in this repository). Acceptance is still open.

Evidence: evidence/TOKENOMICS_APPROVED_SOURCE.json, batch-6/APPROVAL.json, batch-6/integration-followup/APPROVAL.json, decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json, decision-register/ordinary-client-compatibility/RECORD_STATUS.md (historical records, not in this repository).

### D08-Q02 — PARTIALLY_APPROVED

What initial DRT supply, fee bootstrap distribution and restrictions apply?

Approved portion (P01, 29 September 2026): genesis creates a fixed liquid amount of ordinary, transferable DRT, counted in genesis supply, and the genesis manifest assigns it to named accounts such as validator operators and custody accounts.

Recorded approval: [29 September policy approvals](approvals/P01_E04_POLICY_2026-09-29.json).

Amount (P01, 30 September 2026): 1,000,000 DRT ([E05 values, second set](approvals/P01_E05_VALUES_2_2026-09-30.json)).

Remaining inputs: the recipient rows (D08-Q03).

Required output: DRT genesis policy.

Proposed owner role: Genesis coordinator with custody and economics reviewers. Named assignment and reviewer remain as recorded in the structured register.

Evidence: evidence/TOKENOMICS_APPROVED_SOURCE.json, batch-6/APPROVAL.json, batch-6/integration-followup/APPROVAL.json (historical records, not in this repository).

### D08-Q03 — OPEN

Which verified recipients and custody records implement the approved DRT bootstrap?

Required output: DRT genesis input set.

Proposed owner role: Genesis coordinator with custody and economics reviewers. Named assignment and reviewer remain as recorded in the structured register.

Preparation: D08-Q03 packet (historical record decision-register/parallel-tracks-20260912/track-4/records/D08-Q03.json, not in this repository). Acceptance is still open.

Evidence: evidence/TOKENOMICS_APPROVED_SOURCE.json, batch-6/APPROVAL.json, batch-6/integration-followup/APPROVAL.json, decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json, decision-register/ordinary-client-compatibility/RECORD_STATUS.md (historical records, not in this repository).

## D09 — Validators, evidence, penalties and withdrawals

Gate references: G01, G02, G04, G05, G07, G08, G15, G16, G17, G18, G21, G22, G30, G34.

**Recorded approval scope and historical implementation context:** All seven Batch 8 rules approved: effective bonded power; permissioned initial admission with key proof; funded self-bond and capacity checks; H+2 changes; owner-specific unbonding held through both evidence limits, margins and penalty settlement; authorized key rotation; atomic recovery. Batch 9 penalty values are synthetic.

### D09-Q01 — APPROVED

What minimum self-bond, maximum active set and operator-registry amendment authority apply?

Approved portion (P01, 27 September 2026): governance amends the operator registry through the validator registry action class (add an approved operator, or remove one with no registered validator; at most 64) and governs `min_self_bond` and `max_active` within genesis bounds. Admission and H+2 rules are unchanged.

Recorded approval: [governance v1](../node/docs/architecture/governance-v1.md).

Values (P01, 30 September 2026): `max_active` 16, governed between 4 and 32; `min_self_bond` 100,000 DGT, governed between 10,000 and 1,000,000 DGT ([E05 values, second set](approvals/P01_E05_VALUES_2_2026-09-30.json)).

Required output: Validator admission configuration.

Proposed owner role: Protocol and staking leads with validator coordinator. Named assignment and reviewer remain as recorded in the structured register.

Evidence: batch-8/implementation/APPROVAL.json, batch-8/POLICY_PROPOSAL.json, batch-9/SCOPE.json, batch-9/POLICY.md (historical records, not in this repository).

### D09-Q02 — OPEN

Which operators, control groups, keys, custody arrangements and signed acceptances form the initial set?

**Solo launch** ([P01, 3 October 2026](approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json)): the founder is the only operator at launch, running one validator; more validators join later through on-chain registration.

Required output: Initial validator register.

Proposed owner role: Protocol and staking leads with validator coordinator. Named assignment and reviewer remain as recorded in the structured register.

Preparation: D09-Q02 packet (historical record decision-register/parallel-tracks-20260912/track-4/records/D09-Q02.json, not in this repository). Acceptance is still open.

Evidence: batch-8/implementation/APPROVAL.json, batch-8/POLICY_PROPOSAL.json, batch-9/SCOPE.json, batch-9/POLICY.md, decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json, decision-register/ordinary-client-compatibility/RECORD_STATUS.md (historical records, not in this repository).

### D09-Q03 — APPROVED

What evidence age limits in blocks and seconds, and what processing margins, apply?

Approved (P01, 30 September 2026): 14 days. Evidence stays admissible for 1,209,600 seconds and 241,920 blocks, with margins of 3,600 seconds and 720 blocks; the engine's evidence parameters equal them. Unbonded stake matures after both limits plus the margins.

Recorded approval: [E05 values, first set](approvals/P01_E05_VALUES_1_2026-09-30.json).

Required output: Evidence retention and unbond timing configuration.

Proposed owner role: Protocol and staking leads with validator coordinator. Named assignment and reviewer remain as recorded in the structured register.

Evidence: batch-8/implementation/APPROVAL.json, batch-8/POLICY_PROPOSAL.json, batch-9/SCOPE.json, batch-9/POLICY.md (historical records, not in this repository).

### D09-Q04 — APPROVED

Which faults, penalty rates, repeat-fault treatment, reinstatement rules and penalty-reserve rules apply?

Approved portion (P01, 30 September 2026): double-signing only, with removal. A validator's first duplicate vote deducts a fixed share from all stake bonded to it at that height, self-bond, delegators and vesting-locked stake alike. The validator is removed for good two blocks later and its consensus key is never reused; later evidence adds nothing. Light-client attacks are recorded only; there is no downtime penalty or jailing. Penalized DGT moves to an escrow that nothing can spend.

Recorded approval: [30 September penalty approvals](approvals/P01_E04_PENALTIES_2026-09-30.json). Design: [penalties v1](../node/docs/architecture/penalties-v1.md).

Rate (P01, 30 September 2026): 1/20 (5%) ([E05 values, first set](approvals/P01_E05_VALUES_1_2026-09-30.json)).

Required output: Production penalty specification.

Proposed owner role: Protocol and staking leads with validator coordinator. Named assignment and reviewer remain as recorded in the structured register.

Evidence: batch-8/implementation/APPROVAL.json, batch-8/POLICY_PROPOSAL.json, batch-9/SCOPE.json, batch-9/POLICY.md (historical records, not in this repository).

### D09-Q05 — PARTIALLY_APPROVED

How do locked-principal liability, completed settlement, withdrawal activation and parameter migration work?

Approved portion (P01, 30 September 2026): withdrawals work from genesis. An unbond can be withdrawn once both evidence age limits plus margins have passed and no penalty on it is unsettled. Vesting-locked stake carries the same penalty risk; a penalty comes off the amount still locked, on the same vesting dates. The launch configuration carries the lifecycle and penalty profiles.

Recorded approval: [30 September penalty approvals](approvals/P01_E04_PENALTIES_2026-09-30.json). Design: [penalties v1](../node/docs/architecture/penalties-v1.md).

Remaining inputs: parameter migration rules.

Required output: Withdrawal and locked-liability specification.

Proposed owner role: Protocol and staking leads with validator coordinator. Named assignment and reviewer remain as recorded in the structured register.

Evidence: batch-8/implementation/APPROVAL.json, batch-8/POLICY_PROPOSAL.json, batch-9/SCOPE.json, batch-9/POLICY.md (historical records, not in this repository).

## D10 — Keys, accounts and recovery

Gate references: G03, G04, G08, G10, G11, G12, G13, G20, G23, G24, G25, G29, G30, G31, G33, G34, G35.

**Recorded approval scope and historical implementation context:** Lost-or-compromised-key recovery, the detailed two-of-three rules, local recovery signing and RF01 through RF06 remain approved. AS01 through AS06 select stable address-v1 Bech32m identity, exact ordinary-v2 bytes, current-key and generation checks, one authoritative spending nonce with an equal native mirror, actual debit-owner checks, generation-bound discretionary grants and shared client format. Recovery-only sponsorship stays separate. Production role allowlists, complete client acceptance, account creation or migration beyond explicit local records, and production activation remain open. AS05 selects generation-bound discretionary grants. A generation change invalidates an old Dms or queued discretionary grant until the current owner reauthorizes it. RF06 protection applies to the actor, actual debit owner and fee payer. Mandatory liabilities and committed H+2 schedules remain intact. OF01 through OF05 now select the ordinary fee outcome dependency. Local runtime and durable checks are recorded separately; production and client qualification remain open. The current result records local OF-W04 and OF-W05 runtime and durable integration checks within its reported scope. This closes neither the parent production work nor OF-W06 client, production parameter, custody, genesis, operator or release qualification. The current result records the locally checked OF-W06-CLIENT slice: explicit ordinary-v2 SDK, wallet CLI, committed node client views, genesis identity compatibility and documented local pipe-adapter acceptance. This is not an actual CometBFT engine or live RPC result. Full OF-W06 remains open for hosted wallet UI, live engine/RPC and light-client assurance, production values/roles/records and release qualification.

### D10-Q01 — PARTIALLY_APPROVED

Which identity bytes, address codec, role algorithms, versions and signing envelope apply?

Required output: Account and signing specification.

Proposed owner role: Protocol cryptography lead with wallet and SDK leads. Named assignment and reviewer remain as recorded in the structured register.

Recorded approval: decision-register/ordinary-signing-implementation/APPROVAL.json (historical record, not in this repository).

Evidence: batch-2/IDENTITY_DECISION.json, batch-8/implementation/APPROVAL.json, decision-register/recovery-design/APPROVAL.json, decision-register/recovery-implementation/APPROVAL.json, decision-register/recovery-signing/APPROVAL.json, decision-register/recovery-signing/SPEC.md, decision-register/recovery-signing/REPORT.md, decision-register/recovery-signing/TEST_RESULTS.json, decision-register/recovery-signing/SCOPE.json, decision-register/recovery-fee-storage/PROPOSAL.json, decision-register/recovery-fee-storage/AUTHORIZATION.json, decision-register/recovery-fee-storage/CONTRACT.md, decision-register/recovery-fee-storage/FEE_REVIEW.md, decision-register/recovery-fee-storage/STORAGE_MAP.md, decision-register/recovery-fee-storage/WORK_ITEMS.json, decision-register/recovery-fee-storage/ACCEPTANCE.json, decision-register/recovery-fee-implementation/APPROVAL.json, decision-register/recovery-fee-implementation/SCOPE.json, decision-register/recovery-fee-implementation/REPORT.md, decision-register/recovery-fee-implementation/TEST_RESULTS.json, decision-register/ordinary-signing-implementation/APPROVAL.json, decision-register/recovery-acceptance-followup/ORDINARY_SIGNING_PROPOSAL.md, decision-register/recovery-acceptance-followup/ORDINARY_SIGNING_PROPOSAL.json, decision-register/recovery-acceptance-followup/LEGACY_ENTRYPOINTS.md, decision-register/ordinary-signing-implementation/SCOPE.json, decision-register/ordinary-signing-implementation/REPORT.md, decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.json, decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.md, decision-register/ordinary-signing-implementation/TEST_RESULTS.json, decision-register/ordinary-runtime-integration/APPROVAL.json, decision-register/ordinary-runtime-integration/WORK_ITEMS.json, decision-register/ordinary-runtime-integration/README.md, decision-register/ordinary-runtime-integration/DECISION_STATUS.md, decision-register/ordinary-runtime-integration/REPORT.md, decision-register/ordinary-runtime-integration/TEST_RESULTS.json, decision-register/ordinary-client-compatibility/APPROVAL.json, decision-register/ordinary-client-compatibility/SCOPE.json, decision-register/ordinary-client-compatibility/WORK_ITEMS.json, decision-register/ordinary-client-compatibility/README.md, decision-register/ordinary-client-compatibility/DECISION_STATUS.md, decision-register/ordinary-client-compatibility/REPORT.md, decision-register/ordinary-client-compatibility/TEST_RESULTS.json (historical records, not in this repository).

### D10-Q02 — PARTIALLY_APPROVED

Which production timings and remaining signing, fee, custody and migration inputs complete the approved recovery contract?

Account template timing (P01, 30 September 2026): recovery delay, finalization window, policy delay and policy window of 120,960 blocks (7 days) each, and a 17,280-block (1-day) submission lifetime ([E05 values, second set](approvals/P01_E05_VALUES_2_2026-09-30.json)). Recovery fees (P01, 30 September 2026): the ordinary scale, so each sponsored recovery action costs the sponsor at least 1 DRT ([E05 values, third set](approvals/P01_E05_VALUES_3_2026-09-30.json)).

Required output: Account recovery specification.

Proposed owner role: Protocol cryptography lead with wallet and SDK leads. Named assignment and reviewer remain as recorded in the structured register.

Recorded approval: decision-register/ordinary-signing-implementation/APPROVAL.json (historical record, not in this repository).

Evidence: batch-2/IDENTITY_DECISION.json, batch-8/implementation/APPROVAL.json, decision-register/recovery-design/APPROVAL.json, decision-register/recovery-implementation/APPROVAL.json, decision-register/recovery-signing/APPROVAL.json, decision-register/recovery-signing/SPEC.md, decision-register/recovery-signing/REPORT.md, decision-register/recovery-signing/TEST_RESULTS.json, decision-register/recovery-signing/SCOPE.json, decision-register/recovery-fee-storage/PROPOSAL.json, decision-register/recovery-fee-storage/AUTHORIZATION.json, decision-register/recovery-fee-storage/CONTRACT.md, decision-register/recovery-fee-storage/FEE_REVIEW.md, decision-register/recovery-fee-storage/STORAGE_MAP.md, decision-register/recovery-fee-storage/WORK_ITEMS.json, decision-register/recovery-fee-storage/ACCEPTANCE.json, decision-register/recovery-fee-implementation/APPROVAL.json, decision-register/recovery-fee-implementation/SCOPE.json, decision-register/recovery-fee-implementation/REPORT.md, decision-register/recovery-fee-implementation/TEST_RESULTS.json, decision-register/ordinary-signing-implementation/APPROVAL.json, decision-register/recovery-acceptance-followup/ORDINARY_SIGNING_PROPOSAL.md, decision-register/recovery-acceptance-followup/ORDINARY_SIGNING_PROPOSAL.json, decision-register/recovery-acceptance-followup/LEGACY_ENTRYPOINTS.md, decision-register/ordinary-signing-implementation/SCOPE.json, decision-register/ordinary-signing-implementation/REPORT.md, decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.json, decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.md, decision-register/ordinary-signing-implementation/TEST_RESULTS.json, decision-register/ordinary-runtime-integration/APPROVAL.json, decision-register/ordinary-runtime-integration/WORK_ITEMS.json, decision-register/ordinary-runtime-integration/README.md, decision-register/ordinary-runtime-integration/DECISION_STATUS.md, decision-register/ordinary-runtime-integration/REPORT.md, decision-register/ordinary-runtime-integration/TEST_RESULTS.json, decision-register/ordinary-client-compatibility/APPROVAL.json, decision-register/ordinary-client-compatibility/SCOPE.json, decision-register/ordinary-client-compatibility/WORK_ITEMS.json, decision-register/ordinary-client-compatibility/README.md, decision-register/ordinary-client-compatibility/DECISION_STATUS.md, decision-register/ordinary-client-compatibility/REPORT.md, decision-register/ordinary-client-compatibility/TEST_RESULTS.json (historical records, not in this repository).

### D10-Q03 — OPEN

Who controls each signing role, and what custody, backup and recovery arrangements will they accept?

**Solo launch** ([P01, 3 October 2026](approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json)): the founder holds every root key in five key kits (kit N holds key N of each role: genesis, upgrade, freeze, resume) stored in up to five separate places; any three act. Keys stay distinct per role and the 3-of-5 thresholds are unchanged. Disclosed in TRUST_MODEL.md.

Required output: Signing custody register.

Proposed owner role: Protocol cryptography lead with wallet and SDK leads. Named assignment and reviewer remain as recorded in the structured register.

Preparation: D10-Q03 packet (historical record decision-register/parallel-tracks-20260912/track-4/records/D10-Q03.json, not in this repository); the [upgrade custodian intake](custody/upgrade/INTAKE.md) (E05-c) collects the five upgrade custodians' public records, and the [genesis signer intake](custody/genesis/INTAKE.md) the five genesis signers', separate from both other groups. Its checker emits the public signer policy the offline signer and the node read. Acceptance is still open.

Roles (P01, 30 September 2026): five root genesis signers, separate from the emergency and upgrade custodians; the upgrade custodians also hold release handover and halt restart ([production activation approval](approvals/P01_E05_ACTIVATION_2026-09-30.json), design [production activation v1](../node/docs/architecture/production-activation-v1.md)).

Evidence: batch-2/IDENTITY_DECISION.json, batch-8/implementation/APPROVAL.json, decision-register/recovery-design/APPROVAL.json, decision-register/recovery-implementation/APPROVAL.json, decision-register/recovery-signing/APPROVAL.json, decision-register/recovery-signing/SPEC.md, decision-register/recovery-signing/REPORT.md, decision-register/recovery-signing/TEST_RESULTS.json, decision-register/recovery-signing/SCOPE.json, decision-register/recovery-fee-storage/PROPOSAL.json, decision-register/recovery-fee-storage/AUTHORIZATION.json, decision-register/recovery-fee-storage/CONTRACT.md, decision-register/recovery-fee-storage/FEE_REVIEW.md, decision-register/recovery-fee-storage/STORAGE_MAP.md, decision-register/recovery-fee-storage/WORK_ITEMS.json, decision-register/recovery-fee-storage/ACCEPTANCE.json, decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json, decision-register/ordinary-client-compatibility/RECORD_STATUS.md (historical records, not in this repository).

## D11 — Governance and emergency authority

Gate references: G17, G19, G21, G22, G23, G24, G31.

**Recorded approval scope and historical implementation context:** Only bonded stake gives voting power. Liquid DGT gives zero. Count each owner stake once; do not add validator aggregate stake.

### D11-Q01 — PARTIALLY_APPROVED

What governance snapshot, validator eligibility and delegated vote ownership rules apply?

Approved portions (25 September 2026): registered accounts with effective bonded DGT at the finalized parent block, by stable account ID; each owner votes its own bond only; validators have no special vote; no delegation; the snapshot is fixed at the parent block before voting starts; a proposer is a registered account with positive effective bond.

Recorded approval: 25 September governance rules (historical record decision-register/core-function-alignment/E04_GOVERNANCE_RULE_APPROVAL_2026-09-25.json, not in this repository). Design: [governance v1](../node/docs/architecture/governance-v1.md).

Required output: Governance electorate specification.

Proposed owner role: Governance lead with protocol and custody reviewers. Named assignment and reviewer remain as recorded in the structured register.

Evidence: batch-5/eligibility-followup/APPROVAL.json (historical record, not in this repository).

### D11-Q02 — APPROVED

What quorum, approval, veto, abstention, deposit, voting-period and timelock rules apply?

Approved portions (25 September 2026): abstain counts for quorum only; integer basis points with required weight rounded up; deposits held in accounted DGT escrow and refunded once at every terminal outcome, with no burn or sweep; execution at an explicit finalized height with the action bound before voting; no cancellation. Added 27 September 2026: automatic transitions run before signed transactions; a failed execution refunds and continues; finished proposals are removed with their votes, snapshots and escrow records at the next block start after the refund, keeping running totals.

Recorded approvals: 25 September governance rules (historical record decision-register/core-function-alignment/E04_GOVERNANCE_RULE_APPROVAL_2026-09-25.json, not in this repository), [governance v1](../node/docs/architecture/governance-v1.md).

Values (P01, 30 September 2026): quorum 3,340, approval 5,000 and veto 3,340 basis points; 120,960-block (7-day) deposit and voting periods; a 34,560-block (2-day) timelock; a minimum deposit of 10,000 DGT ([E05 values, first set](approvals/P01_E05_VALUES_1_2026-09-30.json)).

Required output: Governance parameter specification.

Proposed owner role: Governance lead with protocol and custody reviewers. Named assignment and reviewer remain as recorded in the structured register.

Evidence: batch-5/eligibility-followup/APPROVAL.json (historical record, not in this repository).

### D11-Q03 — PARTIALLY_APPROVED

Which actions, parameter bounds, treasury powers, emergency powers and upgrade or recovery authorities apply?

**Solo launch** ([P01, 3 October 2026](approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json)): the founder holds all five upgrade keys and all ten emergency keys in five key kits, replacing the separate groups of independent custodians; thresholds and keys per role are unchanged.

Approved portions: transaction freeze while consensus continues; separate resume; persistent upgrade hold; three signatures from five independent custodians with distinct freeze/resume keys; measured height-based validity windows; continued mandatory transitions; evidence-bound resume; separate candidate-specific upgrade clearance; and a separate full-halt procedure. Routine governance (27 September 2026): the action classes are parameter change and validator registry; the governed parameters are new ordinary fee profile versions (gas price, resource costs, account creation fee), `min_self_bond` and `max_active`, each within genesis bounds, and governance is the fee authority; everything else, including where fees go, changes only by upgrade; upgrades stay root-signed only; treasury spending is POST MAINNET. Upgrade authority (30 September 2026): a separate group of five upgrade custodians, distinct from the emergency custodians, with three SLH-DSA signatures to admit and three fresh ones to activate, and a fixed minimum notice between admission and activation; the configuration check refuses an upgrade key that holds an emergency role.

Values (P01, 30 September 2026): an upgrade activates at least 120,960 blocks (7 days) after admission; genesis bounds of 4 to 32 for `max_active`, 10,000 to 1,000,000 DGT for `min_self_bond`, and 1 to 100 DRT for the account creation fee, with a basic transfer governed between 0.1 and 10 DRT. Fee bounds (P01, 30 September 2026): gas price 1 to 100 and every per-resource cost 0 to 100,000; the node refuses a fee-profile proposal that puts a reference basic Send outside 0.1 to 10 DRT, a check built in production activation step A6 as the genesis bound `reference_send_fee_udrt` ([E05 values, third set](approvals/P01_E05_VALUES_3_2026-09-30.json), [fee range approval](approvals/P01_E05_FEE_RANGE_2026-10-02.json)).

Approvals: initial emergency rules (historical record decision-register/emergency-transaction-freeze/policy/APPROVAL.json, not in this repository), six additional recommendations (historical record decision-register/emergency-release-staging/policy/APPROVAL.json, not in this repository), [governance v1](../node/docs/architecture/governance-v1.md), [30 September operations approvals](approvals/P01_E04_OPERATIONS_2026-09-30.json), [E05 values, second set](approvals/P01_E05_VALUES_2_2026-09-30.json).

Remaining inputs: custodian names, keys and epochs; numeric timing limits; acceptance of production control formats; upgrade-clearance membership and threshold; the upgrade custodians; the measured upgrade and handover window bounds; halt/restart authority; genesis bounds for the gas price and per-resource costs; and recovery authorities. Version 2 emergency bindings and the first actual receipt-index upgrade executor are implemented with development-only inputs. Cross-binary and production qualification remain open. Use the [custodian intake packet](custody/emergency/INTAKE.md) for the emergency custodians and the [upgrade custodian intake](custody/upgrade/INTAKE.md) for the upgrade custodians to supply public records. This implementation adds no policy approval.

Parameter set (P01, 30 September 2026): root, emergency and upgrade custodian keys use SLH-DSA-SHAKE-256s, the set the node's root verifier implements ([custody approval](approvals/P01_E05_CUSTODY_2026-09-30.json)).

Production activation (P01, 30 September 2026): root genesis is signed 3-of-5 by its own group of genesis signers; upgrade schema 2 and handover v2 sign over a finalized anchor and a bounded window, as emergency freeze v2 does; the upgrade custodians, three of five, control release handover and halt restart, which keeps its exact-height binding ([production activation approval](approvals/P01_E05_ACTIVATION_2026-09-30.json), design [production activation v1](../node/docs/architecture/production-activation-v1.md)). Notice scope (P01, 1 October 2026): the 120,960-block notice applies to release handovers as well as state-migration upgrades ([approval](approvals/P01_E05_UPGRADE_NOTICE_2026-10-01.json)). Upgrade schema 2 and handover schema 2 are implemented (step A3); their window bounds and anchor ages are measured values. Root controls (P01, 1 October 2026): a production configuration must carry all three controls at schema 2, and production builds accept only production control policies (step A4, [approval](approvals/P01_E05_ROOT_CONTROLS_2026-10-01.json)).

Required output: Governance authority matrix.

Proposed owner role: Governance lead with protocol and custody reviewers. Named assignment and reviewer remain as recorded in the structured register.

Evidence: batch-5/eligibility-followup/APPROVAL.json (historical record, not in this repository).

## D12 — Infrastructure and service objectives

Gate references: G01, G07, G08, G09, G24, G25, G26, G27, G28, G29, G30, G31, G32, G34, G35.

**Recorded approval scope and historical implementation context:** Existing servers are on Hetzner. The observed host is not an approved production topology.

### D12-Q01 — PARTIALLY_APPROVED

What resources, regions, account separation, peer topology, access controls and capacity budget apply?

**Solo launch** ([P01, 3 October 2026](approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json)): one operator, the founder, with one validator, one sentry (also the archive node) and one endpoint.

Approved portion (P01, 30 September 2026): validators are private and peer only with their own sentries, pinned by full key; sentries face the network; separate endpoint nodes serve the client channel. Validator hosts run no RPC and no management port, and operator access is console-only.

Recorded approval: [30 September operations approvals](approvals/P01_E04_OPERATIONS_2026-09-30.json).

Production activation (P01, 30 September 2026): validators allow only local owner-only Unix sockets, no network listener; the supervisor checks the engine at startup and then relies on kernel limits, with no periodic pauses; one IP per node, with static 1:1 NAT for public sentries; a published partial mesh within 64 pins ([production activation approval](approvals/P01_E05_ACTIVATION_2026-09-30.json), design [production activation v1](../node/docs/architecture/production-activation-v1.md)). Replaced (P01, 6 October 2026, [host setup](approvals/P01_E05_HOSTS_2026-10-06.json)): each host's public IPv4 is on its own interface, with no NAT.

Host setup ([P01, 6 October 2026](approvals/P01_E05_HOSTS_2026-10-06.json)): Ubuntu 24.04 LTS on every host; each host's public IPv4 on its own interface with no NAT; node keys made offline and delivered in digest-checked host bundles, with the keys sealed under a passphrase (AES-256); the service account `dytallix` (41001) and the `/opt`, `/etc` and `/var/lib/dytallix` layout with one unit ([host setup v1](../node/docs/architecture/host-setup-v1.md)). Host values ([P01, 6 October 2026](approvals/P01_E05_HOST_VALUES_2026-10-06.json)): 6 GiB memory, 4,096 tasks and 65,536 open files per host; ports 26656, 26670 and 8080; root-owned code; the catalog, observation and root helper bounds; 30-second root and emergency verifier timeouts ([SETUP_VALUES.json](hosts/SETUP_VALUES.json)). Host sealing ([P01, 6 October 2026](approvals/P01_E05_HOST_SEALING_2026-10-06.json)): each host's node keys sealed by `dytallix-root-sign seal` under its own 256-bit seal code (AES-256-GCM, key from SHAKE256), printed once as a checked paper line, written twice and kept with two different kits' papers. Release switch and validator fencing ([P01, 6 October 2026](approvals/P01_E05_HOST_RECOVERY_2026-10-06.json)): a host stages a new release beside the running one and switches the unit when the old release stops at activation; before a replacement validator is installed, the old validator server is deleted or reinstalled in the provider console.

Supervisor production mode (P01, 1 October 2026): the engine, application, bridge and HTTP adapter are observed once at startup and then held by kernel limits, while helpers keep paused checks; readiness waits for catch-up up to a per-host budget, the E05 value `catch_up_millis` ([supervisor approval](approvals/P01_E05_SUPERVISOR_2026-10-01.json)).

Host settings (P01, 2 October 2026), from the [E05 values, fourth set](approvals/P01_E05_VALUES_4_2026-10-02.json):
- **Supervisor:** process limits, a 1 s pause-free monitor, and a six-hour catch-up budget.
- **Block history:** the retained window everywhere, with archive only on designated archive nodes.
- **Adapter:** its compiled ceilings.
- **Emergency verifier:** its byte bounds.

Remaining inputs: counts, hosts, regions, account separation, failure domains and the capacity budget (E05).

Required output: Production topology and budget.

Proposed owner role: SRE lead with budget owner and validator coordinator. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [PRODUCTION_INFRASTRUCTURE_DRAFT.json](PRODUCTION_INFRASTRUCTURE_DRAFT.json), batch-1/OPERATIONS_EVIDENCE.json (historical record, not in this repository).

### D12-Q02 — APPROVED

What service targets, on-call coverage, retention, backup and recovery objectives apply?

Approved (P01, 3 October 2026, [operations objectives approval](approvals/P01_E05_OPERATIONS_OBJECTIVES_2026-10-03.json)):

- **Service targets** per month, measured by the independent monitor, planned upgrades excluded: a block committed in 99.9% of minutes (about 43 minutes of budget); at least one client endpoint within 3 blocks of the head in 99.5% of minutes (about 3.6 hours).
- **On-call:** 24/7 paging. Severity 1 (chain halted, a validator at double-sign risk, a key compromise, monitoring down) is acknowledged within 15 minutes, with a responder at a console within 30; severity 2 within 4 hours; severity 3 the next business day. At least two people in the rotation plus an escalation contact.
- **Retention:** full history on at least two archive nodes run by different operators on different providers, plus a monthly offline export; logs 90 days, redacted; metrics 13 months; incident, alert and audit records permanently.
- **Backups and recovery:** RPO zero committed blocks. The approved daily snapshots are copied, encrypted with AES-256, to a second provider; keys move only through custody; a validator's signing state is never restored from a backup, and a replacement signs only after the old host is fenced off. RTO: validator 8 hours (2 hours to provision plus the approved 6-hour catch-up), sentry 4 hours, endpoint service 15 minutes by failover, monitoring 1 hour. Restore drills quarterly on staging.

**Replaced the same day by the solo launch profile** ([approval](approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json)): the targets become published goals of 99.5% (chain) and 99.0% (endpoints); the founder is paged 24/7, best effort, with no fixed response time, by a free hosted uptime checker; one archive node (the sentry) plus a monthly offline export to the founder's drive; the endpoint service recovers in 4 hours. The backup rules, the RPO and the validator, sentry and monitoring recovery times stand ([operations objectives](operations/OBJECTIVES.md)).

Alert signals and routes (A22), providers (D12-Q03) and the written recovery procedures (F17, F19) remain open.

Required output: Operations acceptance specification ([operations/OBJECTIVES.md](operations/OBJECTIVES.md), [OBJECTIVES.json](operations/OBJECTIVES.json)).

Proposed owner role: SRE lead with budget owner and validator coordinator. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [PRODUCTION_INFRASTRUCTURE_DRAFT.json](PRODUCTION_INFRASTRUCTURE_DRAFT.json), batch-1/OPERATIONS_EVIDENCE.json (historical record, not in this repository).

### D12-Q03 — OPEN

Which exact assets, operators, account owners and funded commitments implement the topology?

**Solo launch** ([P01, 3 October 2026](approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json)): three hosts (validator, sentry that is also the archive node, endpoint); before genesis they are the staging environment for E03, the measurements and the seven simulations, then wiped and re-provisioned.

Required output: Approved infrastructure inventory.

Proposed owner role: SRE lead with budget owner and validator coordinator. Named assignment and reviewer remain as recorded in the structured register.

Preparation: D12-Q03 packet (historical record decision-register/parallel-tracks-20260912/track-4/records/D12-Q03.json, not in this repository). Acceptance is still open.

Evidence: [PRODUCTION_INFRASTRUCTURE_DRAFT.json](PRODUCTION_INFRASTRUCTURE_DRAFT.json), batch-1/OPERATIONS_EVIDENCE.json, decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json, decision-register/ordinary-client-compatibility/RECORD_STATUS.md (historical records, not in this repository).

## D13 — Canonical network identity

Gate references: G04, G05.

**Recorded approval scope and historical implementation context:** One permanent mainnet genesis is required. AS01 selects address network codes and Bech32m prefixes: 1/dytallix, 2/tdytallix, 3/ddytallix. AS03 bounds the combined profile chain ID to 1 through 128 UTF-8 bytes. The network identity is approved (D13-Q01, 3 October 2026); the genesis time itself and the final digest (D13-Q02) remain open.

### D13-Q01 — APPROVED

What display name, unused chain ID and genesis timestamp procedure apply?

Approved (P01, 3 October 2026, [network identity approval](approvals/P01_E05_CHAIN_IDENTITY_2026-10-03.json)):

- **Chain ID** `dytallix-mainnet-1`. It names mainnet, so every development build of the node and the engine refuses it. The `-1` is the revision for a later chain-ID change. Transactions bind the genesis digest as well as the chain ID, so earlier configurations that used the same string cannot replay into the chain.
- **Display name** Dytallix; the other networks are Dytallix Testnet and Dytallix Devnet, matching the address prefixes.
- **Genesis time procedure.** Set at the final input freeze, in whole seconds UTC, at 14:00:00 on a weekday and at least 72 hours after the final build. In that window operators reproduce the digest, three of five genesis signers sign, and hosts install the files and start early. If the signatures or the T−6h go/no-go are not complete, a new time is set: a new build, digest and signatures, never reused.

The machine-readable record is [genesis/IDENTITY.json](genesis/IDENTITY.json). The genesis input resolver makes a build production-eligible only when its records name this identity, and `network_identity.py` checks a genesis time against the procedure ([genesis builder](../node/docs/mainnet/e05-genesis-builder.md)).

Required output: Network identity specification ([genesis/IDENTITY.json](genesis/IDENTITY.json)).

Proposed owner role: Genesis and release leads. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [USER_LAUNCH_REQUIREMENTS.txt](USER_LAUNCH_REQUIREMENTS.txt).

### D13-Q02 — OPEN

What final identity manifest and deterministic genesis digest will the release signers approve?

Required output: Signed genesis commitment.

Proposed owner role: Genesis and release leads. Named assignment and reviewer remain as recorded in the structured register.

Preparation: D13-Q02 packet (historical record decision-register/parallel-tracks-20260912/track-4/records/D13-Q02.json, not in this repository). Acceptance is still open.

Evidence: [USER_LAUNCH_REQUIREMENTS.txt](USER_LAUNCH_REQUIREMENTS.txt), decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json, decision-register/ordinary-client-compatibility/RECORD_STATUS.md (historical records, not in this repository).

## D14 — Release and independent review

Gate references: G01, G02, G03, G04, G06, G07, G08, G09, G10, G11, G12, G13, G14, G15, G16, G17, G18, G19, G20, G21, G22, G23, G24, G25, G26, G27, G28, G29, G30, G31, G32, G33, G34, G35.

**Recorded approval scope and historical implementation context:** Reproducible release, deterministic genesis, required features, closure of launch blockers and seven full launch simulations remain required. Local test passes do not satisfy these gates. LR01 authorizes local retirement of the identified development timer and direct legacy staking/emission mutations. Preserve explicit RewardState adapters, shared planning, historical reads and staking ownership compatibility records. Deployment, migration and state deletion are not authorized. The legacy rounding defect remains historical diagnostic evidence; retirement is not an arithmetic repair.

### D14-Q01 — APPROVED

Which release targets, build environment, registry, signing authority and review independence criteria apply?

**Solo launch** ([P01, 3 October 2026](approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json)): the independent review is a 30-day public review of the frozen candidate with a DGT bug bounty and a separate review labeled as AI, signed off by the founder; the launch is labeled unaudited until a human audit is funded. A clean GitHub Actions runner and a fresh local container reproduce the release byte for byte.

Approved portion (P01, 30 September 2026): release binaries are built without the development entry points ([production activation approval](approvals/P01_E05_ACTIVATION_2026-09-30.json), design [production activation v1](../node/docs/architecture/production-activation-v1.md)).

E06 ([P01, 5 October 2026](approvals/P01_E06_RELEASE_2026-10-05.json)): Linux x86_64, the wallet also from source; the pinned builder image ([release/builder](../release/builder/Dockerfile)) and no node container; GitHub Releases on DytallixHQ/dytallix after the repository move; no release-signing key, since the root-signed genesis binds the first release and the upgrade keys approve later ones. Review independence is the solo launch profile's public review.

Release libraries ([P01, 5 October 2026](approvals/P01_E06_STATIC_RELEASE_2026-10-05.json)): every release binary is a static executable and the build refuses any other, so the release manifest ([release_manifest.py](../release/release_manifest.py)) names no runtime libraries; library fixes ship as new releases.

Repository ([P01, 5 October 2026](approvals/P01_E06_REPOSITORY_2026-10-05.json)): DytallixHQ/dytallix is MIT OR Apache-2.0, with third-party code under its own license; outside contributions are signed off under the DCO, with no CLA; the history moves with the founder's GitHub noreply address; afterwards mainnet/ in exocognosis/dytallix becomes a pointer. The move was made on 5 October 2026 ([the move](../release/MOVE.md)).

Required output: Release and review specification.

Proposed owner role: Release and QA leads with independent reviewers. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [USER_LAUNCH_REQUIREMENTS.txt](USER_LAUNCH_REQUIREMENTS.txt), [LAUNCH_GATES.json](LAUNCH_GATES.json), batch-10/REPORT.md (historical record, not in this repository).

### D14-Q02 — PARTIALLY_APPROVED

What workload, finality, recovery and acceptance thresholds, calendar and launch authority apply?

**Solo launch** ([P01, 3 October 2026](approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json)): the P02 precondition for root signing becomes the 30-day public review and the founder's sign-off; the E03 runs, measurements and seven simulations run on the production hosts before genesis.

Approved portion (P01, 30 September 2026): a node runs the production profiles only when the root genesis signatures verify over the exact genesis files and release, and the root signers sign only after the P02 review, E06 release acceptance and gate acceptance ([production activation approval](approvals/P01_E05_ACTIVATION_2026-09-30.json), design [production activation v1](../node/docs/architecture/production-activation-v1.md)).

Remaining inputs: workload, finality, recovery and acceptance thresholds, and the calendar.

Required output: Qualification and launch acceptance plan.

Proposed owner role: Release and QA leads with independent reviewers. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [USER_LAUNCH_REQUIREMENTS.txt](USER_LAUNCH_REQUIREMENTS.txt), [LAUNCH_GATES.json](LAUNCH_GATES.json), batch-10/REPORT.md (historical record, not in this repository).

### D14-Q03 — OPEN

Who accepts each engineering, operations, signing and independent review role?

**Solo launch** ([P01, 3 October 2026](approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json)): the founder holds every engineering, operations and signing role. The independent review is a 30-day public review with a DGT bug bounty plus a separate AI review labeled as AI, signed off by the founder; the launch is labeled unaudited until a human audit is funded.

Required output: Release responsibility register.

Proposed owner role: Release and QA leads with independent reviewers. Named assignment and reviewer remain as recorded in the structured register.

Preparation: D14-Q03 packet (historical record decision-register/parallel-tracks-20260912/track-4/records/D14-Q03.json, not in this repository). Acceptance is still open.

Evidence: [USER_LAUNCH_REQUIREMENTS.txt](USER_LAUNCH_REQUIREMENTS.txt), [LAUNCH_GATES.json](LAUNCH_GATES.json), batch-10/REPORT.md, decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json, decision-register/ordinary-client-compatibility/RECORD_STATUS.md (historical records, not in this repository).

The pre-consolidation document (historical record decision-register/gate-consolidation-20260912/evidence/before/DECISIONS_REQUIRED.md, not in this repository) preserves the complete earlier narrative. Its implementation status statements do not override the master list.
