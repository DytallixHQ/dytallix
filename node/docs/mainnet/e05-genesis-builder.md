# E05-d: deterministic genesis builder

Engineering task E05 (genesis from approved inputs), step d. The builder
turns the approved values and records into the three genesis files, the
native genesis, the application configuration and the engine genesis, plus a
manifest of their digests. The same inputs always give the same bytes.

**One mode per build** (production activation v1, A7). A development build
writes the `rehearsal` mode in the local-qualification and development
profiles, and `--verify` starts a chain from it. A production build (cargo
feature `production`) writes the `production` mode in the production profiles
with all three root controls. That genesis opens only from its root genesis
signed three of five, so `--verify` refuses it, and the signed production
test opens it instead. Building accepts nothing: approved values for every
open input, accepted records, the release, the root signatures and gate
acceptance remain required.

## Pipeline

```text
launch/E05_VALUES.json (approved values)  ┐
launch/genesis/PROPOSALS.json (labeled)   ├─ resolve_genesis_inputs.py ─> inputs + resolution report
records (custody system; here synthetic)  ┘
inputs ─ dytallix-genesis-build ─> native-genesis.json, application-config.json,
                                    genesis.json (engine), BUILD_MANIFEST.json
```

1. **Resolve** (`tools/mainnet-preparation/resolve_genesis_inputs.py`). Every
   value the builder needs comes from exactly one source: an APPROVED entry in
   `E05_VALUES.json`, or a labeled entry (`PROPOSED` or `MEASURE_PLACEHOLDER`)
   in the proposals file for a value still open. A proposal for an approved
   value, an open value with no proposal, an unknown name, or a wrongly typed
   value is refused. The resolution report lists each value's source and
   always says `production_eligible: false` in this version.
2. **Build** (`dytallix-genesis-build`, `src/genesis_build.rs` in the node). It
   uses the node's own types, so it serializes exactly what the node parses,
   and it runs the node's own configuration validation (every E05-a coupling)
   before writing anything. `--verify` also opens the application on a fresh
   database and runs InitChain.

```text
python3 -B tools/mainnet-preparation/resolve_genesis_inputs.py --mode rehearsal \
  --values ../launch/E05_VALUES.json --proposals ../launch/genesis/PROPOSALS.json \
  --records RECORDS.json --inputs GENESIS_INPUTS.json --resolution RESOLUTION.json
cargo run -p dytallix-fast-node --bin dytallix-genesis-build -- \
  --inputs GENESIS_INPUTS.json --out BUILD_DIR --verify
```

For the production mode, resolve with `--mode production` and build with
`cargo run -p dytallix-fast-node --features production --bin
dytallix-genesis-build -- --inputs GENESIS_INPUTS.json --out BUILD_DIR`. The
resolution report is `production_eligible` only in the production mode with
every value approved and every record accepted.

## What the builder fixes and derives

Nothing is defaulted. The builder fixes only code constants (versions,
profile names, the ML-DSA-65 algorithm, activation heights and sequences of
1, `initial_schema` 0) and derives values that approved rules determine:

| Value | Derivation |
| --- | --- |
| Account addresses and IDs | From each account's ML-DSA-65 origin key, network and chain (`AccountAddress::from_origin_key`) |
| Recovery accounts | One per genesis account, with the approved account template timing; genesis balances go only to registered accounts |
| `app_state_sha256`, recovery and governance genesis digests, root policy `genesis_sha256` | SHA-256 of the exact native genesis bytes |
| Reward positions and `staking.delegations` | Each validator's self-bond from its operator account, plus delegations |
| Validator power | The stake bonded to it, in uDGT |
| `reward_v2.max_validators` | The governed `max_active` upper bound (E05-a rule 4) |
| Ordinary `max_fee_cap` | `max_transaction_gas` × the highest governed gas price (E05-a rule 3) |
| Recovery `max_fee_cap` | `max_transaction_gas` × gas price |
| `ordinary.max_transport_bytes` | 43 + the base64 size of `max_wire_bytes` (E05-a rule 5) |
| Validator proof profile digest | The node's `validator_profile_digest` of the lifecycle configuration |
| Root control bounds and signature counts | The largest transaction, and each authority's threshold (E05-a rule 6) |
| Engine evidence limits | The lifecycle's, in whole seconds |
| Top-level `gas_price` | The ordinary gas price |
| Profile names, `development_only`, the emergency transition rule's name, penalty activation | The build (`build_profile`): development or production |
| Root controls | Emergency freeze v2, upgrade schema 2 and release handover schema 2, in both modes |
| Handover authority and epoch | The upgrade custodians' (P01, 30 September 2026), so the configuration check's identical-authority rule holds |

