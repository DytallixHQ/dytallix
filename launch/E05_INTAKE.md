# E05 intake packet

Engineering task E05 (genesis from approved inputs), step b. This packet lists
every production value and record that genesis needs, where each goes and
what the code enforces, with proposals for P01 review. A value stays open
until P01 approves it; approved values are marked. Decision IDs refer to
[MAINNET_DECISION_REGISTER.json](MAINNET_DECISION_REGISTER.json).

## How it works

- **Values** are in [E05_VALUES.json](E05_VALUES.json): 208 configuration and
  genesis values, each with its path, unit, the bounds the code enforces, the
  fixture value used in tests (never a recommendation), its decision and its
  couplings. Each has a tier:

  | Tier | Count | Meaning |
  | --- | --- | --- |
  | decide | 65 | Economic, governance and security choices P01 makes |
  | operate | 100 | Operational settings with an engineering default; P01 confirms |
  | measure | 18 | Set from measurements on dedicated staging hosts |
  | derived | 25 | Fixed by an approved rule or another value |

  `proposed` is a proposal for review, never an approved value. Approved
  values are recorded under [approvals/](approvals/) and marked in the file.
- **Records** use the binding review's `PRODUCTION_INPUTS` format
  (`node/tools/mainnet-preparation/`) and, for custodians, the custodian
  intake packet held outside this repository. Only public references and
  hashes enter this repository: no private keys, seeds, backup locations,
  host addresses or personal details.
- The node refuses unusable combinations at load (E05-a,
  [genesis checks](../node/docs/mainnet/e05-genesis-checks.md)).

## Order of work

1. **Chain identity** (D13-Q01): approved 3 October 2026. Chain ID
   `dytallix-mainnet-1`, display name Dytallix, and the genesis time set at
   the final freeze ([identity](genesis/IDENTITY.json)).
2. **People and hosts in parallel:** operators and validators (D09-Q02),
   dedicated hosts and topology (D12-Q01, D12-Q03), and custodians (D10-Q03):
   the root genesis signers, five emergency custodians with freeze and
   resume keys, and five upgrade custodians.
3. **Allocations:** beneficiaries and vesting in the five approved buckets
   (D08-Q01), the treasury recipient (D02-Q02), the DRT bootstrap rows
   (D08-Q03).
4. **Values:** the decide tier in batches, then the operate tier.
5. **Measurements** on the dedicated hosts: emergency, upgrade and handover
   validity windows and anchor ages, capacity values, fault assumptions (E03,
   T02, T05).
6. **Genesis build** from the frozen inputs, the binding review, then the
   genesis digest (D13-Q02) and the release signers' acceptance.
7. **Host files** from the accepted pin plan (D12-Q01), the per-host values
   and the engine genesis: `dytallix-host-config` writes each host's engine
   files and binding ([host configuration](../node/docs/mainnet/host-configuration.md)).

## Values to decide

The block interval is approved at about 5 seconds (`timeout_commit` 4 s); every
block count follows from it. Approved values are marked and recorded in the
[first](approvals/P01_E05_VALUES_1_2026-09-30.json),
[second](approvals/P01_E05_VALUES_2_2026-09-30.json),
[third](approvals/P01_E05_VALUES_3_2026-09-30.json) and
[fourth](approvals/P01_E05_VALUES_4_2026-10-02.json) sets, the
[notice scope](approvals/P01_E05_UPGRADE_NOTICE_2026-10-01.json) and the
[fee range placement](approvals/P01_E05_FEE_RANGE_2026-10-02.json).

