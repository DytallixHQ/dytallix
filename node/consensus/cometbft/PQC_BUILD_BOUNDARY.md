# PQC build boundary

The fork has one build: the PQC-only build. It is not a production approval.

The build uses ML-DSA-65 key codecs, key defaults and consensus parameters,
and unsupported wire key types return errors. The peer listener requires an
explicit authenticated upgrade before it binds; the upgrade is the PQC
transport (`internal/pqcp2p`). RPC is served on the engine's Unix sockets;
there is no HTTP RPC server.

## Removed classical code (E04 gap 20)

P01 decided on 29 September 2026 to delete from the source the classical code
that the production tags used to exclude. The 94 files that the tags
`dytallix_pqc_only` and `dytallix_pqc_ipc` excluded were deleted, and the 32
files that required those tags lost the requirement. The deletions cover:
- **Classical keys:** `crypto/ed25519`, `crypto/secp256k1`,
  `crypto/secp256k1eth`, `crypto/bls12381` and `lp2p`.
- **Classical transport and signing:**
  - SecretConnection in `p2p/conn`;
  - the remote signer: its client, server, endpoints, socket dialers and
    listeners, message wrapping and Noise listener, plus
    `cmd/priv_val_server`.
  `privval` now holds only the file signer (`doc.go`, `errors.go`,
  `file.go`), and `node` rejects a configured remote-signer address.
- **HTTP, TLS, gRPC and SQL paths:**
  - the HTTP RPC server and its TLS;
  - the HTTP light client used by the old state provider and by
    `cometbft light`;
  - the gRPC ABCI client and server;
  - the Prometheus exporters;
  - the SQL block indexer;
  - the engine's legacy HTTP and default profiles.
- **Upstream tests:**
  - the upstream end-to-end framework, apart from its sample ABCI app,
    which `proxy` uses;
  - the tests that exercised the removed code or built classical keys.
    Test hygiene then restored the state, types, light, p2p and codec tests
    with ML-DSA-65 keys and deleted the rest of the HTTP RPC, gRPC and TLS
    test code. See `UPSTREAM_TESTS.md`.
- **Also changed:**
  - the fixture generator's `legacy-cometbft-loopback-only` profile is
    gone, so `--p2p-profile` must be given explicitly;
  - `cometbft show-node-id --libp2p` is gone;
  - the mock peer uses ML-DSA-65 keys.

Upstream still carries configuration fields for the removed features, such as
the libp2p settings and the remote-signer address. The engine's isolation
checks refuse them.

F-c removed the two tags from CI, the build tools and the docs. The G35 graph
check (`scripts/check_consensus_go_graph.py`) refuses a fork source file with
a Dytallix build constraint, and the boundary checker refuses an engine built
with any tag.

`scripts/check_consensus_go_graph.py` (from `node`) enforces the
boundary in CI. It fails when a selected graph imports a prohibited package,
when a removed classical or gRPC package returns to the fork, or when
remote-signer sources return to `privval`. The selected engine, bridge and root-verifier
graphs contain no `crypto/tls`, `crypto/x509`, `net/http` or gRPC packages.

## Build a candidate

Build and inspect one local candidate from this directory:

```sh
python3 tools/build_pqc_candidate.py --output-dir /ABSOLUTE/NEW/CANDIDATE
```

The output directory must not exist. The command uses locked dependencies,
disables CGO and ambient build switches, retains symbols, and requires an
identical rebuild. It writes the executable, build log, crypto inventory and
candidate record. A blocked exclusion check returns status 1. It does not
package, sign, publish or deploy a release. It never grants G35 approval.

Use `GOOS=linux GOARCH=amd64` for the checked cross-build target.
Cross-building does not establish Linux runtime qualification. The local
engine still rejects production startup and accepts only the existing loopback
test profile.

## Remaining exclusion work

The selected graphs still contain the Go standard library's
`crypto/internal/boring` and `crypto/internal/boring/sig`. With CGO disabled
these are disabled stubs and no-op markers, but the checker reports them for
provider review. Classifying them in the compiled executable belongs to T01.
Package and symbol rules cannot detect every renamed or unknown algorithm, so
a clean inventory alone cannot establish PQC compliance.

## Evidence limits

- **Upstream tests.** CI vets and tests every upstream package in the
  PQC-only build. 43 reactor tests that connect switches through the p2p
  test helpers are skipped: the helpers have no authenticated PQC upgrade.
  See `UPSTREAM_TESTS.md`.
- **Open work.** Remote release-tag equivalence, independent build
  reproduction, root authorization, private-key validation, custody and
  production qualification remain open.
- **Records.** The local source copy retains its upstream license and copy
  manifest. The manifest records the import before integration edits, so it
  still lists the deleted files.

Mainnet remains NO GO.
