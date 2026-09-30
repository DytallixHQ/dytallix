# Mempool rule v1 (E04)

Status: normative (P01, 30 September 2026: adopt the implemented rule).
Engineering task E04. Covers MEM-002 and MEM-003 of the
[E04.1 triage](../mainnet/e04-requirement-triage.md) and the rule part of
D06-Q02. It sets no capacity numbers: the queue, mempool and peer limits are
E05 inputs.

This records the rule the node already implements (gap 6, #270), so that
clients and operators can rely on it. Changing it needs a new version of
this document and an upgrade.

## Rule

1. **Engine.** The engine runs CometBFT's flood mempool with recheck. Its
   `max_tx_bytes` is no larger than the genesis block's transaction limit.
2. **Order.** Proposals take mempool transactions in the engine's order,
   which is arrival order, and drop any the block cannot admit. There is no
   priority by fee: every transaction pays one gas price, with no tips.
3. **One transaction per nonce.** Each account nonce (or recovery sponsor
   nonce) has at most one pending transaction. A second transaction with the
   same nonce is refused (`NonceConflict`). The same bytes submitted again
   are already reserved; a re-signed copy of a reserved intent is refused
   (identity mismatch), in CheckTx, rechecks and proposals.
4. **Reservation.** Admission reserves each transaction's fee cap and action
   debits against the payer's eligible balance, after vesting and custody
   checks. A transaction the balance cannot cover is refused.
5. **No replacement or eviction.** A pending transaction is never replaced
   by another, and a full queue refuses new transactions rather than evicting
   old ones. The queue is bounded by entries, wire bytes and signature work
   (`queue_max_entries`, `queue_max_wire_bytes`, `queue_max_signature_work`).
6. **Expiry.** Every transaction names an expiry height within the fee
   profile's maximum lifetime. From that height on it is refused, and
   recheck drops it.
7. **Release at each head.** After each committed block the engine rechecks
   the remaining transactions: each releases its old reservation and is
   admitted again against the new state.

## Not in the rule

- Capacity values: mempool size and bytes, cache, queue bounds and peer
  rates (D06-Q02, E05).
- A per-account cap on pending transactions (not adopted).
