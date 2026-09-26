# Deployment Separation Audit

[Docs hub](README.md) | [Repository map](repository-map.md) | [Build and run](build-and-run.md)

## Summary

The current production-style server layout mixes Dytallix and legacy deployment
assets. The evidence gathered during live debugging shows this is an
operational boundary problem, not a `dytallix-fast-node` or `dytallix-node`
source-architecture dependency.

## Findings

### 1. Node source ownership is Dytallix-only

- [`dytallix-fast-launch/node`](../dytallix-fast-launch/node) is the public RPC
  node package.
- [`blockchain-core`](../blockchain-core) is the core chain package.
- The Rust fast-node sources do not contain external-product references.

### 2. Production deployment currently runs the node from a legacy PM2 root

Observed on the live server:

- active PM2 ecosystem file under a legacy `/opt` root
- active node script path: `./dytallix-fast-launch/node/target/release/dytallix-fast-node`
- active node port: `3030`

This means the Dytallix node is being launched from a legacy-managed PM2
application root even though the binary itself is Dytallix-owned.

### 3. Dytallix also has its own separate deployment tree on the server

Observed on the live server:

- `/opt/dytallix-fast-launch`
- `/opt/dytallix-fast-launch/ecosystem.config.cjs`

That separate config expects the blockchain node to be managed outside PM2 and
describes `3030` as the blockchain node port. This confirms the server has two
different operational models present at once.

### 4. Legacy-named side services still exist inside the Dytallix fast-launch tree

Observed on the live server:

- `/opt/dytallix-fast-launch/services/legacy-wallet-api`
- legacy service name `legacy-wallet-api`
- configured side-service port `3031`

This is naming and service-boundary residue. It is adjacent to the node, but it
is not evidence that the Rust node package depends on external legacy logic.

Follow-up inspection showed:

- the service package describes itself as a backend for encrypted asset storage
  and proof anchoring
- its own server implementation exposes upload, asset retrieval, registration,
  and verification flows
- current first-party app source trees did not show active callers to `3031`
  or `legacy-wallet-api`
- the remaining references are confined to the service itself plus deployment
  scripts, PM2 config, archived docs, and old readiness reports

That makes `legacy-wallet-api` a separate legacy sidecar, not a hidden runtime
dependency of the Dytallix node.

### 5. One repo-side branding residue was present in the published source snapshot

The published snapshot included an unreferenced sidecar UI asset under
`blockchain-core/src/api/`. It was not referenced elsewhere in the published
tree and did not represent a wired Rust or RPC dependency.

## Classification

### Rename / separation issue

- Running `dytallix-fast-node` from a legacy PM2 ecosystem root
- Keeping Dytallix binaries under a legacy deployment tree
- Leaving legacy sidecar service naming inside the Dytallix deployment bundle
  after the node has already been separated

### Cleanup required, but not a node refactor

- Split, archive, or remove leftover legacy-branded side services unless
  product requirements prove they are part of the Dytallix surface
- Audit whether any remaining sidecar UI assets belong in this repository at
  all

### Not a simple rename by default

- Do not automatically rename legacy side services into Dytallix service names
  just because they sit under `/opt/dytallix-fast-launch`
- The current evidence fits a legacy adjacent service better than a misnamed
  Dytallix component
- A rename should happen only if a product owner confirms the encrypted-asset
  storage and proof-anchoring workflow is now part of Dytallix

### Not supported by current evidence

- A claim that `dytallix-fast-node` or `dytallix-node` has a source-level
  runtime dependency on an external legacy stack

## Recommended Remediation

1. Move the node to a Dytallix-owned deployment root and process manager entry.
2. Deploy from a clean `dytallix-node` checkout instead of an unpacked subtree.
3. Remove the temporary `DYTALLIX_DEFAULT_GAS_LIMIT=8000` workaround after the
   clean deployment is confirmed.
4. Remove legacy wallet side-service entries from active Dytallix deployment
   manifests unless that service is intentionally being operated.
5. If the service is still needed, move it under its own product-specific
   deployment root and ownership boundary before any rename decision.
6. Rename the legacy side-service package only if product ownership confirms it
   is part of the Dytallix surface.
7. Audit and remove unreferenced sidecar UI assets if they are not
   intentionally shared.

## Reference Artifacts

This repository now includes clean Dytallix-owned deployment templates:

- PM2: [`../deploy/pm2/dytallix-fast-node.ecosystem.cjs`](../deploy/pm2/dytallix-fast-node.ecosystem.cjs)
- systemd: [`../deploy/systemd/dytallix-fast-node.service`](../deploy/systemd/dytallix-fast-node.service)

These templates intentionally scope the node to `/opt/dytallix-node` and do not
reference legacy-owned paths or service names.

## Decision

Treat this as a deployment separation and legacy side-service cleanup effort.

Do not treat it as evidence of a required Rust node refactor unless new source
references are discovered.
