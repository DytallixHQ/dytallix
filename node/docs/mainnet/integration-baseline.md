# Integration baseline

This batch repairs gas and mempool test fixtures. It also removes a panic from
insufficient-funds logging. It does not approve a mainnet fee model.

## Current contracts under test

- Execution charges the full upfront `gas_limit * gas_price` in `udrt`.
  Transfers in these fixtures use `udgt`.
- An out-of-gas transaction consumes its nonce and upfront fee. It must not
  transfer the requested amount. A failure before fee deduction consumes neither.
- Replay tests must execute successful transfers before comparing their results.
  Two equal error results do not establish successful replay behavior.
- Mempool fixtures use the active signature backend and canonical transaction
  representation. Tests submit through normal signature verification.
- A changed signed transaction must fail signature verification.
- Mempool admission rejects a stale nonce. It holds a future nonce outside the
  eligible snapshot. Promotion after a nonce gap closes remains a separate gate.
- Test fixtures retain their temporary database directory until the test ends.

## Logging boundary

`Mempool::validate_tx_funds_only` must return `InsufficientFunds` when the balance
is too small. Logging must not interrupt that return.

The sender display now uses a 12-character format limit. It does not slice at
byte 12. Normal ASCII addresses keep the same display prefix. Short strings and
UTF-8 strings cannot cause a string-boundary panic at this statement.

Tests cover normal admission, trusted admission, and `basic_validate`. They check
empty, short, 12-character, long ASCII, and multibyte senders. They assert the
rejection values, unchanged pool and account state, and subsequent funded
admission. These helper-level cases do not establish remote API reachability or
approve those strings as network addresses.

## Open integration gates

1. Resolve the fee contract. `Mempool::reserved_amounts_for_tx` currently reserves
   `fee + gas_limit * gas_price` in `udgt`. Execution deducts gas from `udrt`.
   Align denomination, amount, reservation release, and settlement after D04/D05
   are approved. Then test admission through execution using the same state.
2. Repair deferred-transaction promotion against the existing reverse-arrival
   determinism test. Check all admitted transactions, reserved funds, byte counts,
   and eligible snapshots. Do not discard failed admissions in the test.
3. Qualify performance with a defined build profile and test machine. Current
   admission timing includes fixture creation and signing. Do not increase limits
   only to obtain a pass.
4. Repair governance and staking integration fixtures against the selected
   lifecycle. Keep incomplete protocol writes disabled until D06/D07 are resolved.
5. Provide the missing registry WASM fixture from reproducible source.
6. Repair blockchain-core test compilation against its current public API.

The release gate also requires the mathematical, cryptographic, consensus,
validator-operation, recovery, and genesis evidence listed in
[specification-decisions.md](specification-decisions.md). Local tests alone do not
satisfy those requirements.
