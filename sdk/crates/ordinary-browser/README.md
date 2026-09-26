# Ordinary browser codec

This crate exposes public ordinary-v2 codecs through WebAssembly. It uses the SDK's vendored `dytallix-protocol-types`. It does not import the native SDK or core crypto crates.

The crate has no private-key API, signature implementation, random generator, network client, storage API or production activation flag. All inspection and receipt results state `signature_verified: false`. Receipt checks also state `consensus_verified: false`.

## JavaScript API

Load the generated `wasm-bindgen` module and call its default initializer before these functions. JSON inputs and outputs are strings. Byte inputs use `Uint8Array`; byte-returning functions return `Uint8Array`. A returned error throws a JavaScript string. Treat it as a rejection. Do not infer successful validation from an absent result.

| Export | Inputs | Result |
| --- | --- | --- |
| `context` | anchor JSON, ProfileView JSON, AccountView JSON, current KeyIdentity JSON | SDK-compatible SigningContext JSON after comparison. |
| `prepare` | SigningContext JSON, ProfileView JSON, AccountView JSON, anchor JSON, intent JSON, current KeyIdentity JSON | JSON with `body`, `signing_bytes` array, lowercase `transaction_id`, `key_id`, `profile_digest`, and `signature_verified: false`. |
| `attach_signature` | OrdinaryTransaction body JSON, ProfileView JSON, signature bytes | Public attached result below. Exact signature length is checked. Cryptographic validity is not checked. |
| `inspect_signed` | SignedOrdinary JSON, ProfileView JSON | The same public attached result. It accepts no private key. |
| `validate_receipt` | ReceiptView JSON, SignedOrdinary JSON, raw FeeProfile JSON | JSON with `consistent: true`, transaction ID, and both verification flags set to false. Any mismatch throws. |
| `parse_json` | JSON text | Re-serialized JSON after recursive duplicate-key, syntax and size checks. This generic function has no schema. Typed exports also reject unknown fields. |
| `signing_bytes` | OrdinaryTransaction JSON, limits JSON | Full canonical message bytes. |
| `encode_signed` | SignedOrdinary JSON, limits JSON | Canonical binary envelope bytes. |
| `decode_envelope` | Envelope bytes, limits JSON | SignedOrdinary JSON. |
| `decode_transport` | Exact canonical outer transport JSON, ProfileView JSON | Public attached result. Noncanonical outer JSON is rejected so the returned Comet hash always identifies the supplied bytes. |
| `profile_digest` | Raw FeeProfile JSON | Lowercase SHA3-256 digest hex. |
| `profile_bytes` | Raw FeeProfile JSON | Canonical profile bytes. |
| `decode_profile` | Canonical profile bytes | Raw FeeProfile JSON. |
| `account_address` | Network code, lowercase account-ID hex | Stable Bech32m address. |
| `decode_address` | Network code, stable address | Lowercase account-ID hex. |
| `origin_address` | Network code, chain ID, exact algorithm, public-key bytes | JSON with stable address and account-ID hex. Use only for an origin identity. |
| `parse_token_units` | Canonical token amount string, at most six fractional places | Exact base-unit decimal string. |

The attached result has these fields:

- `body` and `signed` use the existing vendored protocol JSON schemas.
- `signing_bytes`, `envelope_bytes` and `transport_bytes` are JSON arrays of unsigned bytes.
- `transaction_id` is SHA3-256 of the full signing message.
- `envelope_hash` is SHA3-256 of the binary signed envelope.
- `comet_hash` is SHA-256 of the exact outer JSON transport bytes.
- `key_id` identifies the current public key.
- `signature_verified` is always false.

A length-correct placeholder signature can produce an attached result. That result is not a submission permit.

## Input shapes

All objects reject unknown fields. Imported JSON rejects duplicate object keys at every nesting level. Public views are the exact `ordinary_client.rs` DTOs. Ordinary bodies and actions are the exact `ordinary.rs` DTOs. Profile JSON uses `ordinary_fees.rs` decimal-string views.

