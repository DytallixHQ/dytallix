# Contract Quickstart

This is the canonical path from zero to a first contract build on Dytallix,
followed by deployment against the public testnet gateway.

It uses:

- the public `dytallix` CLI
- the live testnet faucet and transfer path
- the minimal deployable WASM example published in `dytallix-sdk`

## What You Need

- Rust and Cargo
- network access to `https://dytallix.com`
- the `wasm32-unknown-unknown` Rust target

## 1. Install The CLI

```bash
cargo install --git https://github.com/DytallixHQ/dytallix-sdk.git dytallix-cli --bin dytallix
```

## 2. Create And Fund A Wallet

```bash
dytallix init
```

This creates an ML-DSA-65 keypair, writes `~/.dytallix/keystore.json`, and
requests faucet funds for the active address.

Confirm the wallet state:

```bash
dytallix wallet info
dytallix balance
```

## 3. Clone The SDK Example Contract

```bash
git clone https://github.com/DytallixHQ/dytallix-sdk.git
cd dytallix-sdk
rustup target add wasm32-unknown-unknown
```

The canonical quickstart contract lives at:

```text
examples/contracts/minimal_contract
```

## 4. Build A Deployable WASM Artifact

```bash
cargo build \
  --manifest-path examples/contracts/minimal_contract/Cargo.toml \
  --target wasm32-unknown-unknown \
  --release
```

The resulting artifact is:

```text
examples/contracts/minimal_contract/target/wasm32-unknown-unknown/release/minimal_contract.wasm
```

## 5. Optional: Point The CLI At Another Node

The default testnet profile already targets `https://dytallix.com`, and the
public gateway now accepts `POST /contracts/deploy` and
`POST /contracts/call`.

If you want to test against a direct node endpoint or a local node instead,
override the endpoint:

```bash
dytallix config set endpoint http://localhost:3030
```

Or for a one-off shell session:

```bash
export DYTALLIX_ENDPOINT=http://localhost:3030
```

## 6. Deploy It

```bash
dytallix contract deploy \
  examples/contracts/minimal_contract/target/wasm32-unknown-unknown/release/minimal_contract.wasm
```

Expected result:

- a transaction hash
- a predicted contract address
- a success message indicating the deployment transaction was submitted

## 7. Inspect The Contract Lifecycle

Useful follow-up commands:

```bash
dytallix contract info <CONTRACT_ADDRESS>
dytallix contract query <CONTRACT_ADDRESS> ping
dytallix contract call <CONTRACT_ADDRESS> ping
dytallix contract events <CONTRACT_ADDRESS>
```

If you set an endpoint override, it applies to `contract deploy`, `info`,
`query`, `call`, and `events` without changing your faucet profile.

Useful public pages:

- Explorer page: `https://dytallix.com/build/blockchain`
- Docs: `https://dytallix.com/docs`

## Public Rollout Status

The current node and CLI support contract routes at:

- `POST /contracts/deploy`
- `POST /contracts/call`
- `GET /api/contracts/<address>`
- `GET /api/contracts/<address>/query/<method>`
- `GET /api/contracts/<address>/events`

These routes were verified live through `https://dytallix.com` on April 16,
2026. A direct node endpoint or local node is still useful for debugging,
local testing, or custom infrastructure.

## Related Repositories

- SDK and CLI: https://github.com/DytallixHQ/dytallix-sdk
- Reference contracts: https://github.com/DytallixHQ/dytallix-contracts
