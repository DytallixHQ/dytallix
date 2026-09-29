# CLI Reference

[Docs hub](README.md) | [Getting started](getting-started.md) | [FAQ](faq.md)

This is the mainnet candidate CLI for the consensus chain. It contains no
public testnet client.

## Install

```bash
cargo install --git https://github.com/DytallixHQ/dytallix-sdk.git dytallix-cli --bin dytallix
```

Global help:

```bash
dytallix --help
```

## Local State

- Keystore: `~/.dytallix/keystore.json`, written owner-only (mode 0600).
  Version 2 encrypts each private key with AES-256-GCM under an Argon2id key
  from your passphrase (typed without echo, or read from the owner-only file
  named by `DYTALLIX_KEYSTORE_PASSPHRASE_FILE`). A version 1 file holds
  plaintext keys; `dytallix wallet migrate` encrypts it.
- Pinned chain: `~/.dytallix/chain.json` (`dytallix config pin-chain`)

Each file carries a `version`; a file written before versions were added
reads as version 1. Every JSON object the consensus commands print,
including errors, carries `output_version`.

## Top-Level Commands

| Command | Purpose | Example |
| --- | --- | --- |
| `wallet` | Create, import, export, switch, list, rotate, inspect, and migrate wallets | `dytallix wallet info` |
| `balance` | Show an account's DGT and DRT on the pinned chain | `dytallix balance` |
| `send` | Send DGT or DRT on the pinned chain | `dytallix send --to <address> --amount 1.5 --gas-limit <n> --maximum-fee-udrt <n>` |
| `stake` | Bond, begin unbonding and claim rewards on the pinned chain | `dytallix stake bond --validator <id> --amount 10 --gas-limit <n> --maximum-fee-udrt <n>` |
| `governance` | Propose, deposit and vote on the pinned chain (ordinary v3) | `dytallix governance vote --proposal-id 7 --choice yes --gas-limit <n> --maximum-fee-udrt <n>` |
| `ordinary` | Prepare, sign, and submit ordinary-v2 transactions with explicit context ([ordinary CLI](ordinary-cli.md)) | `dytallix ordinary query-profile --endpoint http://127.0.0.1:26657 --output profile.json` |
| `recovery` | Account recovery: prepare, sign offline per party, assemble, sponsor, submit ([recovery CLI](recovery-cli.md)) | `dytallix recovery prepare --view view.json --request request.json --expiry-height <h> --output op.json` |
| `crypto` | Key generation, signing, verification, and keystore inspection | `dytallix crypto keygen` |
| `config` | Show the pinned chain; pin the consensus chain | `dytallix config pin-chain ...` |
| `gateway` | Serve a browser wallet on this machine and relay it to the pinned chain | `dytallix gateway serve --listen 127.0.0.1:4173` |

## Command Groups

### `wallet`

Subcommands:

- `create [--name NAME]`
- `import --key-file PATH [--name NAME]`
- `export --output PATH`
- `list`
- `switch NAME`
- `rotate`
- `info`
- `migrate`

`create` generates a keypair and does not fund it. There is no faucet. Once
a chain is pinned, `info` also prints the wallet's address on that chain.

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

### Reaching a node

There is no TLS (E04 gap 19). `--endpoint`, on `pin-chain` and on every
command that reads or writes, takes one of two forms:

- **A node on this machine:** `http://127.0.0.1:PORT` or `http://[::1]:PORT`.
  Plain HTTP goes only to a literal loopback address. HTTPS, DNS names and
  other hosts are refused.
- **A remote node:** the path of its endpoint pin file. The operator
  publishes that file:

  ```json
  {"version":1,"network":"CHAIN_ID","address":"HOST:PORT","public_key_base64":"..."}
  ```

  - **The channel.** Requests cross the post-quantum client channel:
    ML-KEM-768 key exchange, then an ML-DSA-65 signature by the endpoint's
    key. The key is pinned in full.
  - **Refusals.** An endpoint that cannot prove that key gets no request.
    A pin for another chain is refused.
  - **Trust.** Take the pin from a source you trust, and compare its key
    fingerprint.

