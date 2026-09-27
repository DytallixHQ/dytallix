# PQC build boundary

`dytallix_pqc_only` selects a reduced engine build. It is not a production approval.

The selected build uses ML-DSA-65 key codecs, key defaults and consensus parameters. Unsupported wire key types return errors. The peer listener requires an explicit authenticated upgrade before it binds. Production binaries are built with `dytallix_pqc_only,dytallix_pqc_ipc`; the IPC tag replaces HTTP RPC with a Unix socket.

Under `dytallix_pqc_only`:

- The Comet classical key packages (`crypto/ed25519`, `crypto/secp256k1`, `crypto/secp256k1eth`, `crypto/bls12381`) and `lp2p` have no buildable files. An import of any of them fails compilation.
- `p2p/conn` excludes SecretConnection. `privval` contains only the file signer (`doc.go`, `errors.go`, `file.go`); the remote-signer client, server, endpoints, socket dialers and listeners, message wrapping and Noise listener are not compiled. `node` rejects a configured remote-signer address.
- The selected engine, bridge and root-verifier graphs contain no `crypto/tls`, `crypto/x509`, `net/http` or gRPC packages.

`scripts/check_consensus_go_graph.py` (from `mainnet/node`) enforces these properties in CI: it fails on a prohibited package in a selected graph, on a classical package that becomes buildable under the tags, and on remote-signer sources in `privval`.

Build and inspect one local candidate from this directory:

```sh
python3 tools/build_pqc_candidate.py --output-dir /ABSOLUTE/NEW/CANDIDATE
```

The output directory must not exist. The command uses locked dependencies, disables CGO and ambient build switches, retains symbols, and requires an identical rebuild. It writes the executable, build log, crypto inventory and candidate record. A blocked exclusion check returns status 1. It does not package, sign, publish or deploy a release. It never grants G35 approval.

Use `GOOS=linux GOARCH=amd64` for the checked cross-build target. Cross-building does not establish Linux runtime qualification. The local engine still rejects production startup and accepts only the existing loopback test profile.

## Remaining exclusion work

The selected graphs still contain the Go standard library's `crypto/internal/boring` and `crypto/internal/boring/sig`. With CGO disabled these are disabled stubs and no-op markers, but the checker reports them for provider review. Classifying them in the compiled executable belongs to T01. Package and symbol rules cannot detect every renamed or unknown algorithm, so a clean inventory alone cannot establish PQC compliance.

## Evidence limits

The default development build retains compatibility paths. Separate test helpers can contain classical compatibility code. They are not production-qualified artifacts.

The local source copy retains its upstream license and copy manifest. Remote release-tag equivalence, independent build reproduction, root authorization, private-key validation, custody and production qualification remain open. Mainnet remains NO GO.
