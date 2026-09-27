# CLI Reference

[Docs hub](README.md) | [Getting started](getting-started.md) | [FAQ](faq.md)

Keypair, faucet, transfer, and basic contract lifecycle are available for experimentation on the public testnet. Staking, governance, and some advanced or operator paths are not yet production-complete.

## Install

```bash
cargo install --git https://github.com/DytallixHQ/dytallix-sdk.git dytallix-cli --bin dytallix
```

Global help:

```bash
dytallix --help
```

## Local State

- Keystore: `~/.dytallix/keystore.json`
- Config: `~/.dytallix/config.json`

## Top-Level Commands

| Command | Purpose | Example |
| --- | --- | --- |
| `init` | Create a wallet, save it, and request faucet funds | `dytallix init` |
| `wallet` | Create, import, export, switch, list, rotate, and inspect wallets | `dytallix wallet info` |
| `balance` | Show DGT and DRT balances | `dytallix balance` |
| `send` | Send DGT or DRT | `dytallix send <daddr> 100` |
| `faucet` | Request faucet funds or inspect eligibility | `dytallix faucet status` |
| `stake` | View staking state publicly, or use direct-node staking writes | `dytallix stake status` |
| `governance` | Consensus-chain governance transactions (ordinary v3); legacy testnet reads under `legacy` | `dytallix governance query-profile --endpoint <rpc> --output governance.json` |
| `contract` | Deploy, call, query, and inspect contracts | `dytallix contract info <address>` |
| `node` | Operate or inspect a local node workflow | `dytallix node status` |
| `chain` | Query block, epoch, status, and chain params | `dytallix chain status` |
| `crypto` | Key generation, signing, verification, and keystore inspection | `dytallix crypto keygen` |
| `dev` | Small developer utilities and quick links | `dytallix dev benchmark` |
| `config` | Show, set, reset, and switch CLI config | `dytallix config network testnet` |

## Command Groups

### `init`

Bootstraps the default testnet developer flow:

```bash
dytallix init
```

This command:

- generates an ML-DSA-65 keypair
- derives a D-Addr
- writes the keystore
- submits a faucet request
- waits for DGT and DRT to appear

For Milestone 2, create a separate recipient wallet after `init` and send to
that address rather than self-sending the funded default wallet.

### `wallet`

Subcommands:

- `create [--name NAME]`
- `import --key-file PATH [--name NAME]`
- `export --output PATH`
- `list`
- `switch NAME`
- `rotate`
- `info`

Examples:

```bash
dytallix wallet create --name default
dytallix wallet list
dytallix wallet info
```

### `balance`, `send`, and `faucet`

Examples:

```bash
dytallix balance
dytallix balance <daddr>
dytallix send --token dgt <daddr> 25
dytallix faucet
dytallix faucet status
```

Current public faucet policy:

- successful requests fund `10 DGT` and `100 DRT`
- the public cooldown is `60` seconds
- the public cap is `20` requests per hour
- `send` submits the signed transaction, prints the hash, and waits for
  `/tx/<hash>` to leave `Pending` when the public receipt route is already
  indexing

### `stake`

Subcommands:

- `delegate <validator> <amount>`
- `undelegate <validator> <amount>`
- `claim`
- `status`

Examples:

```bash
DYTALLIX_ENDPOINT=http://localhost:3030 dytallix stake delegate <validator> 1000
dytallix stake status
```

Current public behavior:

- `status` reads `https://dytallix.com/api/staking/balance/<D-ADDR>`
- the CLI consults `GET /api/capabilities` on compatible nodes when deciding
  whether public staking writes should stay blocked
- `delegate`, `undelegate`, and `claim` are disabled on the default public website gateway
- write testing for staking still requires a local node or direct node endpoint

### `governance`

Governance on the consensus chain uses ordinary-v3 transactions sent through
an explicit CometBFT JSON-RPC endpoint. Like `dytallix ordinary`, it works
from captured views and an explicit signing context, and prepares and signs
offline.

Subcommands:

- `query-profile --endpoint <rpc> --output <file>`: the committed governance
  fee profile and the next proposal ID
- `propose`: one change, `--max-active <n>`, `--min-self-bond-udgt <n>`,
  `--fees <FeeValues JSON>`, `--registry-add <validator-id> --owner <address>`
  or `--registry-remove <validator-id>`; it takes the captured next proposal ID
- `deposit --proposal-id <id> --amount-udgt <n>`
- `vote --proposal-id <id> --choice <yes|no|no-with-veto|abstain>`
- `sign`, `inspect` and `submit`, as for `dytallix ordinary`

`propose`, `deposit` and `vote` take `--profile`, `--account`, `--context` and
`--governance-profile` (captured views), plus `--expiry-height`,
`--gas-limit`, `--maximum-fee-udrt`, `--output` and an optional `--memo`.

Example:

```bash
dytallix governance vote --profile profile.json --account account.json \
  --context context.json --governance-profile governance.json \
  --proposal-id 7 --choice no-with-veto --expiry-height 1200 \
  --gas-limit 600 --maximum-fee-udrt 1200 --output vote.json
```

Behavior:

- `submit` refreshes the views first; they may be at a later height, but must
  show the same account authority and fee profiles, and a proposal must still
  hold the next proposal ID
