# Ordinary-v2 local SDK integration

This module uses the exact canonical protocol snapshot in
`vendor/dytallix-protocol-types`. A sibling node checkout is not required.
`vendor/protocol-types-source.json` records the source file hashes and provenance.
Run `python3 scripts/check_protocol_vendor.py` to verify the snapshot. See
[source release packaging](source-release.md) for standalone build instructions.
These APIs do not change the legacy SDK, CLI defaults, or public testnet routes.

`ordinary_v2` exposes all twelve approved actions through the shared protocol
`Action` type. It uses versioned stable account IDs. Do not convert a legacy
`DAddr` into a stable account ID or derive a new ID when an account rotates its key.

## Prepare and sign

1. Obtain the explicit fee profile and current account state.
2. Establish trust in the committed context through your own verification process.
3. Build `SigningContext` with the expected domain, current key, authorization
   generation, spending nonce, committed height and application hash, profile
   digest, and protection state.
4. Call `validate_views` to compare profile and account responses with that context.
5. Call `FeeQuote::from_profile` for the required cap and minimum charge. The quote
   does not estimate execution work or establish a final charge.
6. Call `prepare` with explicit actions, memo, expiry, gas limit, and maximum fee.
7. Review `PreparedTransaction::body()` and sign with `KeypairSigner` or an
   implementation of `OrdinarySigner`.
8. Encode with `encode_transport` and an explicit transport byte limit.

`validate_identity_views` checks identity records without requiring a spendable
account. It permits protected accounts, exhausted nonces, and later profile
activation. Use `validate_views` and `prepare` for signing checks.

`prepare` checks the next candidate block height with checked arithmetic. The
profile must be active at that height. Expiry must be greater than that height
and within the profile lifetime. Later execution can still reject stale state,
expired authorization, insufficient eligible funds, or changed action state.

Signing uses the complete canonical binary body with pure ML-DSA and an empty
FIPS context. It does not use the legacy JSON hash. External signers must report
the exact `mldsa65` or `mldsa87` algorithm and current public key. The SDK verifies
the returned signature. SLH-DSA is not an ordinary transaction signing role.
`MlDsa65Signer` is a restricted convenience adapter. `KeypairSigner` accepts both
approved ML-DSA formats, subject to the supplied account-role profile.

`parse_token_units` accepts unsigned decimal token amounts with up to six
fractional digits. It returns base units as `u128`. It rejects signs, whitespace,
exponents, noncanonical integer parts, excess fractional digits, and overflow.
It does not use floating-point arithmetic or saturating conversion.

## Explicit RPC client

Enable the `network` feature for `ordinary_client::CometClient`.
`CometClient::new(endpoint, max_response_bytes)` requires an explicit HTTP RPC
endpoint and response limit. It disables redirects and uses a 30-second timeout.
It does not use the legacy website gateway or try alternate endpoints.

Queries use JSON-RPC `abci_query`, height `0` for current committed state, and
`prove:false`. The profile and account context must match exactly before signing.
A query can race a new commit; fetch the views again if their contexts differ.
These responses are reported state. They contain no qualified consensus proof.

- `query_profile` reports whether ordinary execution is configured.
- `query_account` returns an optional current account view.
- `query_receipt` returns an optional committed receipt view. `None` is not an
  execution failure and does not imply that a transaction was submitted.
- `check_tx` asks the application to check admission. It does not broadcast.
- `submit_sync` explicitly calls `broadcast_tx_sync`. A zero code means CheckTx
  admission only. It does not mean commit or execution success.

The client validates the returned Comet transaction hash against SHA-256 of the
exact submitted transport bytes. This engine hash differs from the ordinary
transaction ID. Neither hash establishes inclusion on its own. The client does
not retry an uncertain broadcast or automatically submit prepared transactions.

Call `validate_receipt` with the exact signed envelope and fee profile. It checks
identifiers, domain, profile, block/expiry bounds, gas and metadata fields, nonce
advancement, approved failure classifications, exact charge, and cap conservation.
Client receipt amounts and counters use canonical decimal strings. A successful
consistency check does not authenticate the reporting endpoint or prove commit.

## Local checks

The SDK tests consume the node's independent minimal and twelve-action codec
vectors. Placeholder vector signatures are explicitly rejected by real signature
verification. Other tests cover real ML-DSA-65/87 signing, precision boundaries,
stale context, strict transport, receipt accounting, RPC bounds, response identity,
and submission hash binding.

These checks qualify local client compatibility. They do not establish mainnet
activation, public endpoint availability, hardware-wallet support, or a published
SDK release.
