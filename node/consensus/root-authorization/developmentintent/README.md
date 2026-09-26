# Development root intent recorder

This package validates and records one pending upgrade or halt intent. It uses the existing SLH-DSA envelope verifier and atomic `ExecutionStore.Update` contract. It does not execute an upgrade or halt. It has no production entry point.

The existing root verifier command and Rust application do not import this package. Their compiled inputs remain unchanged. No root private key is loaded by this package. Tests generate temporary in-memory test keys.

## Wire schema version 1

`Artifact` defines the exact field order. Encode with Go `encoding/json.Marshal` on this struct. Include all fields. Use `null` for the absent action body. No whitespace, alternate key order, duplicate keys, escaped key names, unknown fields, trailing data or alternative number representation is accepted. Each scalar has its declared Go type. Heights use unsigned 64-bit integers. Schema versions use unsigned 32-bit integers. These integers must retain their exact value in any future implementation.

The caller must supply a nonzero artifact byte limit no larger than 8,192 bytes. This implementation cap is not an approved mainnet limit. The decoder rejects input above the supplied bound before signature verification. The state document has a 16,384-byte implementation cap. Every SHA-512 digest and the intent identifier uses 128 lowercase hexadecimal characters and cannot represent zero. Chain IDs use 1 through 128 ASCII letters, digits, periods, underscores or hyphens.

Common artifact fields bind version, development mode, chain ID, action, intent ID, current release digest, approval-record digest and activation height. Exactly one action body is permitted.

An upgrade body binds the next release digest, source and target state-schema versions, migration digest and last rollback height. The source schema must match trusted current state. The target schema must be nonzero. Equal source and target schemas are permitted for a release with no schema change. The next release must differ from the current release. The rollback boundary cannot precede activation. This field records a proposed boundary; it does not prove migration reversibility.

A halt body binds scope, finalized anchor height and digest, review height and incident digest. The only development scope is `consensus`. The anchor must match the independently supplied policy and stored state. The review height cannot precede activation. Recording this field neither resumes a chain nor automatically removes the intent.

## Explicit trusted inputs

Supply `TrustedPolicy` independently of submitted bytes. It requires development mode, chain, permitted action, current finalized height, release, state schema, finalized anchor, exact approved artifact digest, approval-record digest, exact envelope validity interval, permitted activation-height interval and artifact byte limit.

No quorum, delay, authority membership, spending limit or production threshold has a default. The approval-record digest is a commitment to a reference. It does not prove approval, authority or controller independence. A future production admission layer must establish those facts before it supplies trusted policy.

`InitialState` constructs only the application document for this development component. The caller separately supplies the trusted key, permitted action sequences and finalized height to the root store. Any caller that advances finalized height or anchor must update the complete consistent state through the same transaction boundary.

## Atomic transition

`Record` performs these steps:

1. Validate the independent policy and raw byte bound before copying the artifact. Then check its canonical representation and schema.
2. Require the signed envelope validity interval to match the supplied policy exactly.
3. Call the existing root executor. The executor reads the trusted key, revocation state, finalized height and consumed sequence inside `Update`.
4. Verify the SLH-DSA signature and next sequence.
5. Check the stored application version, development marker, chain, current release, state schema and finalized anchor.
6. Reject any existing pending intent. Recheck the exact artifact in the callback.
7. Return new application bytes containing the intent receipt. The executor returns the consumed sequence in the same state update.

The receipt records the sequence, admission height, artifact digest and complete typed intent. The active release, schema and anchor remain unchanged. The package exposes no replace, clear, activate, expire, resume or migration operation. One pending upgrade also blocks admission of a halt intent. This conservative development rule requires a separate production decision on emergency priority and cancellation. The store must persist the returned application document and sequence together, or neither. After an uncertain acknowledgement, reopen and inspect durable state before retrying.

The local file-store tests exercise this whole document. Do not use that file store as a second replay ledger for a RocksDB chain transition. No production chain database adapter exists for this component. Disk hardware failure, rollback of backups, multi-node consensus and installed-service execution are not qualified by these tests.

## Required production decisions and implementation

Resolve D11-Q03 authority classes, quorum, delay, validity, approval verification, upgrade migration bounds, halt authority and separate resume authority. Resolve D10-Q03 controller appointments, custody, backup, key rotation, revocation and recovery procedures. Resolve D14-Q01 release identity, manifest schema, build target and independent reproduction criteria. Approve the complete production wire schema and state transition separately.

Then implement a chain database adapter and consensus admission path. Preserve the intent receipt, sequence and application state in one chain transaction. Implement and qualify the independently checked operational consumer, fencing, acknowledgement, restart recovery, rollback boundary and cancellation/resume semantics. Binary installation and process replacement must remain outside the consensus transaction. Repeat review on the frozen production distribution.

Development mode is a mandatory marker. It is not a security boundary that makes arbitrary callers or stores production-safe. This package grants no new production authority and does not close a launch gate.
