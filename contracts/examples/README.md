# Example Contracts

- [ping](ping) - minimal exported-method contract for signed call smoke tests
- [counter](counter) - minimal stateful counter contract
- [reward_splitter](reward_splitter) - emission distribution wrapper around the staking module
- [algorithm_guard](algorithm_guard) - algorithm-registry gated attestation contract

Run them individually:

```bash
cargo test --manifest-path examples/ping/Cargo.toml
cargo test --manifest-path examples/counter/Cargo.toml
cargo test --manifest-path examples/reward_splitter/Cargo.toml
cargo test --manifest-path examples/algorithm_guard/Cargo.toml
```

