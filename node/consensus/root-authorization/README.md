# Exceptional root authorization

Status: maintained standalone module with local integration evidence. Mainnet remains **NO GO**.

The module uses CIRCL v1.6.3 `sign/slhdsa` with the provisional `SLH-DSA-SHAKE-256s` parameter set. No dependency version changed. Production parameter approval and independent review remain open. This implementation does not establish FIPS 140 validation.

The module accepts exceptional `genesis`, `upgrade`, and `emergency` envelopes. It does not authorize routine transaction, vote, proposal, wallet, or peer signatures. Each signed envelope binds profile, version, chain, action, nonzero sequence, validity heights and the SHA-512 artifact digest. Fixed field order, length prefixes and an action-specific context separate these signatures.

The public key has 64 bytes. The private key has 128 bytes. The signature has 29,792 bytes. The chain ID has at most 128 ASCII bytes from `A-Z`, `a-z`, `0-9`, `-`, `_`, and `.`.

## Trusted signing boundary

Use `SignForPolicy` in a separate trusted signer. Supply policy from independently approved inputs. It validates policy before key access, checks the private/public key pair with a separate challenge, signs the envelope, and verifies the result. `Verify` uses the same policy validation helper.

`ValidatePublicKey` checks fixed-size encoding and round-trip representation. An SLH-DSA public key contains byte strings. Encoding validity does not establish custody, trusted origin or key ownership. `CheckKeyPair` verifies a signature challenge because decoding alone cannot detect inconsistent private seeds and an embedded public root.

The low-level `Sign` function remains for compatibility and is deprecated. Do not expose it through RPC or an automatic approval service. Keep private-key loading and custody outside the node and wallet processes. These functions do not generate or persist production keys. Go does not guarantee complete memory erasure.

## Atomic execution boundary

`Execute` integrates signature verification with a caller-supplied application transition. It requires an `ExecutionStore` and a reviewed `ArtifactTransition`. The maintained `FileStore` adapter commits a complete local document. The Rust development genesis and emergency consumers invoke the verification-only helper and commit their own receipts and sequences in the chain RocksDB batch. They do not use `FileStore` as a second replay database. The Go `Execute` adapter remains separate from these Rust consumers. No upgrade executor or production chain adapter for `Execute` exists. See [emergency transaction freeze](../../docs/mainnet/emergency-transaction-freeze.md).

1. Establish trusted persistent chain identity, public key, revocation state, finalized height and explicitly initialized consumed sequences.
2. Supply the approved chain, action, artifact digest and expected finalized height through `ExecutionIntent`. Do not derive these values from the submitted envelope.
3. Serialize all root authority, application and finalized-height updates through the same storage transaction.
4. Call `Execute`. It checks current chain, finalized height, revocation, configured action and sequence exhaustion inside that transaction.
5. Verify the envelope against the currently trusted root key, next sequence and approved artifact.
6. Run a deterministic transition that validates the reviewed artifact schema and returns application bytes. The transition must have no external effects.
7. Commit the application bytes and consumed sequence atomically. On callback or pre-commit write failure, persist neither. After an uncertain acknowledgement, inspect durable state before retrying.

The store must retain consumed sequences during key replacement, revocation, reactivation and restore. Never decrease or reset a consumed sequence. Coordinate backups and authority changes with the same transaction boundary. A valid old signature must not become usable after restoration or reactivation. The final adapter must enforce this rule; the standalone module cannot constrain privileged external database writes.

Do not perform deployment, executable replacement, signing, RPC or other external effects inside the transition callback. These effects cannot share this atomic database transaction. Actual upgrade execution needs a reviewed durable intent and operational procedure. This module does not invent those semantics.

The caller must set admission and artifact-size limits. The library's fixed signature size does not bound request volume. Review verification work inside the serialized transaction before production use.

## Validation and remaining integration

Local tests exercise checked signing, malformed key encodings, key-pair consistency, policy and validity binding, and all three exceptional action labels. The new disk-backed fixture exercises atomic application/sequence commitment, restart, concurrent submissions, failed writes, lost acknowledgements, failed actions, revocation, key replacement, sequence exhaustion, stale finalized state and corrupt fixture storage.

The older `diskFixture` remains test-only and uses a mutex for one process. The maintained `FileStore` below adds an operating-system process lock and durable same-document persistence. Its tests include separate-process lock exclusion. Neither adapter qualifies physical disk failure, power loss or an actual chain action route. Test keys remain in memory. Temporary fixture files contain public root keys and synthetic state only.

Run `go test -mod=readonly -count=1 -json ./...` and `go vet -mod=readonly ./...`. The bounded race suite uses `go test -mod=readonly -race -count=1 -run '^(TestAtomicExecution|TestPublicKeyEncoding|TestSigningPolicyRejectsBeforePrivateKeyAccess)$' ./...`.

Production still requires approved artifact schemas, trusted configuration, a consensus storage adapter, actual action consumers, bounded admission, root parameters, custody, key ceremonies, rotation/revocation/recovery procedures, exact executable review and independent acceptance. This module cannot approve a launch or close G35.

## Maintained durable file adapter

`CreateFileStore` and `OpenFileStore` support macOS and Linux local filesystems. The directory must exist, belong to the effective user and exclude group/other access. The caller supplies an explicit maximum state-file size. The adapter rejects nonprivate, linked, nonregular or malformed state files. It opens files relative to a pinned directory descriptor.

Every update takes an in-process mutex and a nonblocking operating-system file lock. `ErrStoreBusy` means another process holds the lock. The adapter reads and validates the current canonical, checksummed document before the callback. It persists a complete new file, synchronizes that file, atomically renames it and synchronizes the directory. Reopening validates and synchronizes the directory before returning a snapshot.

A failure after replacement returns `ErrCommitUncertain`. The instance then rejects further reads and writes. Close it, reopen it and inspect durable application state and sequence before retrying. Do not interpret a storage error as proof that no state committed.

The adapter prevents chain/root-key changes, action-set changes, sequence or finalized-height rollback, and revocation reversal. Root replacement/reactivation requires a separately reviewed mechanism. The checksum detects corruption; it does not authenticate state against a privileged operator. Operators must not replace the directory or lock file while open, or restore old snapshots without approved anti-rollback procedures.

This is a maintained adapter for its own complete state transaction. It does not synchronize with the Rust RocksDB chain state. Never place a chain action in one database and its replay sequence in this file. Tests cover real adapter restart, signed execution, process exclusion, two-instance concurrency, failed writes, uncertain commits and strict state guards. Physical power loss, network filesystems, storage hardware and production hosting remain unqualified.

`FileStore.Update` is a trusted storage primitive. It does not authenticate a root envelope. Use `Execute` for signed actions. Do not expose raw store callbacks or authority updates through an untrusted service interface.
