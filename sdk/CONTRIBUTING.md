# Contributing to Dytallix SDK

Dytallix was built by one person. Contributions are welcome.

Start with the [README](README.md), then use the [docs hub](docs/README.md) to
find the right reference page for the area you are changing.

## Getting Started

1. Fork the repository.
2. Clone your fork.
3. Build the workspace.
4. Run the test suite.
5. Open an issue before starting significant work.

```bash
cargo build --all
cargo test --all
```

## Code Standards

- `cargo fmt --all -- --check` must pass.
- `cargo clippy --all-targets --all-features -- -D warnings` must pass.
- All public items should have doc comments.
- All new functionality should have tests.
- User-facing behavior changes should update the relevant markdown in
  [`README.md`](README.md), [`docs/`](docs/README.md), or [`examples/`](examples/README.md).

## Pull Request Checklist

- Keep changes scoped to one problem.
- Add or update tests when behavior changes.
- Update docs when command output, install steps, or public APIs change.
- Call out any network assumptions or endpoint changes in the PR description.

## Questions

Open a GitHub issue or join [Discord](https://discord.gg/eyVvu5kmPG).

## Other Repositories

- [dytallix-contracts](https://github.com/DytallixHQ/dytallix-contracts)
- [dytallix-docs](https://github.com/DytallixHQ/dytallix-docs)
- [dytallix-explorer](https://github.com/DytallixHQ/dytallix-explorer)
- [dytallix-faucet](https://github.com/DytallixHQ/dytallix-faucet)
