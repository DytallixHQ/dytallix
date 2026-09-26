# Reward Splitter Example

Reference application contract showing how to compose the shared staking module with a simple epoch-emission splitter.

It demonstrates:

- fixed-basis-point reward routing
- treasury and ecosystem accrual tracking
- staking reward distribution through the shared reward index

Run:

```bash
cargo test --manifest-path examples/reward_splitter/Cargo.toml
```

