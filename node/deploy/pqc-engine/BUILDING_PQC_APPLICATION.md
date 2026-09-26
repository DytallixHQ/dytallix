# Selected Rust consensus application

The `pqc-consensus` feature selects the application used by `consensus_stdio`. It does not authorize production activation. Existing configuration checks still refuse production profiles.

## Exact selection

Use Rust 1.88.0 and the workspace Cargo.lock. Select one package and one binary. Do not use workspace-wide feature unification for this artifact.

```sh
python3 scripts/check_consensus_cargo_profile.py --target x86_64-unknown-linux-gnu --output cargo-profile.json
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=1 cargo build --locked --offline -p dytallix-fast-node --no-default-features --features pqc-consensus --bin consensus_stdio
```

The build command above produces a native development-profile qualification artifact. On the intended Linux x86_64 builder, verify that the host target is `x86_64-unknown-linux-gnu`. It is not a release-profile or cross-compilation command. Capture target, compiler, environment, source and lockfile hashes. Use the existing bounded Linux procedure for isolated resources and provenance. A release-profile candidate needs its own build and qualification.

Run the dependency check for the actual target before each build. The check rejects known prohibited cryptographic and legacy network packages. Its list is a review aid, not proof that all algorithms in every dependency are approved. Inspect the resulting executable, dynamic libraries, static provider code and selected dependency sources. Hash the exact files. Keep G35 open until independent review accepts the full boundary.

Do not add default features or feature overrides. Compile-time guards reject legacy surfaces and alternate transaction signature backends. Dependency-local guards reject an Ed25519 bridge or alternate runtime backend added through feature unification. The graph check remains required because arbitrary dependency changes cannot be covered by a finite feature guard. It also requires exactly the selected FIPS 204 features: `default-rng` and `ml-dsa-65`.

## Preserved application behavior

The profile retains ordinary transaction validation, all twelve ordinary action variants, fees, nonce reservations, receipts, recovery, root initialization, block settlement, issuance, reward accounting, validator lifecycle and penalty processing. It uses the existing code for these operations. Unsupported ordinary action tags fail decoding. Existing configuration validation rejects unsupported engine profiles. The feature change does not add a handler or bypass signature, fee or policy checks.

Signature policy now resides in `dytallix-signature-policy`. The initial extraction is byte-for-byte identical to the former policy module. The legacy blockchain core reexports the same types. Both consumers use one implementation. Algorithm metadata now resides in `dytallix-protocol-types`; the legacy PQC crate reexports the same type. The selected application does not import that crate or its pre-standard algorithm implementations. Historical algorithm names remain as metadata for decoding and explicit rejection.

## Excluded surfaces

The legacy HTTP and WebSocket server, gossip module, bridge RPC and Ed25519 bridge storage, AI/oracle routes, Vault client and WASM contract runtime have been removed from this tree. The engine and separate HTTP adapter provide their own qualified transport boundary. The external root verifier remains a separate pinned executable.

The node's default feature is `pqc-consensus`, which selects the FIPS 204 ML-DSA-65 backend. No alternate signature backend or legacy service feature remains.

## Tests

Run shared signature-policy tests. Run the selected application library and pipe-protocol tests. Supply the separately built root verifier and fixture signer when running ignored root integration tests. Test rejected combinations for node legacy features, storage bridge features and alternate runtime backends. Check default library/main compilation separately. Do not combine default and selected tests into one Cargo workspace command.

Local passing tests establish the tested component scope. Repeat combined engine, root, application and hosted wallet qualification against the final Linux x86_64 artifact.

## Artifact and storage checks

Run `scripts/inspect_consensus_artifact.py` against the unstripped application and its independently recorded build hash. The check requires ML-DSA-65, Snappy and LZ4 implementation markers. It rejects the recorded classical, pre-standard PQC and unselected ML-DSA implementation symbols. It inspects direct libraries. These finite checks do not cover every provider or prove algorithm absence.

The storage crate explicitly selects RocksDB LZ4 and Snappy. It no longer inherits compression support through the legacy core. The earlier `production-boundary-qualification` artifact remains LZ4-only. Run the storage `compression_compatibility` tests to qualify persisted compressed SST reads for the current artifact configuration. Codec compatibility does not qualify a complete database migration or a production upgrade.

Keep final production compression selection and cross-version database qualification open. Do not deploy the selected artifact over an existing database based on the fresh-genesis tests. This is a storage compatibility limit, separate from the cryptographic boundary.
