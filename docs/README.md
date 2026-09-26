# Dytallix Documentation

Official documentation for Dytallix, a PQC-native Layer 1 blockchain.

Keypair, faucet, transfer, and basic contract lifecycle are available for experimentation on the public testnet. Staking, governance, and some advanced or operator paths are not yet production-complete.

This repository now contains the first complete public docs set for the Dytallix
SDK, CLI, public RPC, explorer endpoints, node snapshot, token model, security
assumptions, FAQ, and whitepaper distribution.

This repository is the canonical public documentation source. It is not the live website frontend source for `dytallix.com`.

The content in `docs/` was reconciled against:

- the public `dytallix-sdk` repository
- the public `dytallix-node` repository
- the local explorer and faucet READMEs
- the three Dytallix whitepapers
- live public endpoint checks performed on April 6, 2026
- [`public-surface.json`](public-surface.json), which is checked in CI

## Prerequisites

Use Python 3 to run the public-surface guard scripts in this repository.
Install MkDocs only if you want to preview the documentation site locally.

## Repository Layout

- [Home / Doc Map](docs/index.md) — landing page and navigation entrypoint
- [Publication Status](docs/publication-status.md) — repo roles, source availability, and current publication boundaries
- [Implementation Status](docs/implementation-status.md) — verified public behavior and known mismatches
- [Getting Started](docs/getting-started.md) — SDK and CLI quickstart
- [Contract Quickstart](docs/contract-quickstart.md) — canonical public WASM deploy path
- [Core Concepts](docs/core-concepts.md) — accounts, tokens, gas, and transaction model
- [CLI Reference](docs/cli-reference.md) — command reference for `dytallix`
- [SDK Reference](docs/sdk-reference.md) — Rust SDK crate and API reference
- [RPC Reference](docs/rpc-reference.md) — public RPC, explorer API, and faucet API reference
- [Node Operators](docs/node-operators.md) — local node and operator notes
- [Tokenomics](docs/tokenomics.md) — token roles and current public testnet behavior
- [Security Model](docs/security-model.md) — cryptographic and protocol security model
- [Whitepapers Index](docs/whitepapers.md) — whitepaper index and bundled documents
- [FAQ](docs/faq.md) — common questions and answers
- [Foundational White Paper (PDF)](docs/assets/whitepapers/dytallix-foundational-white-paper.pdf)
- [Technical White Paper (PDF)](docs/assets/whitepapers/dytallix-technical-white-paper.pdf)
- [Tokenomics Paper (PDF)](docs/assets/whitepapers/dytallix-tokenomics-paper.pdf)

## Local Preview

If you want to preview the docs as a site, this repository includes
[mkdocs.yml](mkdocs.yml).

```bash
pip install mkdocs
mkdocs serve
```

## Public Surface Guard

The canonical public integration values live in
[`public-surface.json`](public-surface.json). CI runs
[`scripts/check_public_surface.py`](scripts/check_public_surface.py) to catch
retired hosts, stale local ports, and install-command drift in the Markdown
docs before those inconsistencies land on `main`.

## Links

- Website: https://dytallix.com
- SDK: https://github.com/DytallixHQ/dytallix-sdk
- Explorer: https://github.com/DytallixHQ/dytallix-explorer
- Faucet: https://github.com/DytallixHQ/dytallix-faucet
- Docs site: https://dytallix.com/docs
- Discord: https://discord.gg/eyVvu5kmPG

The explorer repository remains a docs-only service-surface repo. The live website frontend for `dytallix.com` is a hosted public surface, not a separate public source repo, and the live faucet backend source is now public in `dytallix-faucet`.

## Contributing

Docs changes should preserve the current public boundaries. If a command,
endpoint, publication-status statement, or other guard-checked public wording
changes, update the relevant Markdown page and run
`python3 scripts/check_public_surface.py` before opening a PR. If the change
updates public behavior wording or required strings enforced by the
guard/config, update `public-surface.json` in the same PR.
