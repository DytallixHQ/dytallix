# Contributing to Dytallix Node

Start with the [README](README.md) and [docs hub](docs/README.md) so changes to
the public node, APIs, and internal docs stay aligned.

## Getting Started

1. Clone the repository.
2. Build the workspace.
3. Run the relevant tests for the area you are changing.
4. Update documentation when behavior or public surfaces change.

Common commands:

```bash
cargo build --workspace --locked
cargo fmt --all
cargo test --workspace --locked
```

## Documentation Expectations

If your change touches any of the following, update the linked docs in the same
pull request:

- public node endpoints or request shapes
  Update [docs/rpc-and-apis.md](docs/rpc-and-apis.md) and
  [dytallix-fast-launch/node/README_RPC.md](dytallix-fast-launch/node/README_RPC.md).
- PulseGuard API behavior
  Update [blockchain-core/src/risk/pulseguard/api/OPENAPI.md](blockchain-core/src/risk/pulseguard/api/OPENAPI.md).
- PQC verification behavior or feature flags
  Update [dytallix-fast-launch/node/PQC_IMPLEMENTATION.md](dytallix-fast-launch/node/PQC_IMPLEMENTATION.md).
- secrets-loading behavior
  Update [blockchain-core/SECRETS_README.md](blockchain-core/SECRETS_README.md).

## Pull Request Checklist

- Keep the change set scoped.
- Run the most relevant checks for the area you changed.
- Update docs for API, config, or operational changes.
- Call out any snapshot assumptions or omitted deployment-only pieces in the PR
  description.

## Questions

Open a GitHub issue for normal discussion or join
[Discord](https://discord.gg/eyVvu5kmPG).
