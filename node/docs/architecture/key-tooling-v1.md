# Key and operator tooling (E04 gap 17)

Engineering task E04, gap 17 of the [E04.1 triage](../mainnet/e04-requirement-triage.md)
(KEY-004). Writing the incident runbooks (gap 15) found three missing tools:
- a production validator key proof signer;
- a builder for recovery transactions;
- an operator socket client.

## Decisions (P01, 28 September 2026)

1. **One gap, after gap 16**, covering the three tools.
2. **Validator key tool: generate and proof, in Go.** It runs on the
   validator host, so the key never leaves it, and it reuses the engine's
   key handling.
3. **Operator socket client: read-only diagnostics.**
4. **Edge TLS is a new gap (19).** No Dytallix client path may use
   classical public-key cryptography. The SDK and CLI drop `rustls`, `ring`
   and reqwest TLS from every feature, and the public gateway gets a design.

## T-a: `dytallix-validator-key`

`consensus/cometbft/cmd/dytallix-validator-key`:

- **`generate --key-file FILE --state-file FILE`.**
  - Writes a new ML-DSA-65 key (`priv_validator_key.json`) and a fresh
    signing state, both owner-only.
  - It never replaces a file, and it writes the state first so that a key
    is never left without its state.
  - It prints the address and public key.
- **`proof --key-file FILE --genesis ENGINE_GENESIS --operation
  register|rotate --validator ID --owner OWNER --nonce N --expiry-height H
  [--amount-udgt A]`.**
  - It builds the node's `proof_sign_bytes` itself:
    `dytallix-validator-key-proof-v1\0`, then the compact JSON tuple (chain,
    operation, validator, owner, key, nonce, expiry, amount).
  - The chain is the engine genesis chain, and the key is the one the file
    holds.
  - Register needs a positive amount; rotate takes none. Identifiers follow
    the node's rule.
  - It signs with ML-DSA-65 and an empty context, which is what the node
    verifies, and checks the signature itself.
  - It prints the `ValidatorRegister` or `ValidatorRotateKey` action for
    `dytallix ordinary prepare --actions`.
  - It reads only an owner-only key file, and signs nothing it did not
    build.
- **Encoding check.** A golden vector (`main_test.go`) is asserted by the
  node too (`proof_sign_bytes_match_the_operator_tool`). Its owner contains
  a quote, a backslash, a non-ASCII letter and `<&>`, on which serde's and
  Go's escaping must agree.

## T-b: `dytallix-operator-rpc`

`consensus/cometbft/cmd/dytallix-operator-rpc --home HOME METHOD [--query Q]`:

- **Methods:** `net_info`, `consensus_state`, `dump_consensus_state`,
  `num_unconfirmed_txs`, `status`, `health` and `validators`. Others are
  refused, so it submits nothing.
- **Transport:** one version-1 IPC request over `HOME/data/rpc-operator.sock`
  (four-byte big-endian framing).
- **Output:** the engine's JSON-RPC response. It fails when the status is
  not 200.

Both tools build with the PQC-only tags in the production build step, and
the Go graph check (G35) covers them: neither graph holds a classical
package.

## T-c: recovery transactions

Decisions (P01, 29 September 2026):

1. **A typed recovery view.** The state a client needs is readable today
   only as raw state proofs.
2. **All nine actions**, through one generic flow. The actions are Enroll,
   Rotate, Start, Finalize, Cancel, Resume, StagePolicy, ActivatePolicy and
   CancelPolicy.
3. **Offline multi-party signing.** `dytallix recovery prepare` writes the
   unsigned operation and lists the required signers. Each signer then signs
   on their own machine. `assemble` orders the signatures, and `sponsor`
   adds the paying account's signature and fee bounds. An SDK module that
   wallets can reuse underlies all of it.

The protocol sets the rest:
- Every recovery transaction is paid by a separate sponsor account, never
  the target.
- Guardians sign in the operation role. New keys (guardians, replacements)
  sign in the possession role.
- The client chooses request and update IDs.

**T-c1: `/recovery/account/{account_id}`.** It returns a
`RecoveryAccountView` (protocol types, vendored into the SDK) or null. The
view holds:
- the domain, address, status, active key and generation;
- the spending and sponsor nonces;
- the policy and its version;
- the recovery and policy sequences;
- the pending recovery and pending policy, with their heights;
- the timing and the accepted algorithms;
- the recovery fee profile's version and digest, and the gas and charge
  bounds a sponsor signs.

Counters are decimal strings. Due expiries are already applied.

## Steps

| Step | Content |
| --- | --- |
| T-a | `dytallix-validator-key`: generate, proof; golden vector on both sides; E01 route |
| T-b | `dytallix-operator-rpc`: read-only diagnostics |
| T-c1 | The typed recovery view `/recovery/account/{account_id}` |
| T-c2 | The SDK recovery module and the `dytallix recovery` commands: prepare, sign, assemble, sponsor, submit, receipt |
