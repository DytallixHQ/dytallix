# Transaction record batch: 9 September 2026

The selected node now stores versioned JSON transaction records. Each public submission
retains its original parsed signed envelope beside the normalized execution transaction.
The [contract](transaction-record-contract.md) defines the format, write order, and limits.

Admission writes the record and Pending receipt together. Settlement preserves the original
envelope and writes the final record with the receipt and monetary effects. A conflicting
body cannot replace a stored transaction under the same hash. Block recovery checks each
record against its committed transaction and requires original envelopes for signed inputs.
The new record endpoint distinguishes missing data from invalid storage.

Nine new tests cover all stored message variants, full-width values, envelope field retention,
version and key checks, pending restart, immutable records, explicit legacy rejection,
committed record checks, public submission through block settlement and restart, restored
signature verification, and record-query status codes. The public fixture now uses funded
genesis. No real keys or live network services were used.

Thirty of 31 final checks passed. The full workspace test command failed.
The same 154 focused test executions passed in debug and release builds. All 16 shared-storage
tests passed in both modes. The workspace recorded 786 passes, 14 failures, and one ignored
test. The failed test names match the previous DGT supply batch. Release qualification
remains open.

The release binary, default workspace build, genesis/key/profile/supply startup checks,
formatting, module policy, module-policy tests, and workflow syntax checks passed.
Clippy completed with warnings. No warning identifies the new record codec or its tests.
The evidence package records exact commands, outputs, and the failure comparison.

This batch provides storage implementation and local functional evidence. The parent reviewed
its callers and commit boundaries. No independent candidate review or new full Codex Security
scan completed. The previous automatic reviewer filter block remains unresolved. No review
request was resubmitted to bypass that block.

Mainnet remains unqualified. Signer ownership, normalization revalidation, algorithm policy,
distributed replay, envelope commitments, pending queue recovery, vesting, stake transitions,
adaptive settlement, protocol decisions, consensus, cryptography, performance, operators,
migration, independent review, and release tests remain open. Local test counts do not define
a launch completion percentage.

Older transaction indexes require reviewed migration. No data was migrated, pushed remotely,
deployed, or activated on mainnet. The restored repository lacks original upstream ancestry.
Do not force-push this history over the upstream repository.
