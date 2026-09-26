# Contributing to Dytallix PQC

Start with the [README](README.md) and [docs hub](docs/README.md).

## Getting Started

```bash
cargo build --release
cargo test
```

## Contribution Expectations

- Keep cryptographic changes scoped and well explained.
- Update documentation when algorithms, key formats, binary behavior, or
  security assumptions change.
- Prefer adding or extending tests when changing signature, KEM, or bridge
  verification behavior.

## Important Docs To Update

- algorithm support or implementation notes
  Update [docs/algorithms.md](docs/algorithms.md)
- CLI behavior
  Update [docs/cli-reference.md](docs/cli-reference.md)
- bridge signing or benchmarking behavior
  Update [docs/bridge-and-benchmarks.md](docs/bridge-and-benchmarks.md)

## Questions

Open a GitHub issue for normal discussion or join
[Discord](https://discord.gg/eyVvu5kmPG).
