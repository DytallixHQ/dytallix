# RPC controls (E04 gap 10)

Status: approved (P01, 28 September 2026: decisions below). Engineering
task E04, gap 10 of the [E04.1 triage](../mainnet/e04-requirement-triage.md)
(API-002). Production topology, budgets and rate values stay with D12-Q01
(the SRE owner); this document fixes what the node serves and what any
public gateway in front of it must do.

## Problems

| ID | Problem | Where |
| --- | --- | --- |
| R1 | The engine served CometBFT's whole safe route map: search, mempool contents, consensus dumps, peer addresses and commit-waiting broadcasts included. | `upstream/rpc/core/routes.go`, `upstream/node/rpc_ipc.go` |
| R2 | Nothing stated what a public gateway must enforce; the engine cannot terminate TLS (G35 keeps TLS out of its dependency graph). | — |
| R3 | Limits and error semantics were spread across code and undocumented. | `upstream/rpc/jsonrpc/ipc`, `pqc-http-adapter` |

## Decisions (P01, approved 28 September 2026)

1. **Two sockets.** `HOME/data/rpc.sock` serves the client allowlist; the
   loopback HTTP adapter and the supervisor use it, so it is what any
   gateway reaches. `HOME/data/rpc-operator.sock` adds diagnostics for the
   node's owner. Both are mode 0600 in a 0700 directory. Every other route
   is served on neither.
2. **Gateway contract.** The node stays on local sockets and loopback. A
   public gateway terminates TLS and enforces the contract below; client
   methods need no authentication, since transactions are signed and reads
   are public chain data. D12-Q01 sets the topology and the values.

## Methods

Defined in `upstream/rpc/jsonrpc/ipc/allowlist.go`. `Select` fails when a
listed method is missing from the route map, so an upstream rename cannot
silently remove one.

| Socket | Methods |
| --- | --- |
| Client (`rpc.sock`) | `abci_info`, `abci_query`, `block`, `block_by_hash`, `block_results`, `blockchain`, `broadcast_evidence`, `broadcast_tx_sync`, `check_tx`, `commit`, `consensus_params`, `genesis_chunked`, `header`, `header_by_hash`, `health`, `status`, `tx`, `validators` |
| Operator (`rpc-operator.sock`) | the client methods, plus `consensus_state`, `dump_consensus_state`, `net_info`, `num_unconfirmed_txs` |

Served nowhere, with the reason:

| Method | Reason |
| --- | --- |
| `tx_search`, `block_search` | Unbounded index scans; explorers use blocks and receipts (interfaces v1, decision 3). |
| `unconfirmed_txs` | Exposes pending transactions and costs memory per call. |
| `broadcast_tx_commit` | Holds a connection until commitment; clients poll receipts instead. |
| `broadcast_tx_async` | Returns before CheckTx; `broadcast_tx_sync` reports admission. |
| `subscribe`, `unsubscribe`, `unsubscribe_all` | WebSocket only; the production build has no WebSocket. |
| `genesis` | Unbounded response; `genesis_chunked` serves it in bounded parts. |
| `dial_seeds`, `dial_peers`, `unsafe_flush_mempool` | Unsafe routes, already refused (`rpc.unsafe` must be false). |

The development HTTP build (default tags, `existing-http-rpc`) keeps
upstream behavior for comparison and is refused by every seed-backed and
production profile.

## Limits

Pinned in the production build:

| Limit | Value | Where |
| --- | --- | --- |
| IPC frame | 2 MiB | `ipc.MaxFrameBytes` |
| Request body | 1 MiB | `ipc.MaxRequestBodyBytes`, adapter `MAX_REQUEST_BODY` |
| Response body | 1.5 MB | `ipc.MaxResponseBodyBytes`, adapter `MAX_RESPONSE_BODY` |
| Connections per socket | 32 | `ipc.MaxConnections`, adapter `MAX_CONNECTIONS` |
| Request deadline | 10 s | `ipc.RequestTimeout`, adapter `DEADLINE` |
| JSON-RPC batch | 10 | `NewHandler` |
| HTTP headers | 64, 64 KiB | adapter `MAX_HEADERS`, `MAX_HEADER_BYTES` |
| ABCI query path | at most 1024 hex characters for state keys; identifiers checked per path | `consensus_stdio::query_path` |

