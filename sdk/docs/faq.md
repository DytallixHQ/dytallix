# FAQ

[Docs hub](README.md) | [Getting started](getting-started.md) | [CLI reference](cli-reference.md)

This is the mainnet candidate SDK and CLI for the consensus chain. It contains
no public testnet client.

## How do I install the SDK if it is not on crates.io yet?

Use the Git repository directly:

```bash
cargo add dytallix-sdk --git https://github.com/DytallixHQ/dytallix.git
```

Add the node client with:

```bash
cargo add dytallix-sdk --git https://github.com/DytallixHQ/dytallix.git --features comet-rpc
```

## What is the difference between DGT and DRT?

- `DGT` is the governance and delegation token.
- `DRT` is used for fees and rewards.
- The consensus chain charges transaction fees in uDRT.

The SDK models both through the `Token` enum.

## Why does wallet rotation not preserve the same address?

The D-Addr is derived from the ML-DSA-65 public key. Rotating the key changes
the public key, which changes the derived address. The CLI surfaces that
directly and recommends creating a new wallet instead of pretending rotation can
keep the same identity.

## When do I need the `comet-rpc` feature?

Without `comet-rpc`, the SDK stays small and supports offline flows such as:

- key generation
- address derivation
- signing and verification
- keystore operations
- ordinary-v2, ordinary-v3, recovery and legacy transaction construction

Enable `comet-rpc` when you need `ordinary_client::CometClient` to query or
submit to a node.

## Where does the CLI store keys and the pinned chain?

Under `~/.dytallix/`:

- `keystore.json` stores named key entries and the active wallet
- `chain.json` stores the pinned chain and its endpoint

## Can I use an SLH-DSA keypair as a normal Dytallix wallet?

No. The normal wallet and D-Addr use ML-DSA-65. FIPS 205 root authorization uses a separate component. The SDK legacy `SlhDsa` API is SPHINCS+-SHAKE-192s-simple. It is not FIPS 205. The CLI rejects `crypto keygen --scheme slh-dsa`.

## How do I choose which node the CLI uses?

Pin the chain with its endpoint:

```bash
dytallix config pin-chain --endpoint http://127.0.0.1:26657 --network <mainnet|testnet|development> \
  --chain-id <chain-id> --genesis-digest <sha256-of-genesis-hex>
```

`--endpoint` is a loopback URL for a node on this machine, or the path of a
remote endpoint's pin file. There is no default endpoint. HTTPS and plain HTTP
to a remote host are refused. Commands that read or write also take
`--endpoint` for one call. `dytallix config show` prints the pinned chain and
its endpoint.

## How do I get funds?

There is no faucet. An account receives funds at genesis or by a transfer from
a funded account.

## Can I stake and vote from the CLI?

Yes, on the pinned chain:

- `dytallix stake bond`, `unbond` and `claim`
- `dytallix stake status` and `dytallix stake validators`
- `dytallix governance propose`, `deposit` and `vote`
- `dytallix governance show --proposal-id <id>`

Every write requires `--gas-limit` and `--maximum-fee-udrt`. See the
[CLI reference](cli-reference.md).

## Where should I ask for help or report issues?

- General issues or feature requests: open a GitHub issue in the repository
- Security issues: follow the private process in [SECURITY.md](../SECURITY.md)
- Community questions: join [Discord](https://discord.gg/eyVvu5kmPG)
