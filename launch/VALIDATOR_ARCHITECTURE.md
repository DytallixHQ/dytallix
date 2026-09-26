> Current PQC profile: use ML-DSA-65 for validator and peer authentication roles. Use ML-KEM-768 for P2P key establishment. Use separate SLH-DSA root authorization. Active upstream P2P remains unqualified. Mainnet remains NO GO. See [PQC architecture](PQC_ARCHITECTURE.md). This notice supersedes conflicting algorithm selections below.

> Decision status: use [the current decision register](DECISIONS_REQUIRED.md). This technical draft contains historical proposals and implementation statements. The current register supersedes conflicting status statements. Original text is preserved in [the prior records](decision-register/evidence/prior-launch-records/).

# Validator architecture

Current local extension: [Batch 10](batch-10/REPORT.md) filters durable transaction receipts during admission and proposal selection. It preserves execution, custody and production policy. Earlier admission-gap statements are historical for this narrow receipt-filtering behavior.

Current local extension: [Batch 9](batch-9/REPORT.md) adds historical principal tracking, duplicate-vote metadata accounting, H+2 penalty settlement and local owner withdrawals. Production evidence coverage and penalty policy remain unqualified.

Current validator policy: B8-01 through B8-07 are approved. See the [Batch 8 implementation](batch-8/implementation/REPORT.md) for local behavior and tests. Production qualification and inputs remain open. The older design sections below are historical or draft where they conflict with the approved rules.

Status: **DRAFT — NOT FROZEN — NOT APPROVED FOR LAUNCH**.
Prepared: 9 September 2026.
Source baseline: dytallix-node `5e86d1046ae2423b98f125ee8fd09f7d7d837fc7`.
Actual registered operators: **0**. Actual approved validators: **0**.
These counts describe this register. They do not assert that no operators exist elsewhere.

This document creates a reviewable architecture proposal. It does not select consensus, approve economic parameters, authorize production signing, or attest infrastructure.
See [source assessment](/Users/rickglenn/Developer/Dytallix-mainnet-launch/assessment/consensus-genesis.md) for source references and blocker closure tests.

## Required protocol decision

Production consensus approval remains open. [Batch 7](batch-7/ARCHITECTURE.md) implements the local CometBFT v0.40.0 candidate. Four synthetic ML-DSA-65 validator keys have fixed power 10 each. The local test verifies real commit signatures, application agreement, quorum loss and restart recovery. All four processes run on one host under one operator; this does not establish operator or infrastructure independence.

The Rust application prepares finalized blocks and commits monetary state atomically. Development block writers reject its database. This fixture does not implement validator changes or qualify production signing. The earlier timer path remains separate.

Before implementation selection, record proposal rules, vote rules, quorum, locking, finality, persistent state, timing assumptions, proposer selection, synchronization, and validator-set transitions. State the permitted fault model. Review compatibility with post-quantum signatures. Account for signature size and verification cost in block, vote, network, and resource limits.

## Conditional qualification counts

| Proposal | Independent operators | Equal-power quorum | Assumed Byzantine limit | Status |
|---|---:|---:|---:|---|
| Minimum qualification configuration | 4 | 3 | 1 | Proposed only |
| Target qualification configuration | 7 | 5 | 2 | Proposed only |

These counts assume a conventional Byzantine fault tolerant protocol with `n=3f+1`, equal voting power, and quorum greater than two-thirds. A Byzantine operator can violate protocol rules. The selected protocol must prove safety and state liveness conditions. Counts alone prove neither.

For total voting power `S`, finality quorum must be strictly greater than `2S/3` under this proposed fault model. Two such quorums intersect in power greater than `S/3`. Less than one-third Byzantine power and correct locking/signing rules are also required. Exactly one-third power can prevent quorum by withholding votes. A count of seven nodes does not imply two-fault tolerance if voting power is unequal.

Equal weights above support qualification planning only. Mainnet weight assignment, stake conversion, caps, minimum bond, penalties, rewards, and delegation rules remain unanswered. Do not publish equal weights as approved tokenomics.

## Independence and common failures

Register each operator's ultimate control group separately from its public name. Count nodes under shared control as one failure source. Document shared key custodians and administrators. Record cloud account, provider, region, network, data center, power, and backup dependencies. Use non-sensitive identifiers in this register.

Calculate the voting power lost or controlled by each shared group. Review combinations permitted by the fault model. Keep each untrusted control group below one-third power for the proposed BFT model. Do not infer independence from separate virtual machines, regions, corporate names, or signer addresses.

No cloud provider, region, host, operator, or custodian is selected by this draft. Inventory evidence and responsible reviewers must establish the actual configuration.

## Signing and custody requirements

1. Separate consensus signing keys from account, governance, bridge, and deployment keys.
2. Define the approved key algorithm, public-key encoding, address derivation, and rotation procedure.
3. Require proof that each operator controls its registered public key.
4. Store persistent anti-double-signing state with the signer. Bind messages to chain ID, protocol version, height, round, and message type as specified by the selected protocol.
5. Prevent concurrent use of the same consensus identity. Define fencing before failover.
6. Preserve signing history during backup, replacement, and restore. Reject unsafe state rollback.
7. Rehearse loss, compromise, rotation, revocation, and recovery without exposing private keys.

Record key fingerprints and evidence references only after validation. Keep private keys, credentials, recovery material, and sensitive custody details outside this document and JSON register. Secret-store references must be opaque identifiers.

## Lifecycle decisions required

| Transition | Required decision | Current status |
|---|---|---|
| Registration | Authentication, public-key proof, operator acceptance, admission limits, self-bond, control-group review | Unanswered |
| Join | Queue rules, activation height/epoch, eligibility, set-hash update, stake snapshot, proposer eligibility | Unanswered |
| Delegation | Validator attribution, effective height, voting-power conversion, rounding, reward checkpoint | Unanswered |
| Weight change | Snapshot height, activation delay, cap, minimum power, overflow and rounding rules | Unanswered |
| Key change | Old/new authorization, activation boundary, revoked-key rejection, signing-state continuity | Unanswered |
| Exit | Request authorization, cessation of voting, unbonding delay, pending evidence window, final withdrawal | Unanswered |
| Penalty/removal | Evidence format, verification, fault classification, penalty arithmetic, appeal/governance boundary | Unanswered |
| Emergency action | Authorized role, threshold, permitted action, expiry, audit record, safety constraints | Unanswered |

Do not equate reward-accounting records with a consensus validator set. The frozen selected staking path tracks aggregate delegator stake and disables public stake writes. Mainnet requires the full lifecycle if the selected protocol is stake based.

## Approval and qualification gates

Protocol, security, custody, staking/economics, release, and operator reviewers remain unassigned. Separate preparation, evidence verification, and approval. Registration alone does not activate a validator.

Before freezing the architecture, approve the protocol and threat assumptions. Resolve lifecycle and economics fields. Populate actual operators and fault domains. Freeze the approved validator public keys and power in genesis. Produce deterministic set-update vectors. Complete multi-operator finality, restart, replacement, network interruption, key-rotation, and upgrade qualification. Save exact source and binary digests, genesis digest, state roots, logs, and reviewer decisions.

No launch approval is created by generating this document or its companion register.
