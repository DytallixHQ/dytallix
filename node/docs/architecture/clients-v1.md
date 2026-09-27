# Clients for the consensus chain (E04 gap 8)

Status: approved (P01, 27 September 2026: decisions below). Engineering
task E04, gap 8 of the [E04.1 triage](../mainnet/e04-requirement-triage.md) (TXN-001,
API-003). Paths: `sdk/` is `mainnet/sdk`, `node/` is `mainnet/node`.

## Problems

| ID | Problem | Where |
| --- | --- | --- |
| K1 | The SDK's vendored protocol types lack v3, fee profile v3 and the signature-algorithm module, and carry debug prints in `tx.rs`. No script syncs them; CI checks their hashes but not drift from the node. | `sdk/vendor/dytallix-protocol-types`, `sdk/scripts/check_protocol_vendor.py` |
| K2 | The CLI's default commands (send, balance, stake, governance, chain, node) use the legacy testnet REST API (`/submit`, `/balance`, `/api/...`), which the consensus stack does not serve, and the chain refuses legacy signed requests under the recovery profile. Only `dytallix ordinary` speaks to the chain. | `sdk/crates/dytallix-cli/src/commands/`, `sdk/crates/dytallix-sdk/src/client.rs` |
| K3 | `stake delegate` and `stake undelegate` send a plain DGT transfer to the validator: the transaction builder emits a Send when an amount is set and silently drops the payload. | `sdk/crates/dytallix-sdk/src/transaction.rs` (`build`) |
| K4 | CLI wallets use the legacy address (Bech32m of BLAKE3), not the chain's v1 address (Bech32m of SHA3 of the origin key); real chain addresses are refused. | `sdk/crates/dytallix-core/src/address.rs` |
| K5 | No first spend: an account created by a transfer to a new address has no committed account record, and every SDK path requires one. | `sdk/crates/dytallix-sdk/src/ordinary_v2.rs` (`validate_views`) |
| K6 | No governance (v3) path in the SDK, and no query returns the v3 fee profile. | `node/.../governance_execution.rs` (`fee_profile`) |
| K7 | No v3 test vectors; the v2 vectors' generator is not in either repo. | `node/crates/protocol-types/tests/fixtures/` |
| K8 | The SDK never verifies state: queries send `prove=false` and read values as reported. | `sdk/crates/dytallix-sdk/src/ordinary_client.rs` |

What works: `dytallix ordinary` prepares, signs and submits ordinary v2
transactions through CometBFT JSON-RPC; the node serves
`/state/proof/{key}` with a JMT proof, the state root, the head's anchor
and application hash.

## Proposed rules

1. **Protocol types.** The SDK vendors the node's crate exactly, through a
   sync script; CI fails on drift from the node.
2. **Consensus commands (decision 1).** The default CLI speaks only to the
   consensus chain: send, stake (reward bond, begin unbond, claim),
   governance (proposal, deposit, vote) and balance, all as ordinary v2 or v3
   transactions over CometBFT JSON-RPC, with v1 addresses. The legacy
   testnet REST commands stay behind the non-default `legacy-network`
   feature until the testnet retires.
3. **Builder safety.** A transaction builder refuses an amount together
   with a payload; nothing is dropped.
4. **First spend.** An uninitialized account's context comes from its
   origin key and the chain profile: generation 0, nonce 0, the origin key.
   Its funding and absent record are shown by state proofs.
5. **Governance fee profile (decision 2).** A node query,
   `/ordinary/profile_v3`, returns the effective v3 fee profile bound to the
   committed head, as `/ordinary/profile` does for v2.
6. **Vectors.** An independent generator writes v2 and v3 vectors (signing
   bytes, envelope, transaction ID, envelope hash, key ID, governance action
   digest, fee profile v3 bytes and digest); both workspaces check them.
7. **State proofs (decision 3).** The SDK verifies a value's JMT proof to
   the state root and the root, with the head's anchor, to the application
   hash, against an application hash the caller trusts (their own node, or a
   header verified with the light-block tooling). Header verification in the
   SDK comes later.

## Decisions (P01, approved 27 September 2026)

1. Legacy testnet REST commands: behind the non-default `legacy-network`
   feature; the default CLI speaks only to the consensus chain.
2. v3 fee profile: a node query, `/ordinary/profile_v3`.
3. State proofs: verified to an application hash the caller trusts; SDK
   header verification later.

## K-a implementation notes

- **Builder.** `TransactionBuilder::build` refuses an amount together with a
  payload; `stake delegate` and `stake undelegate` now fail instead of
  sending DGT to the validator. They move to ordinary v2 in K-c.
- **Vendoring.** `sdk/scripts/sync_protocol_vendor.py --node-root
  mainnet/node` copies the node crate and `LICENSE` byte for byte (skipping
  build and Python caches) and rewrites the manifest. The SDK job in the
  node repository's CI runs `check_protocol_vendor.py --node-root ../node`,
  so drift fails the build. The first sync adds `ordinary_v3`,
  `ordinary_fees_v3` and `signature_algorithm` and removes the debug prints
  the vendored `tx.rs` carried.
- **Vectors.** `tests/fixtures/generate_ordinary_vectors.py` is an
  independent Python encoder. It reproduces the committed v2 vectors byte
  for byte and writes `ordinary_v3_vectors.json`: a proposal, a deposit and
  a vote (one per v3 action, both algorithms), each with its signing bytes,
  envelope, identifiers and, for the proposal, the governance action digest,
  plus a v3 fee profile's bytes and digest. The Rust codec matches all of
  them, and each vector is a valid signed request for the vector profile.
  The SDK workspace runs the same check through the vendored crate.

## Steps

| Step | Content |
| --- | --- |
| K-a | Builder safety fix; exact vendoring with a sync script and CI drift check; independent v2 and v3 vector generator |
| K-b | SDK ordinary v3 and the v3 fee profile source; CLI governance on v3 |
| K-c | CLI send, stake and balance on ordinary v2 with v1 addresses; first spend |
| K-d | SDK state-proof verification; CLI balance and account reads verified |
