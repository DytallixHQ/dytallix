# Selected executable PQC boundary checker

This tool checks one Go executable and its selected source dependency graph. It does not approve G35 or mainnet launch.

Run from the CometBFT module directory:

```sh
CGO_ENABLED=0 go build -mod=readonly -trimpath -buildvcs=false \
  -tags=dytallix_pqc_only -o /tmp/dytallix-pqc-candidate ./cmd/dytallix-pqc-engine
python3 tools/pqc_boundary/check_boundary.py \
  --module-dir "$PWD" --binary /tmp/dytallix-pqc-candidate \
  --profile pqc-engine-v1 --tags dytallix_pqc_only --rebuild \
  --output /tmp/dytallix-pqc-boundary.json
```

Add `--expected-sha256 HEX` to check a previously recorded executable digest. The checker does not execute the engine. It disables module downloads and uses read-only module resolution. It writes the report only at the requested output path. The optional rebuild creates and removes a temporary executable.

Exit code 0 means `KNOWN_CLASSICAL_EXCLUSIONS_CHECKED` for this executable and graph. Exit code 1 means `FAIL`. Both results retain `launch_status: NO_GO` and `g35_status: NOT_GRANTED`.

The report records:

- Executable SHA256 and embedded Go build information.
- Go, compiler, linker, assembler, and symbol-reader digests.
- Selected package imports, selected source file digests, module lock digests, and ignored source filenames.
- Complete symbol inventory and known prohibited packages and symbols.
- Separate provider-container review entries for BoringCrypto packages and symbols.
- Shortest import paths from the engine to prohibited packages.
- Byte-identical rebuild result and checks for input changes during inspection.

The required profile uses either the exact `dytallix_pqc_only` tag or the exact `dytallix_pqc_only,dytallix_pqc_ipc` tag selection, `CGO_ENABLED=0`, `-trimpath`, and `-buildvcs=false`. It retains symbols. Missing executables, unexpected digests, unknown profiles or tags, unreadable symbols, stripped executables, custom linker flags, or unproven source binding fail the check. Run without `--rebuild` to inventory an existing baseline; the source binding remains unproven and the result fails.

The rules include known classical asymmetric algorithms, standard TLS and X.509, CometBFT legacy key implementations, libp2p, Noise, QUIC, DTLS, WebRTC, and SecretConnection symbols. Protobuf format declarations alone do not count as implementations. Symmetric encryption and hashes do not count as classical asymmetric cryptography.

BoringCrypto package names can refer to disabled backend stubs or marker functions. The checker reports these names under `provider_review_packages` and `provider_review_symbols`. It does not count these entries as classical algorithm implementations. An unresolved provider review still fails the check. Direct RSA, ECDH, ECDSA, Ed25519, and their FIPS 140 implementation packages remain prohibited. Prohibited entries also include excluded protocol containers; the number of entries is not an algorithm count.

Known-name matching cannot identify renamed or novel implementations. The tool does not prove absence of every prohibited algorithm. It does not qualify other executables, external libraries, deployment, key custody, SLH-DSA root authority, or protocol behavior. Independent source and release review remain necessary. Removing names, stripping symbols, or renaming algorithms does not satisfy this boundary.

Run tests:

```sh
python3 -m unittest discover -s tools/pqc_boundary -v
```

The combined tag selects the experimental private Unix RPC adapter. It requires `--rpc-profile dytallix-pqc-unix-v1` at engine startup. A separately inventoried HTTP adapter must serve ordinary clients. This does not exempt that adapter or other release executables from G35. The prior tag and the default build retain their existing HTTP adapter.

Use `tools/build_pqc_candidate.py --tags dytallix_pqc_only,dytallix_pqc_ipc --output-dir NEW_DIRECTORY` for a fresh bounded candidate and inventory. Both tag selections use the same crypto classification and provider review rules. The combined tag alone does not establish a passing boundary result.
