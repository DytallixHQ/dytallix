# FAQ

[Docs hub](README.md) | [Getting started](getting-started.md) | [CLI reference](cli-reference.md)

Keypair, faucet, transfer, and basic contract lifecycle are available for experimentation on the public testnet. Staking, governance, and some advanced or operator paths are not yet production-complete.

## How do I install the SDK if it is not on crates.io yet?

Use the Git repository directly:

```bash
cargo add dytallix-sdk --git https://github.com/DytallixHQ/dytallix-sdk.git
```

Enable network support with:

```bash
cargo add dytallix-sdk --git https://github.com/DytallixHQ/dytallix-sdk.git --features network
```

## What is the difference between DGT and DRT?

- `DGT` is the governance and delegation token.
- `DRT` is used for rewards and burns.
- The current public node charges transaction fees in `udgt`.

The SDK models both through the `Token` enum.

## Why does wallet rotation not preserve the same address?

The D-Addr is derived from the ML-DSA-65 public key. Rotating the key changes
the public key, which changes the derived address. The CLI surfaces that
directly and recommends creating a new wallet instead of pretending rotation can
keep the same identity.

## Why do I need the `network` feature?

Without `network`, the SDK stays small and supports offline flows such as:

- key generation
- address derivation
- signing and verification
- keystore operations
- transaction construction

Enable `network` when you need the async node client or faucet client.

## Where does the CLI store keys and config?

Under `~/.dytallix/`:

- `keystore.json` stores named key entries and the active wallet
- `config.json` stores the selected network profile and free-form config values

## Can I use an SLH-DSA keypair as a normal Dytallix wallet?

No. The normal wallet and D-Addr use ML-DSA-65. FIPS 205 root authorization uses a separate component. The SDK legacy `SlhDsa` API is SPHINCS+-SHAKE-192s-simple. It is not FIPS 205. The CLI rejects `crypto keygen --scheme slh-dsa`.

## How do I switch between testnet and local development?

Use:

```bash
dytallix config network testnet
dytallix config network local
```

The active profile controls which node and faucet endpoints the CLI uses. The
`mainnet` profile remains reserved in config files, but the public CLI does not
offer a selectable mainnet endpoint.

## What are the public faucet limits?

The canonical public testnet faucet currently grants `10 DGT` and `100 DRT`
per successful request.

The canonical limiter is:

- `60` second cooldown between successful requests
- `20` requests per hour

When the faucet is cooling down, the CLI should surface a retry window in
seconds instead of treating the service as unreachable.

## Are staking and governance writes public-ready?

No. Public staking and governance writes are currently disabled on the default
website gateway.

You can still use:

- `dytallix stake status`
- `dytallix governance proposals`
- `dytallix governance status <id>`

For experimental write testing, point the CLI at a local node or direct node
endpoint.

## Where should I ask for help or report issues?

- General issues or feature requests: open a GitHub issue in the repository
- Security issues: follow the private process in [SECURITY.md](../SECURITY.md)
- Community questions: join [Discord](https://discord.gg/eyVvu5kmPG)
