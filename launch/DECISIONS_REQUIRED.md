# Dytallix decision and record register

Gate readiness has one source: [LAUNCH_GATES.json](LAUNCH_GATES.json), rendered as [the master list](MAINNET_GATE_MASTER.md). This document tracks policy questions and required records. It does not grant launch authority.

The current register contains 20 OPEN policy questions, six PARTIALLY_APPROVED policy questions and eight OPEN required records. Explicit approval records identify the accepted portions. Unset production values, assignees and reviewers remain unset.

Current evidence includes the [emergency controls and upgrade execution package](decision-register/emergency-upgrade-execution/REPORT.md). The master credits implementation and qualification within each report's stated scope. Production acceptance remains incomplete.

Each recorded approval scope below retains its original implementation context. Read current implementation progress and remaining work in the linked gates. Exact question fields, approvals and supersession records remain in [MAINNET_DECISION_REGISTER.json](MAINNET_DECISION_REGISTER.json).

## D01 — Adaptive issuance

Gate references: G14, G19.

**Recorded approval scope and historical implementation context:** Corrected adaptive controller selected. Atomic epoch issuance and explicit observation inputs authorized for local implementation.

### D01-Q01 — OPEN

Which production controller version, gains, bounds, initial state and initial command apply?

Required output: Production controller configuration.

Proposed owner role: Protocol and economics lead. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [snapshots/dytallix-node/docs/mainnet/specification-decisions.md](snapshots/dytallix-node/docs/mainnet/specification-decisions.md), [batch-6/issuance-timing/APPROVAL.json](batch-6/issuance-timing/APPROVAL.json).

### D01-Q02 — OPEN

Which observations, sources, authentication, aggregation and missing-input or resume rules apply?

Required output: Observation contract.

Proposed owner role: Protocol and economics lead. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [snapshots/dytallix-node/docs/mainnet/specification-decisions.md](snapshots/dytallix-node/docs/mainnet/specification-decisions.md), [batch-6/issuance-timing/APPROVAL.json](batch-6/issuance-timing/APPROVAL.json).

## D02 — DRT reward allocation

Gate references: G14, G16, G17, G19, G23.

**Recorded approval scope and historical implementation context:** 40/30/30 validator/staker/treasury split. Six decimals. Checked floor allocation by eligible stake. Sole recipient gets the budget. Staking pool pays owners with no commission. Separate rounding and inactive reserves have no automatic recipient or sweep authority. LR01 authorizes local retirement of the identified development timer and direct legacy staking/emission mutations. Preserve explicit RewardState adapters, shared planning, historical reads and staking ownership compatibility records. Deployment, migration and state deletion are not authorized. The legacy rounding defect remains historical diagnostic evidence; retirement is not an arithmetic repair.

### D02-Q01 — OPEN

What eligibility and allocation rules apply to the separate validator reward pool?

Required output: Validator reward specification.

Proposed owner role: Reward accounting and treasury leads. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [evidence/TOKENOMICS_APPROVED_SOURCE.json](evidence/TOKENOMICS_APPROVED_SOURCE.json), [batch-6/APPROVAL.json](batch-6/APPROVAL.json), [batch-6/integration-followup/APPROVAL.json](batch-6/integration-followup/APPROVAL.json), [batch-6/issuance-timing/APPROVAL.json](batch-6/issuance-timing/APPROVAL.json).

### D02-Q02 — OPEN

Who receives treasury rewards, and which custody records authorize receipt?

Required output: Treasury reward configuration.

Proposed owner role: Reward accounting and treasury leads. Named assignment and reviewer remain as recorded in the structured register.

Preparation: [D02-Q02 packet](decision-register/parallel-tracks-20260912/track-4/records/D02-Q02.json). Acceptance is still open.

Evidence: [evidence/TOKENOMICS_APPROVED_SOURCE.json](evidence/TOKENOMICS_APPROVED_SOURCE.json), [batch-6/APPROVAL.json](batch-6/APPROVAL.json), [batch-6/integration-followup/APPROVAL.json](batch-6/integration-followup/APPROVAL.json), [batch-6/issuance-timing/APPROVAL.json](batch-6/issuance-timing/APPROVAL.json), [decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json](decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json), [decision-register/ordinary-client-compatibility/RECORD_STATUS.md](decision-register/ordinary-client-compatibility/RECORD_STATUS.md).

## D03 — Interval timing

Gate references: G14, G19.

**Recorded approval scope and historical implementation context:** One finalized block per reward interval, using parent finalized state. Explicit N finalized blocks per issuance epoch. Even per-bucket scheduling gives extra units to first blocks. Separate split reserve. Batch 8 makes stake and membership changes effective at H+2.

### D03-Q01 — OPEN

What production epoch length N and observation sampling windows apply?

Required output: Production issuance timing matrix.

Proposed owner role: Protocol lead. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [batch-6/integration-followup/APPROVAL.json](batch-6/integration-followup/APPROVAL.json), [batch-6/issuance-timing/APPROVAL.json](batch-6/issuance-timing/APPROVAL.json), [batch-8/implementation/APPROVAL.json](batch-8/implementation/APPROVAL.json).

## D04 — Fees

Gate references: G10, G11, G12, G14, G20.

**Recorded approval scope and historical implementation context:** Recovery RF01 through RF06 and ordinary actor-paid udrt/no-tips authority remain selected. OF01 through OF05 select ordinary fee contract format 1, canonical profile binding, deterministic metering, zero-charge preacceptance rejection, measured accepted application failure, accepted out-of-gas, shared reservations, atomic cap release and nonce outcomes, and undistributed udrt custody. Production values and actual runtime or durable integration remain separate. Reserve metadata gas before acceptance and count it once. Initial vesting/custody eligibility remains a zero-charge funding condition; a later breached reservation guarantee is an internal fault. Dms authority checks precede acceptance while maturity is a typed post-acceptance state condition. Shared aggregate limits supplement recovery-specific ceilings; mandatory expiry retains its own budget. General distribution and burn policy remain open. The current result records local OF-W04 and OF-W05 runtime and durable integration checks within its reported scope. This closes neither the parent production work nor OF-W06 client, production parameter, custody, genesis, operator or release qualification. The current result records the locally checked OF-W06-CLIENT slice: explicit ordinary-v2 SDK, wallet CLI, committed node client views, genesis identity compatibility and documented local pipe-adapter acceptance. This is not an actual CometBFT engine or live RPC result. Full OF-W06 remains open for hosted wallet UI, live engine/RPC and light-client assurance, production values/roles/records and release qualification.

