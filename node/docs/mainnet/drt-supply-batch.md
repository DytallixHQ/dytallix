# DRT supply batch: 9 September 2026

The selected node now checks DRT conservation at recovery and before each block write.
The identity is genesis plus emitted DRT equals liquid balances plus withheld fees plus
emission pools. The [contract](drt-supply-contract.md) gives the proof and its limits.

The default emission configuration now uses funded genesis DRT. An explicit initial-supply
mismatch fails startup. Restart checks the original genesis allocation counter. A new
read-only DRT endpoint returns exact decimal amounts. The legacy supply helper now reads
verified storage and returns errors instead of adding unchecked configuration and cache values.

Eleven new Rust tests cover custody conservation, fee retention, pending reward exclusion,
configuration agreement, percentage emission, full-width amounts, proposed-state validation,
record formats, original genesis counters, and public supply responses. A new executable
check covers default, matching, and mismatched configuration at initial startup and restart.
Each executable case stops before signing or network startup at a controlled failure gate.

Twenty-nine of 30 final checks passed. The workspace test command failed.
The same 142 focused test executions passed in debug and release builds. Ten shared-storage
tests passed. The full workspace recorded 768 passes, 14 failures, and one ignored test.
The failed test names match the previous admission batch. This is not a passing release gate.

The release binary, default workspace build, genesis/key/profile/supply startup checks,
formatting, module policy, module-policy tests, and workflow syntax checks passed.
Clippy completed with warnings. No warning identifies the new supply module or its new
tests. The evidence package records exact commands and results. A final route declaration
move changed source order only; route tests, release build, Clippy, and format checks were
repeated for that source revision.

Block fixtures now use funded genesis. The full-width emission test starts with zero genesis
DRT, so its maximum emission fits total supply. Restart fixtures restore the matching initial
supply configuration. The lost-acknowledgement fixture retains its pre-commit account cache.
No test assertion was removed to accept a monetary mismatch.

This batch adds accounting implementation and local functional evidence. It is not an
independent security review or a new full Codex Security scan. The previous candidate-review
filter block remains unresolved. No review task was resubmitted to bypass that block.

Mainnet remains unqualified. DGT accounting, vesting, reward-liability checks, adaptive
settlement, unresolved protocol decisions, transaction records, consensus, cryptography,
performance, operators, migration, independent review, and release tests remain open.
The supply query currently scans history and state. It requires bounded serving before
mainnet exposure. Local pass counts do not define a launch completion percentage.

No remote push, deployment, or mainnet activation occurred. The restored repository still
lacks original upstream ancestry. Do not force-push this history over the upstream repository.
