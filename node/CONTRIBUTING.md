# Contributing to Dytallix Node

Start with the [README](README.md) and [docs hub](docs/README.md).

## Getting Started

1. Clone the repository.
2. Build the workspace.
3. Run the relevant tests for the area you are changing.
4. Update documentation when behavior or public surfaces change.

Common commands:

```bash
cargo build --workspace --all-targets --locked
cargo fmt --all
cargo test --workspace --locked
(cd consensus/cometbft && go test -mod=readonly ./...)
```

## Documentation Expectations

If your change touches any of the following, update the linked docs in the same
pull request:

- a component contract in [docs/mainnet/](docs/mainnet/)
  Update that contract document.
- consensus engine behavior
  Update [consensus/cometbft/README.md](consensus/cometbft/README.md) or
  [PQC_ENGINE_INTEGRATION.md](consensus/cometbft/PQC_ENGINE_INTEGRATION.md).
- module dependencies
  Update [docs/architecture/module-policy.json](docs/architecture/module-policy.json).

## Pull Request Checklist

- Keep the change set scoped.
- Run the most relevant checks for the area you changed.
- Update docs for API, config, or operational changes.
- Call out any snapshot assumptions or omitted deployment-only pieces in the PR
  description.

## Questions

Open a GitHub issue for normal discussion or join
[Discord](https://discord.gg/eyVvu5kmPG).
