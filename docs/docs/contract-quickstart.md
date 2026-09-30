# Contract Quickstart

**The mainnet candidate has no contract runtime.** Its consensus build runs
ordinary, governance and recovery transactions only. No contract can be
deployed, called or queried on it, and its CLI has no `contract` commands.

Whether a contract runtime is ever added is part of open decision D07-Q01
(the launch module matrix in `mainnet/launch/MAINNET_DECISION_REGISTER.json`).

## What The Chain Runs Instead

| Purpose | Transactions | CLI |
| --- | --- | --- |
| Transfers and staking | ordinary v2 | `dytallix send`, `dytallix stake` |
| Governance | ordinary v3 | `dytallix governance` |
| Account recovery | recovery | `dytallix recovery` |

See the [CLI reference](cli-reference.md).

## Contracts Toolkit

`mainnet/contracts` holds reference WASM contracts and examples. The node
does not depend on it, and nothing in it runs on the mainnet candidate
chain. See [its README](../../contracts/README.md).

## Public Testnet

The public testnet is a separate chain. Its node and an earlier release of
the CLI had contract routes and `contract` commands (`POST /contracts/deploy`,
`POST /contracts/call` and reads under `/api/contracts/`), verified through
`https://dytallix.com` on April 16, 2026. The mainnet candidate CLI removed
the testnet commands (E04 gap 19), and this page no longer documents that
flow.
