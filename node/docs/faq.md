# FAQ

[Docs hub](README.md) | [Build and run](build-and-run.md) | [RPC and API docs](rpc-and-apis.md)

Keypair, faucet, transfer, and basic contract lifecycle are available for experimentation on the public testnet. Staking, governance, and some advanced or operator paths are not yet production-complete.

## Is this the full production repository or a published snapshot?

This repository is a cleaned source snapshot published from the running
Dytallix testnet server on April 5, 2026.

## What was intentionally excluded from the snapshot?

Runtime data, RocksDB files, launch evidence, local keys, Finder metadata, and
temporary backup artifacts were intentionally excluded from the published tree.

## Which binary should I run for the public testnet-style RPC node?

Use `dytallix-fast-node` from
[`dytallix-fast-launch/node`](../dytallix-fast-launch/node).

## What is `blockchain-core` for if there is also a fast node package?

`blockchain-core` contains the broader chain, consensus, risk, secrets, and
runtime logic. `dytallix-fast-launch/node` is the lean public RPC node and
execution-engine package built on top of that broader stack.

## Where are the actual RPC docs?

The main RPC reference is
[dytallix-fast-launch/node/README_RPC.md](../dytallix-fast-launch/node/README_RPC.md).

## Where are the secrets-management docs?

See [blockchain-core/SECRETS_README.md](../blockchain-core/SECRETS_README.md).

## Where are the post-quantum crypto docs?

See [dytallix-fast-launch/node/PQC_IMPLEMENTATION.md](../dytallix-fast-launch/node/PQC_IMPLEMENTATION.md)
for node-side verification details and
[`pqc-crypto`](../pqc-crypto) for the reusable crypto crate and CLI tools.

## Where are the contract examples?

See [`smart-contracts/examples/counter`](../smart-contracts/examples/counter)
and the rest of [`smart-contracts`](../smart-contracts).

## Why does the repo mention a removed `../interoperability` dependency?

The published snapshot removed a missing dev-only local dependency from
`pqc-crypto` so the open-source tree can build cleanly outside the original
server environment.

## Are public staking and governance writes available?

No. Public staking and governance reads are exposed, but unfinished write paths
are intentionally disabled or rejected until they are production-complete.

The near-term public contract is:

- validator discovery returns only D-Addr-compatible validator addresses
- public staking and governance read routes remain queryable
- write routes and reserved `stake:` and `governance:` submit payloads are not accepted as public-ready flows
