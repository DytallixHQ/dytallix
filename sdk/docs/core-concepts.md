# Core Concepts

[Docs hub](README.md) | [Getting started](getting-started.md) | [SDK reference](sdk-reference.md)

Keypair, faucet, transfer, and basic contract lifecycle are available for experimentation on the public testnet. Staking, governance, and some advanced or operator paths are not yet production-complete.

## Identity and Addresses

- Dytallix accounts are PQC-native.
- The standard hot-wallet path uses ML-DSA-65 keypairs.
- A canonical D-Addr is derived from the ML-DSA-65 public key and encoded as
  Bech32m.
- The SDK retains a legacy SPHINCS+ compatibility API. It is not FIPS 205 SLH-DSA.
- FIPS 205 root authorization uses a separate component. The operational CLI rejects `crypto keygen --scheme slh-dsa`.

Relevant types:

- [`DytallixKeypair`](../crates/dytallix-core/src/keypair.rs)
- [`DAddr`](../crates/dytallix-core/src/address.rs)
- [`verify_mldsa65`](../crates/dytallix-core/src/signature.rs)

## Tokens

The SDK models two canonical tokens:

| Token | Purpose |
| --- | --- |
| `DGT` | Governance, delegation, and gas fees |
| `DRT` | Rewards and burns |

Relevant types:

- [`Token`](../crates/dytallix-sdk/src/lib.rs)
- [`Balance`](../crates/dytallix-sdk/src/lib.rs)

## Accounts and Nonces

An account state includes:

- The canonical address
- The public-key hash
- DGT and DRT balances
- The next transaction nonce
- The key scheme

The SDK exposes this as [`AccountState`](../crates/dytallix-sdk/src/lib.rs).

## Transactions and Fees

Transactions are created with
[`TransactionBuilder`](../crates/dytallix-sdk/src/transaction.rs).

Each transaction includes:

- `from` and `to` addresses
- An amount and token type
- `c_gas_limit` for compute gas
- `b_gas_limit` for bandwidth gas
- A sender nonce
- Optional `data` bytes for contract, staking, and governance payloads

Default behavior:

- Compute gas defaults to `21_000`
- Bandwidth gas defaults to `data.len() as u64`
- Fees are always denominated in DGT micro-units.

The fee estimate is represented by
[`FeeEstimate`](../crates/dytallix-sdk/src/lib.rs) and split into compute and
bandwidth components.

## Keystore Model

The SDK ships with a file-backed keystore:

- Path: `~/.dytallix/keystore.json`
- Format: JSON
- Behavior: stores named entries and tracks one active wallet

The CLI builds on top of
[`Keystore`](../crates/dytallix-sdk/src/keystore.rs) and treats the active
entry as the default sender for commands such as `balance`, `send`, `stake`,
and `governance`.

## Network Profiles

The public CLI currently supports two network profiles:

| Profile | Node endpoint | Faucet |
| --- | --- | --- |
| `testnet` | `https://dytallix.com` | `https://dytallix.com/api/faucet` |
| `local` | `http://localhost:3030` | `http://localhost:3030/dev/faucet` |

The current profile is stored in `~/.dytallix/config.json` and can be changed
with `dytallix config network <testnet|local>`.

Compatibility aliases such as `/api/status` and `/api/blockchain/...` are
still available for older clients while canonical reads live on root routes
like `/status`, `/account/<daddr>`, and `/balance/<daddr>`.

## Public Faucet Policy

The canonical public testnet faucet grants a fixed `10 DGT` and `100 DRT` per
successful request.

The current public limiter is:

- `60` second cooldown between successful requests
- `20` requests per hour

The public faucet is distinct from the local development faucet. Testnet flows
use `https://dytallix.com/api/faucet`, while local development uses
`POST /dev/faucet` with explicit micro-unit `udgt` and `udrt` amounts.

## Contracts, Governance, and Staking

The current network surface is intentionally split between public-ready flows
and unfinished operator-preview flows.

Contract lifecycle on the public gateway uses dedicated contract endpoints.
Some local or direct-node helpers still encode higher-level intent into
transaction `data` bytes, but those prefixes should not be treated as a stable
public protocol.

In particular, `stake:*` and `governance:*` payloads are not public-ready write
messages on the default public gateway. Compatible nodes should reject those
generic submit-path payloads until staking and governance are implemented end to
end as typed, production-complete flows.

On the default public website gateway:

- basic contract lifecycle flows are available for experimentation
- public staking writes are disabled
- public governance writes are disabled
- validator-set and delegation legacy JSON reads still require a direct node
- compatible nodes expose `GET /api/capabilities` for machine-readable runtime
  contract discovery

Use a local node or direct endpoint for unfinished write paths and operator
workflows.

## Current Scope

This repository currently focuses on:

- Core cryptographic primitives
- Transaction building and signing
- Optional node and faucet clients
- A developer-oriented CLI for testnet workflows

For the current command surface, see [CLI reference](cli-reference.md). For the
Rust API surface, see [SDK reference](sdk-reference.md).
