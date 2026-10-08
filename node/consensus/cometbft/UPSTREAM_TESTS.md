# Upstream tests in the PQC-only build

CI vets and tests every package of the upstream copy
(`github.com/cometbft/cometbft/...`) in the PQC-only build, in addition to
this module's own packages. Before E04 gap 20 test hygiene it ran only the
upstream `privval` and `node` tests, and many other upstream test packages
did not compile.

This record lists every upstream package whose vet, tests or build failed
after F-b (commit `1cb0bcb8`), grouped by cause, and what was done. Passing
upstream tests are development evidence only. They are not T-suite or launch
evidence.

## Failures and decisions

### 1. Missing module sums: tidy

The fork's `go.sum` was copied at import and never tidied, so it lacked the
sums that upstream tests need:
- testify's `mock` package needs `github.com/stretchr/objx`;
- the `p2p/conn` tests need `github.com/fortytw2/leaktest`.

Affected packages: every `*/mocks` package (`abci/client`, `abci/types`,
`crypto`, `evidence`, `light/rpc`, `mempool`, `p2p`, `proxy`, `rpc/client`,
`state`, `state/indexer`, `state/txindex`, `statesync`), and `blocksync`,
`cmd/cometbft/commands`, `consensus`, `evidence`, `inspect`, `mempool`,
`p2p/conn`, `proxy`, `rpc/core`, `state`, `statesync` and `types`.

`go mod tidy` records them. Tidy also drops the entries the deleted code
used: libp2p, pion, QUIC, secp256k1, the gRPC module and `golang.org/x/crypto`
(the standard library keeps its own vendored copy). It lists the optional
`cometbft-db` backends because tidy ignores build tags; none of them is
compiled. `go.mod` gains `tool github.com/cometbft/cometbft/cmd/cometbft` so
tidy keeps the upstream CLI, which CI builds, buildable with `-mod=readonly`.
The package graphs and module versions of the engine, bridge,
validator-key and operator-rpc commands are unchanged.

### 2. Tests of removed features: delete

| Removed feature | Deleted |
| --- | --- |
| gRPC ABCI | `abci/client/grpc_client_test.go`, `abci/server/grpc_server_test.go`, the gRPC half of kvstore `TestClientServer` |
| gRPC broadcast API | `rpc/grpc`, its generated service `proto/tendermint/rpc/grpc`, and `test/app` (a gRPC client and HTTP RPC scripts) |
| HTTP RPC | the test node `rpc/test` and its users: the `rpc/client` main, RPC, event and example tests, `light/example_test.go`, `light/provider/http/http_test.go`; the Dredd contract tests `cmd/contract_tests` and `dredd.yml`; the websocket load harness `test/loadtime` |
| RPC TLS | `TestServeTLS` and its classical key pair `rpc/jsonrpc/server/test.{crt,key}` |
| SQL indexer | `state/indexer/sink/psql/psql_test.go` (a Docker PostgreSQL test) |

`scripts/check_consensus_go_graph.py` requires `rpc/grpc` and
`proto/tendermint/rpc/grpc` to stay absent.

### 3. Unused packages that tidy leaves unbuildable: delete

`crypto/armor`, `crypto/xchacha20poly1305` and `crypto/xsalsa20symmetric`
had no importer and needed `golang.org/x/crypto`.

### 4. Tests that used classical keys: port to ML-DSA-65

| Package | Cause | Port |
| --- | --- | --- |
| `internal/test` | Ed25519 genesis and validator-key fixtures broke every test root (`store`, `inspect`, and all tests built on `ResetTestRoot`) | an ML-DSA-65 validator key derived from a fixed seed |
| `abci/example/kvstore` | `RandVal` built 32-byte untyped keys | a fresh ML-DSA-65 key |
| `consensus` | the WAL generator went with its build tag | restored as `wal_generator_test.go`, without RPC or gRPC addresses |
| `light` | `helpers_test.go` was deleted | restored with ML-DSA-65 keys |
| `state` | helpers and the state, execution, store and validation tests were deleted | all five restored with ML-DSA-65 keys |
| `types` | genesis, protobuf, validator-set and vote tests were deleted; parameter tests used `ed25519` | restored and ported; a stub key type covers the mixed-type check |
| `types`, `evidence` | part counts and sizes assumed Ed25519 | ML-DSA-65 values: one duplicate-vote evidence is 6,864 bytes |
| `p2p` | key, node-info and peer-set tests were deleted | restored; keys compare by bytes |
| `crypto/encoding` | the codec tests were deleted | rewritten for ML-DSA-65; other key types are rejected |
| `node` | `node_test.go` was deleted | the proposal-block tests, which need no running node, are in `proposal_block_test.go` |

