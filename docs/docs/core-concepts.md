# Core Concepts

This page explains the developer-facing concepts that show up across the SDK,
CLI, public RPC, and whitepapers.

## Accounts And Identity

Dytallix uses post-quantum accounts by default.

- public account scheme: `ML-DSA-65`
- address format: Bech32m
- human-readable prefix: `dytallix`
- canonical address prefix: `dytallix1`
- address payload: 32-byte BLAKE3 hash of the public key

In the public SDK and CLI, there is no legacy or hybrid account mode for normal
wallet operation.

The core flow is:

1. generate an ML-DSA-65 keypair
2. derive a D-Addr from the public key
3. sign canonical transaction bytes with that keypair
4. submit the signed transaction to the public node

## Tokens And Denoms

Dytallix uses two named tokens in public developer tooling:

- `DGT`: governance and staking token
- `DRT`: reward token

Micro-denoms:

- `udgt`
- `udrt`

Important current-testnet note:

- the public node currently reports `fee_denom: "udgt"` on `/status`
- several older notes describe `DRT` as the gas token
- integrators should rely on the live public node behavior, which is currently
  `DGT`-denominated fees

## Transactions

The published SDK models the live transaction envelope rather than only a
high-level transfer abstraction.

Important pieces:

- `chain_id`
- `nonce`
- `msgs`
- `fee`
- `memo`

The live SDK currently exposes two message kinds:

- `send`
- `data`

A `send` message contains:

- `from`
- `to`
- `denom`
- `amount`

A `data` message anchors arbitrary text payloads on-chain.

## Nonce Rules

The sender nonce is strictly sequential.

- read the account first
- use the current `nonce`
- submit the next signed transaction with that exact value

The public node increments nonce after the transaction has passed validation and
paid its upfront fee.

## Gas And Fees

The current public node exposes gas schedule fields through `GET /status`.

Fields returned on the public testnet include:

- `min_gas_price`
- `default_gas_limit`
- `default_signed_fee`
- `transfer_base`
- `per_byte`
- `per_additional_signature`
- `per_kv_read`
- `per_kv_write`

The published SDK splits fee estimates into:

- compute gas (`c_gas`)
- bandwidth gas (`b_gas`)

High-level model:

- intrinsic gas covers transfer base, transaction size, and signature overhead
- execution gas covers state reads, writes, and payload work
- total fee is derived from total gas times the public minimum gas price

## Transaction Hashing And Signing

The published SDK:

- serializes transactions into canonical sorted JSON
- hashes the canonical bytes with `SHA3-256`
- signs that digest with ML-DSA-65
- encodes signatures and public keys as base64 in the submit envelope

The public submit payload includes:

- `tx`
- `signature`
- `public_key`
- `algorithm`
- `version`

## Public Interface Layers

Today there are two practical API layers:

### Root RPC

Used by the SDK and by direct integrations:

- `/status`
- `/account/:address`
- `/balance/:address`
- `/block/:id`
- `/blocks`
- `/tx/:hash`
- `/api/blockchain/submit`

### Explorer Read API

Used by the site-hosted explorer page:

- `/api/blockchain/status`
- `/api/blockchain/blocks`
- `/api/blockchain/block/:id`
- `/api/blockchain/transactions`
- `/api/blockchain/transactions/:hash`
- `/api/blockchain/metrics`

## Design Language vs Current Public Behavior

The whitepapers and research notes describe the target protocol design. The
current public node is the implementation developers integrate with today.

When there is a difference, this documentation follows the current public node
first and calls out the mismatch explicitly.