### D04-Q01 — PARTIALLY_APPROVED

Which fee denomination, metering, formula, prices, bounds and tips apply?

Required output: Fee parameter specification.

Proposed owner role: Fee accounting lead. Named assignment and reviewer remain as recorded in the structured register.

Recorded approval: [decision-register/ordinary-fee-implementation/APPROVAL.json](decision-register/ordinary-fee-implementation/APPROVAL.json).

Evidence: [batch-2/ECONOMIC_DECISIONS.json](batch-2/ECONOMIC_DECISIONS.json), [batch-10/REPORT.md](batch-10/REPORT.md), [decision-register/recovery-fee-storage/PROPOSAL.json](decision-register/recovery-fee-storage/PROPOSAL.json), [decision-register/recovery-fee-storage/AUTHORIZATION.json](decision-register/recovery-fee-storage/AUTHORIZATION.json), [decision-register/recovery-fee-storage/CONTRACT.md](decision-register/recovery-fee-storage/CONTRACT.md), [decision-register/recovery-fee-storage/FEE_REVIEW.md](decision-register/recovery-fee-storage/FEE_REVIEW.md), [decision-register/recovery-fee-storage/STORAGE_MAP.md](decision-register/recovery-fee-storage/STORAGE_MAP.md), [decision-register/recovery-fee-storage/WORK_ITEMS.json](decision-register/recovery-fee-storage/WORK_ITEMS.json), [decision-register/recovery-fee-storage/ACCEPTANCE.json](decision-register/recovery-fee-storage/ACCEPTANCE.json), [decision-register/recovery-fee-implementation/APPROVAL.json](decision-register/recovery-fee-implementation/APPROVAL.json), [decision-register/recovery-fee-implementation/SCOPE.json](decision-register/recovery-fee-implementation/SCOPE.json), [decision-register/recovery-fee-implementation/REPORT.md](decision-register/recovery-fee-implementation/REPORT.md), [decision-register/recovery-fee-implementation/TEST_RESULTS.json](decision-register/recovery-fee-implementation/TEST_RESULTS.json), [decision-register/ordinary-signing-implementation/APPROVAL.json](decision-register/ordinary-signing-implementation/APPROVAL.json), [decision-register/ordinary-fee-implementation/APPROVAL.json](decision-register/ordinary-fee-implementation/APPROVAL.json), [decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.md](decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.md), [decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.json](decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.json), [decision-register/ordinary-fee-implementation/SCOPE.json](decision-register/ordinary-fee-implementation/SCOPE.json), [decision-register/ordinary-fee-implementation/WORK_ITEMS.json](decision-register/ordinary-fee-implementation/WORK_ITEMS.json), [decision-register/ordinary-fee-implementation/README.md](decision-register/ordinary-fee-implementation/README.md), [decision-register/ordinary-fee-implementation/DECISION_STATUS.md](decision-register/ordinary-fee-implementation/DECISION_STATUS.md), [decision-register/ordinary-fee-implementation/REPORT.md](decision-register/ordinary-fee-implementation/REPORT.md), [decision-register/ordinary-fee-implementation/TEST_RESULTS.json](decision-register/ordinary-fee-implementation/TEST_RESULTS.json), [decision-register/ordinary-runtime-integration/APPROVAL.json](decision-register/ordinary-runtime-integration/APPROVAL.json), [decision-register/ordinary-runtime-integration/WORK_ITEMS.json](decision-register/ordinary-runtime-integration/WORK_ITEMS.json), [decision-register/ordinary-runtime-integration/README.md](decision-register/ordinary-runtime-integration/README.md), [decision-register/ordinary-runtime-integration/DECISION_STATUS.md](decision-register/ordinary-runtime-integration/DECISION_STATUS.md), [decision-register/ordinary-runtime-integration/REPORT.md](decision-register/ordinary-runtime-integration/REPORT.md), [decision-register/ordinary-runtime-integration/TEST_RESULTS.json](decision-register/ordinary-runtime-integration/TEST_RESULTS.json), [decision-register/ordinary-client-compatibility/APPROVAL.json](decision-register/ordinary-client-compatibility/APPROVAL.json), [decision-register/ordinary-client-compatibility/SCOPE.json](decision-register/ordinary-client-compatibility/SCOPE.json), [decision-register/ordinary-client-compatibility/WORK_ITEMS.json](decision-register/ordinary-client-compatibility/WORK_ITEMS.json), [decision-register/ordinary-client-compatibility/README.md](decision-register/ordinary-client-compatibility/README.md), [decision-register/ordinary-client-compatibility/DECISION_STATUS.md](decision-register/ordinary-client-compatibility/DECISION_STATUS.md), [decision-register/ordinary-client-compatibility/REPORT.md](decision-register/ordinary-client-compatibility/REPORT.md), [decision-register/ordinary-client-compatibility/TEST_RESULTS.json](decision-register/ordinary-client-compatibility/TEST_RESULTS.json).

### D04-Q02 — PARTIALLY_APPROVED

How do reservation, failures, refunds, minimum charges and recipient credits work?

Required output: Fee settlement specification.

Proposed owner role: Fee accounting lead. Named assignment and reviewer remain as recorded in the structured register.

Recorded approval: [decision-register/ordinary-fee-implementation/APPROVAL.json](decision-register/ordinary-fee-implementation/APPROVAL.json).

