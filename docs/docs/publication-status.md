# Publication Status

Keypair, faucet, transfer, and basic contract lifecycle are available for experimentation on the public testnet. Staking, governance, and some advanced or operator paths are not yet production-complete.

This page explains what each public DytallixHQ repo is for and where public
source currently lives.

## Repository Roles

| Repository | Role | Current publication state |
| --- | --- | --- |
| `dytallix-sdk` | Public SDK and CLI source | Canonical public client source |
| `dytallix-node` | Public node and runtime source | Canonical public node/backend source with clean-checkout production provenance |
| `dytallix-faucet` | Public faucet backend source | Canonical public faucet backend source with edge compatibility config |
| `dytallix-explorer` | Explorer surface description | Docs-only explorer surface repo for the hosted `dytallix.com/build/blockchain` surface |
| `dytallix-docs` | Canonical public documentation | Docs source only, not the live website frontend source |

## Important Boundaries

- The live website frontend remains a hosted public surface rather than a separate public source repo.
- The live explorer frontend remains documented in `dytallix-explorer`, but its hosted explorer frontend source is not currently published as a separate public repo.
- The live faucet backend source is now published in the public `dytallix-faucet` repo.
- The public `GET /api/faucet/status` and `GET /api/faucet/check/:address` compatibility endpoints are currently provided at the nginx edge and documented in `dytallix-faucet/deploy/nginx/faucet-compat.conf`.
- Production node provenance is now evidenced from a clean public checkout rooted at `/opt/dytallix-node`.

## What Is Publicly Verifiable Today

- The public node serves `GET /api/capabilities`, which exposes a machine-readable contract for the supported public surface.
- The SDK and CLI consume that contract or an embedded fallback manifest.
- The docs, SDK, node, faucet, and explorer repos can be aligned against live endpoint behavior and the currently published source boundaries.

## What Is Still Missing

- a public website or explorer frontend source repo if source-backed publication is required again
- edge and backend parity work whenever the public faucet compatibility routes change
- continued publication of live deployment evidence as the node moves to future commits

For the current public node and faucet backend, the source-backed publication gap is closed. The hosted website and explorer frontend remain public runtime surfaces rather than published frontend source repos.