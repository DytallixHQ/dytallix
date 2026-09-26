# Runtime signature profiles

The selected consensus application uses `--no-default-features --features pqc-consensus`.
This profile compiles FIPS 204 ML-DSA-65 for ordinary, recovery, sponsor and operational verification.
It rejects `mldsa87-development`, `pqc-real`, `pqc-mock`, `falcon` and `sphincs` feature combinations.
It does not activate mainnet or approve a production release.

The `fips204` dependency disables its default feature set. The selected graph enables only `default-rng` and `ml-dsa-65`.
The maintained `scripts/check_consensus_cargo_profile.py` checks the exact selected graph.
It rejects legacy `dytallix-pqc` and `pqcrypto-*` packages and unexpected FIPS 204 feature selection.
A dependent package can unify additional upstream features. The graph check and final artifact inspection remain required.

The `pqc-fips204` feature alone now selects ML-DSA-65.
Use `mldsa87-development` explicitly for isolated ML-DSA-87 development compatibility.
The fast-node `legacy-services` feature includes that development feature. Existing node defaults retain that compatibility.
ML-DSA-87 does not authorize ordinary, recovery or sponsor signatures for network domain 1, even in the development-compatible build.
The selected consensus build rejects ML-DSA-87 signatures in every network domain.
No verifier translates legacy Dilithium names or bytes into ML-DSA.

Protocol codecs retain legacy algorithm identifiers and byte encodings for compatibility tests and historical data.
Those identifiers contain no cryptographic implementation and do not authorize execution.
The `SignatureAlgorithm` metadata enum now resides in `dytallix-protocol-types::signature_algorithm`.
The legacy `dytallix-pqc` crate reexports the same enum. Its names, variants and Serde encodings are unchanged.
The shared signature policy and storage metadata use the protocol module directly.
