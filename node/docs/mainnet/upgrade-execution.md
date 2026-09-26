# Chain-atomic upgrade execution

This implementation qualifies one real state-schema migration inside the current root-bound candidate. Production activation remains disabled. It does not replace executable files or qualify cross-binary upgrades.

## Authority and execution

An explicit genesis configuration supplies the chain, genesis digest, release, authority epoch, upgrade keys, threshold, initial sequence and resource bounds. No transaction can supply its own trusted authority. Production membership, threshold and limits remain unset.

Admission, activation and cancellation are separately signed under the root `upgrade` action and a versioned artifact domain. Admission stores one immutable plan and consumes one upgrade sequence. It changes no active schema. Activation requires a fresh signature over the exact plan, admission receipt, committed predecessor, target height, current emergency receipt and evidence digest. An expired activation requires new authorization. There is no automatic execution job.

A freeze or resume control in the same block prevents upgrade execution at every transaction position. A committed freeze also prevents execution. After resume, the global upgrade hold remains set. A fresh activation can authorize only its named plan under the exact current emergency history. It cannot clear other holds. A later freeze invalidates that context.

The application prepares migration writes only after the emergency checks. The index, schema marker, upgrade sequence, authorization receipt, block record and head commit in the existing synchronous RocksDB batch. Finalization alone writes nothing. A failed pre-write leaves old state unchanged. After a lost acknowledgement, exact replay reads the committed result and does not execute the migration twice.

## First migration

`emergency-receipt-digest-index-v1` builds a persistent digest-to-sequence index over verified emergency receipts. It preserves the original sequence-keyed receipt bytes. It changes no balances, supply rules, account keys or transaction nonces. The plan binds the compiled migration digest and explicit receipt-count, source-byte and write-byte limits.

Before activation, `/emergency/receipt/{sha256}` reports `index_unavailable`. After activation, the query reads the index and returns either `receipt_absent` or `committed_receipt_reported`, with chain, genesis, committed height and application-hash context. It does not build an index on demand or claim an independent inclusion proof.

Later emergency receipts add their index entry in the same block batch. Restart reconstructs the required mapping from block-bound receipt history. Missing, extra, incorrect or orphan records reject startup. Startup also replays the root signatures with the real helper.

## Version preservation and remaining qualification

The migration digest binds the retained `upgrade/v1/upgrade.rs` source. The top-level `upgrade.rs` compiles a versioned registry and checks the retained v1 source hash before use. A later edit to v1 fails that check. A future migration needs a new retained implementation and registry entry; it must preserve v1 serialization and replay semantics so existing history can reopen. The registry does not qualify a cross-binary upgrade by itself.

Current tests use synthetic authority and the same running candidate. Full production authority, numeric timing, migration limits, cross-binary compatibility, installation, rollback boundaries, native engine qualification and independent release acceptance remain required. No procedure resets signing history or creates a new genesis.
