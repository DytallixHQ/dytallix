# Build And Run

[Docs hub](README.md) | [Repository map](repository-map.md) | [FAQ](faq.md)

## Workspace Build

Build everything in the repository:

```bash
cargo build --workspace --locked
```

Format the workspace:

```bash
cargo fmt --all
```

Run tests:

```bash
cargo test --workspace --locked
```

### Build modes

The default build is the selected mainnet consensus application
(`dytallix-fast-node` feature `pqc-consensus`). `cargo build --workspace` and
`cargo test --workspace` use it.

The archived legacy node (HTTP/WebSocket RPC, bridge, contracts and the retired
block timer used by the current testnet) is a separate build. Enable it
explicitly:

```bash
cargo test -p dytallix-fast-node --no-default-features --features pqc-fips204,metrics,oracle,contracts,legacy-services,mldsa87-development --locked
```

Legacy-only test targets declare `required-features`, so the default build
skips them.

For focused changes in this large workspace, it is reasonable to run the most
relevant crate or test target and document that scope in the PR.

## Public RPC Node

Build the public RPC node:

```bash
cargo build -p dytallix-fast-node --bin dytallix-fast-node --release --no-default-features --features pqc-fips204,metrics,oracle,contracts,legacy-services,mldsa87-development --locked
```

Run it:

```bash
cargo run -p dytallix-fast-node --bin dytallix-fast-node --release --no-default-features --features pqc-fips204,metrics,oracle,contracts,legacy-services,mldsa87-development
```

The deeper runtime and endpoint notes live in
[Fast node RPC reference](../dytallix-fast-launch/node/README_RPC.md).

### Clean production separation

For production-style deployment, keep the node under a Dytallix-owned root such
as `/opt/dytallix-node` instead of launching it from a legacy PM2 tree.

Reference artifacts in this repository:

- PM2: [`deploy/pm2/dytallix-fast-node.ecosystem.cjs`](../deploy/pm2/dytallix-fast-node.ecosystem.cjs)
- systemd: [`deploy/systemd/dytallix-fast-node.service`](../deploy/systemd/dytallix-fast-node.service)

Recommended install flow:

```bash
git clone https://github.com/DytallixHQ/dytallix-node.git /opt/dytallix-node
cd /opt/dytallix-node
cargo build -p dytallix-fast-node --bin dytallix-fast-node --release --no-default-features --features pqc-fips204,metrics,oracle,contracts,legacy-services,mldsa87-development --locked
mkdir -p /etc/dytallix /var/log/dytallix
```

Populate `/etc/dytallix/dytallix-fast-node.env` with the runtime environment you
intend to keep. Do not carry forward temporary incident workarounds such as
`DYTALLIX_DEFAULT_GAS_LIMIT=8000` unless you have explicitly revalidated that
override against current source.

Choose one process manager path and keep it canonical. Do not run the same node
from both PM2 and systemd.

For `systemd`, first complete the [restricted-account migration](../deploy/systemd/README.md).
Preserve the existing validator identity and database before changing the service account.
The unit requires an explicit environment file and existing key configuration.

For `systemd`:

```bash
cp deploy/systemd/dytallix-fast-node.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable dytallix-fast-node
systemctl restart dytallix-fast-node
systemctl status dytallix-fast-node
```

For `pm2`:

```bash
pm2 start deploy/pm2/dytallix-fast-node.ecosystem.cjs
pm2 save
pm2 status dytallix-fast-node
```

Smoke-check the deployed public contract after restart:

```bash
curl --fail http://127.0.0.1:3030/status
curl --fail http://127.0.0.1:3030/api/capabilities
```

If the rebuilt binary is not the process serving those endpoints, the deployment
is still drifting from source.

## Core Node

Build the core chain binary:

```bash
cargo build -p dytallix-node --bin dytallix-node --release --locked
```

Build the bridge server:

```bash
cargo build -p dytallix-node --bin bridge-server --release --locked
```

## PQC Utilities

Build the PQC CLI tools:

```bash
cargo build -p dytallix-pqc --release --locked
```

The package provides CLI binaries for key generation, raw key export, signing,
verification, and evidence generation:

- [`keygen`](../pqc-crypto/src/bin/keygen.rs)
- [`keygen_raw`](../pqc-crypto/src/bin/keygen_raw.rs)
- [`pqc_evidence`](../pqc-crypto/src/bin/pqc_evidence.rs)
- [`sign`](../pqc-crypto/src/bin/sign.rs)
- [`verify`](../pqc-crypto/src/bin/verify.rs)

## Smart Contracts

Build the contracts package:

```bash
cargo build -p dytallix-contracts --release --locked
```

See the example contract in
[`smart-contracts/examples/counter`](../smart-contracts/examples/counter).

## Operational References

- [Fast node RPC reference](../dytallix-fast-launch/node/README_RPC.md)
- [PQC implementation](../dytallix-fast-launch/node/PQC_IMPLEMENTATION.md)
- [Secrets management](../blockchain-core/SECRETS_README.md)
- [Deployment separation audit](deployment-separation-audit.md)
