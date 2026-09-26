# CLI Reference

[Docs hub](README.md) | [Getting started](getting-started.md) | [FAQ](faq.md)

This reference uses the current FIPS-era names. The implementation still uses
legacy `Dilithium5` naming internally for the default signing path.

## Binaries

### `keygen`

Generates the default key bundle and writes it to JSON.

```bash
cargo run --bin keygen -- ./pqc_keys.json
```

### `keygen_raw`

Generates raw ML-DSA-87 signer key files.

```bash
cargo run --bin keygen_raw -- ./keys
```

Outputs:

- `pk.bin`
- `sk.bin`

### `sign`

Signs an input file using an ML-DSA-87 secret key file and prints hex-encoded
signed-message bytes.

```bash
cargo run --bin sign -- ./keys/sk.bin ./message.bin
```

### `verify`

Verifies an ML-DSA-87 signed-message blob against the original input file.

```bash
cargo run --bin verify -- ./keys/pk.bin ./message.bin ./signature.hex
```

### `pqc_evidence`

Generates key, signature, tamper-fail, and summary artifacts.

```bash
cargo run --features pqc-real --bin pqc_evidence -- ./pqc_artifacts
```

Typical outputs include:

- `pubkey.hex`
- `signed_tx.json`
- `verify_ok.log`
- `verify_fail_tamper.log`
