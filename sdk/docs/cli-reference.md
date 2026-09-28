# CLI Reference

[Docs hub](README.md) | [Getting started](getting-started.md) | [FAQ](faq.md)

Keypair, faucet, transfer, and basic contract lifecycle are available for experimentation on the public testnet. Staking, governance, and some advanced or operator paths are not yet production-complete.

## Install

```bash
cargo install --git https://github.com/DytallixHQ/dytallix-sdk.git dytallix-cli --bin dytallix --features legacy-network
```

`legacy-network` adds the public testnet commands: `init`, `faucet`,
`contract`, `chain`, `node`, `dev` and `legacy`. The default build has only
the consensus-chain commands (`send`, `stake`, `balance`, `governance`,
`ordinary`, `wallet`, `crypto` and `config`):

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
| `balance` | Show an account's DGT and DRT on the pinned chain | `dytallix balance` |
| `send` | Send DGT or DRT on the pinned chain | `dytallix send --to <address> --amount 1.5 --gas-limit <n> --maximum-fee-udrt <n>` |
| `faucet` | Request faucet funds or inspect eligibility | `dytallix faucet status` |
| `stake` | Bond, begin unbonding and claim rewards on the pinned chain | `dytallix stake bond --validator <id> --amount 10 --gas-limit <n> --maximum-fee-udrt <n>` |
| `governance` | Propose, deposit and vote on the pinned chain (ordinary v3) | `dytallix governance vote --proposal-id 7 --choice yes --gas-limit <n> --maximum-fee-udrt <n>` |
| `contract` | Deploy, call, query, and inspect contracts | `dytallix contract info <address>` |
| `node` | Operate or inspect a local node workflow | `dytallix node status` |
| `chain` | Query block, epoch, status, and chain params | `dytallix chain status` |
| `crypto` | Key generation, signing, verification, and keystore inspection | `dytallix crypto keygen` |
| `dev` | Small developer utilities and quick links | `dytallix dev benchmark` |
| `config` | Show, set, reset, and switch CLI config; pin the consensus chain | `dytallix config pin-chain ...` |
| `legacy` | Testnet REST balance, send, stake and governance (`legacy-network` feature) | `dytallix legacy stake status` |

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

### Consensus chain: `balance`, `send`, `stake` and `governance`

These commands use the consensus chain through a CometBFT JSON-RPC endpoint.
Pin the chain first, with the chain ID and genesis digest from a source you
trust (never from the node itself):

```bash
dytallix config pin-chain --endpoint http://127.0.0.1:26657 --network testnet \
  --chain-id <chain-id> --genesis-digest <sha256-of-genesis-hex>
```

`pin-chain` asks the node which chain it reports and refuses a mismatch;
`--no-check` stores the pin without asking.

Each write is one step. The CLI reads the account and fee profile from the
node, refuses them unless the node reports the pinned chain, then prepares,
signs, submits, and waits up to `--wait-seconds` (default 30; 0 returns after
CheckTx) for the committed result. `--gas-limit` and `--maximum-fee-udrt` are
required on every write; the CLI never chooses them. The output shows the
required cap from the fee profile. A dishonest node can make a transaction
fail or cost up to your cap, but cannot change its recipient or amount.

- Signer: `--wallet <name>` or `--key-file <file>`, else the active wallet.
- Account: the address the signing key derives on the pinned chain, or
  `--account <address>` after a key rotation.
- `--expiry-blocks` (default 100) and `--memo` are optional.
- Amounts are tokens with up to six decimal places (1 DRT = 1000000 uDRT).

```bash
dytallix balance
dytallix send --to <address> --amount 1.5 --token drt --gas-limit 10000 --maximum-fee-udrt 20000
dytallix stake bond --validator <validator-id> --amount 10 --gas-limit 10000 --maximum-fee-udrt 20000
dytallix stake unbond --validator <validator-id> --amount 5 --gas-limit 10000 --maximum-fee-udrt 20000
dytallix stake claim --gas-limit 10000 --maximum-fee-udrt 20000
```

- `balance [address]` reads the account's native record. The node's report
  is not yet checked against a trusted application hash (`proof_verified`
  is false).
- `send` to an address with no account creates it and burns the chain's
  account creation fee.
- An account a transfer created has no record yet. Its first transaction is
  signed by the key its address derives from, at nonce zero, and must be an
  ordinary transaction (send or stake), not governance.
- `send` and `stake` report the committed receipt, checked against the
  signed transaction.

### `governance`

Governance uses ordinary-v3 transactions:

- `propose`: one change, `--max-active <n>`, `--min-self-bond-udgt <n>`,
  `--fees <FeeValues JSON>`, `--registry-add <validator-id> --owner <address>`
  or `--registry-remove <validator-id>`. It takes the node's next proposal ID.
- `deposit --proposal-id <id> --amount <DGT>`
- `vote --proposal-id <id> --choice <yes|no|no-with-veto|abstain>`

They run in one step, like `send`:

```bash
dytallix governance vote --proposal-id 7 --choice no-with-veto --gas-limit 10000 --maximum-fee-udrt 20000
```

For offline signing, `prepare propose|deposit|vote` builds a body from
captured views (`--profile`, `--account`, `--context`,
`--governance-profile`, `--expiry-height`, `--gas-limit`,
`--maximum-fee-udrt`, `--output`), `sign` signs it, `inspect` checks it, and
`submit` sends it. `query-profile` captures the governance profile.

Behavior:

- `submit` refreshes the views first; they may be at a later height, but must
  show the same account authority and fee profiles, and a proposal must still
  hold the next proposal ID
- once the chain admits a governance transaction, a failed governance rule
  (for example a proposal ID another proposal took first) is still charged
- CheckTx acceptance is not commitment; the spent account nonce is, since the
  chain writes no governance receipt yet

### `faucet`

```bash
dytallix faucet
dytallix faucet status
```

Current public faucet policy:

- successful requests fund `10 DGT` and `100 DRT`
- the public cooldown is `60` seconds
- the public cap is `20` requests per hour

### `legacy`

The testnet REST commands, built with the `legacy-network` feature. The
consensus chain refuses all of them.

```bash
dytallix legacy balance <daddr>
dytallix legacy send --token dgt <daddr> 25
DYTALLIX_ENDPOINT=http://localhost:3030 dytallix legacy stake delegate <validator> 1000
dytallix legacy stake status
dytallix legacy governance proposals
```

- `legacy send` submits the signed transaction, prints the hash, and waits
  for `/tx/<hash>` to leave `Pending` when the public receipt route is
  already indexing
- `legacy stake status` reads `https://dytallix.com/api/staking/balance/<D-ADDR>`
- `legacy governance proposals` reads `https://dytallix.com/api/governance/proposals`;
  `legacy governance status <id>` filters it
- the CLI consults `GET /api/capabilities` on compatible nodes when deciding
  whether public staking and governance writes should stay blocked
- `legacy stake delegate`, `undelegate` and `claim`, and `legacy governance
  vote` and `propose`, are disabled on the default public website gateway
- write testing for these still requires a local node or direct node endpoint

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
- `pin-chain --endpoint <rpc> --network <mainnet|testnet|development> --chain-id <id> --genesis-digest <hex>`

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
