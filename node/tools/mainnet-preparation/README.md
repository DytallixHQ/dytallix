# Public runtime binding review

`check_bindings.py` checks supplied data. It never creates genesis, starts a node, signs a record, or enables production.

Run from the node repository:

```text
python3 -B tools/mainnet-preparation/check_bindings.py \
  --bindings BINDINGS.json --records PRODUCTION_INPUTS.json \
  --native NATIVE_GENESIS.json --application APPLICATION_CONFIG.json
```

The native and application files are optional. Without them, the result lists the missing runtime bytes. Exit code 1 means the review remains BLOCKED. Exit code 2 means a supplied field or source binding failed validation. No exit code grants acceptance.

The records argument uses the existing `PRODUCTION_INPUTS` record IDs and row arrays. Run the separate intake checker to validate the complete record schema and acceptance state. This tool checks only the records used by its supported bindings. A record reference does not establish approval.

## Supported typed inputs

| Input | Check |
|---|---|
| Exact source bytes | Match SHA-256 for the supplied records, native genesis, and application configuration. Match the application's embedded native-genesis digest. |
| Native amounts | Require decimal strings, u128 bounds, the existing DGT cap, unique accounts, explicit vesting, and checked stake funding. Match funded delegations to reward positions. |
| Native reward and issuance inputs | Check the current development versions, activation height, decimals, resource limits, validator population, controller bounds, and epoch budget. |
| Application configuration | Check the fixed-validator local profile, chain ID, gas and byte limits, canonical ML-DSA-65 public key encoding, unique identities, positive bounded power, and reward-validator agreement. |
| Chain identity | Resolve the D13-Q02 identity policy reference to a typed public document. Match its chain ID to both runtime files. |
| Beneficiary bindings | Resolve D08-Q01 account references. Match amounts, explicit vesting documents, staking permission, and operator-specific funded delegations. Require every native account and supplied allocation row to map exactly once. |
| DRT bootstrap | Match each D08-Q03 row to its recipient account and policy reference. Reconcile all rows and the policy total with native DRT balances. |
| Validator bindings | Resolve D09-Q02 operator references to exact public key documents. Match keys to the application validators. Reject duplicate operator or validator mappings. |
| Public transport manifest | Check the Go loopback profile, timeout and peer limits, full public key pins, SHA-256-derived 20-byte peer IDs, addresses, and persistent-peer agreement. |

`source_digests` identifies exact input bytes. `runtime_inputs` retains the eleven existing preparation field names. `public_documents` resolves public references with typed payloads and hashes. The fixture shows every supported field shape.

Public document entries contain `reference`, `sha256`, and `document`. Supported document kinds are `account`, `validator_key`, `peer_key`, `vesting_terms`, and `identity_policy`. Each kind rejects unknown fields. Private key fields are not accepted. Hash each document as sorted-key compact ASCII JSON with one final newline. This package convention is not a general JSON canonicalization standard.

No reference causes a file or network fetch. The tool reads only explicit command arguments. It limits each input to 8 MiB and JSON nesting to 64 levels. Duplicate keys and non-finite numbers fail.

## Consumer limits

These checks implement a strict supported subset of the current Rust and Go consumers. The native subset requires explicit `udgt` and `udrt` fields, vesting, delegations, reward state, and issuance inputs. It rejects omitted development defaults and alternate delegation representations. Canonical decimal strings are stricter than the native parser's digit-only strings.

This adapter permits one validator per operator record. It does not establish the production policy for operators that control several validators. The record format permits one initial operator delegation per beneficiary row. This version does not combine beneficiary rows or invent a multi-operator record. Allocation amounts must match native genesis credits exactly. A policy that separates lifetime allocation entitlement from partial genesis mint needs an explicit adapter. This tool does not infer full initial issuance from the one-billion-token allocation.

The application check enforces the consumer's 65,536-byte limit on the exact supplied file bytes. The public transport check covers the typed manifest and peer tuples. Scoped IPv6 addresses are rejected because the Go loopback parser does not accept them. It does not inspect a full Go TOML configuration, its canonical transport file bytes, loaded private keys, private file permissions, or the complete engine genesis. It does not test a handshake or key possession.

## Unsupported fields remain open

Genesis time, governance parameters, SLH-DSA root authorization, and approval-bundle semantics have no typed adapter in this tool. They remain UNSUPPORTED when populated. Lifecycle, penalty, recovery, and ordinary application configurations also remain UNSUPPORTED. No populated string or object can make `runtime_complete` true.

The tool does not establish production activation, custody, signature authenticity, validator admission, stake-to-power policy, or independent review. The supported runtime profiles remain local development profiles. Mainnet remains NO GO.

## Tests

Run `python3 -B -m unittest discover -s tools/mainnet-preparation -p test_bindings.py`.

The fixtures are synthetic. They reuse public keys and selected local parameters from prior development evidence. They contain no production private key and establish no production operator, beneficiary, amount, vesting schedule, network address, or approval. Runtime startup and heavy builds are outside this test scope.