Evidence: [batch-2/ECONOMIC_DECISIONS.json](batch-2/ECONOMIC_DECISIONS.json), [batch-10/REPORT.md](batch-10/REPORT.md), [decision-register/recovery-fee-storage/PROPOSAL.json](decision-register/recovery-fee-storage/PROPOSAL.json), [decision-register/recovery-fee-storage/AUTHORIZATION.json](decision-register/recovery-fee-storage/AUTHORIZATION.json), [decision-register/recovery-fee-storage/CONTRACT.md](decision-register/recovery-fee-storage/CONTRACT.md), [decision-register/recovery-fee-storage/FEE_REVIEW.md](decision-register/recovery-fee-storage/FEE_REVIEW.md), [decision-register/recovery-fee-storage/STORAGE_MAP.md](decision-register/recovery-fee-storage/STORAGE_MAP.md), [decision-register/recovery-fee-storage/WORK_ITEMS.json](decision-register/recovery-fee-storage/WORK_ITEMS.json), [decision-register/recovery-fee-storage/ACCEPTANCE.json](decision-register/recovery-fee-storage/ACCEPTANCE.json), [decision-register/recovery-fee-implementation/APPROVAL.json](decision-register/recovery-fee-implementation/APPROVAL.json), [decision-register/recovery-fee-implementation/SCOPE.json](decision-register/recovery-fee-implementation/SCOPE.json), [decision-register/recovery-fee-implementation/REPORT.md](decision-register/recovery-fee-implementation/REPORT.md), [decision-register/recovery-fee-implementation/TEST_RESULTS.json](decision-register/recovery-fee-implementation/TEST_RESULTS.json), [decision-register/ordinary-signing-implementation/APPROVAL.json](decision-register/ordinary-signing-implementation/APPROVAL.json), [decision-register/ordinary-fee-implementation/APPROVAL.json](decision-register/ordinary-fee-implementation/APPROVAL.json), [decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.md](decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.md), [decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.json](decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.json), [decision-register/ordinary-fee-implementation/SCOPE.json](decision-register/ordinary-fee-implementation/SCOPE.json), [decision-register/ordinary-fee-implementation/WORK_ITEMS.json](decision-register/ordinary-fee-implementation/WORK_ITEMS.json), [decision-register/ordinary-fee-implementation/README.md](decision-register/ordinary-fee-implementation/README.md), [decision-register/ordinary-fee-implementation/DECISION_STATUS.md](decision-register/ordinary-fee-implementation/DECISION_STATUS.md), [decision-register/ordinary-fee-implementation/REPORT.md](decision-register/ordinary-fee-implementation/REPORT.md), [decision-register/ordinary-fee-implementation/TEST_RESULTS.json](decision-register/ordinary-fee-implementation/TEST_RESULTS.json), [decision-register/ordinary-runtime-integration/APPROVAL.json](decision-register/ordinary-runtime-integration/APPROVAL.json), [decision-register/ordinary-runtime-integration/WORK_ITEMS.json](decision-register/ordinary-runtime-integration/WORK_ITEMS.json), [decision-register/ordinary-runtime-integration/README.md](decision-register/ordinary-runtime-integration/README.md), [decision-register/ordinary-runtime-integration/DECISION_STATUS.md](decision-register/ordinary-runtime-integration/DECISION_STATUS.md), [decision-register/ordinary-runtime-integration/REPORT.md](decision-register/ordinary-runtime-integration/REPORT.md), [decision-register/ordinary-runtime-integration/TEST_RESULTS.json](decision-register/ordinary-runtime-integration/TEST_RESULTS.json), [decision-register/ordinary-client-compatibility/APPROVAL.json](decision-register/ordinary-client-compatibility/APPROVAL.json), [decision-register/ordinary-client-compatibility/SCOPE.json](decision-register/ordinary-client-compatibility/SCOPE.json), [decision-register/ordinary-client-compatibility/WORK_ITEMS.json](decision-register/ordinary-client-compatibility/WORK_ITEMS.json), [decision-register/ordinary-client-compatibility/README.md](decision-register/ordinary-client-compatibility/README.md), [decision-register/ordinary-client-compatibility/DECISION_STATUS.md](decision-register/ordinary-client-compatibility/DECISION_STATUS.md), [decision-register/ordinary-client-compatibility/REPORT.md](decision-register/ordinary-client-compatibility/REPORT.md), [decision-register/ordinary-client-compatibility/TEST_RESULTS.json](decision-register/ordinary-client-compatibility/TEST_RESULTS.json).

## D05 — Burning and supply authority

Gate references: G05, G12, G13, G14, G15, G20, G23.

**Recorded approval scope and historical implementation context:** Fixed one-billion DGT total and category shares approved. Production burn rules and supply authority remain open. RF05 approves undistributed recovery fee custody with no burn, recipient or sweep operation in this integration. The general DRT burn policy remains open. OF05 selects existing undistributed udrt custody with no recipient, burn or sweep operation. General DRT burn policy remains open. Fee delta must be reconciled separately from successful action balance changes.

### D05-Q01 — OPEN

Which DRT fee components burn, and what custody debit and supply change occur on success or failure?

Required output: DRT burn specification.

Proposed owner role: Economics and supply accounting leads. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [evidence/TOKENOMICS_APPROVED_SOURCE.json](evidence/TOKENOMICS_APPROVED_SOURCE.json), [batch-2/ECONOMIC_DECISIONS.json](batch-2/ECONOMIC_DECISIONS.json), [decision-register/recovery-fee-implementation/APPROVAL.json](decision-register/recovery-fee-implementation/APPROVAL.json).

### D05-Q02 — OPEN

Will all DGT issue at genesis, with no later mint authority and no initial DGT burn?

Required output: DGT issuance and authority specification.

Proposed owner role: Economics and supply accounting leads. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [evidence/TOKENOMICS_APPROVED_SOURCE.json](evidence/TOKENOMICS_APPROVED_SOURCE.json), [batch-2/ECONOMIC_DECISIONS.json](batch-2/ECONOMIC_DECISIONS.json), [decision-register/recovery-fee-implementation/APPROVAL.json](decision-register/recovery-fee-implementation/APPROVAL.json).

## D06 — Consensus and validation

Gate references: G01, G02, G09, G25, G26, G30, G35.

**Recorded approval scope and historical implementation context:** CometBFT v0.40.0 local integration and qualification authorized. [D06-Q01 partial approval](decision-register/core-function-alignment/E01_PRODUCTION_PROFILE_APPROVAL_2026-09-25.json) selects the post-quantum role and pinned-peer policy. The exact production engine commit, addresses and acceptance remain unset.

### D06-Q01 — PARTIALLY_APPROVED

Which exact engine version, cryptographic roles and transport authentication profile will production freeze?

Required output: Production consensus profile.

Proposed owner role: Protocol lead with cryptography and network reviewers. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [batch-7/APPROVAL.json](batch-7/APPROVAL.json), [batch-9/REPORT.md](batch-9/REPORT.md), [batch-10/REPORT.md](batch-10/REPORT.md).

Approved portion: ML-KEM-768 and ML-DSA-65 authenticated pinned peers; ML-DSA-65 validators; distinct role keys; exact approved IP endpoints; no classical or plaintext fallback. The exact engine commit, production chain ID, addresses, keys, custody and candidate-bound review remain open.

### D06-Q02 — OPEN

