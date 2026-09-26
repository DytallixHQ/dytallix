# Repository Map

[Docs hub](README.md) | [Build and run](build-and-run.md) | [RPC and API docs](rpc-and-apis.md)

## Workspace Packages

| Path | Cargo package | Purpose |
| --- | --- | --- |
| [`blockchain-core`](../blockchain-core) | `dytallix-node` | Core chain logic, consensus, secrets, risk systems, runtime, and storage |
| [`dytallix-fast-launch/node`](../dytallix-fast-launch/node) | `dytallix-fast-node` | Public RPC node and execution engine |
| [`pqc-crypto`](../pqc-crypto) | `dytallix-pqc` | Post-quantum crypto primitives and CLI tools |
| [`smart-contracts`](../smart-contracts) | `dytallix-contracts` | Contract runtime, bridges, and examples |

## Main Binaries

### `blockchain-core`

- `dytallix-node` from [`blockchain-core/src/main.rs`](../blockchain-core/src/main.rs)
- `bridge-server` from [`blockchain-core/src/bin/bridge-server.rs`](../blockchain-core/src/bin/bridge-server.rs)

Notable source areas:

- [`consensus/`](../blockchain-core/src/consensus)
- [`risk/`](../blockchain-core/src/risk)
- [`runtime/`](../blockchain-core/src/runtime)
- [`secrets/`](../blockchain-core/src/secrets)
- [`wasm/`](../blockchain-core/src/wasm)

### `dytallix-fast-launch/node`

- `dytallix-fast-node` from [`dytallix-fast-launch/node/src/main.rs`](../dytallix-fast-launch/node/src/main.rs)
- `pqc_signer` from [`dytallix-fast-launch/node/src/bin/pqc_signer.rs`](../dytallix-fast-launch/node/src/bin/pqc_signer.rs)
- `txhash` from [`dytallix-fast-launch/node/src/bin/txhash.rs`](../dytallix-fast-launch/node/src/bin/txhash.rs)

Notable source areas:

- [`rpc/`](../dytallix-fast-launch/node/src/rpc)
- [`runtime/`](../dytallix-fast-launch/node/src/runtime)
- [`storage/`](../dytallix-fast-launch/node/src/storage)
- [`mempool/`](../dytallix-fast-launch/node/src/mempool)
- [`crypto/`](../dytallix-fast-launch/node/src/crypto)

### `pqc-crypto`

Library:

- [`pqc-crypto/src/lib.rs`](../pqc-crypto/src/lib.rs)

CLI tools:

- [`keygen`](../pqc-crypto/src/bin/keygen.rs)
- [`keygen_raw`](../pqc-crypto/src/bin/keygen_raw.rs)
- [`pqc_evidence`](../pqc-crypto/src/bin/pqc_evidence.rs)
- [`sign`](../pqc-crypto/src/bin/sign.rs)
- [`verify`](../pqc-crypto/src/bin/verify.rs)

### `smart-contracts`

Library:

- [`smart-contracts/src/lib.rs`](../smart-contracts/src/lib.rs)

Examples and test harness:

- [`smart-contracts/examples/counter`](../smart-contracts/examples/counter)
- [`smart-contracts/test-harness`](../smart-contracts/test-harness)
- [`smart-contracts/tests`](../smart-contracts/tests)

## Existing Deep-Dive Docs

- [Fast node RPC reference](../dytallix-fast-launch/node/README_RPC.md)
- [PQC implementation](../dytallix-fast-launch/node/PQC_IMPLEMENTATION.md)
- [Secrets management](../blockchain-core/SECRETS_README.md)
- [PulseGuard API draft](../blockchain-core/src/risk/pulseguard/api/OPENAPI.md)