### 4a. Dytallix changes to upstream behavior

| Package | Change | Test |
| --- | --- | --- |
| `statesync`, `node`, `config` | A restore from a local copy of a snapshot (disaster recovery v1, R5): `statesync.RestoreLocal` runs the state sync's own offer, apply and verify with chunks read from the copy instead of peers (a refetched chunk is read once more, a third request fails); `node.LocalSnapshot` and `StateSync.LocalRestore` (never read from a file) run it synchronously at start, also on the only validator, and refuse a node that holds state. `node.StateStore()` exposes the state store beside `BlockStore()` (R4). | `statesync/local_test.go` |
| `statesync` | `NewReactor` keeps `StateSync.TempDir` for the chunk queue. Upstream never set it, so every sync, from peers or from a local copy, used the system temp directory, which the production unit does not let the engine write; the engine defaults the directory to the node's data. | `statesync/local_test.go` |
| `p2p` | A persistent (pinned) peer is redialed for as long as the switch runs, each backoff wait at most `persistent_peers_max_dial_period` (P01, 2 October 2026). Upstream gave up after about 24.6 hours and left the peer to peer exchange, which the pinned mesh disables. | `switch_redial_test.go` |

### 5. Reactor tests that connect switches: skip

43 tests connect switches through the p2p test helpers (`MakeSwitch`,
`MakeConnectedSwitches`, `Connect2Switches`): 6 in `blocksync`, 14 in
`consensus`, 3 in `evidence`, 7 in `mempool`, 11 in `p2p/pex` and 2 in
`rpc/core`. The PQC-only transport refuses a peer without the authenticated
PQC upgrade. Only the engine installs one, and upstream packages cannot
import `internal/pqcp2p`. Each test calls `test.SkipWithoutPQCUpgrade(t)`.

Running them needs a test upgrade in the p2p helpers. That would add an
authenticated but unencrypted connection path to the fork's source, so it
is left for a decision. The engine's own tests exercise the reactors over
the real PQC transport.

### 6. Vet findings and flaky tests: fix

- `libs/bits`: `UnmarshalJSON` copied a `BitArray` including its mutex.
  It now assigns the fields, as its `null` branch already did.
- `consensus`: `TestWALPeriodicSync` waited one flush interval, so it failed
  on a loaded host. It now waits for the flush.
- `blocksync`: `TestBlockPoolBasic` hung in CI for its 10-minute timeout.
  The pool ignores a peer until its height reaches the peer's base. When the
  random height of the peer based at the start height equalled the start
  height (about 1 run in 1,000), the pool made one requester, could not pop
  it, and never counted the other peers. The test peers' heights now start
  above every base. `pool.go` is unchanged upstream code. In a node, the same
  state makes `IsCaughtUp` true, so the reactor would switch to consensus
  rather than hang.
- `light/store/db`: `Test_Concurrency` hung in CI for its 10-minute timeout
  (#344). `Prune` wrote its batch with its iterator still open. A MemDB
  iterator holds the database's read lock until it is closed and buffers 64
  entries, and the write needs the write lock. So pruning while more than 64
  entries stayed in range deadlocked, which the test's concurrent saves
  sometimes caused. `Prune` now closes the iterator, once, before the write.
  Upstream `main` still has the old order. The new
  `Test_PruneLeavingManyEntriesDoesNotDeadlock` deadlocked on every run before
  the fix. In a node, the light client's MemDB store (`internal/lightblocks`)
  would hit it at its first prune, once one client stored more than 1,000
  verified heights (the default pruning size). It saves one per verification.

## Not restored

These F-b deletions stay deleted: the classical key packages, `lp2p`,
SecretConnection and its tests, the remote signer and its tests, the
upstream end-to-end framework, and the `p2p` peer, switch and transport
tests. The node start, pprof, HTTP RPC and remote-signer tests from
`node_test.go` are also not restored. The fork's own
`p2p/authenticated_transport_test.go` covers the upgrade seam.

## Remaining HTTP code

The upstream CLI still carries HTTP code that no production graph reaches:
- `inspect` serves JSON-RPC over HTTP through `rpc/jsonrpc/server`;
- `rpc/client/http`, `light/provider/http`, `light/proxy` and `light/rpc`
  are HTTP clients;
- `cometbft reindex-event` reaches the `psql` sink.

Their remaining tests run. Removing this code is separate work.
