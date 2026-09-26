# RPC And API Docs

[Docs hub](README.md) | [Repository map](repository-map.md) | [Build and run](build-and-run.md)

Keypair, faucet, transfer, and basic contract lifecycle are available for experimentation on the public testnet. Staking, governance, and some advanced or operator paths are not yet production-complete.

## Public Node RPC

The main public-node protocol reference is:

- [Fast node RPC reference](../dytallix-fast-launch/node/README_RPC.md)

That document describes:

- local default base URL and environment flags
- transaction, block, balance, stats, peer, and websocket endpoints
- nonce and receipt rules
- RocksDB storage keys
- AI risk oracle endpoints
- bridge ingestion and custody behavior

## Core RPC Endpoints Called Out In The Current Docs

The fast-node RPC reference currently documents these major surfaces:

- `POST /submit`
- `GET /tx/{hash}`
- `GET /balance/{address}`
- `GET /block/{height|hash|latest}`
- `GET /blocks`
- `GET /stats`
- `GET /peers`
- `WS /ws`
- `POST /oracle/ai_risk`
- bridge endpoints under `/bridge/*`

Use the nested RPC doc for request shapes and examples rather than duplicating
that protocol surface here.

Compatible node deployments now expose a machine-readable public contract at
`GET /api/capabilities`.

Stake and governance write routes are intentionally not public-complete. Public
read routes remain available, while unfinished write paths should return
explicit unsupported errors until they are ready end to end.

## PulseGuard

The PulseGuard draft API lives at:

- [PulseGuard API draft](../blockchain-core/src/risk/pulseguard/api/OPENAPI.md)

The current draft calls out:

- `POST /pulseguard/score`
- `GET /pulseguard/stream` as planned SSE or websocket output

## Security And Cryptography References

- [PQC implementation](../dytallix-fast-launch/node/PQC_IMPLEMENTATION.md)
- [Secrets management](../blockchain-core/SECRETS_README.md)

These docs cover algorithm support, feature flags, secret-provider configuration,
and the current operational patterns around the node’s security-sensitive
surfaces.
