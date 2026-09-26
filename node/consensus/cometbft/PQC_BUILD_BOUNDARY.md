# PQC build boundary

`dytallix_pqc_only` selects a reduced engine build. It is not a production approval.

The selected build uses ML-DSA-65 key codecs, key defaults and consensus parameters. It excludes the Comet classical key implementations, legacy SecretConnection, libp2p and Noise. Unsupported wire key types return errors. The peer listener requires an explicit authenticated upgrade before it binds. Remote signer adapters reject use.

Build and inspect one local candidate from this directory:

```sh
python3 tools/build_pqc_candidate.py --output-dir /ABSOLUTE/NEW/CANDIDATE
```

The output directory must not exist. The command uses locked dependencies, disables CGO and ambient build switches, retains symbols, and requires an identical rebuild. It writes the executable, build log, crypto inventory and candidate record. A blocked exclusion check returns status 1. It does not package, sign, publish or deploy a release. It never grants G35 approval.

Use `GOOS=linux GOARCH=amd64` for the checked cross-build target. Cross-building does not establish Linux runtime qualification. The local engine still rejects production startup and accepts only the existing loopback test profile.

## Remaining exclusion work

Standard Go HTTP/TLS dependencies still include prohibited asymmetric cryptography. The remaining roots include configuration HTTP types, RPC HTTP/WebSocket services, generated ABCI gRPC definitions and PostgreSQL support. Runtime listener settings do not remove these dependencies from the executable.

Separate optional services and generated gRPC code from the selected core build. Then replace or remove each remaining HTTP dependency without adding an unreviewed cryptographic implementation or parser. Verify each resulting executable and its interfaces. Moving a service to another process does not create a G35 exemption. Every service used for production trust remains in the required inventory.

The checker separates BoringCrypto provider containers from concrete prohibited packages. The selected backend source requires review. Package and symbol rules cannot detect every renamed or unknown algorithm. A clean inventory alone cannot establish PQC compliance.

## Evidence limits

The default development build retains compatibility paths. Separate test helpers can contain classical compatibility code. They are not production-qualified artifacts.

The local source copy retains its upstream license and copy manifest. Remote release-tag equivalence, independent build reproduction, root authorization, private-key validation, custody and production qualification remain open. Mainnet remains NO GO.
