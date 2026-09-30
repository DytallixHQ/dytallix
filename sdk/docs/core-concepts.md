# Core Concepts

[Docs hub](README.md) | [Getting started](getting-started.md) | [SDK reference](sdk-reference.md)

This is the mainnet candidate SDK and CLI for the consensus chain. It contains
no public testnet client.

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
| `DGT` | Governance and delegation |
| `DRT` | Fees and rewards |

On the consensus chain, fees are capped and charged in uDRT
(1 DRT = 1000000 uDRT), and every fee is burned. DGT pays no
fees.

Relevant types:

- [`Token`](../crates/dytallix-sdk/src/lib.rs)
- [`Balance`](../crates/dytallix-sdk/src/lib.rs)

## Accounts and Nonces

The node reports an account through `ordinary_client` (`comet-rpc`):
- `AccountView` holds the account's authority: its origin and current
  keys, its generation and its counters.
- `AccountSummaryView` holds its liquid balances, nonce, bonds, unbonding
  and claimable rewards.

Both are the node's report. `CometClient::query_balances` proves the
balances against the pinned chain.

## Transactions and Fees

The consensus chain uses ordinary-v2 transactions for transfers and staking,
ordinary-v3 transactions for governance, and recovery transactions. See
[Ordinary-v2](ordinary-v2.md), the [CLI reference](cli-reference.md) and the
[Recovery CLI](recovery-cli.md). Every write carries an explicit gas limit and
a maximum fee in uDRT.

The [`transaction`](../crates/dytallix-sdk/src/transaction.rs) module is the
legacy transaction model. It builds and signs transactions locally and makes
no network calls. Legacy transactions are created with `TransactionBuilder`.

Each legacy transaction includes:

- `from` and `to` addresses
- Either an amount and token type, or `data` bytes, but not both
- `c_gas_limit` for compute gas
- `b_gas_limit` for bandwidth gas
- A sender nonce

Default behavior:

- Gas limits default to an estimate from the message
  (`estimate_default_gas_limits`)
- Legacy fees are denominated in DGT micro-units. The consensus chain charges
  no fee in DGT.

The fee estimate is represented by
[`FeeEstimate`](../crates/dytallix-sdk/src/lib.rs) and split into compute and
bandwidth components.

## Keystore Model

The SDK ships with a file-backed keystore:

- Path: `~/.dytallix/keystore.json`
- Format: JSON, version 2
- Encryption: each private key is encrypted under a key derived from your
  passphrase. A version 1 file holds plaintext keys; `dytallix wallet migrate`
  encrypts it.
- Behavior: stores named entries and tracks one active wallet

The CLI builds on top of
[`Keystore`](../crates/dytallix-sdk/src/keystore.rs) and treats the active
entry as the default sender for commands such as `balance`, `send`, `stake`,
and `governance`.

## Reaching a Node

The CLI uses one pinned consensus chain, stored in `~/.dytallix/chain.json` by
`dytallix config pin-chain`. There is no default endpoint and no TLS.

- A node on this machine is reached over plain HTTP to a literal loopback
  address, such as `http://127.0.0.1:26657`.
- A remote node is reached through its endpoint pin file, which its operator
  publishes. Requests cross the post-quantum client channel: ML-KEM-768 key
  exchange, then an ML-DSA-65 signature by the endpoint's pinned key.

HTTPS and plain HTTP to a remote host are refused. See
[Reaching a node](cli-reference.md#reaching-a-node) in the CLI reference.

## Funding

There is no faucet. An account receives funds at genesis or by a transfer from
a funded account. A transfer to an address with no account creates it and
burns the chain's account creation fee.

## Staking and Governance

`dytallix stake` bonds, begins unbonding and claims rewards on the pinned chain.
`dytallix governance` proposes, deposits and votes with ordinary-v3
transactions. The chain has no contract runtime.

## Current Scope

This repository currently focuses on:

- Core cryptographic primitives
- Ordinary-v2, ordinary-v3 and recovery transaction building and signing
- An encrypted keystore
- An optional Comet JSON-RPC client (`comet-rpc`)
- The `dytallix` CLI for the consensus chain
- The legacy transaction model, which makes no network calls

For the current command surface, see [CLI reference](cli-reference.md). For the
Rust API surface, see [SDK reference](sdk-reference.md).
