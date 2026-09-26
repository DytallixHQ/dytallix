# Account model v2

Status: proposal. Phase B1 of [state model v2](state-model-v2.md), engineering
task E04. Account creation is a protocol rule, so it needs an approved decision
(P01) before implementation. This changes the account storage layout, so it
must land before genesis.

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

3. **Staged view.** Replace the in-memory book with a view that reads accounts
   lazily from one snapshot and keeps a per-block overlay. Per-block validation
   covers touched accounts only; the complete history check still validates
   every account.
4. **Replay protection without full history.** Sponsor receipts currently must
   cover every sponsor nonce. Replace that with the nonce itself plus an
   authorization expiry window, like ordinary receipts in phase A3.
5. Remove `MAX_ACCOUNTS` and the 64 MiB book bound.

Per-block cost becomes O(accounts touched), not O(all accounts).

## Implementation steps

| Step | Content | Depends on |
| --- | --- | --- |
| B1a | Deletions in `Writes`, digest, commit and supply | Nothing |
| B1b | Per-account storage and staged view; remove the account cap; no change to who can transact | B1a |
| B1c | Account creation per the approved option, plus the creation fee | Decision on A/B and the fee |
| B1d | Sponsor receipt and operation-index windows | B1a, B1b |

B1a and B1b do not depend on the decision. Genesis accounts keep working
throughout, so existing fixtures remain valid.

## Tests

- Accounts beyond 4,096; per-block reads proportional to touched accounts.
- Deletion round-trip: digest, commit, reopen and complete check.
- Under A: send to a new address; first spend with a matching key succeeds; a
  mismatched key is rejected; the creation fee is charged exactly once.
- Determinism across independent databases for every step.

## Open questions for P01

1. Account creation: option A or B?
2. Creation fee: amount, and burn or treasury?
3. Should a balance-only account below some minimum be prunable (an
   existential deposit), or kept indefinitely?
