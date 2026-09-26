> Decision status: use [the current decision register](DECISIONS_REQUIRED.md). This technical draft contains historical proposals and implementation statements. The current register supersedes conflicting status statements. Original text is preserved in [the prior records](decision-register/evidence/prior-launch-records/).

# Dytallix Mainnet v1 specification

Current local extension: [Batch 10](batch-10/REPORT.md) filters durable transaction receipts during admission and proposal selection. It preserves execution, custody and production policy. Earlier admission-gap statements are historical for this narrow receipt-filtering behavior.

Current local extension: [Batch 9](batch-9/REPORT.md) adds historical principal tracking, duplicate-vote metadata accounting, H+2 penalty settlement and local owner withdrawals. Production evidence coverage and penalty policy remain unqualified.

Current validator policy: B8-01 through B8-07 are approved. See the [Batch 8 implementation](batch-8/implementation/REPORT.md) for local behavior and tests. Production qualification and inputs remain open. The older design sections below are historical or draft where they conflict with the approved rules.

See [the Batch 2 decision package](batch-2/DECISION_REVIEW.md) for concrete proposals and remaining approval fields. All unapproved rules remain drafts.

Status: DRAFT — NOT FROZEN — NO GO.

This document defines the required specification boundary. It does not replace unresolved values with development defaults. One canonical genesis must create the permanent network. No later economic genesis is permitted.

## Required protocol fields

| Field | Current evidence or selected direction | Freeze condition |
|---|---|---|
| Chain ID and network name | Development files use dyt-local-1. Production ID is unset. Proposed display name: Dytallix Mainnet v1. | Approve distinct permanent identity and network separation. |
| Consensus | Selected Rust fast node uses a development timer. No selected Cosmos SDK or CometBFT/Tendermint runtime. | Select engine/version, finality rules, authenticated PQC votes, persistence, synchronization, and fault assumptions. |
| Block time and timebase | Approved issuance epochs use explicit finalized-block count N. The development timer is not finality. | Select production N and qualify finality, observation and timestamp rules under D03. |
| Block, transaction, and network limits | Development limits exist. No production capacity qualification. | Freeze byte, gas, signature, queue, connection, propagation, timeout, and rate limits from evidence. |
| Gas and fees | Selected source charges DRT using a scalar calculation. Paper proposes compute and bandwidth charges. | Resolve D04/D05, exact integer math, maximum liability, failures, refunds, burn, and recipients. |
| Account model and address | Balance/nonce model exists. Versioned identity changes are outside the pinned committed assessment. | One node/SDK/wallet encoding, network binding, current-key authorization, recovery, rotation, migration. |
| Validator model | No integrated production validator lifecycle. | Resolve D06/D09. Define admission/removal, weights, key custody, peer topology, replacement, and transitions. |
| Staking and delegation | Some genesis custody exists. Required signed lifecycle is disabled or absent in selected runtime. | Define bonding, delegation, commission, exit delay, redelegation if supported, and atomic state transitions. |
| Slashing and jailing | Separate modules do not establish selected-node enforcement. | Define evidence, penalty arithmetic, destinations, jail/unjail, tombstone policy, and key-compromise recovery. |
| Rewards | Development issuance enters pools. Claims reject. | Apply approved 40/30/30 rewards. Complete D01 integration, payable conserved rewards, rounding, and residual ownership. |
| DRT | Selected corrected adaptive model remains required. Initial supply and calibrated parameters are unset. | Approve initial fee funding, issuance limits/rules, rewards, transferability, burn, treasury distribution, and controls. |
| DGT | Approved page specifies 1 billion DGT and initial shares 30/20/15/15/20. Native uses six decimals; testnet allocation shares differ. | Apply the approved supply and bucket shares. Complete recipient/custody mapping, staking/governance roles, transferability, locks, mint policy, and burns if any. |
| Governance | Full initial governance is required. Selected startup rejects activation. | Resolve D11, electorate, snapshots, custody, quorum, actions, bounds, timelocks, and durable execution. |
| Treasury | Testnet bucket labels exist. Production beneficiary and authority are unset. | Approve custody, spending rules, release schedules, limits, reporting, and governance interactions. |
| PQC algorithms | Selected node defaults to FIPS-204 ML-DSA-87 under a legacy label. SDK uses ML-DSA-65. | Freeze explicit algorithm identifiers and parameters per role. Validate bytes and cross-implementation vectors. |
| ML-KEM and SLH-DSA | Library support does not establish required active roles. Some labels identify legacy implementations. | Define required roles/parameters or explicitly mark unused. Do not claim standardized use from legacy labels. |
| Cryptographic transitions | Complete authorization and migration mechanism is unqualified. | Define versioning, domain separation, replay, chain binding, key encryption, rotation, and recovery with vectors. |
| Upgrade authority and process | No qualified permanent-history upgrade path. | Define activation, approved authority, schema migration, version compatibility, late nodes, rollback limits, and recovery. |
| Emergency procedures | Production authority and signer recovery are unset. | Define bounded incident powers, coordination, signer isolation, safe restart, and recovery limits. |
| RPC and REST | HTTP and WebSocket source interfaces exist. Mainnet service contract is unqualified. | Freeze methods, encoding, errors, finality semantics, versioning, limits, availability, and client compatibility. |
| gRPC | No service found in the selected snapshot. | Decide whether initial developer requirements need it. State absent/deferred status explicitly if approved. |
| CLI, wallet, and developer interfaces | SDK has network, fee, storage, and receipt integration gaps. | Publish immutable client release and demonstrate complete supported workflows against the candidate. |
| Genesis | Multiple example/constructor paths disagree. Approved allocation shares exist on the user-designated page. Executable allocation/validator records remain incomplete. | Complete GENESIS_SPEC, ceremony, validation, manifest, and independent digest agreement. |
| Parameter updates | Current defaults do not establish governed limits. | For each parameter define authority, admissible range, activation, migration, rollback limit, and invariant tests. |

## Permanent and upgradeable state

Treat the canonical genesis bytes and digest, genesis allocation history, origin account identities, and initial network identity as permanent records. Do not overwrite this history during an upgrade.

Make only explicitly specified parameters upgradeable. Each update must pass authorization, bounds, timing, deterministic execution, and invariant checks. A general configuration file is not a governed protocol parameter interface. Changes to signature formats, account identity, storage schema, validator rules, supply rules, and fee rules require explicit version transitions.

Maintain these invariants across genesis, transactions, empty blocks, rewards, penalties, governance, upgrades, and recovery:

1. Token supply equals initial supply plus recognized minting minus recognized burning.
2. Supply equals the sum of disjoint custody accounts, including locked balances, stake, fee custody, reward pools, treasury, and owned rounding residuals.
3. Genesis allocation totals exactly equal approved initial supply. An allocation cannot enter two custody categories.
4. A block transition either commits all related state or commits none of it.
5. Validators agree on finalized history and authoritative state commitments.
6. A signature must satisfy algorithm, encoding, network, chain, replay, and account-authorization rules.
7. A state or signer restore must not introduce a second active signer or reverse finalized history.

## Freeze and change control

Freeze only after D01–D14 are resolved and two independent engineers approve one interpretation. Attach exact source references and test evidence. A source implementation is evidence of behavior; it is not approval of the rule.

After candidate freeze, permit only launch-blocking fixes. Each fix requires a new candidate, documented diff, affected regression tests, and security review. Genesis-relevant changes require new deterministic ceremony evidence before real launch.

No field in this draft grants authority to mint tokens, select beneficiaries, operate production keys, deploy infrastructure, or announce mainnet readiness.
