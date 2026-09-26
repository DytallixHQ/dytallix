# Signed input verification batch

Outcome: implementation complete for this batch; release verification blocked.
Base revision: ddfe1b2db04320790e061477c698a1f881f87f6f.

The node now uses one signed-input conversion function for public submission and stored
record verification. Selected block settlement verifies every original signed input before
execution or rejected receipt writes. Recovery verifies original signatures, validates the
stored chain ID, reconstructs every execution field, and compares the record with the block.

The conversion retains current aliases, message order, display fields, exact arithmetic,
and fee errors. It uses stored development gas price for recovery. It preserves unsigned
internal development records. No storage schema, economic settings, or address rules changed.

Seven new tests check aliases, all message variants, self-send display behavior, full-width
amounts, arithmetic errors, chain checks, signatures, complete record comparison, and
unsigned development compatibility. Unit tests reject an invalid signature and inconsistent
execution fields. Source review confirms these checks run before selected execution. The
public lifecycle test passes submission, settlement, query, restart, and recovery after the
queue configuration changes. No exploit reproduction or live network test ran.

All 161 focused test executions pass in debug and release builds. All 16 storage tests pass
in each build. Thirty-two of 33 final checks pass. The full workspace records 793 passes,
14 failures, and one ignored test. Failed names match the previous transaction-record batch.
Matching failed names does not prove that all regressions are absent.

The first full workspace attempt stopped with signal 15 after 416 passes and eight failures.
Its cause is unknown. The retry completed with the results above. Both logs are retained.
The release binary, workspace build, temporary genesis/key/profile/supply startup checks,
format, module policy, module-policy tests, and workflow syntax checks pass. Clippy passes
with warnings, including formatting warnings in the moved gas-fee helper. The check manifest
records exact commands and exit codes.

The Codex Security fix-finding workflow included fresh read-only investigation and candidate
review. The reviewer found no concrete defect or regression in this scope. The reviewed
candidate source matches the final tested source. This is a scoped source review. It is not
a new full organization scan, independent cryptographic audit, or mainnet approval.

Signer ownership, address policy, authenticated mainnet gas policy, full signed-envelope
commitments, pending queue recovery, and production performance remain open. Reverification
adds signature work to the existing full-history recovery pass before each block. Adaptive
monetary integration, consensus, cryptographic assurance, governance, custody, and release
qualification remain open. No remote push, deployment, migration, or mainnet activation ran.

See signed-input-contract.md for scope and address-format-proposal.md for the Bech32m
recommendation. The user has not yet approved an address format. Local test counts do not
define a mainnet completion percentage.
