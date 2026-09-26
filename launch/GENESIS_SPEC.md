> Decision status: use [the current decision register](DECISIONS_REQUIRED.md). This technical draft contains historical proposals and implementation statements. The current register supersedes conflicting status statements. Original text is preserved in [the prior records](decision-register/evidence/prior-launch-records/).

# Genesis specification

Status: **DRAFT. NOT FROZEN. NOT IMPORTABLE.**

This document defines the required input contract and remaining schema decisions. It does not define final engine genesis bytes. The engine decision, account contract, and monetary rules must pass review before a generator can produce those bytes.

## Authority

The user approved one billion DGT and bucket shares of 30/20/15/15/20. The user approved DRT reward shares of 40/30/30. These proportions do not identify beneficiaries, initial DRT supply, custody, vesting, or validator voting power.

The [Batch 6 approval](batch-6/APPROVAL.json) adopts six decimal places for both tokens. The approved fixed DGT total is `1,000,000,000,000,000 udgt`. The five approved bucket amounts are `300,000,000,000,000`, `200,000,000,000,000`, `150,000,000,000,000`, `150,000,000,000,000` and `200,000,000,000,000 udgt`. This unit approval does not approve full issuance at genesis, beneficiary allocations, mint or burn rules.

Production beneficiary vesting schedules are required explicit inputs under the same approval. Do not copy historical schedules into production genesis. Missing schedules remain missing. This requirement does not approve schedule values, release functions or custody.

Batch 6 also approves interval recipient rewards from a supplied eligible-stake snapshot using checked wide arithmetic and `floor(budget * weight / total_weight)`. A sole eligible recipient receives the entire budget. Record unallocated integer remainders in a separate reward-pool reserve with no automatic recipient or sweep authority. The [later integration approval](batch-6/integration-followup/APPROVAL.json) adopts finalized-block intervals from parent-state eligible funded bonds, owner entitlements and separate inactive-period reserves. Fresh genesis must bind version, units and eligibility parameters. Reject unconverted legacy reward state. Actual migration plans, production finality and activation remain unapproved or unqualified. Epoch-to-block issuance rules are approved separately below. The current development fixture adapter has local test evidence. It does not create canonical mainnet genesis or establish production consensus finality.

**APPROVED — issuance timing inputs.** The [issuance timing approval](batch-6/issuance-timing/APPROVAL.json) requires explicit positive epoch length `N` in finalized blocks and an explicit epoch-zero command `E0`. Bind these inputs and their rule version into genesis configuration. `E0` is the first epoch's issuance command; it does not specify initial liquid DRT supply. Production values remain unselected.

Split each epoch command into validator 40%, staker 30% and treasury 30% budgets by integer floor. Put the remainder in the separate `issuance_reserve` reserve. Schedule each of all four budgets evenly over `N` blocks, with extra base DRT units in the earliest blocks. Keep this reserve distinct from staking-recipient rounding and inactive staking reserves. Assign no automatic recipient or sweep authority.

Later commands use the completed previous epoch's observation bound to the expected parent. The development adapter rejects a boundary with missing or invalid input and creates no substitute observation or catch-up issuance. Controller journal, issuance, rewards, supply and block settlement share one atomic storage batch. Production finality, observation authentication and aggregation, pause-and-resume rules, numeric parameters and beneficiary records remain unqualified or missing.

The final genesis must initialize all required economics and governance at one permanent network start. Do not allocate extra initial DGT to validators on top of the approved one-billion-token total. Initial stake must reference ownership and move or encumber existing allocation units under the approved custody model.

## Required records

