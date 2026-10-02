# Public runtime binding review

`check_bindings.py` checks supplied data. It never creates genesis, starts a node, signs a record, or enables production.

Run from the node repository:

```text
python3 -B tools/mainnet-preparation/check_bindings.py \
  --bindings BINDINGS.json --records PRODUCTION_INPUTS.json \
  --native NATIVE_GENESIS.json --application APPLICATION_CONFIG.json \
  --service SERVICE_CONFIG.json --engine ENGINE_GENESIS.json --manifest BUILD_MANIFEST.json
```

The native, application, service, engine genesis (`--engine`) and builder manifest (`--manifest`) files are optional. Without them, the result lists the missing runtime bytes. Exit code 1 means the review remains BLOCKED. Exit code 2 means a supplied field or source binding failed validation. No exit code grants acceptance.

The records argument uses the existing `PRODUCTION_INPUTS` record IDs and row arrays. Run the separate intake checker to validate the complete record schema and acceptance state. This tool checks only the records used by its supported bindings. A record reference does not establish approval.

## Supported typed inputs

| Input | Check |
|---|---|
| Exact source bytes | Match SHA-256 for the supplied records, native genesis, and application configuration. Match the application's embedded native-genesis digest. |
| Native amounts | Require decimal strings, u128 bounds, the existing DGT cap, unique accounts, explicit vesting, and checked stake funding. Match funded delegations to reward positions. Report `full_dgt_issuance` as missing unless genesis issues the whole 1,000,000,000 DGT: all DGT is issued at genesis and nothing mints it later (D05-Q02). |
| Native reward and issuance inputs | Check the current development versions, activation height, decimals, resource limits, validator population, controller bounds, and epoch budget. |
| Application configuration | Check the profile, chain ID, gas and byte limits, canonical ML-DSA-65 public key encoding, unique identities, positive bounded power, and reward-validator agreement. Report `recovery_and_ordinary_profiles` as missing unless both profiles are present: they are the only user-transaction paths (E04 gap 14). Report `lifecycle_and_penalty_profiles` as missing unless both are present: they penalize double-signing and allow withdrawals (penalties v1). |
| Full configuration (E05-d2) | `config_checks.review` re-derives, independently of the node: lifecycle, penalty, recovery, ordinary, governance and root-control structure; every account address from its origin key (SHA3-256 and Bech32m); the validator-proof profile digest; the E05-a rules (fee caps, transport bound, governance thresholds and bounds, root control bounds, evidence seconds); validator power as bonded stake and self-bonds at the minimum; recovery accounts equal to native accounts; root policies bound to the native genesis; upgrade keys disjoint from emergency keys; and the development gates the node still requires. |
| Engine genesis (E05-d2) | The engine genesis ends with the exact native genesis as `app_state`; chain, initial height, key profile, evidence limits equal to the lifecycle's, validator addresses and powers equal to the application's. Its digest binds through `source_digests.engine_genesis_sha256`. |
| Builder manifest (E05-d2) | Each file's size, SHA-256 and SHA-512, and the build digest. |
| Service configuration | Report `service_configuration` as missing unless supplied, and `metrics_output` unless it sets `metrics` (an absolute `directory` and `interval_seconds` from 1 to 3600): the incident runbooks read the metrics files (E04 gap 15). No other service field is reviewed. |
| Chain identity | Resolve the D13-Q02 identity policy reference to a typed public document. Match its chain ID to both runtime files. |
| Genesis time and governance | `genesis_time` equals the engine genesis's; `governance_parameters` equals the configuration's governance section exactly. |
| Beneficiary bindings | Resolve D08-Q01 account references. Match amounts, explicit vesting documents, staking permission, and operator-specific funded delegations. Require every native account and supplied allocation row to map exactly once. |
| DRT bootstrap | Match each D08-Q03 row to its recipient account and policy reference. Reconcile all rows and the policy total with native DRT balances. |
| Validator bindings | Resolve D09-Q02 operator references to exact public key documents. Match keys to the application validators. Reject duplicate operator or validator mappings. |
| Public transport manifest | Check the Go loopback profile, timeout and peer limits, full public key pins, SHA-256-derived 20-byte peer IDs, addresses, and persistent-peer agreement. |

`source_digests` identifies exact input bytes. `runtime_inputs` retains the eleven existing preparation field names. `public_documents` resolves public references with typed payloads and hashes. The fixture shows every supported field shape.

