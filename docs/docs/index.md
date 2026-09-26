# Dytallix Documentation

Dytallix is a post-quantum-native Layer 1 blockchain built around ML-DSA-65
accounts, canonical Bech32m addresses, a two-token economic model, and a public
testnet with SDK, CLI, faucet, node, and explorer surfaces.

Keypair, faucet, transfer, and basic contract lifecycle are available for experimentation on the public testnet. Staking, governance, and some advanced or operator paths are not yet production-complete.

This docs set is intended to do two things at once:

1. explain the protocol and developer interfaces clearly
2. document what the public testnet actually exposes today

The second point matters because several local source trees and older notes use
slightly different endpoint paths, fee-token descriptions, and local-node
defaults. Those differences are captured in
[`implementation-status.md`](implementation-status.md).

It is not the live website frontend source for `dytallix.com`.

## Public Snapshot

The content in this repository was checked against the live Dytallix public
gateway on April 13, 2026.

Public surfaces verified on that date:

- `GET https://dytallix.com/api/capabilities`
- `GET https://dytallix.com/status`
- `GET https://dytallix.com/account/:address`
- `GET https://dytallix.com/balance/:address`
- `GET https://dytallix.com/block/:id`
- `GET https://dytallix.com/blocks?limit=n`
- `GET https://dytallix.com/tx/:hash`
- `POST https://dytallix.com/api/blockchain/submit`
- `GET https://dytallix.com/api/blockchain/*` explorer reads
- `GET|POST https://dytallix.com/api/faucet/*`

## Start Here

- [`getting-started.md`](getting-started.md) if you want a funded wallet and
  your first transaction
- [`contract-quickstart.md`](contract-quickstart.md) if you want the canonical
  contract build path and the direct-node deploy flow
- [`core-concepts.md`](core-concepts.md) if you want the mental model first
- [`cli-reference.md`](cli-reference.md) if you are using `dytallix`
- [`sdk-reference.md`](sdk-reference.md) if you are integrating from Rust
- [`rpc-reference.md`](rpc-reference.md) if you are calling the public gateway

## Documentation Map

- [`publication-status.md`](publication-status.md) records repo roles,
  current source boundaries, and which repos are canonical for each live surface
- [`implementation-status.md`](implementation-status.md) records what was
  verified and where the current code and docs disagree
- [`node-operators.md`](node-operators.md) covers the published node snapshot,
  environment flags, and local RPC notes
- [`tokenomics.md`](tokenomics.md) explains DGT, DRT, micro-denoms, and the
  current fee-denom caveat
- [`security-model.md`](security-model.md) summarizes the cryptographic and
  protocol-level security posture
- [`whitepapers.md`](whitepapers.md) links the three bundled Dytallix
  whitepapers
- [`faq.md`](faq.md) answers the recurring questions quickly

## Quick Links

- Website: https://dytallix.com
- Explorer page: https://dytallix.com/build/blockchain
- Faucet: https://dytallix.com/api/faucet/status
- GitHub org: https://github.com/DytallixHQ
- Discord: https://discord.gg/eyVvu5kmPG