`pin-chain` stores a pin file's address and key in `chain.json`, version
2. A version 1 `chain.json` with a remote `http://` endpoint still loads,
but it cannot connect: pin the chain again with the endpoint's pin file.

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
dytallix stake status
dytallix stake validators
```

- `stake status [address]` shows the account's liquid balances, bonds by
  validator, pending bond, unbonding (with each entry's effective height)
  and claimable rewards. `stake validators` shows the next block's
  validator set. Both are the pinned node's report; `balance` proves the
  balances.

- `balance [address]` reads the account's native record with its state
  proof, and checks the proof against the application hash in the pinned
  node's next block header (`app_hash_source: node_header`). The header's
  signatures are not checked, so this trusts the pinned node, as the other
  commands do. The SDK also accepts an application hash you obtained
  elsewhere.
- `send` to an address with no account creates it and burns the chain's
  account creation fee.
- An account a transfer created has no record yet. Its first transaction is
  signed by the key its address derives from, at nonce zero, and must be an
  ordinary transaction (send or stake), not governance. Before signing, the
  CLI proves the account is funded and has no record (`first_spend_proof`);
  this waits for the next block.
- `send` and `stake` report the committed receipt, checked against the
  signed transaction.

### `governance`

Governance uses ordinary-v3 transactions:

- `propose`: one change, `--max-active <n>`, `--min-self-bond-udgt <n>`,
  `--fees <FeeValues JSON>`, `--registry-add <validator-id> --owner <address>`
  or `--registry-remove <validator-id>`. It takes the node's next proposal ID.
- `deposit --proposal-id <id> --amount <DGT>`
- `vote --proposal-id <id> --choice <yes|no|no-with-veto|abstain>`
- `show --proposal-id <id> [--voter <address>]`: the proposal's phase,
  deposits and tally, and one account's vote

The writes run in one step, like `send`:

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

### `config`

Subcommands:

- `show`
- `pin-chain --endpoint <http://loopback:port | endpoint-pin-file> --network <mainnet|testnet|development> --chain-id <id> --genesis-digest <hex>`

`show` prints the pinned chain and its endpoint.

Examples:

```bash
dytallix config show
dytallix config pin-chain --endpoint ./endpoint-pin.json --network <mainnet|testnet|development> \
  --chain-id <chain-id> --genesis-digest <sha256-of-genesis-hex>
```

### `gateway`

Browsers cannot use post-quantum certificates, so there is no public HTTPS
endpoint. `dytallix gateway serve` is the browser path. It runs on your
machine, at a literal loopback address, which browsers treat as a secure
context:

```bash
dytallix gateway serve --listen 127.0.0.1:4173
dytallix gateway serve --listen 127.0.0.1:4173 --bundle ./wallet --bundle-sha256 <digest>
dytallix gateway bundle-digest ./wallet
```

It prints its URL. Open that URL in the browser.

- **`POST /rpc`** relays one JSON-RPC request to the pinned chain's
  endpoint, over the client channel or loopback HTTP. `--endpoint`
  overrides the endpoint, as on other commands. The node's own method
  allowlist applies.
- **`GET /chain`** reports the pinned network, chain ID, genesis digest
  and endpoint.
- **With `--bundle`,** it serves a wallet page from a directory. The
  page's digest must match `--bundle-sha256`, the SHA-256 of the bundle's
  manifest. The manifest is one `<sha256>  <path>` line per file, sorted
  by path, as `sha256sum` prints it; `bundle-digest` computes it. Take the
  digest from a source you trust.
  - The files are read once, at startup.
  - A changed file stops startup.
  - Symlinks are refused.

Only pages the gateway serves can use it. It refuses:
- a `Host` other than its own address, such as a rebound DNS name or
  `localhost`;
- an `Origin` other than its own;
- a cross-site or same-site fetch;
- a POST that is not `application/json`.

It answers no CORS preflight. Every response forbids caching, framing and
cross-origin use, and carries a Content-Security-Policy that lets a page
run only its own scripts and WebAssembly and talk only to the gateway.
There is no key custody in the gateway: a page signs its own
transactions.

## Production cryptographic profile

Wallet creation and legacy transaction signing require exact ML-DSA-65.
The `crypto verify` command returns failure for invalid signatures.
The `crypto keygen --scheme slh-dsa` command fails because root authorization
uses a separate FIPS 205 component. The SDK legacy SPHINCS+ backend is not
FIPS 205. A 48-byte public key does not identify the signing standard.
See [PQC profile](pqc-profile.md) for scope and launch gates.