What timing, capacity, fault assumptions, synchronization and history requirements apply?

Required output: Consensus operating specification.

Proposed owner role: Protocol lead with cryptography and network reviewers. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [batch-7/APPROVAL.json](batch-7/APPROVAL.json), [batch-9/REPORT.md](batch-9/REPORT.md), [batch-10/REPORT.md](batch-10/REPORT.md).

## D07 — Required launch scope

Gate references: G31, G32, G33.

**Recorded approval scope and historical implementation context:** The launch brief requires the initial lifecycle, governance, wallet, PQC, RPC, monitoring, recovery and upgrade features. One permanent mainnet is required. These requirements are not optional by default. LR01 authorizes local retirement of the identified development timer and direct legacy staking/emission mutations. Preserve explicit RewardState adapters, shared planning, historical reads and staking ownership compatibility records. Deployment, migration and state deletion are not authorized. The legacy rounding defect remains historical diagnostic evidence; retirement is not an arithmetic repair.

### D07-Q01 — OPEN

What exact enabled-module and interface matrix satisfies the required launch scope?

Required output: Launch feature matrix.

Proposed owner role: Product and protocol leads. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [USER_LAUNCH_REQUIREMENTS.txt](USER_LAUNCH_REQUIREMENTS.txt), [batch-1/REQUIREMENTS_RECONCILIATION.md](batch-1/REQUIREMENTS_RECONCILIATION.md).

## D08 — Genesis allocations and vesting

Gate references: G04, G05, G12, G13, G14, G15, G16, G17, G18, G20.

**Recorded approval scope and historical implementation context:** One billion DGT; shares 30/20/15/15/20. Six decimals give 10^15 base units. Explicit beneficiary vesting inputs required. Locked DGT may stake only when its schedule permits; locks survive bonding and unbonding.

### D08-Q01 — OPEN

Which beneficiaries, recipients, custody approvals, exact vesting schedules and initial delegations apply?

Required output: Signed allocation and vesting input set.

Proposed owner role: Genesis coordinator with custody and economics reviewers. Named assignment and reviewer remain as recorded in the structured register.

Preparation: [D08-Q01 packet](decision-register/parallel-tracks-20260912/track-4/records/D08-Q01.json). Acceptance is still open.

Evidence: [evidence/TOKENOMICS_APPROVED_SOURCE.json](evidence/TOKENOMICS_APPROVED_SOURCE.json), [batch-6/APPROVAL.json](batch-6/APPROVAL.json), [batch-6/integration-followup/APPROVAL.json](batch-6/integration-followup/APPROVAL.json), [decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json](decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json), [decision-register/ordinary-client-compatibility/RECORD_STATUS.md](decision-register/ordinary-client-compatibility/RECORD_STATUS.md).

### D08-Q02 — OPEN

What initial DRT supply, fee bootstrap distribution and restrictions apply?

Required output: DRT genesis policy.

Proposed owner role: Genesis coordinator with custody and economics reviewers. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [evidence/TOKENOMICS_APPROVED_SOURCE.json](evidence/TOKENOMICS_APPROVED_SOURCE.json), [batch-6/APPROVAL.json](batch-6/APPROVAL.json), [batch-6/integration-followup/APPROVAL.json](batch-6/integration-followup/APPROVAL.json).

### D08-Q03 — OPEN

Which verified recipients and custody records implement the approved DRT bootstrap?

Required output: DRT genesis input set.

Proposed owner role: Genesis coordinator with custody and economics reviewers. Named assignment and reviewer remain as recorded in the structured register.

Preparation: [D08-Q03 packet](decision-register/parallel-tracks-20260912/track-4/records/D08-Q03.json). Acceptance is still open.

Evidence: [evidence/TOKENOMICS_APPROVED_SOURCE.json](evidence/TOKENOMICS_APPROVED_SOURCE.json), [batch-6/APPROVAL.json](batch-6/APPROVAL.json), [batch-6/integration-followup/APPROVAL.json](batch-6/integration-followup/APPROVAL.json), [decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json](decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json), [decision-register/ordinary-client-compatibility/RECORD_STATUS.md](decision-register/ordinary-client-compatibility/RECORD_STATUS.md).

## D09 — Validators, evidence, penalties and withdrawals

Gate references: G01, G02, G04, G05, G07, G08, G15, G16, G17, G18, G21, G22, G30, G34.

**Recorded approval scope and historical implementation context:** All seven Batch 8 rules approved: effective bonded power; permissioned initial admission with key proof; funded self-bond and capacity checks; H+2 changes; owner-specific unbonding held through both evidence limits, margins and penalty settlement; authorized key rotation; atomic recovery. Batch 9 penalty values are synthetic.

### D09-Q01 — OPEN

What minimum self-bond, maximum active set and operator-registry amendment authority apply?

Required output: Validator admission configuration.

Proposed owner role: Protocol and staking leads with validator coordinator. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [batch-8/implementation/APPROVAL.json](batch-8/implementation/APPROVAL.json), [batch-8/POLICY_PROPOSAL.json](batch-8/POLICY_PROPOSAL.json), [batch-9/SCOPE.json](batch-9/SCOPE.json), [batch-9/POLICY.md](batch-9/POLICY.md).

### D09-Q02 — OPEN

Which operators, control groups, keys, custody arrangements and signed acceptances form the initial set?

Required output: Initial validator register.

Proposed owner role: Protocol and staking leads with validator coordinator. Named assignment and reviewer remain as recorded in the structured register.

Preparation: [D09-Q02 packet](decision-register/parallel-tracks-20260912/track-4/records/D09-Q02.json). Acceptance is still open.

Evidence: [batch-8/implementation/APPROVAL.json](batch-8/implementation/APPROVAL.json), [batch-8/POLICY_PROPOSAL.json](batch-8/POLICY_PROPOSAL.json), [batch-9/SCOPE.json](batch-9/SCOPE.json), [batch-9/POLICY.md](batch-9/POLICY.md), [decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json](decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json), [decision-register/ordinary-client-compatibility/RECORD_STATUS.md](decision-register/ordinary-client-compatibility/RECORD_STATUS.md).

### D09-Q03 — OPEN

What evidence age limits in blocks and seconds, and what processing margins, apply?

Required output: Evidence retention and unbond timing configuration.

