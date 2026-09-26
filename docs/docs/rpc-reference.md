# RPC & Explorer Reference

This page documents the public HTTP interfaces that are actually useful today.

There are two main surfaces:

- root RPC under `https://dytallix.com`
- explorer read APIs under `https://dytallix.com/api/blockchain`

Compatible nodes also expose a machine-readable public contract at
`https://dytallix.com/api/capabilities`.

## Canonical Root RPC

These routes are the most relevant for SDK and direct integration work.

### `GET /status`

Returns health and gas schedule data.

Example:

```bash
curl https://dytallix.com/status
```

Key fields:

- `status`
- `latest_height`
- `syncing`
- `mempool_size`
- `chain_id`
- `gas.version`
- `gas.fee_denom`
- `gas.min_gas_price`
- `gas.default_gas_limit`

Observed notes on April 13, 2026:

- `chain_id` was `dyt-local-1`
- `fee_denom` was `udgt`
- `min_gas_price` was `1000`

### `GET /api/capabilities`

Returns the machine-readable public capability contract for compatible node
deployments.

Example:

```bash
curl https://dytallix.com/api/capabilities
```

Key fields:

- `canonicalStatement`
- `publicNode.contractEndpoint.path`
- `publicNode.disabledPublicWrites`
- `publicNode.directNodeOnlyRoutes`
- `features.stakingWrites`
- `features.governanceWrites`

### `GET /account/:address`

Returns raw balances and nonce.

```bash
curl https://dytallix.com/account/<D-ADDR>
```

Shape:

```json
{
  "address": "dytallix1...",
  "balances": {
    "udgt": 7951000,
    "udrt": 101000000
  },
  "nonce": 1
}
```

### `GET /balance/:address`

Returns formatted per-denom balances and descriptions.

```bash
curl https://dytallix.com/balance/<D-ADDR>
```

The current response includes token metadata such as:

- governance vs reward type
- human-formatted balance
- description text

### `GET /block/:id`

Accepts:

- block height
- block hash
- `latest`

```bash
curl https://dytallix.com/block/latest
```

### `GET /blocks?limit=n`

Returns recent blocks in descending order.

```bash
curl 'https://dytallix.com/blocks?limit=2'
```

### `GET /tx/:hash`

Returns a transaction receipt.

```bash
curl https://dytallix.com/tx/<TX_HASH>
```

Current receipt fields include:

- `tx_hash`
- `status`
- `success`
- `block_height`
- `fee`
- `gas_limit`
- `gas_price`
- `gas_used`
- `gas_refund`
- `from`
- `to`
- `nonce`

## Transaction Submission

### `POST /api/blockchain/submit`

This is the live public transaction submit route used by the published SDK.

Example structure:

```json
{
  "signed_tx": {
    "tx": {
      "chain_id": "dyt-local-1",
      "nonce": 0,
      "msgs": [
        {
          "type": "send",
          "from": "dytallix1...",
          "to": "dytallix1...",
          "denom": "udrt",
          "amount": "1000000"
        }
      ],
      "fee": "2049000",
      "memo": ""
    },
    "signature": "BASE64_SIGNATURE",
    "public_key": "BASE64_PUBLIC_KEY",
    "algorithm": "mldsa65",
    "version": 1
  }
}
```

Example:

```bash
curl -X POST https://dytallix.com/api/blockchain/submit \
  -H 'content-type: application/json' \
  -d @signed-tx.json
```

Important note:

- `GET /v1/transactions` is not the public route to use today
- the website gateway currently serves HTML for `GET /v1/*` reads

## Explorer Read API

The site-hosted explorer uses a parallel read-only API surface.

Explorer page:

```text
https://dytallix.com/build/blockchain
```

Useful endpoints:

- `GET /api/blockchain/status`
- `GET /api/blockchain/blocks?limit=10`
- `GET /api/blockchain/block/:id`
- `GET /api/blockchain/transactions?limit=10`
- `GET /api/blockchain/transactions/:hash`
- `GET /api/blockchain/account/:address`
- `GET /api/blockchain/balance/:address`
- `GET /api/blockchain/api/staking/validators`
- `GET /api/blockchain/metrics`

The explorer-facing endpoints are especially useful if you want:

- recent transaction lists
- recent block lists
- staking validator summaries
- Prometheus-style metrics text

### `GET /api/blockchain/transactions?limit=n`

Example:

```bash
curl 'https://dytallix.com/api/blockchain/transactions?limit=1'
```

Example shape:

```json
{
  "total": 1,
  "transactions": [
    {
      "amount": "1000000",
      "block_height": 264655,
      "denom": "udrt",
      "fee": "2049000",
      "from": "dytallix1...",
      "hash": "0xea54025c...",
      "nonce": 0,
      "status": "confirmed",
      "timestamp": 1775406617,
      "to": "dytallix1..."
    }
  ]
}
```

### `GET /api/blockchain/api/staking/validators`

Example:

```bash
curl https://dytallix.com/api/blockchain/api/staking/validators
```

The current response includes:

- `active_validators`
- `total_validators`
- `reward_index`
- validator list with D-Addr-compatible `address`, `moniker`, `status`, `source`, and `pqc_enabled`

The live public response now reports a single current proposer entry rather than
the older placeholder four-validator shape.

## Faucet API

Canonical base:

```text
https://dytallix.com/api/faucet
```

Routes:

- `GET /api/faucet/status`
- `GET /api/faucet/check/:address`
- `POST /api/faucet/request`

Example status check:

```bash
curl https://dytallix.com/api/faucet/status
```

Example eligibility check:

```bash
curl https://dytallix.com/api/faucet/check/<D-ADDR>
```

Example fund request:

```bash
curl -X POST https://dytallix.com/api/faucet/request \
  -H 'content-type: application/json' \
  -d '{
    "address": "<D-ADDR>",
    "dgt_amount": 10,
    "drt_amount": 100
  }'
```
