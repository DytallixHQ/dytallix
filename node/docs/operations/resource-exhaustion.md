# Resource exhaustion

Runbook v1 ([index](README.md)). The roles, thresholds and channel are
unset (D14-Q03, D12-Q02).

The limit values are E05 inputs (D06-Q02, D12-Q01). The service unit
renders `MemoryMax` and `TasksMax` for the node's one cgroup, and
`LimitNOFILE` for each process. It fixes `MemorySwapMax=0` and
`LimitCORE=0`. The adapter's connection, body, header and deadline limits
can only be lowered (`adapter_limits`).

## Signals

The node's metrics do not measure disk, memory or descriptors. Watch those
with the host's own monitoring agent (D12-Q02).

| Signal | Resource |
| --- | --- |
| The application exits `resource` (17): no space, quota, memory, descriptors or tasks | Host |
| The application exits `storage` (16) | Often disk space or inodes; check them first |
| The engine exits at startup with status 32 + 8 × stage + 5 (37, 45, …, 93; class 5 is resource) | Host, during engine startup |
| A child exits `KILLED` with signal 9 in the supervisor report | Memory: the cgroup reached `MemoryMax` |
| `dytallix_metrics_written_timestamp_seconds` goes stale | The writer is stopped, or the metrics directory is full |
| `dytallix_app_admission_queue_entries` stays high; CheckTx answers code 1 `Queue resource capacity exceeded` | Admission capacity |
| `dytallix_engine_mempool_size` or `_size_bytes` stays high; broadcasts fail with `mempool is full`; `_rejected_txs` rises | Engine mempool |
| The adapter closes connections, or answers 413, 502 or 504 | RPC capacity or deadline |
| `dytallix_app_block_execution_seconds` or `_commit_seconds` grows, and `dytallix_engine_consensus_block_interval_seconds` grows with it | CPU or disk latency |

## Host resources: disk, memory, descriptors, tasks

1. **Record** the supervisor report and the host's resource state: free
   space and inodes on each filesystem that holds the home, the metrics
   and the snapshots, and the memory and task counts of the unit.
2. **The state is safe.** A failed commit writes nothing, since the batch
   is atomic and synchronous, and the engine replays the block after a
   restart. Nothing needs repair.
3. **Restore the resource.**
   - **Disk.** Add capacity, or move unrelated files off the volume. Never
     delete files from `appdb`, `data`, the consensus WAL or the snapshot
     directory. A node set to `block_history: archive` grows with the
     chain. The window mode's growth is bounded by the retained window
     (`dytallix_app_retained_from_height` rises and
     `dytallix_app_block_records_pruned_total` counts the pruned records),
     but account state is never pruned.
   - **Memory, tasks or descriptors.** Raise the unit's `resources` values
     and render and install the unit again (E02,
     `tools/native-execution-policy`). Never add swap: keys and signing
     state must not reach it.
4. **Restart once** and confirm the height rises. If the same class recurs
   with the resource available, it is not this runbook's case: see the
   [index](README.md#application-exit-classes).

## Admission and mempool capacity

Transactions are signed and pay fees, so load cannot corrupt state.
Capacity limits refuse transactions rather than fail the node.

1. **Confirm the chain is live.** The height must still rise. A full queue
   with a stopped chain is a halt: [halt.md](halt.md).
2. **Identify the source** from the engine and application metrics. A flood
   of byte-distinct copies of one signed intent is refused at admission
   (E04 gap 6), so sustained growth means distinct, fee-paying
   transactions.
3. **Keep refusing.** Refusal under load is the designed behavior. Do not
   raise the mempool or queue limits beyond the approved values (D06-Q02).
   Until those are approved, the mempool values are operator settings.
4. **Public endpoint.** Its concurrency bounds are in the public endpoint
   contract ([RPC controls v1](../architecture/rpc-controls-v1.md)).
   Lower the `adapter_channel` connection limits if its connections are
   exhausted, and `adapter_limits` if the loopback adapter is overloaded.
   Rates are D12-Q01's.

## Do not

- Delete database, WAL or snapshot files to free space.
- Enable swap, or raise `LimitCORE`.
- Disable a limit to get through a peak.