Proposed owner role: Protocol and staking leads with validator coordinator. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [batch-8/implementation/APPROVAL.json](batch-8/implementation/APPROVAL.json), [batch-8/POLICY_PROPOSAL.json](batch-8/POLICY_PROPOSAL.json), [batch-9/SCOPE.json](batch-9/SCOPE.json), [batch-9/POLICY.md](batch-9/POLICY.md).

### D09-Q04 — OPEN

Which faults, penalty rates, repeat-fault treatment, reinstatement rules and penalty-reserve rules apply?

Required output: Production penalty specification.

Proposed owner role: Protocol and staking leads with validator coordinator. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [batch-8/implementation/APPROVAL.json](batch-8/implementation/APPROVAL.json), [batch-8/POLICY_PROPOSAL.json](batch-8/POLICY_PROPOSAL.json), [batch-9/SCOPE.json](batch-9/SCOPE.json), [batch-9/POLICY.md](batch-9/POLICY.md).

### D09-Q05 — OPEN

How do locked-principal liability, completed settlement, withdrawal activation and parameter migration work?

Required output: Withdrawal and locked-liability specification.

Proposed owner role: Protocol and staking leads with validator coordinator. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [batch-8/implementation/APPROVAL.json](batch-8/implementation/APPROVAL.json), [batch-8/POLICY_PROPOSAL.json](batch-8/POLICY_PROPOSAL.json), [batch-9/SCOPE.json](batch-9/SCOPE.json), [batch-9/POLICY.md](batch-9/POLICY.md).

## D10 — Keys, accounts and recovery

Gate references: G03, G04, G08, G10, G11, G12, G13, G20, G23, G24, G25, G29, G30, G31, G33, G34, G35.

**Recorded approval scope and historical implementation context:** Lost-or-compromised-key recovery, the detailed two-of-three rules, local recovery signing and RF01 through RF06 remain approved. AS01 through AS06 select stable address-v1 Bech32m identity, exact ordinary-v2 bytes, current-key and generation checks, one authoritative spending nonce with an equal native mirror, actual debit-owner checks, generation-bound discretionary grants and shared client format. Recovery-only sponsorship stays separate. Production role allowlists, complete client acceptance, account creation or migration beyond explicit local records, and production activation remain open. AS05 selects generation-bound discretionary grants. A generation change invalidates an old Dms or queued discretionary grant until the current owner reauthorizes it. RF06 protection applies to the actor, actual debit owner and fee payer. Mandatory liabilities and committed H+2 schedules remain intact. OF01 through OF05 now select the ordinary fee outcome dependency. Local runtime and durable checks are recorded separately; production and client qualification remain open. The current result records local OF-W04 and OF-W05 runtime and durable integration checks within its reported scope. This closes neither the parent production work nor OF-W06 client, production parameter, custody, genesis, operator or release qualification. The current result records the locally checked OF-W06-CLIENT slice: explicit ordinary-v2 SDK, wallet CLI, committed node client views, genesis identity compatibility and documented local pipe-adapter acceptance. This is not an actual CometBFT engine or live RPC result. Full OF-W06 remains open for hosted wallet UI, live engine/RPC and light-client assurance, production values/roles/records and release qualification.

### D10-Q01 — PARTIALLY_APPROVED

Which identity bytes, address codec, role algorithms, versions and signing envelope apply?

Required output: Account and signing specification.

Proposed owner role: Protocol cryptography lead with wallet and SDK leads. Named assignment and reviewer remain as recorded in the structured register.

Recorded approval: [decision-register/ordinary-signing-implementation/APPROVAL.json](decision-register/ordinary-signing-implementation/APPROVAL.json).

Evidence: [batch-2/IDENTITY_DECISION.json](batch-2/IDENTITY_DECISION.json), [batch-8/implementation/APPROVAL.json](batch-8/implementation/APPROVAL.json), [decision-register/recovery-design/APPROVAL.json](decision-register/recovery-design/APPROVAL.json), [decision-register/recovery-implementation/APPROVAL.json](decision-register/recovery-implementation/APPROVAL.json), [decision-register/recovery-signing/APPROVAL.json](decision-register/recovery-signing/APPROVAL.json), [decision-register/recovery-signing/SPEC.md](decision-register/recovery-signing/SPEC.md), [decision-register/recovery-signing/REPORT.md](decision-register/recovery-signing/REPORT.md), [decision-register/recovery-signing/TEST_RESULTS.json](decision-register/recovery-signing/TEST_RESULTS.json), [decision-register/recovery-signing/SCOPE.json](decision-register/recovery-signing/SCOPE.json), [decision-register/recovery-fee-storage/PROPOSAL.json](decision-register/recovery-fee-storage/PROPOSAL.json), [decision-register/recovery-fee-storage/AUTHORIZATION.json](decision-register/recovery-fee-storage/AUTHORIZATION.json), [decision-register/recovery-fee-storage/CONTRACT.md](decision-register/recovery-fee-storage/CONTRACT.md), [decision-register/recovery-fee-storage/FEE_REVIEW.md](decision-register/recovery-fee-storage/FEE_REVIEW.md), [decision-register/recovery-fee-storage/STORAGE_MAP.md](decision-register/recovery-fee-storage/STORAGE_MAP.md), [decision-register/recovery-fee-storage/WORK_ITEMS.json](decision-register/recovery-fee-storage/WORK_ITEMS.json), [decision-register/recovery-fee-storage/ACCEPTANCE.json](decision-register/recovery-fee-storage/ACCEPTANCE.json), [decision-register/recovery-fee-implementation/APPROVAL.json](decision-register/recovery-fee-implementation/APPROVAL.json), [decision-register/recovery-fee-implementation/SCOPE.json](decision-register/recovery-fee-implementation/SCOPE.json), [decision-register/recovery-fee-implementation/REPORT.md](decision-register/recovery-fee-implementation/REPORT.md), [decision-register/recovery-fee-implementation/TEST_RESULTS.json](decision-register/recovery-fee-implementation/TEST_RESULTS.json), [decision-register/ordinary-signing-implementation/APPROVAL.json](decision-register/ordinary-signing-implementation/APPROVAL.json), [decision-register/recovery-acceptance-followup/ORDINARY_SIGNING_PROPOSAL.md](decision-register/recovery-acceptance-followup/ORDINARY_SIGNING_PROPOSAL.md), [decision-register/recovery-acceptance-followup/ORDINARY_SIGNING_PROPOSAL.json](decision-register/recovery-acceptance-followup/ORDINARY_SIGNING_PROPOSAL.json), [decision-register/recovery-acceptance-followup/LEGACY_ENTRYPOINTS.md](decision-register/recovery-acceptance-followup/LEGACY_ENTRYPOINTS.md), [decision-register/ordinary-signing-implementation/SCOPE.json](decision-register/ordinary-signing-implementation/SCOPE.json), [decision-register/ordinary-signing-implementation/REPORT.md](decision-register/ordinary-signing-implementation/REPORT.md), [decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.json](decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.json), [decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.md](decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.md), [decision-register/ordinary-signing-implementation/TEST_RESULTS.json](decision-register/ordinary-signing-implementation/TEST_RESULTS.json), [decision-register/ordinary-runtime-integration/APPROVAL.json](decision-register/ordinary-runtime-integration/APPROVAL.json), [decision-register/ordinary-runtime-integration/WORK_ITEMS.json](decision-register/ordinary-runtime-integration/WORK_ITEMS.json), [decision-register/ordinary-runtime-integration/README.md](decision-register/ordinary-runtime-integration/README.md), [decision-register/ordinary-runtime-integration/DECISION_STATUS.md](decision-register/ordinary-runtime-integration/DECISION_STATUS.md), [decision-register/ordinary-runtime-integration/REPORT.md](decision-register/ordinary-runtime-integration/REPORT.md), [decision-register/ordinary-runtime-integration/TEST_RESULTS.json](decision-register/ordinary-runtime-integration/TEST_RESULTS.json), [decision-register/ordinary-client-compatibility/APPROVAL.json](decision-register/ordinary-client-compatibility/APPROVAL.json), [decision-register/ordinary-client-compatibility/SCOPE.json](decision-register/ordinary-client-compatibility/SCOPE.json), [decision-register/ordinary-client-compatibility/WORK_ITEMS.json](decision-register/ordinary-client-compatibility/WORK_ITEMS.json), [decision-register/ordinary-client-compatibility/README.md](decision-register/ordinary-client-compatibility/README.md), [decision-register/ordinary-client-compatibility/DECISION_STATUS.md](decision-register/ordinary-client-compatibility/DECISION_STATUS.md), [decision-register/ordinary-client-compatibility/REPORT.md](decision-register/ordinary-client-compatibility/REPORT.md), [decision-register/ordinary-client-compatibility/TEST_RESULTS.json](decision-register/ordinary-client-compatibility/TEST_RESULTS.json).