The adapter's values are ceilings (E04 gap 13, P01 28 September 2026). An
operator can lower each one, never raise it: `--max-connections`,
`--max-request-body-bytes`, `--max-response-body-bytes`, `--max-headers`,
`--max-header-bytes` (at least 8192, Hyper's smallest buffer) and
`--deadline-ms`, each at most once. The supervisor passes them from its
optional `adapter_limits` block. The IPC frame and the engine's socket limits
stay fixed; a larger ceiling needs a new build.

## Errors

| Case | Answer |
| --- | --- |
| GET of a method not served | HTTP 404 `RPC method not found` |
| POST of a method not served | HTTP 200, JSON-RPC error -32601 (method not found) |
| Invalid parameters | GET: HTTP 500; POST: JSON-RPC -32602 |
| Method failure | GET: HTTP 500; POST: JSON-RPC -32603 |
| Batch over 10 | HTTP 400 |
| WebSocket, root browsing, URI-form POST | HTTP 501 |
| Deadline or cancellation | HTTP 504 |
| `abci_query` with `prove=true` | result code 1, log `application proofs are not qualified` (proofs come from `/state/proof`) |
| `abci_query` refused by the application (unknown path, malformed identifier, historical height) | GET: HTTP 500; POST: JSON-RPC -32603, with the application's reason in the message |
| Transaction results | code 0 success, 1 refused and not charged, 2 infrastructure failure, 3 charged failure (interfaces v1) |

## Gateway contract

Gap 19 replaces item 1. The P01 decision of 29 September 2026 allows no TLS
at a public edge, so a public endpoint serves the post-quantum
[client channel](client-channel-v1.md) instead. C-b rewrites this contract
around it.

A public gateway in front of a node's HTTP adapter must:

1. Terminate TLS (1.3) with a certificate for its public name, and speak
   plain HTTP/1.1 only to the adapter on loopback.
2. Forward only the client methods above, in both request forms, and
   answer every other method itself with the node's error form. It never
   reaches the operator socket.
3. Enforce per-client rate and concurrency limits, and request and
   response sizes and deadlines no looser than the node's (1 MiB, 1.5 MB,
   10 s). D12-Q01 sets the rates.
4. Refuse WebSocket upgrades, and cache only responses the adapter marks
   cacheable.
5. Require no authentication for client methods. Operator access is only
   through the host.
6. Log requests without bodies or client credentials; body logging would
   record signed transactions before they are public.

## R-a implementation notes

- **Engine.** `ipc.ServeSockets` selects both route maps and serves
  `rpc.sock` and `rpc-operator.sock`; `startRPC` in the production build
  calls it. `ipc.Listen` accepts only those two socket names.
- **Supervisor.** Before starting the engine it requires both sockets to
  be absent (a leftover one means manual recovery), and waits for the
  client socket. The state-sync harness removes both after a stop.
- **Tests.** Go: the allowlists name only real routes and exclude the
  listed methods; an excluded method is not found in both request forms;
  both sockets are created 0600 and serve their own lists; a start over
  existing sockets is refused. The supervisor refuses a leftover operator
  socket. The inventory lists both sockets.
- **End to end.** The C5 state-sync harness ran four validators on the new
  engine in a Linux container: node 3 joined by state sync (snapshot 20)
  and reached the chain's application hash at height 30.
  `tools/state-sync-join/run-in-docker.sh` now passes `--init`, which the
  harness needs.

## Steps

| Step | Content |
| --- | --- |
| R-a | Allowlists, two sockets, supervisor and harness checks, this contract, inventory entries |
