# Public ML-DSA-87 development fixtures

These files contain public keys, signed transaction data and signatures. They contain no private keys.
They use development network domain 3 and synthetic account data.
They do not authorize a production account or select production policy.

The development backend must verify these exact files. The selected consensus backend must reject their ML-DSA-87 signatures as unsupported.
The sponsor fixture has a valid ML-DSA-65 inner recovery operation and an ML-DSA-87 sponsor. This isolates the sponsor verification boundary.

Regenerate only when intentionally replacing public test vectors:

```sh
DYTALLIX_PUBLIC_FIXTURE_DIR="$PWD/crates/runtime-crypto/tests/fixtures/mldsa87" \
  cargo test --locked --offline -p dytallix-runtime-crypto \
  --features mldsa87-development export_public_development_fixture -- --ignored
```

Fixture generation creates ephemeral development keys. It writes only encoded public envelopes. Run both development and selected consensus tests after regeneration.