### D10-Q02 — PARTIALLY_APPROVED

Which production timings and remaining signing, fee, custody and migration inputs complete the approved recovery contract?

Required output: Account recovery specification.

Proposed owner role: Protocol cryptography lead with wallet and SDK leads. Named assignment and reviewer remain as recorded in the structured register.

Recorded approval: [decision-register/ordinary-signing-implementation/APPROVAL.json](decision-register/ordinary-signing-implementation/APPROVAL.json).

Evidence: [batch-2/IDENTITY_DECISION.json](batch-2/IDENTITY_DECISION.json), [batch-8/implementation/APPROVAL.json](batch-8/implementation/APPROVAL.json), [decision-register/recovery-design/APPROVAL.json](decision-register/recovery-design/APPROVAL.json), [decision-register/recovery-implementation/APPROVAL.json](decision-register/recovery-implementation/APPROVAL.json), [decision-register/recovery-signing/APPROVAL.json](decision-register/recovery-signing/APPROVAL.json), [decision-register/recovery-signing/SPEC.md](decision-register/recovery-signing/SPEC.md), [decision-register/recovery-signing/REPORT.md](decision-register/recovery-signing/REPORT.md), [decision-register/recovery-signing/TEST_RESULTS.json](decision-register/recovery-signing/TEST_RESULTS.json), [decision-register/recovery-signing/SCOPE.json](decision-register/recovery-signing/SCOPE.json), [decision-register/recovery-fee-storage/PROPOSAL.json](decision-register/recovery-fee-storage/PROPOSAL.json), [decision-register/recovery-fee-storage/AUTHORIZATION.json](decision-register/recovery-fee-storage/AUTHORIZATION.json), [decision-register/recovery-fee-storage/CONTRACT.md](decision-register/recovery-fee-storage/CONTRACT.md), [decision-register/recovery-fee-storage/FEE_REVIEW.md](decision-register/recovery-fee-storage/FEE_REVIEW.md), [decision-register/recovery-fee-storage/STORAGE_MAP.md](decision-register/recovery-fee-storage/STORAGE_MAP.md), [decision-register/recovery-fee-storage/WORK_ITEMS.json](decision-register/recovery-fee-storage/WORK_ITEMS.json), [decision-register/recovery-fee-storage/ACCEPTANCE.json](decision-register/recovery-fee-storage/ACCEPTANCE.json), [decision-register/recovery-fee-implementation/APPROVAL.json](decision-register/recovery-fee-implementation/APPROVAL.json), [decision-register/recovery-fee-implementation/SCOPE.json](decision-register/recovery-fee-implementation/SCOPE.json), [decision-register/recovery-fee-implementation/REPORT.md](decision-register/recovery-fee-implementation/REPORT.md), [decision-register/recovery-fee-implementation/TEST_RESULTS.json](decision-register/recovery-fee-implementation/TEST_RESULTS.json), [decision-register/ordinary-signing-implementation/APPROVAL.json](decision-register/ordinary-signing-implementation/APPROVAL.json), [decision-register/recovery-acceptance-followup/ORDINARY_SIGNING_PROPOSAL.md](decision-register/recovery-acceptance-followup/ORDINARY_SIGNING_PROPOSAL.md), [decision-register/recovery-acceptance-followup/ORDINARY_SIGNING_PROPOSAL.json](decision-register/recovery-acceptance-followup/ORDINARY_SIGNING_PROPOSAL.json), [decision-register/recovery-acceptance-followup/LEGACY_ENTRYPOINTS.md](decision-register/recovery-acceptance-followup/LEGACY_ENTRYPOINTS.md), [decision-register/ordinary-signing-implementation/SCOPE.json](decision-register/ordinary-signing-implementation/SCOPE.json), [decision-register/ordinary-signing-implementation/REPORT.md](decision-register/ordinary-signing-implementation/REPORT.md), [decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.json](decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.json), [decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.md](decision-register/ordinary-signing-implementation/ORDINARY_FEE_PROPOSAL.md), [decision-register/ordinary-signing-implementation/TEST_RESULTS.json](decision-register/ordinary-signing-implementation/TEST_RESULTS.json), [decision-register/ordinary-runtime-integration/APPROVAL.json](decision-register/ordinary-runtime-integration/APPROVAL.json), [decision-register/ordinary-runtime-integration/WORK_ITEMS.json](decision-register/ordinary-runtime-integration/WORK_ITEMS.json), [decision-register/ordinary-runtime-integration/README.md](decision-register/ordinary-runtime-integration/README.md), [decision-register/ordinary-runtime-integration/DECISION_STATUS.md](decision-register/ordinary-runtime-integration/DECISION_STATUS.md), [decision-register/ordinary-runtime-integration/REPORT.md](decision-register/ordinary-runtime-integration/REPORT.md), [decision-register/ordinary-runtime-integration/TEST_RESULTS.json](decision-register/ordinary-runtime-integration/TEST_RESULTS.json), [decision-register/ordinary-client-compatibility/APPROVAL.json](decision-register/ordinary-client-compatibility/APPROVAL.json), [decision-register/ordinary-client-compatibility/SCOPE.json](decision-register/ordinary-client-compatibility/SCOPE.json), [decision-register/ordinary-client-compatibility/WORK_ITEMS.json](decision-register/ordinary-client-compatibility/WORK_ITEMS.json), [decision-register/ordinary-client-compatibility/README.md](decision-register/ordinary-client-compatibility/README.md), [decision-register/ordinary-client-compatibility/DECISION_STATUS.md](decision-register/ordinary-client-compatibility/DECISION_STATUS.md), [decision-register/ordinary-client-compatibility/REPORT.md](decision-register/ordinary-client-compatibility/REPORT.md), [decision-register/ordinary-client-compatibility/TEST_RESULTS.json](decision-register/ordinary-client-compatibility/TEST_RESULTS.json).

