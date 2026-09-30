> **Superseded in part (29 September 2026).** This draft is stale on the consensus engine, algorithms, governance, validator lifecycle, rewards and fees. Where it conflicts with the documents below, they are current. The table rows marked *Updated* were corrected from them; unmarked text is the earlier draft.
>
> - Consensus, evidence and restarts: [E04 requirement triage](../node/docs/mainnet/e04-requirement-triage.md) (CONS-001 to CONS-004), [liveness v1](../node/docs/architecture/liveness-v1.md), [state sync v1](../node/docs/architecture/state-sync-v1.md), [restart v1](../node/docs/architecture/restart-v1.md)
> - Algorithms: [PQC architecture](PQC_ARCHITECTURE.md), [client channel v1](../node/docs/architecture/client-channel-v1.md), [E01 source boundary](../node/docs/mainnet/e01-source-boundary.md)
> - Governance: [governance v1](../node/docs/architecture/governance-v1.md)
> - Validator lifecycle: [validator lifecycle retention](../node/docs/architecture/validator-lifecycle-retention.md)
> - Rewards and fees: [fees v1](../node/docs/architecture/fees-v1.md), [adaptive emission v1](../node/docs/mainnet/adaptive-emission-v1.md)
> - Accounts and state: [account model v2](../node/docs/architecture/account-model-v2.md), [state root v2](../node/docs/architecture/state-root-v2.md)
> - Interfaces: [RPC controls v1](../node/docs/architecture/rpc-controls-v1.md), [clients v1](../node/docs/architecture/clients-v1.md)

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
| Consensus | *Updated.* A CometBFT v0.40.0 fork (PQC-only build) runs the Rust consensus application. Upstream CometBFT state machine with a quorum of more than two thirds; a block is final when committed. ML-DSA-65 validator votes. Evidence is recorded only until D09 (liveness v1). State sync is implemented (state sync v1), but the native supervisor refuses it, so a node rejoins from an archive peer. Timeouts are fixture values. | Resolve D06-Q02: block time, timeouts, fault assumptions and state-sync trust inputs. |
| Block time and timebase | *Updated.* Approved issuance epochs use explicit finalized-block count N (`epoch_blocks`). The epoch observation is derived from committed blocks (adaptive emission v1, proposed for D01-Q02). | Select production N (D03-Q01) and block time (D06-Q02); approve the observation contract (D01-Q02). |
| Block, transaction, and network limits | *Updated.* Mempool rules, RPC limits and adapter limits are pinned in code; unit resource limits are required inputs (triage gaps 6, 10, 13). Capacity values are unset. | Set capacity, resource and rate values (D06-Q02, D12-Q01) from evidence. |
| Gas and fees | *Updated.* Fees v1 (P01, 27 September 2026): paid in uDRT by the actor, no tips; charge `max(measured gas, minimum gas) × gas price`, paid also by failed and out-of-gas transactions; every fee burned, as is the account creation fee. Governance sets fee values within genesis bounds; changing where fees go needs an upgrade. | Set production fee values and bounds (E05 inputs). |
| Account model and address | *Updated.* Account model v2 (P01, 26 September 2026): any valid address can receive; the first transfer creates the account and burns a configurable creation fee; the first outgoing transaction proves the key. Stable account IDs survive key rotation; recovery transactions cover rotation and guardians. | One node/SDK/wallet encoding, network binding, current-key authorization, recovery, rotation, migration. |
| Validator model | *Updated.* Validator registration and key rotation with ML-DSA-65 possession proofs, H+2 activation, `max_active` and `min_self_bond` (governed within genesis bounds); lifecycle records pruned at the evidence horizon; consensus keys never reused (validator lifecycle retention). | Resolve D06/D09. Define admission/removal, weights, key custody, peer topology, replacement, and transitions. |
| Staking and delegation | *Updated.* Bond, unbond and reward claims run as ordinary-v2 transactions. An unbond matures after the evidence limits plus margins (values D09-Q03). Without the penalty profile, principal withdrawal is a paid `VALIDATOR_WITHDRAWAL_DISABLED` failure (liveness v1; D09-Q05). | Define bonding, delegation, commission, exit delay, redelegation if supported, and atomic state transitions. |
| Slashing and jailing | *Updated.* Duplicate-vote and light-client-attack evidence is recorded only, with no stake or validator-set effect, until D09 (liveness v1). Production penalties are refused; there is no jailing. | Define evidence, penalty arithmetic, destinations, jail/unjail, tombstone policy, and key-compromise recovery. |
| Rewards | *Updated.* Issuance split 40/30/30 plus the issuance reserve. The validator share is divided every block by voting power among active validators and credited to each operator's owner; the staking share is paid pro rata to bonded stake; `RewardClaim` pays both (fees v1). Treasury spending is POST MAINNET. | Set controller parameters and first command (D01-Q01) and epoch length (D03-Q01). |
| DRT | Selected corrected adaptive model remains required. Initial supply and calibrated parameters are unset (D08-Q02, D01-Q01). *Updated:* every transaction fee is burned (fees v1). | Approve initial fee funding, issuance limits/rules, rewards, transferability, burn, treasury distribution, and controls. |
| DGT | Approved page specifies 1 billion DGT and initial shares 30/20/15/15/20. Native uses six decimals; testnet allocation shares differ. | Apply the approved supply and bucket shares. Complete recipient/custody mapping, staking/governance roles, transferability, locks, mint policy, and burns if any. |
| Governance | *Updated.* Governance v1 (P01, 27 September 2026) runs from the genesis activation height: linear bonded-stake weight with no delegation or decay, DGT deposit escrow refunded at every outcome, two action classes (parameter change for the fee profile, `min_self_bond` and `max_active` within genesis bounds; validator registry). Upgrades stay root-signed only. | Set quorum, approval, veto, deposit, periods and timelock (D11-Q02, E05). |
| Treasury | Testnet bucket labels exist. Production beneficiary and authority are unset. *Updated:* treasury spending is POST MAINNET (governance v1). | Approve custody, spending rules, release schedules, limits, reporting, and governance interactions. |
| PQC algorithms | *Updated.* ML-DSA-65 for transactions, wallets, validators, votes and proposals; the node's generic verifier represents ML-DSA-65 only (E01 source boundary). No classical public-key cryptography anywhere in the stack (P01, 28 September 2026). | Freeze explicit algorithm identifiers and parameters per role. Validate bytes and cross-implementation vectors. |
| ML-KEM and SLH-DSA | *Updated.* ML-KEM-768 for peer transport and client channel key exchange; SLH-DSA for root authorization (genesis, upgrades, emergency controls, restarts), under separate root keys (PQC architecture, client channel v1). | Define required roles/parameters or explicitly mark unused. Do not claim standardized use from legacy labels. |
| Cryptographic transitions | Complete authorization and migration mechanism is unqualified. | Define versioning, domain separation, replay, chain binding, key encryption, rotation, and recovery with vectors. |
| Upgrade authority and process | *Updated.* Root-signed upgrade admission and activation at a height (production activation disabled), a release handover, and a root-signed restart authorization for a fixed release after a halt (upgrade execution, restart v1). Authority and threshold are open (D11-Q03, D14-Q02). | Define activation, approved authority, schema migration, version compatibility, late nodes, rollback limits, and recovery. |
| Emergency procedures | Production authority and signer recovery are unset. *Updated:* a root-signed freeze of user transactions while consensus continues is implemented with production activation disabled, and incident runbooks exist (emergency transaction freeze, incident response v1). | Define bounded incident powers, coordination, signer isolation, safe restart, and recovery limits. |
| RPC and REST | *Updated.* An 18-method CometBFT JSON-RPC allowlist on an owner-only client socket, operator diagnostics on a separate socket; no WebSocket and no TLS; remote clients use the post-quantum client channel (RPC controls v1, client channel v1). Public ingress values are open (D12-Q01). | Freeze methods, encoding, errors, finality semantics, versioning, limits, availability, and client compatibility. |
| gRPC | *Updated.* The production build has no gRPC (E01 source boundary). | Decide whether initial developer requirements need it. State absent/deferred status explicitly if approved. |
| CLI, wallet, and developer interfaces | *Updated.* The SDK and CLI use the consensus chain: ordinary v2 and v3, recovery, state-proof-checked balances, a pinned chain and the client channel; the testnet client is removed (clients v1, triage gaps 8 and 19). Acceptance belongs to T07. | Publish immutable client release and demonstrate complete supported workflows against the candidate. |
| Genesis | Multiple example/constructor paths disagree. Approved allocation shares exist on the user-designated page. Executable allocation/validator records remain incomplete. | Complete GENESIS_SPEC, ceremony, validation, manifest, and independent digest agreement. |
| Parameter updates | *Updated.* Governed: the ordinary fee profile, `min_self_bond` and `max_active`, each within genesis bounds. Everything else changes only by root-signed upgrade (governance v1). | For each parameter define authority, admissible range, activation, migration, rollback limit, and invariant tests. |

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