It refuses a DGT total other than the whole 1,000,000,000 DGT (D05-Q02), an
account DRT total other than the approved bootstrap, a self-bond below
`min_self_bond`, vesting that does not cover the whole DGT balance, a shared
origin or consensus key, the other build's mode, a chain ID naming mainnet or
production in a development build, a production genesis without its root
controls, a genesis beyond its reader's bound, and any unknown input field.

## Byte formats

- **Native genesis:** compact JSON with sorted keys and no trailing newline,
  so the engine carries it unchanged as `app_state` and the application's
  digest binds it.
- **Application configuration:** `serde_json::to_vec` of the node's
  `ConsensusConfig`, the same bytes the node stores. The root genesis bundle
  signs these exact bytes.
- **Engine genesis:** the Go engine's encoding (compact, its field order,
  64-bit integers as strings, durations in nanoseconds, uppercase validator
  addresses, `app_hash` empty) with the native genesis spliced in last.
- **Manifest:** the size, SHA-256 and SHA-512 of each file, the SHA-256 of the
  inputs, and a build digest: SHA-256 over `DYTALLIX/GENESIS-BUILD/v1\0` and
  the three files, each length-prefixed.

## Checks

- `genesis_build_tests.rs`, in both builds: each build's committed rehearsal
  rebuilds byte for byte; derived values follow their rules (a basic transfer
  pays the 1 DRT floor; the build's names; schema 2 controls under the upgrade
  authority); and the refusals above. A development build also starts a chain
  from its rehearsal; a production build refuses an unsigned start.
- `threshold_genesis_signed_three_of_five` (`run_signed_fixture_tests.py
  --production`): the production rehearsal, unchanged, opens on a production
  build from a root genesis signed three of five, with its controls, the
  emergency verifier and a runtime candidate, through InitChain and restart.
- `rehearsal_genesis_test.go` (engine fixture): the engine decodes both
  rehearsals' engine genesis and re-encodes exactly the same bytes, and they
  pass the engine's genesis checks.
- `test_resolve_genesis_inputs.py`: the committed inputs, resolution and
  synthetic records are current, every genesis-affecting E05 value is either
  resolved or derived, and the refusals above.

Two rehearsals are committed under `tools/mainnet-preparation/fixtures/`.
Both use 61 approved values, 49 proposals, 9 measurement placeholders and
synthetic records: invented holders, and public keys that are SHAKE-256
outputs, not key pairs.

- `genesis-rehearsal/`: the development build's, on `dytallix-rehearsal-1`.
- `genesis-production-rehearsal/`: the production build's, on the staging
  chain `dytallix-staging-1` (testnet addresses).

## Limits

- **Production values.** A production genesis also needs every open value
  approved (the operating values, the measured windows and capacities) and
  every record accepted (operators, custodians, genesis signers,
  beneficiaries, chain identity).
- **Root controls.** Development `--verify` starts the chain without the
  emergency, upgrade and handover sections, because they open only with the
  development root helper; configuration validation checks them. The signed
  production test opens the production rehearsal with all three.
- **Not produced:** the engine `config.toml`, the PQC transport file, the
  service configuration and the signed root genesis bundle.
- **Binding review (E05-d2, A7).** `check_bindings.py` reviews the full
  configuration, the engine genesis and the manifest independently of the
  node (`config_checks.py`), in either build's profiles: a configuration must
  be wholly development or wholly production, and a production one must carry
  all three root controls at schema 2. Both rehearsals pass with no errors
  and stay BLOCKED, because their records are synthetic and the review does
  not see the root signatures.
