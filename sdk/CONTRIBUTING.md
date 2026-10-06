# Contributing to Dytallix SDK

Outside contributions are signed off under the Developer Certificate of
Origin; see the repository's [contributing guide](../CONTRIBUTING.md).

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
- `cargo clippy --locked --workspace --exclude dytallix-protocol-types --exclude dytallix-client-channel --all-targets --no-deps -- -D warnings` must pass; the local profiles are linted separately (see `.github/workflows/ci.yml`), since their features cannot be combined.
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

- [dytallix-docs](https://github.com/DytallixHQ/dytallix-docs)
- [dytallix-explorer](https://github.com/DytallixHQ/dytallix-explorer)
