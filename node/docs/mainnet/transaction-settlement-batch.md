# Transaction settlement batch: 9 September 2026

The selected node now commits each accepted transaction's account state, nonce, withheld
fee record, inactivity-switch changes, and receipt in one synchronous database write.
The actual block producer uses this path. Mainnet release remains unapproved.

## Implementation

A transaction-local state map replaces direct writes during execution. It reads durable
account and switch values through fallible decoders. A failed message discards all message
effects and retains the accepted fee and nonce. A storage failure discards the plan and
stops the producer task. The cache changes only after the database commit succeeds.

Both transfer forms handle self-transfers and repeated accounts through the same staged
account map. Checked arithmetic prevents debit underflow, recipient overflow, nonce wrap,
fee-counter overflow, and truncated inactivity periods. Every message must name the same
sender as the enclosing transaction. Signature validation remains upstream.

A versioned settlement record binds the serialized transaction to its height and index.
A matching retry returns its existing receipt without charging again. Changed input or
position requires explicit recovery. This is transaction retry protection, not a complete
block-replay or crash-recovery design.

The new withheld-DRT counter records fees charged by execution version 1. It does not choose
a fee allocation or reconstruct historical balances. The existing burn engine remains an
in-memory diagnostic. Its updates publish only after successful commit. It still performs
no actual token burning. Its full-width arithmetic now avoids intermediate overflow.

The producer uses separate running, paused, and failed states. Storage failure is terminal
for that process. Status and health responses expose failure. Pause/resume return HTTP 503
after failure, including when control requests race with failure. Normal pause/resume remains
available before failure. Recovery and restart remain operator responsibilities.

The [implementation contract](transaction-settlement-contract.md) states the conservation
proof, fee semantics, persistence boundary, retry rules, API changes, and unresolved paths.
The execute_transaction API now returns Result. Existing callers handle storage errors
explicitly. Existing test assertions, gas charges, and legacy error text remain unchanged.

## Verification

The same 47 focused tests pass in debug and release: 13 settlement tests, four execution
tests, seven fee-diagnostic tests, 20 existing gas/receipt tests, and three producer-control
tests. Seventeen unique regression tests were added. Ten shared-storage tests also pass.

Tests cover self-transfers in both denominations and transaction forms, repeated accounts,
message rollback after a switch update, switch registration/ping/claim, checked bounds,
pre-fee rejection, corrupt stored values, injected pre-write failure, database reopen,
matching retries, changed retry input/position, and concurrent execution handles. Producer
tests verify terminal failure and the resume response. These are local functional checks.
The batch does not include real device-failure or power-loss testing.

The selected release builds. Genesis and existing-key startup checks pass. Every default
workspace target builds. The full workspace completes with 720 passed test executions,
14 existing failures, and one existing ignored metadata check. The failure names match
the previous batch. The ignored check is not a cryptographic known-answer test.

The remaining failures are one contract gas-analysis test, two mempool performance tests,
five stake-weighted governance tests, one staking reward-rounding test, one core circuit
breaker test, three core genesis unit assertions, and one signed-response summary test.
Clippy completes with existing warnings. Formatting, patch whitespace, module boundaries,
module-boundary tests, and workflow syntax checks pass. The evidence package records all
22 final commands and exit statuses.

A fresh read-only investigator traced the implementation before edits. A separate fresh
reviewer checked the candidate after focused tests and found no concrete defect within the
transaction boundary. The parent then corrected the confirmed producer-resume issue and
ran the final checks. The package retains the immutable review candidate and final patch
separately. Intermediate gas tests caught error-text compatibility changes; their assertions
were preserved and the implementation was corrected.

Fix-workflow outcome: release validation remains blocked by the 14 existing workspace
failures. The focused evidence establishes the changed transaction behavior. It does not
close a mainnet requirement or a full security scan. The original partial scan remains
sealed. No remote push, deployment, database migration, or mainnet activation occurred.

## Remaining mainnet work

Next, stage one complete block: emission, every charged transaction outcome, all receipts,
account and module state, block contents, height, and state commitment. Commit that block
atomically. Include failed charged transactions in replay. Handle block-write errors and
qualify recovery after interruption before claiming block settlement closure.

The current producer still applies emission before execution, writes blocks separately,
filters failed transactions, and suppresses block-write errors. RPC receipt writes and
legacy account, faucet, staking, emission-claim, governance, and core paths remain outside
the new transaction boundary. Mempool fee reservation also differs from execution charging.

Complete runtime DRT supply accounting and vesting enforcement. Resolve token precision,
adaptive allocation/timebase, fee and burn policy, consensus, and launch-module decisions.
Then integrate the approved adaptive controller with calibrated parameters. Distributed
consensus, independent assurance, original-source provenance, operator qualification,
recovery evidence, and genesis approval remain required. Test counts are not a mainnet
completion percentage.
