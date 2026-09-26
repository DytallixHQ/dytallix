# Examples

## Ping

[`examples/ping`](../examples/ping) is the smallest callable WASM example in the repo. It demonstrates:

- a real exported method that the current public node can invoke by name
- a `cdylib` build configuration that emits a `.wasm` artifact directly
- a minimal smoke-test target for signed `contract call` validation

## Counter

[`examples/counter`](../examples/counter) is the smallest useful example in the repo. It demonstrates:

- explicit instantiate, execute, and query messages
- owner-gated admin behavior
- deterministic state transitions

## Reward Splitter

[`examples/reward_splitter`](../examples/reward_splitter) shows how to compose the shared staking module into an application contract that:

- accepts epoch-level emissions
- routes a portion into staking rewards
- accrues treasury and ecosystem balances
- exposes a simple distribution receipt for downstream accounting

## Algorithm Guard

[`examples/algorithm_guard`](../examples/algorithm_guard) shows how a contract can:

- require an approved PQC algorithm before accepting an attestation
- inherit emergency shutdown semantics from the registry
- store minimal attestation metadata for later verification