| Area | Value | Proposal | Basis or input needed |
| --- | --- | --- | --- |
| Block interval | `timeout_commit` | **Approved:** 4 s | About 5 s blocks; each commit carries ML-DSA-65 votes (3,309 bytes each), so a longer interval keeps history and signature work modest |
| Evidence and unbonding | `evidence_max_age_seconds`, `_blocks` | **Approved:** 14 days; 241,920 | Unbonded stake matures after about 14 days plus the margin |
| | `processing_margin_seconds`, `_blocks` | **Approved:** 1 hour; 720 | |
| Penalty | `penalty_numerator` / `penalty_denominator` | **Approved:** 1 / 20 (5%) | A common first double-sign rate on CometBFT chains |
| Governance | `quorum_bps`, `approval_bps`, `veto_bps` | **Approved:** 3,340; 5,000; 3,340 | Common CometBFT-chain thresholds |
| | `voting_period_blocks`, `deposit_period_blocks`, `timelock_blocks` | **Approved:** 7 days; 7 days; 2 days | |
| | `minimum_deposit_udgt` | **Approved:** 10,000 DGT | 0.001% of supply, refunded at every outcome |
| Validators | `max_active` | **Approved:** 16 | Room above a launch set of four to seven |
| | `bounds_max_active` | **Approved:** 4 to 32 | Never fewer than four validators |
| | `min_self_bond`, `bounds_min_self_bond` | **Approved:** 100,000 DGT; 10,000 to 1,000,000 DGT | 0.01% of supply and 10 times the governance deposit; the 5% penalty costs at least 5,000 DGT |
| Issuance | `epoch_blocks` | **Approved:** 17,280 (1 day) | |
| | `base_udrt`, `max_udrt`, `min_udrt`, `target_ppm`, `initial_epoch_budget_udrt` | **Approved:** 1,000 DRT a block as base and ceiling (about 6.31 billion DRT a year); 500 floor; 50% target; first command equal to the base | Launch issuance equals the published base and never exceeds it; it falls toward half as blocks fill past the target |
| | `window_samples`, soft and hard gains | **Approved:** 1; proportional 17,280,000,000,000, integral and derivative 0 | One-day linear response: 1,000 DRT a block at 50% utilization down to 500 at full blocks |
| | integral limits, `shock_threshold_ppm`, `volatility_threshold_ppm` | **Approved:** 0, 0; 1,000,000; 1,000,000 | Forced by the calibration: no effect with these gains |
| Capacity | block, transaction and signature bounds | **Approved:** 1 MiB blocks, 1,000,000 transaction bytes, 200 signature checks, 1,000 transactions a block | About 180 basic Sends a 5 s block (about 36 a second); T05 tests these values |
| Limits | transaction format, state, retention, governance and migration bounds | **Approved** (fourth set) | 20-minute transaction expiry; at most 10,000 concurrent staking owners |
| Fees | basic transfer | **Approved target:** 1 DRT, governed between 0.1 and 10 DRT | Full blocks of transfers burn about 10% of base issuance |
| | gas price, minimum gas, per-resource costs, bounds | **Approved:** 10 × 100,000 = 1 DRT floor; overhead 10,000, receipt 1,000, wire 2, read 0, write 1, signature and proof 20,000, actions 5,000; price bounds 1 to 100, cost bounds 0 to 100,000 | A basic Send uses about 47,600 gas and pays the floor; free reads keep it flat as shared state grows. The node refuses governed fee changes that put a reference basic Send outside the genesis bound `reference_send_fee_udrt`, approved at 0.1 to 10 DRT (A6) |
| | `account_creation_fee_udrt`, bounds | **Approved:** 10 DRT; 1 to 100 DRT | Accounts are permanent state |
| | governance and recovery costs | **Approved:** the same scale (governance actions 5,000; recovery price 10, minimum 100,000) | Each proposal, deposit, vote and sponsored recovery action costs 1 DRT |
| DRT bootstrap | `drt_bootstrap_total_udrt` | **Approved:** 1,000,000 DRT | Operator startup (7 × 1,000 DRT) and about 90,000 new accounts at 11 DRT; validators earn from block 1. Rows are D08-Q03 |
| Recovery template | `template_recovery_delay`, `_finalization_window`, `_policy_delay`, `_policy_window` | **Approved:** 7 days each | Time for an owner to see and cancel a recovery |
| | `template_submission_lifetime` | **Approved:** 1 day (17,280) | Guardians have a day to collect signatures; sponsor receipts clear within a day |
| Upgrades | `upgrade_notice_blocks`, `handover_notice_blocks` | **Approved:** 7 days each (120,960 blocks) | Time to install a release; urgent problems use the freeze, and a halted chain uses restart, which has no notice |
| Clients | `client_compatibility_window` | — | Input needed (E06): how long a client release must stay compatible |

## Operational settings (100)

Consensus timeouts, mempool and peer settings, state sync, snapshots, the
HTTP adapter and client channel, and the supervisor's timings.

