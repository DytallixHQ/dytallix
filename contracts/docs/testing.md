# Testing

## Core Commands

```bash
cargo fmt
cargo test
```

## Example Commands

```bash
cargo test --manifest-path examples/ping/Cargo.toml
cargo test --manifest-path examples/counter/Cargo.toml
cargo test --manifest-path examples/reward_splitter/Cargo.toml
cargo test --manifest-path examples/algorithm_guard/Cargo.toml
```

## CI

The repository workflow is defined in [`.github/workflows/ci.yml`](../.github/workflows/ci.yml). It runs:

- `cargo fmt --check`
- library and integration tests
- each standalone example contract test suite

## Current Baseline

The imported contract crate originally contained three failing library tests. Those failures were reconciled in this repo so the baseline now passes locally before further contract work is layered on top.

## Prototype Harness

The `test-harness/` directory is currently a prototype interface sketch. It is
not part of the root crate CI surface and should not be treated as the
canonical contract execution harness for Dytallix.

The authoritative publishable checks are the root crate tests plus the example
contract test suites listed above.