Public document entries contain `reference`, `sha256`, and `document`. Supported document kinds are `account`, `validator_key`, `peer_key`, `vesting_terms`, and `identity_policy`. Each kind rejects unknown fields. Private key fields are not accepted. Hash each document as sorted-key compact ASCII JSON with one final newline. This package convention is not a general JSON canonicalization standard.

No reference causes a file or network fetch. The tool reads only explicit command arguments. It limits each input to 8 MiB and JSON nesting to 64 levels. Duplicate keys and non-finite numbers fail.

## Consumer limits

These checks implement a strict supported subset of the current Rust and Go consumers. The native subset requires explicit `udgt` and `udrt` fields, vesting, delegations, reward state, and issuance inputs. It rejects omitted development defaults and alternate delegation representations. Canonical decimal strings are stricter than the native parser's digit-only strings.

This adapter permits one validator per operator record. It does not establish the production policy for operators that control several validators. The record format permits one initial operator delegation per beneficiary row. This version does not combine beneficiary rows or invent a multi-operator record. Allocation amounts must match native genesis credits exactly. D05-Q02 (P01, 29 September 2026) excludes a partial genesis mint, so the review requires the full total rather than inferring it from the one-billion-token allocation.

The application check enforces the consumer's 8 MiB limit (`MAX_CONFIG_BYTES`, E05-a) on the exact supplied file bytes; the configuration carries the genesis recovery accounts, about 15 KB each. The public transport check covers the typed manifest and peer tuples. Scoped IPv6 addresses are rejected because the Go loopback parser does not accept them. It does not inspect a full Go TOML configuration, its canonical transport file bytes, loaded private keys, private file permissions, or the complete engine genesis. It does not test a handshake or key possession.

## Unsupported fields remain open

SLH-DSA root authorization and approval-bundle semantics have no typed adapter in this tool. They remain UNSUPPORTED when populated. No populated string or object can make `runtime_complete` true.

The tool does not establish production activation, custody, signature authenticity, validator admission, stake-to-power policy, or independent review. The supported runtime profiles remain local development profiles. Mainnet remains NO GO.

## Upgrade custodian intake

`upgrade_custodian_intake.py` checks a completed upgrade custodian packet (E05-c). The packet format and collection steps are in [the upgrade intake](../../../launch/custody/upgrade/INTAKE.md).

```text
python3 -B tools/mainnet-preparation/upgrade_custodian_intake.py \
  UPGRADE_INTAKE.working.json --emergency EMERGENCY_INTAKE.working.json
```

It requires exactly five custodians, three of five, one explicit authority epoch, and SLH-DSA-SHAKE-256s keys (P01, 30 September 2026). It checks distinct controllers, control groups and keys, SHA-256 key IDs, purpose- and epoch-bound public evidence, reviewer separation, and that no controller, control group or key also appears in the complete emergency intake. On success it emits `authority_fragment`, the node's upgrade authority shape with keys sorted by key ID. It does not verify signatures, identity or independence, and it never reports production acceptance. Exit code 0 means structurally complete; 2 means incomplete or invalid.

## Genesis inputs (E05-d)

`resolve_genesis_inputs.py --mode rehearsal|production` writes the genesis builder's inputs from the approved values in `launch/E05_VALUES.json`, the labeled proposals in `launch/genesis/PROPOSALS.json`, and the records, with a report of each value's source. `--check` compares instead of writing. The mode must match the builder's build (production activation v1, A7). `genesis_rehearsal_records.py --out fixtures/genesis-rehearsal` writes the synthetic rehearsal records and, once the rehearsal is built, its binding-review packet (`review-records.json`, `review-bindings.json`); add `--staging` for the production-profile rehearsal on the staging chain. The builder itself is the node's `dytallix-genesis-build`; see [the genesis builder](../../docs/mainnet/e05-genesis-builder.md). `fixtures/genesis-rehearsal/` holds the development build's committed rehearsal and `fixtures/genesis-production-rehearsal/` the production build's: records, inputs, resolution, the four built files and the review packet. Review either with:

```text
F=tools/mainnet-preparation/fixtures/genesis-rehearsal
python3 -B tools/mainnet-preparation/check_bindings.py --bindings $F/review-bindings.json \
  --records $F/review-records.json --native $F/native-genesis.json \
  --application $F/application-config.json --engine $F/genesis.json --manifest $F/BUILD_MANIFEST.json
```

## Tests

Run `python3 -B -m unittest discover -s tools/mainnet-preparation -p 'test_*.py'`.

The fixtures are synthetic. They reuse public keys and selected local parameters from prior development evidence. They contain no production private key and establish no production operator, beneficiary, amount, vesting schedule, network address, or approval. Runtime startup and heavy builds are outside this test scope.
