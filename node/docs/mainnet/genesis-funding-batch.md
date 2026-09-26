# Genesis funding batch: 9 September 2026

Genesis monetary initialization now debits stake from allocated DGT and preserves state
on restart. The core runtime no longer adds its default development balance to a supplied
genesis. These are local implementation changes. Mainnet release remains unapproved.

## Changes

The selected node validates every monetary entry before it writes state. One synchronous
database batch writes balances, funded stake, initial allocation counters, chain ID, and
an initialization marker. Reopening with the same source verifies that marker and does not
reapply allocations or stake. Invalid input cannot leave a partially imported allocation.

The core constructor starts with empty balances, sets supply to the checked gross allocation
sum, and subtracts each validator's stake from a matching allocation. Errors propagate to
the caller. The unfunded historical validator template now fails explicitly. A nonzero core
DRT initial supply also fails because that format has no DRT allocation map.

The [implementation contract](genesis-funding-contract.md) proves the initialization identity:
liquid DGT plus staked DGT equals allocated DGT. It defines accepted input, exact storage
behavior, compatibility changes, and the limits of that identity.

The separate reviewer found a cache regression. Direct database initialization left a fresh
account cache empty, so one balance query returned zero. The added regression failed before
the correction. The query now uses the existing database fallback and passes after reopening.
The reviewer found no other concrete new defect within the authorized patch scope.

## Verification

Sixteen new focused tests pass in debug and release: 11 selected-node tests and five core
runtime tests. They cover supply conservation, stake funding, malformed and duplicate input,
overflow, the existing DGT cap, startup compatibility, restart persistence, and cold-cache
balance reads. Both checked-in development genesis files remain accepted.

The selected release executable builds. Its genesis startup check rejects unfunded stake,
accepts funded initialization and reopening, and rejects changed source bytes. The existing
missing-key and unsupported-key checks also pass. All executable checks use private temporary
state and stop at or before validator-key initialization. The checks create no validator keys and start no network services.

Every default workspace target builds. The final full workspace run completes with 703
passed test executions, 14 failures, and one pre-existing ignored metadata check. The same
14 test names failed in the previous batch. The new core tests also execute in the core
binary, so 16 new unique tests add 21 workspace executions. No test expectation was weakened.

The remaining failures cover one contract gas-analysis test, two selected-node mempool
performance tests, five stake-weighted governance tests, one staking reward-rounding test,
one core circuit-breaker test, three core genesis unit-contract assertions, and one signed
response summary test. The ignored metadata check is not a cryptographic known-answer test.

Clippy completes with warnings in unchanged code. Formatting, patch whitespace, module
boundary checks, module-boundary tests, and workflow syntax checks pass. The evidence package
records all 15 final commands and their exit statuses. Focused checks use `cargo test --locked`
with the two genesis filters, in debug and release. The full run uses `--workspace --all-targets
--no-fail-fast`. The package includes the reviewed candidate and the failing cache regression
before correction. Its final patch and source archive identify the corrected revision.

Fix-workflow outcome: release validation remains blocked by the 14 existing workspace failures.
The focused evidence supports the changed initialization behavior. It does not close a full
security scan or a mainnet requirement. The original partial scan remains sealed. No remote
push, deployment, database migration, or mainnet activation occurred.

## Remaining work

Existing nonempty selected-node databases without the new marker need an explicit migration.
Keep their original data and genesis bytes. A changed or reformatted genesis file prevents
reopening initialized storage. No existing database was rewritten by this batch.

Runtime DRT supply reporting still uses the emission engine's independent initial supply and
emitted counter. The new initial DRT allocation counter does not close that discrepancy.
Vesting spend and stake enforcement, validator keys and lifecycle, and atomic runtime token
settlement remain open. Core account creation and core storage still use separate genesis
representations. The default development runtime retains its separate seed.

Next, define one execution transaction for balances, stake, rewards, minting, burning, and
supply counters. Add restart and interrupted-write tests at that boundary. Integrate the
approved adaptive controller only after allocation, timing, parameter calibration, and token
precision are resolved. Do not rescale amounts or select those policy values from defaults.

Distributed consensus, independent cryptographic and security review, release provenance,
operator qualification, recovery evidence, and final genesis approval remain required.
Passing test counts do not provide a mainnet completion percentage.
