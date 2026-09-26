# Cryptographic backend selection

The fast-node manifest selects `pqc-fips204,metrics,oracle,contracts` by default.
The `ActivePQC` implementation in `src/crypto/dilithium_fips204.rs` uses
`fips204::ml_dsa_87`. Its compatibility label remains `dilithium5`.
The label alone does not identify the algorithm encoding or prove compatibility.
Other account verification paths must be checked separately.

The optional `pqc-real` backend uses `pqcrypto` through `src/crypto/dilithium.rs`.
Do not enable `pqc-real` and `pqc-fips204` together. Both export `ActivePQC`.
Do not use `--all-features` to qualify this package.

From the workspace root:

```sh
cargo build --locked --release -p dytallix-fast-node --bin dytallix-fast-node
cargo check --locked -p dytallix-fast-node --all-targets --no-default-features --features pqc-real,metrics,oracle,contracts
```

The `pqc-mock` backend is for development only. It is not release evidence.
Optional Falcon and SPHINCS+ paths require their own supported-feature review.

The `pqc_evidence` helper belongs to `dytallix-pqc`. It checks the PQCManager
backend selected by that crate. It requires `pqc-real`, verifies an ordinary
signature, rejects a changed message, and returns failure if either result is
unexpected. Its output does not qualify the fast-node FIPS 204 backend.

The optional metadata test checks a declared upstream metadata value. It does
not execute known-answer vectors. It is explicitly ignored unless requested
with `DYT_DILITHIUM3_META` pointing to the reviewed fixture. Required FIPS
known-answer vectors, interoperability, key lifecycle, transport, and
side-channel assurance remain release gates.
