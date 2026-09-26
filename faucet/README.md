# Dytallix Faucet

Public faucet backend source for the Dytallix testnet.

This repository now contains the Node.js backend that serves the live token
distribution flow behind `https://dytallix.com/api/faucet`.

Keypair, faucet, transfer, and basic contract lifecycle are available for experimentation on the public testnet. Staking, governance, and some advanced or operator paths are not yet production-complete.

## Repository Role

- Role: canonical public faucet backend source
- Backend entrypoint: `src/server.js`
- Runtime controller: `src/controllers/faucetController-dual.js`
- Compatibility edge config: `deploy/nginx/faucet-compat.conf`

## Public Surface

The live public faucet currently exposes:

- `POST /api/faucet`
- `POST /api/faucet/request`
- `GET /api/faucet/status`
- `GET /api/faucet/check/:address`

The backend service in this repository provides the token send flow and internal
status routes. The public `GET /api/faucet/status` and
`GET /api/faucet/check/:address` compatibility endpoints are currently provided
at the nginx edge, and the matching route configuration is included in this
repo under `deploy/nginx/faucet-compat.conf`.

## Current Live Policy

- `10 DGT` per successful request
- `100 DRT` per successful request
- `60` second cooldown
- `20` requests per hour

## Quick Start

Install dependencies:

```bash
npm ci
```

Prepare a local environment file:

```bash
cp .env.example .env
```

Start the backend:

```bash
npm start
```

## Local Checks

```bash
curl http://127.0.0.1:3001/health
curl http://127.0.0.1:3001/api/info
curl http://127.0.0.1:3001/api/status
npm test
```

## Configuration

The repo ships `.env.example` with the live public-policy defaults:

- `CHAIN_ID=dyt-local-1`
- `RPC_ENDPOINT=http://127.0.0.1:3030`
- `DGT_FAUCET_AMOUNT=10000000udgt`
- `DRT_FAUCET_AMOUNT=100000000udrt`
- `RATE_LIMIT_MAX_REQUESTS=20`
- `IP_COOLDOWN_MS=60000`

## Notes

- This repository replaces the earlier docs-only faucet boundary for the
  backend source itself.
- The live site still fronts the backend with nginx compatibility routes, so
  backend code and edge routing both matter for full public-surface parity.

## Security Reporting

Do not report vulnerabilities in public issues. Use GitHub Security Advisories
for this repository when available, or email hello@dytallix.com.
## Runtime and dependency verification

CI and the container use Node.js 24.20.0. The container pins the base-image
digest. Run `npm ci` and `npm test -- --runInBand` before building the image.
The health command uses Node.js and does not require curl in the image.

The Express dependency overrides `qs` to 6.16.0. Express 4.22.2 otherwise
selects an older `qs` range with reported advisories. Remove this override only
when the selected Express dependency tree resolves a reviewed fixed version.
Check the complete lockfile with `npm audit`; a zero result is not a source
security audit. Review supported-runtime and dependency updates before each
release.
