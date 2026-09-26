# Adaptive state encoding and command journal

Status: implementation candidate. The selected node does not activate this path.
This document specifies persistence of controller state and commands. It does
not specify token allocation, supply settlement, or consensus activation.

## Canonical controller encoding

Encode all integers with fixed-width big-endian bytes. Encode signed integers
as two's-complement i64. Do not encode native pointers or native-size integers.
The logical `window_samples` field uses a u32 on the wire, including on 64-bit hosts.

| Offset | Bytes | Field |
|---:|---:|---|
| 0 | 8 | ASCII magic `DYTAEC01` |
| 8 | 2 | u16 model version, currently 1 |
| 10 | 8 | u64 target utilization |
| 18 | 8 | u64 shock threshold |
| 26 | 8 | u64 volatility threshold |
| 34 | 4 | u32 window sample count |
| 38 | 8 | i64 integral minimum |
| 46 | 8 | i64 integral maximum |
| 54 | 48 | Six u64 gains: soft P/I/D, then hard P/I/D |
| 102 | 24 | Three u64 emission amounts: base, minimum, maximum |
| 126 | 1 | Last-epoch presence tag: 0 or 1 |
| 127 | 8 | u64 last epoch; must be zero when the tag is 0 |
| 135 | 4 | u32 stored error count N |
| 139 | 8*N | i64 errors in oldest-first order |

The complete length is 139+8*N bytes. The maximum is 524,427 bytes.
The decoder checks length, magic, version, configuration, tag, history count,
and error domain. It rejects trailing bytes. A present last epoch of u64::MAX
is invalid. With a present last epoch t, N must equal min(t+1,W).
With no last epoch, N must be zero.

Validate N<=W before allocating the error vector. W is at most 65,536.
This bound makes the length calculation safe on supported 32-bit and 64-bit hosts.
Every accepted byte string re-encodes to itself. Unsupported versions fail;
the decoder never falls back to another format.

## Storage records

The caller supplies a 32-byte binding to its approved genesis/protocol context.
This library does not derive or approve that binding. It checks exact equality
when opening records. It also checks the complete supplied controller configuration.
Configuration changes require a separate migration; no migration is implicit.

The head key is `adaptive:v1:head`. Its value is:
`DYTAEH01` (8 bytes), binding (32 bytes), controller encoding, and SHA-256 of
all preceding bytes (32 bytes). Its maximum length is 524,499 bytes.

Each event key is `adaptive:v1:event:` followed by the input epoch as u64 big endian.
Its value is exactly 176 bytes:

| Offset | Bytes | Field |
|---:|---:|---|
| 0 | 8 | ASCII magic `DYTAEJ01` |
| 8 | 32 | Protocol binding |
| 40 | 24 | u64 input epoch, utilization, and volatility |
| 64 | 8 | u64 command epoch, equal to input epoch plus one |
| 72 | 8 | u64 command amount in uDRT |
| 80 | 32 | Previous head checksum |
| 112 | 32 | New head checksum |
| 144 | 32 | SHA-256 of the preceding 144 bytes |

Checksums detect accidental corruption. They do not authenticate observations,
approve genesis, or protect against an actor that can replace database contents.
Consensus must authenticate those inputs and commit the resulting state root.

## Write and recovery rules

`AdaptiveJournal` holds an exclusive mutable borrow of the storage object.
The adapter owns writes to its key prefix. Do not bypass it through raw database
writes. No controller state is cached across calls.

Initialization is explicit. It rejects an existing head or any event in its prefix.
Opening requires an existing valid checkpoint. Missing or corrupt state does not
become a new genesis. Opening checks the latest event against the current state.
It does not replay the complete history automatically.

For each observation, read and validate the current head. Compute the transition
in memory. Reject invalid input, duplicate/skipped epochs, or an existing event
at the new key. Write the event and new head in one RocksDB WriteBatch with
synchronous writes and the write-ahead log enabled. Unrelated keys remain unchanged.
The batch either records both values or records neither under the database's
atomic-write contract. Hardware and operating-system durability require separate
qualification; the local tests do not simulate power loss.

A reported write error can have an unknown commit outcome. Read the current
checkpoint and event before retrying. If the commit succeeded but its response
was lost, the stored epoch prevents a second transition for the same observation.
Tests inject errors before a write and after a successful write to check both cases.

`verify_history(max_records)` replays from the supplied configuration. It checks
contiguous keys, observations, command amounts, state checksums, and the final
checkpoint. It rejects missing or extra events. It also rejects a history longer
than the caller's explicit limit. This is an internal consistency check, not proof
that observations came from finalized blocks or honest external sources.

## Integration still required

No token mint occurs in this adapter. Its command amount is an instruction for
the following epoch. The first epoch's emission still requires a genesis rule.
Do not treat a journal event as an issued token balance.

The future block writer must commit the controller state, recognized issuance,
allocation balances, total supply, and block/epoch marker in one transaction.
Do not call this journal commit and then separately credit accounts; a failure
between those writes would leave incomplete accounting.
The current adapter establishes the state/command persistence boundary. Its
commit API must join that larger transaction before activation.

Allocation, rounding-residual ownership, observations, fees, burning, parameter
migration, and distributed finality remain separate specifications. Mainnet
activation still requires calibration and independent review of the proof.
