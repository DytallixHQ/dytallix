# Documentation

This folder is the GitHub-friendly documentation hub for the Dytallix node
workspace.

Keypair, faucet, transfer, and basic contract lifecycle are available for experimentation on the public testnet. Staking, governance, and some advanced or operator paths are not yet production-complete.

## Start Here

- [Capability manifest](public-capabilities.json) - machine-readable public
  routes, write restrictions, and feature maturity
- Compatible nodes also expose that contract at `GET /api/capabilities` for
  runtime discovery
- [Public deployment evidence](public-deployment-evidence.md) - publicly
  verifiable live-node contract checks and the remaining provenance gap
- [Repository map](repository-map.md) - packages, binaries, and notable source
  areas
- [Build and run](build-and-run.md) - workspace build commands and main
  entrypoints
- [RPC and API docs](rpc-and-apis.md) - public node endpoints and where the
  deeper protocol docs live
- [FAQ](faq.md) - operational questions about this repository snapshot
- [Deployment separation audit](deployment-separation-audit.md) - classification
  of current Dytallix versus legacy deployment overlap

## Existing Internal Docs

- [Fast node RPC reference](../dytallix-fast-launch/node/README_RPC.md)
- [PQC implementation](../dytallix-fast-launch/node/PQC_IMPLEMENTATION.md)
- [Secrets management](../blockchain-core/SECRETS_README.md)
- [PulseGuard API draft](../blockchain-core/src/risk/pulseguard/api/OPENAPI.md)

## Repository References

- [Project README](../README.md)
- [Contributing](../CONTRIBUTING.md)
- [Security policy](../SECURITY.md)
- [Changelog](../CHANGELOG.md)
- [License](../LICENSE)
