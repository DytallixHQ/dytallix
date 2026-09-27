# Account model v2

Status: approved direction. Phase B1 of [state model v2](state-model-v2.md),
engineering task E04. This changes the account storage layout, so it must land
before genesis.

**Decided (P01, 26 September 2026):** option A, implicit account creation, with
an account-creation fee that is burned. The fee amount is a configurable
parameter; no production value is set here.

## Problems

### P12. One value holds every account, capped at 4,096

`recovery:v1:book` (`recovery_fees.rs`) stores every recovery account, sponsor
receipt, operation-success entry and expiry-index entry in one serialized value
of up to 64 MiB. Its height advances every block, so the whole value is
rewritten every block. `MAX_ACCOUNTS = 4096`. Validation iterates every
account on every block.

### P14. The account set is closed at genesis

- Recovery accounts are created only by `RecoveryBook::new` at genesis. No
  transaction creates one.
- An ordinary `Send` requires a registered recipient: `address()` in
  `ordinary_execution.rs` rejects "Unregistered ordinary account".
- Recovery actions, including `Enroll`, require an existing target ("Unknown
  recovery target").
- Legacy signed transfers, which accept any address, are rejected whenever the
  recovery profile is configured (`consensus_settlement.rs`, "Legacy signed
  requests disabled under recovery profile"), which mainnet requires.

A new user can therefore neither receive nor send. Removing the 4,096 cap alone
does not fix this.

## Fact that shapes the design

An account ID is `SHA3-256(origin domain ‖ network ‖ chain ID ‖ algorithm ‖
public key)` (`protocol-types/src/address.rs`, `initial_address`). An address
commits to its first public key, so the chain can verify an account's first key
without registering it in advance.

## Decision: how accounts come into existence

| Option | Rule | Trade-offs |
| --- | --- | --- |
| **A. Implicit creation (recommended)** | Any valid address can receive. The first transfer to a new address creates a balance-only record. The account's first outgoing transaction carries the full public key; the chain checks that it hashes to the account ID and then initializes the account's authorization state (generation 0, nonce 0, no recovery policy). Recovery policies are enrolled later with the existing `Enroll` action | Standard model (Ethereum, Bitcoin). No pre-registration step. Needs an anti-spam rule (below) |
| B. Explicit registration | A funded account submits a `Register` action with the new account's key identity. Transfers to unregistered addresses are rejected | Explicit and simple to reason about. Onboarding always needs an existing funded account |
| C. Closed set | Keep genesis-only accounts | A permissioned ledger; not viable for a public chain |

**Anti-spam for A or B (recommended):** charge an account-creation fee in uDRT
to whoever causes the account to exist (the first sender under A, the
registrant under B). The fee is burned or sent to the treasury, and its size is
a governed parameter. This bounds state growth by cost instead of by a fixed
cap.

## Storage design (independent of the decision)

1. **Deletions.** `Writes` becomes `BTreeMap<Vec<u8>, Option<Vec<u8>>>`, where
   `None` deletes a key. The state digest, commit batch and supply view apply
   deletions. This also unblocks the deferred pruning of sponsor receipts, the
   issuance journal and emission events.
2. **Per-account keys.**

   | Key | Value |
   | --- | --- |
   | `recovery:v2:header` | version, last height, fee profile |
   | `recovery:v2:account:{id}` | address, recovery state, sponsor nonce |
   | `recovery:v2:receipt:{id}` | sponsor receipt, pruned after its window |
   | `recovery:v2:operation:{operation}` | success index entry, pruned after its window |
   | `recovery:v2:expiry:{height:020}:{id}` | pending-expiry index entry |
   | `ordinary:v2:grant:{id}` | discretionary grant |

3. **Staged view (done in C6).** Replace the in-memory book with a view that
   reads accounts lazily from one snapshot and keeps a per-block overlay.
   Per-block validation covers touched accounts only; the complete history
   check still validates every account.
4. **Replay protection without full history (done in B1d).** Sponsor receipts
   had to cover every sponsor nonce. Now the sponsor nonce alone rejects a
   replayed sponsorship, and each receipt and its success-index entry are kept
   until the operation's submission expiry (`retained_until`), after which
   the operation cannot be submitted again. The window comes from the
   existing per-account `submission_lifetime`; no parameter is added.
5. Remove `MAX_ACCOUNTS` and the 64 MiB book bound. (Done: the byte bound in T5, the account cap in C6.)

Per-block cost becomes O(accounts touched), not O(all accounts).

## Implementation steps

| Step | Content | Depends on |
| --- | --- | --- |
| B1a | Deletions in `Writes`, digest, commit and supply | Nothing |
| B1b | Per-account storage and staged view; remove the account cap; no change to who can transact | B1a |
| B1c | Account creation per the approved option, plus the creation fee | Decision on A/B and the fee |
| B1d (done) | Sponsor receipt and operation-index windows: each receipt records its operation's submission expiry and is pruned with its success entry at that height; retained counters must be distinct and below the sponsor nonce instead of covering it; the history check accepts a missing receipt only when its operation has expired | B1a, B1b |

B1a and B1b do not depend on the decision. Genesis accounts keep working
throughout, so existing fixtures remain valid.

## Touched-account execution (approved, 26 September 2026)

Implicit creation removes the fixed account set, so the ordinary engine must
stop doing work for every registered account on every transaction. Paths below
are relative to `dytallix-fast-launch/node/src/`.

### Problem

Each ordinary transaction currently:

- meters three logical records (native account, recovery account, grant) for
  **every** registered account before acceptance (`meter_records` in
  `ordinary_execution.rs`). Validation gas therefore grows with the account
  count. Past a few thousand accounts, every ordinary transaction exceeds its
  signed gas limit and is rejected;
- loads every account into the settlement overlay and builds a financial
  snapshot of all of them (`snapshot`), so every block rewrites every `acct:`
  key;
- validates every nonce mirror and every grant twice
  (`validate_nonce_mirrors`, `validate_grants` in `ordinary_authority.rs`),
  and each mirror check runs `RecoveryBook::validate`, which is
  O(accounts × counters);
- reconciles the receipt against every registered account
  (`reconcile_receipt`), and resolves the actor and validator principals by
  linear search.

Recovery transactions and block assembly repeat whole-book work in the same
way: `sync_recovery_mirrors`, `reconcile_sponsor_charge`, `eligible_liquidity`,
`recovery_book` and the end-of-block ordinary checks.

### Rule

A transaction reads, meters and validates only the accounts it touches.
Global invariants over all accounts (every nonce mirror, grant, origin and
principal) move to the complete history check, which runs at startup and
after any outside write. Total supply stays in `supply::validate_native`,
which runs every block.

| Transaction | Touched accounts |
| --- | --- |
| Ordinary v2 | Actor, each `Send` recipient, each `DmsRegister` beneficiary (existence only), each `DmsClaim` owner. Validator and reward actions debit or credit only the actor |
| Ordinary v2, `ValidatorExit` or an operator unbond below `min_self_bond` | Also the recovery record of each delegator on that validator (protection check) |
| Recovery | Target and sponsor |
| Block start | Accounts in the expiry index at this height |

Governance v3 is not yet executed by consensus, and legacy signed requests are
unreachable under the recovery profile. Both are covered in the last step.

### Consensus-visible change

Only metering changes an output. With the rule above, each touched account
is charged for reading the same three records as today, and untouched
accounts are not charged. `gas_used`, fee charges, rejected-transaction gas,
block gas packing and stored ordinary receipts all change, so the change needs
a fresh genesis. No mainnet exists, so this has no migration. Reward, pool,
lifecycle and penalty reads are unchanged; they grow with stakers, not
accounts.

### Steps

| Step | Content | Output change |
| --- | --- | --- |
| T1 (done) | Delete the unused fee-plan digest (`snapshot_digest`, `predecessor_digest`). O(1) account lookup by address (`RecoveryBook::account_by_address`). Make `RecoveryBook::validate` linear | None |
| T2 (done) | Touched snapshot, touched mirror and grant checks (`validate_nonce_mirrors_for`, `validate_grants_for`), actor nonce advanced in place instead of cloning the book. Receipt reconciliation checks every account loaded into the settlement overlay (the overlay's key set is the access journal): touched accounts follow the receipt, all others must be unchanged. Metering left unchanged | None |
| T3 (done) | Meter touched accounts only, plus the recovery record of each delegator checked for protection. The overlay, and so the per-action write diff and the block's `acct:` writes, now holds touched accounts only | Gas and fees (fresh genesis) |
| T4 (done) | Recovery mirrors synced for target and sponsor only, without cloning the book. Admission and proposal eligibility computed on first use per owner (`LazyEligibility`). End-of-block ordinary checks cover only accounts loaded into the block's settlement (`OrdinaryState::validate_block`), so a block writes `acct:` keys only for accounts it touched. Ordinary state loads without whole-account checks except in the complete check. Sponsor reconciliation already covers only the loaded accounts, which are now the touched ones | None |
| T5 (done) | Recovery transactions update the book in place with an incremental expiry index and checks on the entries they change, instead of cloning the whole book and validating and serializing it per transaction; the full check still runs at block start and when the block's changes are staged. Ordinary transactions retain their receipt in place instead of cloning and re-validating the fee history. The block diff validates only the new book and serializes only changed entries. Proposal candidates roll back through targeted checkpoints (entry counts verified) instead of cloning the book and ordinary state; the settlement overlay is still cloned per candidate. The obsolete 64 MiB whole-book bound (storage design item 5) is removed; `MAX_ACCOUNTS` stays until B1c | Only the removed 64 MiB bound |
| T6 | Governance v3 and signed admission on touched accounts, before v3 is wired into consensus, including the committed-parent nonce scan (`committed_governance_parent`) that feeds signed admission | None today |

Each step keeps the existing suite green. T2 keeps the whole-set functions as
test oracles.

After T5, per-transaction cost no longer depends on the account count. Per
block, four scans remained O(accounts): the state digest, the supply scan, the
recovery-book load and the `origins` list inside the ordinary state. C1 moved
`origins` into per-account entries and C6 made the recovery book staged, so
two remain: the state digest, which moves to the incremental Merkle root
(state model phase B), and the supply scan. Opening the book still reads every
sponsor receipt and success-index entry (bounded, windowed by B1d), and the
governance parent still loads the complete book (T6).

### Tests

- A `Send`'s gas, metered reads and loaded accounts stay constant with 3, 64
  and 1,024 accounts.
- Prepared writes contain `acct:` keys only for touched accounts.
- For random transactions, the touched path and the whole-set oracle return
  the same result.
- A corrupted mirror on an untouched account no longer affects other
  transactions, but the next complete check rejects it.
- Determinism across independent databases.

## B1c: implicit account creation

**Decided (P01, 26 September 2026):**

- The creation fee is a field of the signed ordinary fee profile, so every
  transaction commits to it through `fee_profile_digest`. Its production value
  is set with the other genesis inputs (E05).
- An account created by receiving gets its recovery settings (timing and
  algorithms) from one chain account template in the genesis configuration,
  with values supplied in E05. Guardians are enrolled later with `Enroll`.
- Only `Send` creates accounts: a transfer to an unknown address creates a
  balance-only account, and the sender pays the creation fee, which is burned.
  A dead-man's-switch beneficiary must already exist, genesis balances go only
  to registered accounts, and recovery actions never create or initialize an
  account.

**Model.** A balance-only account has `acct:balances:{address}` and
`acct:nonce:{address}` = 0 and no recovery record. Its first outgoing
ordinary transaction carries the full public key; the chain checks that it
hashes to the account ID (`AccountAddress::from_origin_key`) and creates the
recovery record from the template (generation 0, no policy), even if the
transaction's actions fail, since its nonce is consumed. Origin keys move out
of `OrdinaryConfig`, which is rewritten every block, into immutable
`recovery:v2:origin:{id}` entries. Burned fees are tracked in
`supply:drt_burned`, and supply conservation becomes
`genesis + emitted - burned`.

| Step | Content | Consensus-visible |
| --- | --- | --- |
| C1 (done) | Origins move from `OrdinaryConfig` (rewritten every block inside `ordinary:v1:state`) to the recovery book, stored as immutable per-account `recovery:v2:origin:{id}` entries; the ordinary configuration checks they cover every account and hash to its ID | Storage layout and configuration schema (fresh genesis) |
| C2 (done) | `account_creation_fee_udrt` in the signed fee profile (codec format 2; must be positive; vectors regenerated by an independent Python encoder, `generate_ordinary_fee_vectors.py`, and synced to the SDK); `AccountTemplate` in the ordinary configuration, checked against both account roles and ML-DSA-65-only on mainnet; `supply:drt_burned` with conservation `genesis + emitted - burned` and a real `burned` in the supply response | Profile digest and codec, configuration schema, supply response |
| C3 (done) | `Send` to an unknown address creates a balance-only account (native record, nonce 0, no recovery record); the actor pays the creation fee, burned before the transfer, once per new recipient per transaction. The fee is reserved before acceptance; a missing recipient's read is metered as an empty native record; the receipt check requires the burn to equal the fee times the accounts created (none on failure); a block check requires every newly staged account to have nonce 0, a canonical address and no registration, and the block's burn to match; the complete check scans every native record (paired, canonical, nonce 0 without a recovery record) | New accepted transactions, gas for recipient reads, supply burns |
| C4 (done) | The first outgoing transaction of a balance-only account initializes it. With no recovery record for the actor, the chain requires the chain domain, generation and nonce 0, a key that hashes to the account ID, a key algorithm in the template and an existing native account, then checks authority against a prospective record built from the template without storing it. The record and origin are stored at acceptance, even if an action fails (the nonce is consumed), and removed again on an internal failure or when a proposal drops the candidate (checkpoints capture origins). Storing them is charged as writes before acceptance, so a limit too low to store them rejects without a fee. Reservation eligibility resolves owners by native address; the block check accepts an account created and initialized in the same block; the load check accepts post-genesis accounts whose chain domain, template configuration, address and origin record match. The ordinary account query stays null until initialization; a wallet signs the first spend at generation 0 and nonce 0 with its origin key | Authority rules, gas for initialization |
| C5 (done) | An initialized implicit account enrolls guardians and is recovered like a genesis account. A recovery whose target or sponsor has no recovery record is rejected before acceptance, with no charge, by a message saying the account is unknown or not yet initialized by its first ordinary transaction | Messages only |
| C6 (done) | Staged recovery view (B1b-2): accounts and origins are read from committed storage on first use, adjusted to the book height and cached, with the block's changes in an overlay (`recovery_store.rs`). The header stores the chain domain. Block start advances only accounts due in the expiry index; block-level validation, the stored diff and candidate checkpoints cover the overlay and the expiry index. Each changed account is checked against genesis or the template at the end of the block; the complete check loads every account and checks them all. Opening the ordinary state re-checks only the latest block's receipt actors. `MAX_ACCOUNTS` is removed. Test builds compare each staged block start, diff and rollback with the complete computation | Capacity |

There is no account cap (C6): the creation fee bounds growth, and a block
reads only the accounts it touches, the accounts with a pending expiry and
the actors of the previous block's receipts.

Client follow-up (not consensus): the SDK and CLI build the signing context
from the ordinary account query, which is null for a balance-only account.
They need a first-spend path that signs at generation 0 and nonce 0 with the
origin key, under the chain domain from the profile context.

## Tests

- Accounts beyond 4,096; per-block reads proportional to touched accounts
  (C6: a 4,100-account book, and a block whose account reads and complete
  loads do not change as accounts are added).
- Deletion round-trip: digest, commit, reopen and complete check.
- Under A: send to a new address; first spend with a matching key succeeds; a
  mismatched key is rejected; the creation fee is charged exactly once.
- Determinism across independent databases for every step.

## Open questions for P01

1. Creation fee amount and account template values (the parameters exist;
   production values are unset).
2. Should a balance-only account below some minimum be prunable (an
   existential deposit), or kept indefinitely?