Approved in the [fourth set](approvals/P01_E05_VALUES_4_2026-10-02.json)
(P01, 2 October 2026):
- **Consensus:** the consensus deltas (500 ms), and empty blocks every 5 s.
- **Validator safety:** a 10-block double-sign startup check on validators.
- **Network:** peers up to the 64-pin bound, a 5 s PQC handshake, and the upstream mempool and state-sync settings, with a 168 h trust period.
- **Supervisor:** process limits, a 1 s monitor, and a six-hour catch-up budget.
- **History and helpers:** the retained block window, the adapter at its compiled ceilings, and the emergency verifier bounds.

Earlier sets cover `timeout_propose` 3 s, `timeout_prevote` and `timeout_precommit` 1 s, and a daily snapshot (17,280 blocks) keeping three. The only operate-tier value still open is `max_open_proposals`, which has no field: governance uses a due index instead.

## Measured values (18)

The emergency, upgrade and handover validity windows and maximum anchor ages
(from measured multi-party signing and propagation; labeled 720-block
placeholders in the rehearsals), mempool, queue and peer capacity,
adapter and channel connection limits, the emergency verifier timeout, and
the fault assumptions. They need the dedicated hosts first.

## Derived values (25)

Set by the genesis builder from approved rules: fee caps (at least the
largest signable fee, at the governed price bound), the transport bound (a
full base64 envelope), control bounds (three hex SLH-DSA signatures), the
3-of-5 upgrade threshold, engine evidence parameters (equal to the
lifecycle's), validator power (equal to bonded stake), reward capacity (at
least the `max_active` bound), and activation heights, sequences and epochs
of 1.

## Records

| Record | What | Status |
| --- | --- | --- |
| D13-Q01 | Display name, unused chain ID, genesis time procedure | Approved (P01, 3 October 2026): `dytallix-mainnet-1`, Dytallix, a weekday 14:00:00 UTC at least 72 h after the final build ([approval](approvals/P01_E05_CHAIN_IDENTITY_2026-10-03.json), [identity](genesis/IDENTITY.json)); the time itself is set at the freeze |
| D09-Q02 | Initial operators, control groups, validator keys and possession proofs, self-bond funding, fault domains, signed acceptances | Open |
| D12-Q03 | Dedicated hosts: assets, providers, regions, account control groups, funded commitments | Open; hosts must run nothing but Dytallix |
| D12-Q01 | Pin plan: each host's label, operator, role, address, peer and validator public keys and pins | Open; released with the network configuration, outside this repository; the [host configuration generator](../node/docs/mainnet/host-configuration.md) checks it against the approved mesh rules |
| D10-Q03 | Signing roles: root genesis signers, 5 emergency custodians (10 keys), 5 upgrade custodians, validator and peer keys; custody, backup and drill records | Open; the emergency intake is outside the repository; the [upgrade intake](custody/upgrade/INTAKE.md) and its checker are public, and completed packets stay in the custody system. Keys use SLH-DSA-SHAKE-256s ([approval](approvals/P01_E05_CUSTODY_2026-09-30.json)) |
| D08-Q01 | Beneficiaries, accounts, amounts, vesting and initial delegations in the five buckets (Ecosystem growth 30%, Team and advisors 20%, Public sale 15%, Private sale 15%, Reserve 20%) | Open |
| D02-Q02 | Treasury recipient account and custody | Open |
| D08-Q03 | DRT bootstrap recipients and amounts | Open; after the bootstrap amount |
| D13-Q02 | Final identity manifest and genesis digest | After the build |
| D14-Q03 | Release roles and the independent reviewer (P02) | Open |

## Next engineering

- Done (E05-c): the [upgrade custodian intake](custody/upgrade/INTAKE.md) and
  `node/tools/mainnet-preparation/upgrade_custodian_intake.py`.
- Done (E05-d, rehearsal): the deterministic
  [genesis builder](../node/docs/mainnet/e05-genesis-builder.md), with the
  open values it needs proposed in [genesis/PROPOSALS.json](genesis/PROPOSALS.json).
- Done (E05-d2): the binding review covers the full configuration, the
  engine genesis and the build manifest (`node/tools/mainnet-preparation/config_checks.py`).
- Production activation: design approved
  ([production activation v1](../node/docs/architecture/production-activation-v1.md));
  steps A1 to A7 implement it, one PR each.
