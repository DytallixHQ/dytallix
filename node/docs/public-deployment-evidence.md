# Public Deployment Evidence

[Docs hub](README.md) | [Build and run](build-and-run.md) | [Deployment separation audit](deployment-separation-audit.md)

## What Publicly Verifiable Evidence Exists Today

The live public node currently exposes:

- `GET https://dytallix.com/status`
- `GET https://dytallix.com/api/capabilities`
- `GET https://dytallix.com/api/blockchain/api/staking/validators`

Those endpoints are enough to verify that the live public behavior matches the
published capability contract in this repo on April 14, 2026:

- the live node advertises `/api/capabilities`
- public staking and governance writes are marked hidden in the capability contract
- validator discovery no longer returns placeholder IDs like `validator1`

## Clean-Checkout Production Provenance

Production provenance is now evidenced from the real host.

Verified operator-side facts on April 14, 2026:

- active PM2 script path: `/opt/dytallix-node/target/release/dytallix-fast-node`
- process working directory: `/opt/dytallix-node`
- deployed commit: `0c076f35b5c19baf9d31a04ee03c230ddef9a380`
- git status in `/opt/dytallix-node`: clean
- build command used from the repo root: `cargo build -p dytallix-fast-node --bin dytallix-fast-node --release --locked`
- production env preserved from `/etc/dytallix/dytallix-fast-node.env`
- PM2 restart completed after cloning a fresh public checkout into `/opt/dytallix-node`

That means the public node behavior and the deployed production binary are now
both attributable to a clean checkout of this public repository.

## Operator Audit Snapshot

The earlier April 13, 2026 audit captured the pre-cutover state and exposed the
host-side drift that had to be removed.

- old PM2 binary path: `/opt/dytallix-node/dytallix-fast-launch/node/target/release/dytallix-fast-node`
- old deployed commit: `00b1ebbffc2c2f261de06c03c662957ca52cf2f9`
- dirty files existed in that older live checkout

That dirty tree was backed up and replaced with a fresh public clone at
`/opt/dytallix-node`, after which PM2 was restarted against the clean release
binary path documented above.

## Public Verification Command

Run this from the repo root:

```bash
python3 scripts/check_live_public_deployment.py
```