An anchor is an explicit caller selection:

```text
{
  domain: RecoveryDomain,
  committed: CommittedContext,
  profile_digest: [32 bytes]
}
```

`RecoveryDomain` has network, chain ID, genesis-digest bytes and stable account-ID bytes. `CommittedContext` uses the public query DTO: chain ID, lowercase genesis-digest hex, decimal-string height and lowercase application-hash hex. Matching this object does not authenticate an RPC endpoint or supply a consensus proof.

A SigningContext matches the native SDK:

```text
{
  domain: RecoveryDomain,
  current_key: { algorithm, public_key: [bytes] },
  authorization_generation: "unsigned decimal",
  spending_nonce: "unsigned decimal",
  committed: CommittedContext,
  profile_digest: [32 bytes],
  protected: false
}
```

An intent has exactly these fields:

```text
{
  actions: [protocol actions],
  memo: "text",
  expiry_height: "unsigned decimal",
  gas_limit: "unsigned decimal",
  maximum_fee: "unsigned decimal"
}
```

Network codes are 1 for mainnet, 2 for testnet and 3 for development. An encoded network identifier does not approve deployment. Algorithm identifiers are exactly `mldsa65` and `mldsa87`; the selected account-role profile must permit the current key.

Keep every u64/u128 value as a decimal string. Do not convert it through JavaScript Number. The low-level `limits_json` interface also requires `max_expiry_lifetime` as a decimal string. Remaining limit fields are bounded u16/u32 JSON numbers. Reject unpaired UTF-16 before passing JavaScript strings to WASM; conversion at the ABI boundary can otherwise replace a surrogate before Rust sees it. JSON escape forms containing unpaired surrogates are rejected by the Rust parser.

## Browser sequence

1. Capture public views and the caller-selected anchor. Supply the current public key separately. `context` compares the stable domain, committed state, profile and key.
2. Call `prepare`. Retain its exact body and full signing bytes for review. Editing any input invalidates review.
3. Sign the full message with Noble's public ML-DSA API and an empty FIPS context. Verify that signature with Noble and the same message and current public key.
4. Only after that verification, call `attach_signature`. Retain the exact returned transport bytes.
5. Refresh public state before submission. Require that preparing the same reviewed intent against the refreshed authority still produces the same transaction ID. Reject a stale review.
6. Submit the retained bytes. Compare the node's Comet hash to `comet_hash`. CheckTx success means admission only.
7. Verify the signed envelope with Noble before using `validate_receipt`. This function checks reported receipt identity, profile, counters, gas, fee conservation, height and outcome. It does not establish signature validity, consensus inclusion or endpoint trust.

For a public signed-file workflow, use `inspect_signed`, verify with Noble, and compare the signed body to a fresh preparation from current views. No private-key import is required.

## Bounds and qualification

Each JSON input is limited to 1 MiB. Selected wire and transport bounds cannot exceed 1 MiB. Profiles are limited to 512 canonical bytes. Signatures are limited to 4,627 bytes and must have the exact algorithm length. Origin public keys are limited to 2,592 bytes. Address text is limited to 128 bytes. The browser must check byte-input bounds before copying large arrays across the WASM ABI.

Native tests use the existing independent wire, profile and address fixtures. Placeholder signatures provide codec evidence only. No expected bytes are generated from this crate. Additional tests cover pinned context changes, values above 2^53, u64/u128 limits, invalid decimal strings, duplicate keys, unknown fields, surrogate escapes, exact transport hashes and receipt outcomes.

The parent build adds this crate to the workspace, builds `wasm32-unknown-unknown`, and runs `wasm-bindgen` 0.2.100. Generated binaries belong under `target/` or outside the source archive. This source does not qualify a hosted wallet, browser custody, dependency supply chain or mainnet launch. Launch remains NO GO.
