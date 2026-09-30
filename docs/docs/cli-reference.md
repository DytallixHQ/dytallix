# CLI Reference

This page covers the mainnet candidate `dytallix` CLI for the consensus
chain. Its canonical reference is the SDK's
[CLI reference](../../sdk/docs/cli-reference.md); where the two differ, that
one is correct.

The CLI has no faucet, no contract commands, no public testnet REST commands
and no TLS. Earlier releases for the public testnet had `init`, `faucet`, `chain`, `dev`,
`node` and `contract` commands. The mainnet candidate CLI has none of them.

## Install

```bash
cargo install --git https://github.com/DytallixHQ/dytallix-sdk.git dytallix-cli --bin dytallix
```

Top-level help:

```bash
dytallix --help
```

## Local State

- Keystore: `~/.dytallix/keystore.json`, written owner-only (mode 0600).
  Version 2 encrypts each private key with AES-256-GCM under an Argon2id key
  from your passphrase. The passphrase is typed without echo, or read from
  the owner-only file named by `DYTALLIX_KEYSTORE_PASSPHRASE_FILE`. A
  version 1 file holds plaintext keys; `dytallix wallet migrate` encrypts
  it.
- Pinned chain: `~/.dytallix/chain.json`, written by
  `dytallix config pin-chain`.

There is no default endpoint and no network profile.

## Top-Level Commands

| Command | Purpose |
| --- | --- |
| `wallet` | Create, import, export, switch, list, rotate, inspect and migrate wallets |
| `balance` | Show an account's DGT and DRT on the pinned chain |
| `send` | Send DGT or DRT on the pinned chain |
| `stake` | Bond, begin unbonding and claim rewards on the pinned chain |
| `governance` | Propose, deposit and vote on the pinned chain (ordinary v3) |
| `ordinary` | Prepare, sign and submit ordinary-v2 transactions with explicit context |
| `recovery` | Account recovery: prepare, sign offline per party, assemble, sponsor, submit |
| `crypto` | Key generation, signing, verification and keystore inspection |
| `config` | Show the pinned chain; pin the consensus chain |
| `gateway` | Serve a browser wallet on this machine and relay it to the pinned chain |

## Reaching A Node

`--endpoint`, on `config pin-chain` and on every command that reads or
writes, takes one of two forms:

- **A node on this machine:** `http://127.0.0.1:PORT` or
  `http://[::1]:PORT`. Plain HTTP goes only to a literal loopback address.
  HTTPS, DNS names and other hosts are refused.
- **A remote node:** the path of its endpoint pin file, which its operator
  publishes:

  ```json
  {"version":1,"network":"CHAIN_ID","address":"HOST:PORT","public_key_base64":"..."}
  ```

  Requests cross the post-quantum client channel: ML-KEM-768 key exchange,
  then an ML-DSA-65 signature by the endpoint's key, which is pinned in
  full. An endpoint that cannot prove that key gets no request, and a pin
  for another chain is refused. Take the pin file from a source you trust,
  and compare its key fingerprint.

Pin the chain first. Take the network, chain ID and genesis digest from a
source you trust, never from the node itself:

```bash
dytallix config pin-chain --endpoint ./endpoint-pin.json --network <mainnet|testnet|development> \
  --chain-id <chain-id> --genesis-digest <sha256-of-genesis-hex>
```

`pin-chain` asks the node which chain it reports and refuses a mismatch;
`--no-check` stores the pin without asking.

## `wallet`

Subcommands:

- `create [--name NAME]`
- `import --key-file PATH [--name NAME]`
- `export --output PATH`
- `list`
- `switch NAME`
- `rotate`
- `info`
- `migrate`

`create` generates a keypair and does not fund it. There is no faucet. An
account receives funds at genesis or by a transfer from a funded account.
Once a chain is pinned, `info` also prints the wallet's address on that
chain.

## `balance`, `send` And `stake`

