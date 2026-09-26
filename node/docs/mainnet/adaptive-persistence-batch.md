# Adaptive persistence batch: 9 September 2026

## Completed

The adaptive controller now has a canonical state format. The encoding uses
fixed-width big-endian integers and a validated bounded history. It contains no
native pointer widths. Python and Rust produce the same bytes for five state vectors.

The storage module now has an atomic command journal. Each transition writes
its next command and controller checkpoint in one synchronous database batch.
It checks the protocol binding and exact configuration during recovery.
Missing or damaged state never triggers an automatic reset.

The journal can replay its complete history from the supplied configuration.
Replay checks commands, state checksums, and consecutive epochs. It rejects
missing or extra records and respects an explicit record-count limit.
Three additional Python vectors verify the initial head, updated head, and event.

The selected node does not call this journal. No mint, allocation, fee, supply,
or consensus rule changed. Supply settlement remains open. It requires an
allocation decision and a transaction that also updates balances and total supply.

## Verification

| Check | Result |
|---|---|
| Controller arithmetic tests | 6 passed |
| Canonical encoding tests | 3 passed |
| Journal and recovery tests | 10 passed |
| Debug and release builds | Same 19 unique tests passed in each mode |
| Independent reference vectors | 13 command vectors, 5 state vectors, 3 journal vectors matched |
| Controller strict lint checks | Passed |
| Storage lint checks | Passed with 3 existing formatting warnings in unchanged state.rs |
| WebAssembly controller compile | Passed |
| Module policy checks | Passed, including 4 policy tests |
| Source format, diff, and workflow syntax | Passed locally |

Recovery tests inject a failure before the database write and a lost response
after a successful write. They then close and reopen the database. A retry of
an already committed epoch fails without repeating the transition.
These tests do not simulate operating-system failure or physical power loss.

The first release attempt used a dependency compiled before the final error-trait
change. It failed to compile that mixed source state. A fresh release run passed
after the final files were in place. The initial log is retained separately.

The previous selected-node result was 212 passed and 8 failed at commit 0db2de0.
This batch did not rerun that full suite or the previously failing core targets.
Those failures remain open. No remote CI run, push, deployment, or launch occurred.

## Next requirements

1. Resolve D02 allocation, including named recipients and rounding-residual ownership.
2. Define epoch observations, first-epoch issuance, and the D03 timebase.
3. Add issuance, allocation balances, supply, and the epoch/block marker to the
   same atomic transaction. Do not commit the journal before separately crediting accounts.
4. Define parameter migration and bind configuration to approved genesis data.
5. Calibrate the plant and gains. Complete independent review of the conditional proof.
6. Close the governance, reward, benchmark, and core compilation failures.
7. Complete distributed consensus, cryptographic assurance, operator qualification,
   and the remaining mainnet gates.

Checksums prove neither input authenticity nor consensus finality. Calibration,
accounting integration, independent review, and mainnet approval remain incomplete.
