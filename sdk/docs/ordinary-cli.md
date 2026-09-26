# Ordinary-v2 CLI

Use `dytallix ordinary` for the qualified ordinary-v2 protocol. Existing CLI commands keep their legacy behavior. This command does not select an endpoint, chain, account, fee profile, key, or fee limit for you.

The command supports explicit ML-DSA-65 and ML-DSA-87 keys. It uses the shared protocol codec. It does not create a new account when a current signing key changes.

## Capture and check the public context

Read the committed profile and account from an explicit Comet RPC endpoint:

```sh
dytallix ordinary query-profile --endpoint http://127.0.0.1:26657 --output profile.json
dytallix ordinary query-account --endpoint http://127.0.0.1:26657 --account-id LOWERCASE_64_HEX_ACCOUNT_ID --output account.json
```

These commands only query the selected endpoint. They do not verify a consensus proof. Establish trust in the endpoint and the expected chain state through your deployment procedure.

Supply `context.json` as the explicit expected `SigningContext`:

| Field | Required value |
| --- | --- |
| `domain` | Network code, exact chain ID, genesis digest, and stable account ID |
| `current_key` | Exact `algorithm` and public-key byte array from current authority state |
| `authorization_generation` | Current generation as a canonical decimal string |
| `spending_nonce` | Current spending nonce as a canonical decimal string |
| `committed` | Exact chain ID, genesis digest, committed height, and app hash |
| `profile_digest` | Exact fee-profile digest as a 32-byte array |
| `protected` | Explicit account protection flag |

`domain` uses the shared `RecoveryDomain` JSON form. Its digest and account ID are byte arrays. `committed` uses the public query form. Its hashes are lowercase hexadecimal strings and its height is a decimal string. Public account queries also use hexadecimal IDs and digests. Do not copy a public account query into `context.json` without the required type conversion.

Preparation checks both captured views against this context. It rejects a different chain, genesis digest, app hash, height, profile, account, key, generation, nonce, or protection flag. The profile must apply at the next block. Expiry must be after that block and within the profile limit.

## Prepare and sign offline

Supply `actions.json` as an array of shared `Action` values. For example, a data action has this form:

```json
[{"type":"Data","data":"local client fixture"}]
```

Token amounts use six decimal places. Action amounts and the CLI fee cap use integer base units. `1000000` uDRT equals one DRT. Do not use floating-point amounts. Large JSON integers use the protocol's decimal-string fields.

```sh
dytallix ordinary prepare --profile profile.json --account account.json --context context.json --actions actions.json --memo '' --expiry-height 100 --gas-limit 100000 --maximum-fee-udrt 200000 --output body.json
dytallix ordinary inspect --profile profile.json --account account.json --context context.json --body body.json
```

The numeric values above are local examples. Set all three limits from the selected profile and intended transaction. The command rejects invalid limits. It does not estimate final execution gas or reserve funds.

Select one existing named wallet or one explicit key file:

```sh
dytallix ordinary sign --profile profile.json --account account.json --context context.json --body body.json --wallet NAME --output signed.json
```

```sh
dytallix ordinary sign --profile profile.json --account account.json --context context.json --body body.json --key-file key.json --output signed.json
```

`key.json` requires exactly these fields:

- `algorithm`: exactly `mldsa65` or `mldsa87`.
- `public_key`: the public-key byte array.
- `private_key`: the private-key byte array.

The command validates the declared scheme and the public/private pair. It then requires an exact match with the current account key. It never uses the legacy wallet address as the ordinary account ID. It does not import, save, migrate, or replace the selected key. Key bytes are not command arguments and do not appear in public output.

On Unix, the private key file or existing keystore must deny group and other access. Existing legacy keystores can require a permission correction before this command reads them. The command reads the existing keystore format once, within the file-size limit. Duplicate selected wallet names are rejected.

```sh
dytallix ordinary inspect --profile profile.json --account account.json --context context.json --signed signed.json
dytallix ordinary transport --profile profile.json --account account.json --context context.json --signed signed.json --output transport.json
```

`transport.json` contains the canonical `ordinary_v2` outer envelope. These commands are offline. They never call CheckTx or submit a transaction.

Preparation and inspection print the exact public body and transaction ID. They show three distinct fee values:

- `maximum_fee_udrt`: the signed cap reserved at admission.
- `required_cap_udrt`: gas limit multiplied by the profile gas price.
- `minimum_charge_udrt`: the profile minimum charge.

`actual_charge_udrt` stays `null` until a committed receipt reports the charge. A cap is not a fee estimate. A cap above the required amount does not imply that execution will consume it.

## Submit explicitly and read the receipt

```sh
dytallix ordinary submit --endpoint http://127.0.0.1:26657 --profile profile.json --account account.json --context context.json --signed signed.json
```

Only `submit` broadcasts. It first refreshes the profile and account queries. The fresh views must match the captured expected context. If the chain has advanced, capture and review the new context before another attempt.

The command calls `broadcast_tx_sync`. A successful response reports `check_tx_accepted` and `committed:false`. CheckTx acceptance does not establish inclusion or successful execution. A rejection returns a nonzero process status. The ordinary transaction ID identifies the signed intent. The Comet engine hash identifies the transported envelope.

