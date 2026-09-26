# Getting Started

[Docs hub](README.md) | [Algorithms](algorithms.md) | [CLI reference](cli-reference.md)

## Build

```bash
cargo build --release
```

## Test

```bash
cargo test
```

## Generate A Default PQC Key Bundle

```bash
cargo run --bin keygen -- ./pqc_keys.json
```

This generates and stores:

- a signature keypair
- a key-exchange keypair
- algorithm metadata

## Generate Raw ML-DSA-87 Key Files

```bash
cargo run --bin keygen_raw -- ./keys
```

This writes `pk.bin` and `sk.bin`.

## Sign And Verify A Message

```bash
printf 'hello dytallix' > message.bin
cargo run --bin sign -- ./keys/sk.bin ./message.bin > signature.hex
cargo run --bin verify -- ./keys/pk.bin ./message.bin ./signature.hex
```

## Generate Evidence Artifacts

```bash
cargo run --features pqc-real --bin pqc_evidence -- ./pqc_artifacts
```

See [CLI reference](cli-reference.md) for the current binary set and outputs.
