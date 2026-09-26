# DGT and stake batch: 9 September 2026

The selected node now checks DGT liquid balances plus individual stake against issued DGT.
It separately checks individual stake against the stored total and enforces the existing
issuance cap. Both native tokens use one snapshot and proposed-write validation boundary.
The [contract](dgt-supply-contract.md) defines the accounting proof and its limits.

A new DGT supply endpoint returns exact amounts. Staking statistics use issued supply,
not the cap, and calculate the stake ratio with integer arithmetic. Statistics read stored
reward metadata and funded stake even when caches are stale or staking activation is off.
The unsupported annual-yield estimate is now null and explicitly unqualified.

Nine new Rust tests cover funded stake, successful and failed transfers, retry and restart,
proposed-state conservation, individual record totals, missing counters, record formats,
aggregate limits, the cap, exact ratios, public reporting, and stale-cache independence.
Two existing fixtures now fund stake through genesis instead of writing only a total.
Their original behavioral assertions remain in place.

Twenty-nine of 30 final checks passed. The full workspace test command failed.
The same 151 focused test executions passed in debug and release builds. Ten shared-storage
tests passed. The workspace recorded 777 passes, 14 failures, and one ignored test.
The failed test names match the previous DRT supply batch. Release qualification remains open.

The release binary, default workspace build, genesis/key/profile/supply startup checks,
formatting, module policy, module-policy tests, and workflow syntax checks passed.
Clippy completed with warnings. No warning identifies the shared supply module or the new
tests. The evidence package retains exact commands, outputs, and the failure comparison.

The parent reviewed the shared validator, block callers, genesis funding, and reporting.
No independent candidate review or new full Codex Security scan completed in this batch.
The previous automated reviewer filter block remains unresolved. No review request was
resubmitted to bypass that block.

Mainnet remains unqualified. Vesting, stake transitions, reward liabilities, adaptive
settlement, signed transaction records, protocol decisions, consensus, cryptography,
performance, operators, migration, independent review, and release tests remain open.
The supply and statistics queries still scan history and state. They need bounded serving
and performance qualification before mainnet exposure. Local test counts do not define
a launch completion percentage.

No remote push, deployment, or mainnet activation occurred. The restored repository lacks
original upstream ancestry. Do not force-push this history over the upstream repository.
