# Minimal Contract

Smallest deployable WASM artifact used by the public Dytallix contract
quickstart and smoke workflow.

Build:

```bash
rustup target add wasm32-unknown-unknown
cargo build --manifest-path examples/contracts/minimal_contract/Cargo.toml --target wasm32-unknown-unknown --release
```

The resulting artifact is:

```text
examples/contracts/minimal_contract/target/wasm32-unknown-unknown/release/minimal_contract.wasm
```
