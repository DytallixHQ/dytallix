# Contributing

## Scope

This repository is for Dytallix contract code, contract-facing utilities, examples, and supporting documentation. Keep website copy, compliance material, and unrelated node changes in their own repositories.

## Local Setup

```bash
cargo fmt
cargo test
cargo test --manifest-path examples/counter/Cargo.toml
cargo test --manifest-path examples/reward_splitter/Cargo.toml
cargo test --manifest-path examples/algorithm_guard/Cargo.toml
```

## Expectations

- Keep new modules documented in [docs/reference-contracts.md](docs/reference-contracts.md).
- Add or update tests with behavior changes.
- Prefer deterministic state-machine logic over implicit global state.
- Keep example contracts small and composable. They should teach one pattern clearly.
- Avoid changing public semantics in `src/tokenomics/`, `src/staking.rs`, `src/governance.rs`, or `src/algorithm_registry.rs` without updating docs.

## Repository Layout

- `src/` contains reusable contract modules and utilities.
- `examples/` contains standalone example contracts.
- `tests/` contains integration coverage for the shared crate.
- `test-harness/` contains prototype harness interfaces that are not yet part of the supported CI surface.
- `docs/` contains repo-local contract documentation.

## Pull Requests

- Explain the user-facing or protocol-facing behavior change.
- Link any relevant docs updates in the PR description.
- Include the commands you ran.
- Keep unrelated formatting-only changes out of functional PRs unless they are required for the touched files.