```bash
dytallix balance
dytallix send --to <address> --amount 1.5 --token drt --gas-limit 10000 --maximum-fee-udrt 20000
dytallix stake bond --validator <validator-id> --amount 10 --gas-limit 10000 --maximum-fee-udrt 20000
dytallix stake unbond --validator <validator-id> --amount 5 --gas-limit 10000 --maximum-fee-udrt 20000
dytallix stake claim --gas-limit 10000 --maximum-fee-udrt 20000
dytallix stake status
dytallix stake validators
```

Each write is one step. The CLI reads the account and fee profile from the
node, refuses them unless the node reports the pinned chain, then prepares,
signs, submits and waits up to `--wait-seconds` (default 30) for the
committed result.

- `--gas-limit` and `--maximum-fee-udrt` are required on every write; the
  CLI never chooses them. Fees are charged in uDRT and burned.
- A dishonest node can make a transaction fail or cost up to your cap, but
  cannot change its recipient or amount.
- Amounts are tokens with up to six decimal places (1 DRT = 1000000 uDRT).
- `balance [address]` checks the account's state proof against the
  application hash in the pinned node's next block header. The header's
  signatures are not checked, so this trusts the pinned node.
- `send` to an address with no account creates it and burns the chain's
  account creation fee.
- `stake status [address]` shows liquid balances, bonds, unbonding and
  claimable rewards; `stake validators` shows the next block's validator
  set. Both are the pinned node's report.

## `governance`

Governance uses ordinary-v3 transactions:

- `propose`: one change, `--max-active <n>`, `--min-self-bond-udgt <n>`,
  `--fees <FeeValues JSON>`, `--registry-add <validator-id> --owner <address>`
  or `--registry-remove <validator-id>`
- `deposit --proposal-id <id> --amount <DGT>`
- `vote --proposal-id <id> --choice <yes|no|no-with-veto|abstain>`
- `show --proposal-id <id> [--voter <address>]`

```bash
dytallix governance vote --proposal-id 7 --choice no-with-veto --gas-limit 10000 --maximum-fee-udrt 20000
```

For offline signing, `prepare`, `sign`, `inspect`, `submit` and
`query-profile` work on captured views; see the
[SDK CLI reference](../../sdk/docs/cli-reference.md#governance).

## `ordinary` And `recovery`

- `ordinary` prepares, signs and submits ordinary-v2 transactions with
  explicit context. See the [ordinary CLI](../../sdk/docs/ordinary-cli.md).
- `recovery` builds account recovery transactions. Each party signs on its
  own machine, and a separate sponsor account pays the fee. See the
  [recovery CLI](../../sdk/docs/recovery-cli.md).

## `crypto`

Subcommands:

- `keygen [--scheme ml-dsa-65|slh-dsa]`
- `sign <message>`
- `verify <message> <signature> <pubkey>`
- `address <pubkey>`
- `inspect <keystore-file>`

Wallets use ML-DSA-65. `keygen --scheme slh-dsa` fails, because root
authorization uses a separate FIPS 205 component. `verify` returns failure
for an invalid signature. See the [PQC profile](../../sdk/docs/pqc-profile.md).

## `config`

- `show` prints the pinned chain and its endpoint.
- `pin-chain --endpoint <http://loopback:port | endpoint-pin-file> --network <mainnet|testnet|development> --chain-id <id> --genesis-digest <hex>`

## `gateway`

Browsers cannot use post-quantum certificates, so there is no public HTTPS
endpoint. `dytallix gateway serve` is the browser path. It runs on your own
machine at a literal loopback address:

```bash
dytallix gateway serve --listen 127.0.0.1:4173
dytallix gateway serve --listen 127.0.0.1:4173 --bundle ./wallet --bundle-sha256 <digest>
dytallix gateway bundle-digest ./wallet
```

- `POST /rpc` relays one JSON-RPC request to the pinned chain's endpoint,
  over the client channel or loopback HTTP. The node's own method allowlist
  applies.
- `GET /chain` reports the pinned network, chain ID, genesis digest and
  endpoint.
- With `--bundle`, it serves a wallet page from a directory whose manifest
  digest must match `--bundle-sha256`.

It refuses a foreign `Host` or `Origin`, cross-site and same-site fetches,
and a POST that is not `application/json`, and it answers no CORS preflight.
It holds no keys: a page signs its own transactions.