- once the chain admits a governance transaction, a failed governance rule
  (for example a proposal ID another proposal took first) is still charged
- CheckTx acceptance is not commitment; the spent account nonce is

`legacy` holds the testnet REST commands, built with the `legacy-network`
feature: `legacy proposals`, `legacy status <id>`,
`legacy vote <id> <yes|no|abstain>` and `legacy propose`. The consensus chain
refuses these requests.

- `legacy proposals` reads `https://dytallix.com/api/governance/proposals`
- `legacy status <id>` filters the public proposals list and prints the matching item
- the CLI consults `GET /api/capabilities` on compatible nodes when deciding
  whether public governance writes should stay blocked
- `legacy vote` and `legacy propose` are disabled on the default public website gateway

### `contract`

Subcommands:

- `deploy <wasm-file>`
- `call <address> <method> [args...]`
- `query <address> <method> [args...]`
- `info <address>`
- `events <address>`

Examples:

```bash
dytallix contract deploy ./my_contract.wasm
dytallix contract query <contract> get_count
```

Current public behavior:

- `deploy` posts WASM bytes to `/contracts/deploy` on the active endpoint
- `deploy` polls `/tx/<hash>` and `/api/contracts/<address>` after submission and prints a confirmed state as soon as one of those public routes is indexed
- `deploy` prints `dytallix contract info <address>` as the canonical contract verification path on the public gateway when `/tx/<hash>` lags
- `call` posts method execution requests to `https://dytallix.com/contracts/call`
- `info <address>` reads `https://dytallix.com/api/contracts/<address>`
- `query` reads `https://dytallix.com/api/contracts/<address>/query/<method>`
- `events` reads `https://dytallix.com/api/contracts/<address>/events`
- for a direct node endpoint or a local node, set `DYTALLIX_ENDPOINT` or run
  `dytallix config set endpoint http://localhost:3030`

Recommended verification flow after deploy:

```bash
dytallix contract deploy ./my_contract.wasm
dytallix contract info <contract-address>
```

### `chain`

Subcommands:

- `status`
- `block <number|hash|latest|finalized>`
- `epoch`
- `capabilities [--require-live]`
- `params`

Examples:

```bash
dytallix chain status
dytallix chain block latest
dytallix chain capabilities
dytallix chain capabilities --require-live
```

Current public behavior:

- `status`, `block`, and `epoch` use public root RPC reads
- `capabilities` prints the runtime contract from `/api/capabilities` when a compatible node exposes it, or the SDK embedded fallback when it does not
- `capabilities` prints a `Source:` line so operators can tell whether the document came from a live node or the SDK fallback
- `capabilities --require-live` fails closed when the runtime endpoint is unavailable instead of silently using the fallback
- `scripts/public_smoke.sh capabilities-require-live` is the CI-friendly smoke path for a compatible node that should already expose live capabilities
- `params` derives the public chain ID and gas schedule from `/status`

### `node`

Subcommands:

- `start`
- `stop`
- `status`
- `peers`
- `logs`

The `start` and `stop` commands look for helper scripts such as
`start-local.sh` and `stop-local.sh` (or `scripts/start-local.sh` and
`scripts/stop-local.sh`) relative to the current directory.

Current public behavior:

- `status` uses the local node profile on `http://localhost:3030`
- `peers` reads the local-only `/peers` route directly from
  `http://localhost:3030/peers`

### `crypto`

Subcommands:

- `keygen [--scheme ml-dsa-65|slh-dsa]`
- `sign <message>`
- `verify <message> <signature> <pubkey>`
- `address <pubkey>`
- `inspect <keystore-file>`

Examples:

```bash
dytallix crypto keygen
dytallix crypto sign "hello dytallix"
```

### `dev`

Subcommands:

- `faucet-server`
- `explorer`
- `docs`
- `discord`
- `github`
- `decode <hex>`
- `encode <text>`
- `simulate-tx <address> <amount>`
- `benchmark`

### `config`

Subcommands:

- `show`
- `set <key> <value>`
- `network <testnet|local>`
- `reset`

Examples:

```bash
dytallix config show
dytallix config network local
```

## Network Profiles

The CLI resolves endpoints from the active network profile:

- `testnet` -> `https://dytallix.com`
- `local` -> `http://localhost:3030`

The public CLI currently exposes only `testnet` and `local` through
`dytallix config network`.

For direct-node testing, contract lifecycle reads, or a custom RPC base, you
can override the active profile endpoint:

```bash
dytallix config set endpoint http://localhost:3030
```

Or for a one-off shell session:

```bash
export DYTALLIX_ENDPOINT=http://localhost:3030
```

For faucet behavior and other operational notes, see [Core concepts](core-concepts.md)
and [FAQ](faq.md).

## Production cryptographic profile

Wallet creation and legacy transaction signing require exact ML-DSA-65.
The `crypto verify` command returns failure for invalid signatures.
The `crypto keygen --scheme slh-dsa` command fails because root authorization
uses a separate FIPS 205 component. The SDK legacy SPHINCS+ backend is not
FIPS 205. A 48-byte public key does not identify the signing standard.
See [PQC profile](pqc-profile.md) for scope and launch gates.
