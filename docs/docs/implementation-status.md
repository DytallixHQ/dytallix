# Implementation Status

This page records the public behavior that was verified on April 13, 2026 and
the practical limitations that still matter for public integrations.

## Treat These As Canonical Today

For developers building against the public testnet, the following should be
treated as canonical:

- root RPC reads under `https://dytallix.com`
- explorer reads under `https://dytallix.com/api/blockchain`
- faucet endpoints under `https://dytallix.com/api/faucet`
- signed transaction submission through
  `POST https://dytallix.com/api/blockchain/submit`
- machine-readable capability discovery through
  `GET https://dytallix.com/api/capabilities`
- ML-DSA-65 addresses and signatures for public Dytallix accounts

## Verified Public Behavior

The following were checked directly against the live gateway.

### Root RPC

- `GET /status` returns chain health, height, gas schedule, and timestamp
- `GET /api/capabilities` returns the canonical public capability contract
- `GET /account/:address` returns address, raw balances, and nonce
- `GET /balance/:address` returns formatted per-denom balances
- `GET /block/:id` returns block details
- `GET /blocks?limit=n` returns recent blocks
- `GET /tx/:hash` returns a transaction receipt including gas fields

### Submit Path

`POST /api/blockchain/submit` is live and expects a JSON body containing a
`signed_tx` object. A malformed body returns HTTP `422`, which confirms the
route is active and validating request shape.

### Faucet

- `GET /api/faucet/status`
- `GET /api/faucet/check/:address`
- `POST /api/faucet/request`

The faucet reported these limits on April 13, 2026:

- `10 DGT` per request
- `100 DRT` per request
- `60` second cooldown
- `20` requests per hour

### End-To-End SDK Check

The published SDK example `first-transaction` was run successfully against the
public testnet on April 6, 2026. The flow:

- generated an ML-DSA-65 keypair
- derived a D-Addr
- funded the address through the faucet
- estimated fees
- submitted a signed transaction
- received a confirmed receipt

## Known Mismatches

### Fee Token Language

Several older docs and adjacent notes describe `DRT` as the fee token. The
current public node and live `/status` endpoint report:

- `fee_denom: "udgt"`
- `min_gas_price: 1000`

The published SDK snapshot also formats fee estimates in `DGT`.

Practical guidance:

- use `DGT` as the current public testnet fee token
- keep `DRT` documented as the reward token and as part of the long-range
  dual-token design language

### Public `GET /v1/*` JSON Routes

The public site gateway does not currently expose `GET /v1/*` JSON responses as
developer-facing read APIs. Requests such as `GET /v1/transactions` currently
fall through to the website HTML shell.

Practical guidance:

- use root RPC paths and `/api/blockchain/*` read paths today
- do not depend on public `GET /v1/*` routes unless the gateway is updated

### Advanced Reads Still Need A Direct Node Endpoint

The public SDK and CLI now align on the live public hosts and the `3030` local
port. The remaining mismatch is narrower:

- validator and delegation reads still require a direct node endpoint
- contract lifecycle reads and writes are implemented on the current node and
  CLI, but the public website gateway still does not forward the contract write
  routes and can still lag the read routes
- the public website gateway remains centered on root RPC reads and selected
  `/api/*` surfaces

### Explorer Source Boundary And Faucet Edge Compatibility

The public `dytallix-explorer` repository remains a docs-only surface map. The hosted explorer frontend source is not currently published as a separate public repo.

The public `dytallix-faucet` repository now contains the live backend source,
but the public `GET /api/faucet/status` and `GET /api/faucet/check/:address`
responses are still served by the nginx edge rather than the backend process
itself. The matching compatibility config is published in that repo.

### Block Interval Default

The older RPC README says `DYT_BLOCK_INTERVAL_MS` defaults to `2000`. The
published node `main.rs` currently defaults it to `15000`.

If you are starting from source, trust `main.rs`.

## Recommendation

For future doc maintenance, use this priority order:

1. `public-surface.json` in this repository
2. live public gateway behavior
3. `dytallix-sdk`
4. `dytallix-node`
5. supporting public service repositories such as explorer docs and faucet deployment compatibility config