### D10-Q03 — OPEN

Who controls each signing role, and what custody, backup and recovery arrangements will they accept?

Required output: Signing custody register.

Proposed owner role: Protocol cryptography lead with wallet and SDK leads. Named assignment and reviewer remain as recorded in the structured register.

Preparation: [D10-Q03 packet](decision-register/parallel-tracks-20260912/track-4/records/D10-Q03.json). Acceptance is still open.

Evidence: [batch-2/IDENTITY_DECISION.json](batch-2/IDENTITY_DECISION.json), [batch-8/implementation/APPROVAL.json](batch-8/implementation/APPROVAL.json), [decision-register/recovery-design/APPROVAL.json](decision-register/recovery-design/APPROVAL.json), [decision-register/recovery-implementation/APPROVAL.json](decision-register/recovery-implementation/APPROVAL.json), [decision-register/recovery-signing/APPROVAL.json](decision-register/recovery-signing/APPROVAL.json), [decision-register/recovery-signing/SPEC.md](decision-register/recovery-signing/SPEC.md), [decision-register/recovery-signing/REPORT.md](decision-register/recovery-signing/REPORT.md), [decision-register/recovery-signing/TEST_RESULTS.json](decision-register/recovery-signing/TEST_RESULTS.json), [decision-register/recovery-signing/SCOPE.json](decision-register/recovery-signing/SCOPE.json), [decision-register/recovery-fee-storage/PROPOSAL.json](decision-register/recovery-fee-storage/PROPOSAL.json), [decision-register/recovery-fee-storage/AUTHORIZATION.json](decision-register/recovery-fee-storage/AUTHORIZATION.json), [decision-register/recovery-fee-storage/CONTRACT.md](decision-register/recovery-fee-storage/CONTRACT.md), [decision-register/recovery-fee-storage/FEE_REVIEW.md](decision-register/recovery-fee-storage/FEE_REVIEW.md), [decision-register/recovery-fee-storage/STORAGE_MAP.md](decision-register/recovery-fee-storage/STORAGE_MAP.md), [decision-register/recovery-fee-storage/WORK_ITEMS.json](decision-register/recovery-fee-storage/WORK_ITEMS.json), [decision-register/recovery-fee-storage/ACCEPTANCE.json](decision-register/recovery-fee-storage/ACCEPTANCE.json), [decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json](decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json), [decision-register/ordinary-client-compatibility/RECORD_STATUS.md](decision-register/ordinary-client-compatibility/RECORD_STATUS.md).

## D11 — Governance and emergency authority

Gate references: G17, G19, G21, G22, G23, G24, G31.

**Recorded approval scope and historical implementation context:** Only bonded stake gives voting power. Liquid DGT gives zero. Count each owner stake once; do not add validator aggregate stake.

### D11-Q01 — OPEN

What governance snapshot, validator eligibility and delegated vote ownership rules apply?

Required output: Governance electorate specification.

Proposed owner role: Governance lead with protocol and custody reviewers. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [batch-5/eligibility-followup/APPROVAL.json](batch-5/eligibility-followup/APPROVAL.json).

### D11-Q02 — OPEN

What quorum, approval, veto, abstention, deposit, voting-period and timelock rules apply?

Required output: Governance parameter specification.

Proposed owner role: Governance lead with protocol and custody reviewers. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [batch-5/eligibility-followup/APPROVAL.json](batch-5/eligibility-followup/APPROVAL.json).

### D11-Q03 — PARTIALLY_APPROVED

Which actions, parameter bounds, treasury powers, emergency powers and upgrade or recovery authorities apply?

Approved portions: transaction freeze while consensus continues; separate resume; persistent upgrade hold; three signatures from five independent custodians with distinct freeze/resume keys; measured height-based validity windows; continued mandatory transitions; evidence-bound resume; separate candidate-specific upgrade clearance; and a separate full-halt procedure.

Approvals: [initial emergency rules](decision-register/emergency-transaction-freeze/policy/APPROVAL.json), [six additional recommendations](decision-register/emergency-release-staging/policy/APPROVAL.json).

Remaining inputs: custodian names, keys and epochs; numeric timing limits; acceptance of production control formats; upgrade-clearance membership and threshold; halt/restart authority; and the remaining governance authority matrix. Version 2 emergency bindings and the first actual receipt-index upgrade executor are implemented with development-only inputs. Cross-binary and production qualification remain open. Use the [custodian intake packet](decision-register/emergency-upgrade-execution/custody/INTAKE.md) to supply public records. This implementation adds no policy approval.

Required output: Governance authority matrix.

Proposed owner role: Governance lead with protocol and custody reviewers. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [batch-5/eligibility-followup/APPROVAL.json](batch-5/eligibility-followup/APPROVAL.json).