```sh
dytallix ordinary receipt --endpoint http://127.0.0.1:26657 --transaction-id LOWERCASE_64_HEX_TRANSACTION_ID --profile profile.json --signed signed.json --output receipt.json
```

An absent receipt reports `receipt_absent` and creates no receipt file. A present receipt must match the signed intent, envelope, chain, fee profile, nonce change, gas, cap, charge, and cap release. The output retains the reported success, application failure, or out-of-gas outcome. It does not convert a paid failure into success. Receipt consistency checks do not verify a consensus inclusion proof.

## Check one genesis identity

```sh
dytallix ordinary check-genesis --profile profile.json --account account.json --context context.json --manifest manifest.json --genesis-file genesis.json
```

`manifest.json` requires exactly `domain`, `address`, `origin_key`, `current_key`, and `fee_profile_digest`. Use the same public types as the signing context. The command requires committed height zero. It uses the captured generation and spending nonce without replacing them with defaults.

The command checks SHA-256 over the exact genesis file bytes. The digest must match the expected domain. The manifest address must occur exactly once in `accounts`. The chain ID must match. The shared origin-key derivation must produce the stable account ID and canonical address. The current key must match captured authority. Origin and current keys can differ.

Genesis identity checks include protected accounts. They check identity consistency without authorizing spending. Preparation and signing still reject protected accounts.

This command checks identity compatibility only. It does not validate all balances, allocations, vesting schedules, validator roles, recovery configuration, or production settings. Run the node's full InitChain validation on the exact same genesis and configuration files. No command creates production allocations or schedules.

## File and output limits

Inputs and outputs are limited to 1 MiB per file. Network responses have a 1 MiB bound and a 30-second request timeout. Redirects and URL credentials are rejected by the client. Inputs must be regular files. Symbolic links are rejected. Output files use exclusive creation and Unix mode `0600`. Existing files are never replaced.

Successful command output is JSON on standard output. Runtime errors use JSON on standard error with a nonzero process status. Command-line usage errors use the existing argument parser's error output. Public artifacts contain public context, transaction bodies, signatures, or receipts. They do not contain private keys.

## Existing website wallet qualification

The existing website wallet is:

`/Users/rickglenn/Documents/ChatGPT/Redo Dytallix.com/dytallix-site/src/Wallet.jsx`

This task did not change that UI. Its current source and adjacent `AGENTS.md` were FileProvider dataless files during inspection. Bounded Git reads also stalled and were stopped. Current wallet behavior could not be checked from those files.

The remaining UI qualification must verify domain-bound stable addresses, current-key signing for both selected ML-DSA variants, committed context checks, exact base-unit amounts, uDRT fee caps, explicit submission, and the distinction between CheckTx acceptance and a committed receipt. SDK and CLI test results do not qualify that UI or a deployed network.

## Local HTTP qualification profile

Build the selected local client with:

```sh
cargo build --locked --offline -p dytallix-cli --no-default-features \
  --features ordinary-http-only --bin dytallix-ordinary-local
```

Run the same `ordinary` subcommands with `dytallix-ordinary-local ordinary ...`.
The binary reuses the ordinary command implementation and JSON output format.
It accepts HTTP endpoints with literal loopback addresses only: `127.0.0.1` or
`[::1]`. It rejects HTTPS before a request. It does not downgrade HTTPS, use a
proxy, follow redirects, or provide an external plaintext transport.

The SDK feature is `ordinary-http-only`. It exposes `ordinary_client` without
legacy network modules or TLS features. Do not combine it with `network`.
The CLI feature cannot be combined with the default `legacy-network` feature.
Build each profile separately. Cargo combines dependency features; inspect the
exact selected dependency graph and compiled artifact before a boundary claim.

The default `dytallix` CLI and SDK `network` feature retain reqwest default
features, including HTTPS. They are outside this selected local profile.
This profile is for local qualification. It does not authorize production use,
qualify a hosted wallet, provide remote transport security, or close G35.

## Strict local ML-DSA-65 profile

For local qualification with only the ML-DSA-65 implementation, build separately:

```sh
cargo build --locked --offline --release -p dytallix-cli --no-default-features \
  --features strict-local-mldsa65 --bin dytallix-ordinary-local
```

Use the same `ordinary` commands. This profile accepts only `mldsa65` for signing
and current account authority. It rejects ML-DSA-87 and legacy SPHINCS+ instead of
selecting another algorithm. It keeps the literal-loopback HTTP restriction,
HTTPS refusal, response limits, and disabled proxies from the local profile.

The strict profile disables the core `compatibility` feature. Its selected
fips204 features are `ml-dsa-65` and `default-rng`. It does not select
pqcrypto-sphincsplus or ML-DSA-44/87 implementations. The ordinary-http-only and
default CLI profiles retain compatibility. The strict core omits legacy key
creation APIs; import and verification of unsupported schemes return errors.

Do not combine strict and compatibility features in one Cargo build. Another
workspace dependency can combine features. Verify the exact selected dependency
graph, binary symbols and runtime providers for each artifact. These checks do
not establish FIPS module validation, G35 acceptance or production approval.
