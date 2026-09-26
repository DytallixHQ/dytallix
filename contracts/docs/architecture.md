# Architecture

## Layout

- [`src/runtime.rs`](../src/runtime.rs) hosts the shared WASM execution runtime.
- [`src/tokenomics/`](../src/tokenomics) contains the DGT, DRT, and emission-controller modules.
- [`src/staking.rs`](../src/staking.rs) implements validator registration, delegation, reward accrual, and slashing.
- [`src/governance.rs`](../src/governance.rs) implements proposal deposits, voting, quorum checks, timelock, and execution routing.
- [`src/algorithm_registry.rs`](../src/algorithm_registry.rs) manages the PQC algorithm allowlist and emergency controls.
- [`src/security/`](../src/security) and [`src/storage_optimizer.rs`](../src/storage_optimizer.rs) provide operational hardening helpers.

## Design Approach

The reusable modules are plain Rust state machines with explicit inputs and outputs. That keeps them easy to test, easy to embed into contract runtimes, and easier to audit than hidden global state.

The repository is intentionally split into:

- reference modules under `src/`
- standalone teaching examples under `examples/`
- integration tests under `tests/`

## Execution Model

The runtime layer is responsible for:

- contract deployment validation
- gas metering and runtime limits
- contract storage reads and writes
- event collection
- optional AI-assisted deployment and execution analysis

The contract modules focus on deterministic protocol logic rather than on host wiring.