## D12 — Infrastructure and service objectives

Gate references: G01, G07, G08, G09, G24, G25, G26, G27, G28, G29, G30, G31, G32, G34, G35.

**Recorded approval scope and historical implementation context:** Existing servers are on Hetzner. The observed host is not an approved production topology.

### D12-Q01 — OPEN

What resources, regions, account separation, peer topology, access controls and capacity budget apply?

Required output: Production topology and budget.

Proposed owner role: SRE lead with budget owner and validator coordinator. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [PRODUCTION_INFRASTRUCTURE_DRAFT.json](PRODUCTION_INFRASTRUCTURE_DRAFT.json), [batch-1/OPERATIONS_EVIDENCE.json](batch-1/OPERATIONS_EVIDENCE.json).

### D12-Q02 — OPEN

What service targets, on-call coverage, retention, backup and recovery objectives apply?

Required output: Operations acceptance specification.

Proposed owner role: SRE lead with budget owner and validator coordinator. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [PRODUCTION_INFRASTRUCTURE_DRAFT.json](PRODUCTION_INFRASTRUCTURE_DRAFT.json), [batch-1/OPERATIONS_EVIDENCE.json](batch-1/OPERATIONS_EVIDENCE.json).

### D12-Q03 — OPEN

Which exact assets, operators, account owners and funded commitments implement the topology?

Required output: Approved infrastructure inventory.

Proposed owner role: SRE lead with budget owner and validator coordinator. Named assignment and reviewer remain as recorded in the structured register.

Preparation: [D12-Q03 packet](decision-register/parallel-tracks-20260912/track-4/records/D12-Q03.json). Acceptance is still open.

Evidence: [PRODUCTION_INFRASTRUCTURE_DRAFT.json](PRODUCTION_INFRASTRUCTURE_DRAFT.json), [batch-1/OPERATIONS_EVIDENCE.json](batch-1/OPERATIONS_EVIDENCE.json), [decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json](decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json), [decision-register/ordinary-client-compatibility/RECORD_STATUS.md](decision-register/ordinary-client-compatibility/RECORD_STATUS.md).

## D13 — Canonical network identity

Gate references: G04, G05.

**Recorded approval scope and historical implementation context:** One permanent mainnet genesis is required. AS01 selects address network codes and Bech32m prefixes: 1/dytallix, 2/tdytallix, 3/ddytallix. The exact display name, unused production chain ID and genesis timestamp procedure remain open. AS03 bounds the combined profile chain ID to 1 through 128 UTF-8 bytes.

### D13-Q01 — OPEN

What display name, unused chain ID and genesis timestamp procedure apply?

Required output: Network identity specification.

Proposed owner role: Genesis and release leads. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [USER_LAUNCH_REQUIREMENTS.txt](USER_LAUNCH_REQUIREMENTS.txt).

### D13-Q02 — OPEN

What final identity manifest and deterministic genesis digest will the release signers approve?

Required output: Signed genesis commitment.

Proposed owner role: Genesis and release leads. Named assignment and reviewer remain as recorded in the structured register.

Preparation: [D13-Q02 packet](decision-register/parallel-tracks-20260912/track-4/records/D13-Q02.json). Acceptance is still open.

Evidence: [USER_LAUNCH_REQUIREMENTS.txt](USER_LAUNCH_REQUIREMENTS.txt), [decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json](decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json), [decision-register/ordinary-client-compatibility/RECORD_STATUS.md](decision-register/ordinary-client-compatibility/RECORD_STATUS.md).

## D14 — Release and independent review

Gate references: G01, G02, G03, G04, G06, G07, G08, G09, G10, G11, G12, G13, G14, G15, G16, G17, G18, G19, G20, G21, G22, G23, G24, G25, G26, G27, G28, G29, G30, G31, G32, G33, G34, G35.

**Recorded approval scope and historical implementation context:** Reproducible release, deterministic genesis, required features, closure of launch blockers and seven full launch simulations remain required. Local test passes do not satisfy these gates. LR01 authorizes local retirement of the identified development timer and direct legacy staking/emission mutations. Preserve explicit RewardState adapters, shared planning, historical reads and staking ownership compatibility records. Deployment, migration and state deletion are not authorized. The legacy rounding defect remains historical diagnostic evidence; retirement is not an arithmetic repair.

### D14-Q01 — OPEN

Which release targets, build environment, registry, signing authority and review independence criteria apply?

Required output: Release and review specification.

Proposed owner role: Release and QA leads with independent reviewers. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [USER_LAUNCH_REQUIREMENTS.txt](USER_LAUNCH_REQUIREMENTS.txt), [LAUNCH_GATES.json](LAUNCH_GATES.json), [batch-10/REPORT.md](batch-10/REPORT.md).

### D14-Q02 — OPEN

What workload, finality, recovery and acceptance thresholds, calendar and launch authority apply?

Required output: Qualification and launch acceptance plan.

Proposed owner role: Release and QA leads with independent reviewers. Named assignment and reviewer remain as recorded in the structured register.

Evidence: [USER_LAUNCH_REQUIREMENTS.txt](USER_LAUNCH_REQUIREMENTS.txt), [LAUNCH_GATES.json](LAUNCH_GATES.json), [batch-10/REPORT.md](batch-10/REPORT.md).

### D14-Q03 — OPEN

Who accepts each engineering, operations, signing and independent review role?

Required output: Release responsibility register.

Proposed owner role: Release and QA leads with independent reviewers. Named assignment and reviewer remain as recorded in the structured register.

Preparation: [D14-Q03 packet](decision-register/parallel-tracks-20260912/track-4/records/D14-Q03.json). Acceptance is still open.

Evidence: [USER_LAUNCH_REQUIREMENTS.txt](USER_LAUNCH_REQUIREMENTS.txt), [LAUNCH_GATES.json](LAUNCH_GATES.json), [batch-10/REPORT.md](batch-10/REPORT.md), [decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json](decision-register/ordinary-client-compatibility/RECORD_DISCOVERY.json), [decision-register/ordinary-client-compatibility/RECORD_STATUS.md](decision-register/ordinary-client-compatibility/RECORD_STATUS.md).

The [pre-consolidation document](decision-register/gate-consolidation-20260912/evidence/before/DECISIONS_REQUIRED.md) preserves the complete earlier narrative. Its implementation status statements do not override the master list.
