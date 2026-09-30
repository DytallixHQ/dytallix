# FAQ

## What is Dytallix?

Dytallix is a PQC-native Layer 1 blockchain with ML-DSA-65 accounts, Bech32m
D-Addrs, a public testnet, and developer tooling centered on a Rust SDK and
CLI.

## Is there a live public testnet?

Yes. Public read endpoints, the faucet, and signed transaction submission were
all verified against `https://dytallix.com` on April 13, 2026.

## How do I get testnet funds?

Use the public faucet:

- `dytallix init` or `dytallix faucet`, in the testnet CLI release
- `POST https://dytallix.com/api/faucet/request`

The mainnet candidate has no faucet, and its CLI has no `init` or `faucet`
command. An account receives funds at genesis or by a transfer from a funded
account.

## Which signature scheme should I use?

Use `ML-DSA-65` for normal Dytallix accounts and D-Addr derivation.

## What does a Dytallix address look like?

A canonical address starts with `dytallix1` and is a Bech32m-encoded hash of an
ML-DSA-65 public key.

## Which token pays gas right now?

On the current public testnet, gas is charged in `DGT` according to the live
`/status` endpoint and the published node source.

On the mainnet candidate, fees are charged in uDRT, with no tips, and every
fee is burned. See [`tokenomics.md`](tokenomics.md).

## Then what is DRT for?

`DRT` is a first-class token in balances, transfers, and reward-token
language across the SDK and explorer metadata. On the mainnet candidate it
pays every fee and all rewards. The public testnet's fee denom is `udgt`.

## Where do I view blocks and transactions?

Use the site-hosted explorer:

```text
https://dytallix.com/build/blockchain
```

The public `dytallix-explorer` repository documents that hosted surface. The
deployed explorer frontend source is not currently published in a separate
public repo.

## Can I call the public node directly?

Yes. The most useful public routes today are:

- `/status`
- `/account/:address`
- `/balance/:address`
- `/block/:id`
- `/blocks`
- `/tx/:hash`
- `/api/blockchain/submit`

## Are all CLI commands fully live on the public gateway?

On the public testnet, not yet. As of April 2026, wallet, balance, faucet,
core transfer flows, and the basic contract deploy/call/query/info/events loop
worked through the public gateway with the testnet CLI release. Governance
writes, staking writes, and some `/v1/*`-backed read surfaces were still ahead
of the public gateway routing.

The mainnet candidate CLI has no faucet, contract or testnet REST commands.
It uses a pinned consensus chain; see [`cli-reference.md`](cli-reference.md).

## What port does the local node use?

The published local node snapshot uses `3030`. The CLI local-profile constant is
still catching up in one place, so direct local integrations should treat `3030`
as the observable node port.

## Is the website frontend source public?

No. The live website frontend is the hosted public surface itself, but the
frontend source is not currently published in a separate public repo.

The live faucet backend source is also public in `dytallix-faucet`. The
`dytallix-explorer` repository remains a docs-only service-surface repo, while
the explorer UI source itself is not currently published as a separate public
repo.

## Where are the whitepapers?

They are bundled in this repository and indexed on
[`whitepapers.md`](whitepapers.md).