| Record | Required contents | Current state |
|---|---|---|
| Network identity | Approved chain ID, network domain, protocol versions, genesis time, engine parameters and canonical encoding | Missing approval |
| Candidate identity | Exact source revisions, production binary and image digests, schema version, parameter bundle and reviewed build provenance | Missing qualified candidate |
| DGT allocation ledger | Every beneficiary, account, bucket, integer amount, custody proof, vesting rule, approval and initial stake reference | Bucket policy exists; beneficiary rows missing |
| DRT bootstrap | Exact initial amount, per-account distribution, source authority and fee-funding rationale | Missing decision |
| Account authorization | Stable account ID, current authorized signing policy, sequence, origin evidence and explicit rotation/recovery policy | Proposed; not implemented |
| Validators | Persistent validator ID, operator/control group, consensus public key, voting power, stake backing, custody and acceptance records | No approved set |
| Economic state | Explicit `N` and `E0`, versioned controller inputs, separate issuance-split/staking-rounding/inactive reserves, disjoint custody, reward liabilities, treasury authority and fee/burn rules | Issuance timing rules approved; production values, remaining terms and qualification incomplete |
| Governance | Electorate, snapshots, proposal/deposit rules, quorum, executable authority, timelocks and upgrade controls | Missing approved contract |
| Recovery boundary | Finalized-state commitment, signer safety, backup trust, upgrade authority and recovery rules | Unqualified |

No secret keys, seed phrases, passwords or recovery shares belong in the public input file. Use opaque custody references. A reference is a pointer to evidence, not proof that the evidence exists or is valid.

## Deterministic generation requirements

1. Apply the approved six-decimal units. Freeze the engine, application schema, address policy and all initial parameter values.
2. Validate signed beneficiary and operator input records against that contract.
3. Reject duplicate record IDs, incompatible addresses, unsupported keys, absent required fields, fractional base units and unexplained balances.
4. Reconcile every DGT bucket with its approved proportion. Reconcile the total with exactly one billion DGT at the approved unit scale.
5. Reconcile DRT bootstrap distribution, stake backing, custody categories, reward liabilities and treasury balances. Reject double counting.
6. Canonically order and encode all records. Define exact numeric encodings and time units. Exclude machine-dependent values and random runtime generation.
7. Generate identical genesis bytes on two independent clean environments from the same reviewed input manifest.
8. Record input and output digests. Verify every operator's installed genesis digest before launch.

The final engine/application schema must specify ordering, serialization and state-root calculation. Those choices remain open. This document does not substitute JSON key sorting for an agreed consensus encoding.

## Input completeness tool

The [Batch 6 input worksheet](batch-6/GENESIS_INPUTS.json) records the approved six-decimal scales. Its [checker](batch-6/tools/check_genesis_inputs.py) requires an explicit vesting object for each beneficiary, including explicit unlocked status. It checks schedule fields and time bounds but does not authenticate schedule approval or enforce locks. The [current result](batch-6/GENESIS_INPUT_CHECK.json) remains incomplete. Batch 2 records below retain their historical scope.

[The historical Batch 2 input worksheet](batch-2/GENESIS_INPUTS.json) retains the approved proportions and explicit missing records. It predates the Batch 6 unit approval. [GENESIS_ALLOCATION_DRAFT.json](GENESIS_ALLOCATION_DRAFT.json) records the current approved scale and bucket conversions. [The current check](batch-2/GENESIS_INPUT_CHECK.json) reports unresolved fields.

Run the offline tool with the worksheet path:

```text
python3 batch-2/tools/check_genesis_inputs.py batch-2/GENESIS_INPUTS.json
```

Exit 1 means the worksheet has missing or inconsistent fields. Exit 0 means only that the checked fields are complete. Even then, the output states `INPUT_FIELDS_COMPLETE_UNVERIFIED`, `mainnet_ready: false`, and `importable_genesis: false`.

The tool checks approved proportions, integer encodings, bucket sums, duplicate allocation/validator identifiers and required reference presence. It does not authenticate approvals, open custody evidence, validate account/key encodings, prove stake backing, establish operator independence, enforce vesting, or generate engine genesis. Those functions require the approved protocol and independent verification.

## Closure

Owner roles: protocol lead, genesis engineer, economics reviewer, release engineer and validator operators. Named owners remain unassigned.

Freeze this specification only after the consensus, identity and monetary decisions agree. The final generator, canonical vectors, full supply reconciliation, independent reproducibility report and operator acknowledgements must pass. Existing core genesis test failures remain open until their implementation and fixtures conform to that same approved contract.
