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
4. Signing context for the default commands (K-c): read in one step from
   the configured node and refused unless it reports the chain ID and
   genesis digest pinned in the CLI configuration. Governance gets the same
   one-step mode; the file-based `ordinary` and `governance prepare` flows
   remain for offline signing.
5. Gas limit and fee cap (K-c): explicit on every write; no configured
   defaults.

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

## K-b implementation notes

- **Node query.** `/ordinary/profile_v3` returns `GovernanceProfileView`:
  the committed v3 fee profile (the candidate's, or the governed one after
  a fee change executes) and `next_proposal_id`, bound to the committed
  head. It is committed state: a fee change due at the next height
  replaces the profile before that block's transactions, which refuses a
  request signed for the old one. Without governance the view is disabled,
  with a null profile and ID zero.
- **Action data.** `FeeValues`, `ParameterChange` and `RegistryChange` move
  to `protocol-types::governance_action`, one definition for the chain and
  clients. Their `action_data` is an encoder written without bincode; a
  node test checks it equals the chain's bincode bytes and decodes back
  canonically.
- **SDK.** `ordinary_v3` validates the v2 identity views plus the v3 view
  against the caller's `SigningContext`, prepares one governance action
  per body (contract version 2, the v3 profile's version and digest, next
  height activation and lifetime), signs and verifies, and writes the
  `ordinary_v3` transport. `CometClient` adds the v3 query, CheckTx and
  broadcast. SDK tests reproduce the shared v3 vectors. The v3 path has no
  receipt check: the chain writes no per-transaction v3 receipt yet (gap
  12), so the spent nonce is the committed evidence.
- **CLI.** `dytallix governance` has `query-profile`, `propose` (maximum
  active validators, minimum self-bond, fee values from a file, registry
  add with a registered owner address, or registry remove), `deposit`,
  `vote` (yes, no, no-with-veto, abstain), `sign`, `inspect` and `submit`.
  A proposal takes the captured `next_proposal_id`; an admitted
  transaction whose governance rule fails is charged. `submit` accepts
  refreshed views at a later height, since the chain commits empty blocks,
  if they show the same authority and profile, the body is valid for the
  next height and a proposal still holds the next ID. `ordinary submit`
  still requires the captured height; K-c brings it to the same rule. The
  legacy REST commands move to `governance legacy`, built only with the
  `legacy-network` feature.
- **Errors.** SDK errors read `ordinary: ...` for both versions.

## K-c1 implementation notes

- **Pinned chain.** `dytallix config pin-chain` stores the endpoint,
  network, chain ID and genesis digest (`~/.dytallix/chain.json`), after
  asking the node which chain it reports (`--no-check` skips that). The SDK's
  `ChainPin` refuses any view whose committed context reports another chain
  before anything is signed.
- **Context from a node.** `ordinary_v2::context_from_views` builds the
  signing context from the profile and account views at one height. The key
  must be the account's current key. `CometClient::signing_context` and
  `governance_context` query the node, checking the pin first, and retry
  when a block commits between queries.
- **First spend (rule 4).** Without an account record the context is the
  pinned domain with the account ID, the signing key as origin key, and
  generation and nonce zero. `validate_first_spend_views` requires the key
  to derive the account ID. A node test checks that this domain is the one
  the chain accepts. Governance needs a record: a first spend is v2.
- **Refresh.** `ordinary_v2::refresh_views` and `ordinary_v3::refresh_views`
  accept views at a later height if they show the captured chain, authority
  and profiles. `ordinary submit` and `governance submit` use them, and
  recheck the body against the new head.
- **Balances.** `protocol-types::native_account` names the balance and nonce
  keys and decodes their records without bincode. A node test reads them
  through `/state/proof` and compares with the chain's bincode.
  `CometClient::query_balances` reports them unverified until K-d.
- **CLI.** `send`, `stake bond|unbond|claim`, `balance` and
  `governance propose|deposit|vote` run in one step: read the context,
  prepare, sign, submit, and wait `--wait-seconds` for the committed
  receipt (v2, checked against the signed envelope) or the spent nonce (v3).
  `--gas-limit` and `--maximum-fee-udrt` are required. K-b's offline
  governance commands move under `governance prepare`. The testnet REST
  send, stake, balance and governance commands move to `dytallix legacy`.
  `wallet info` shows the account address on the pinned chain.
- **Tests.** A fake Comet node in the CLI tests runs the whole pipeline: a
  first-spend send with a validated receipt, a stake at the account's
  nonce, a governance vote waiting for its nonce, a node on another chain
  refused before signing, and a CheckTx refusal.
- **Not yet.** Delegation status needs a node query (the reward state is
  one internal record). A first spend in the file-based `ordinary` flow
  still needs an account view.

## K-c2 implementation notes

- **Features.** The SDK's new `comet-rpc` feature builds the Comet JSON-RPC
  client over HTTP or HTTPS; `network` (the legacy REST client and faucet)
  now includes it. The CLI's default feature is `comet-rpc`, so a default
  build contains no legacy REST client. `legacy-network` adds the testnet
  commands (`init`, `faucet`, `contract`, `chain`, `node`, `dev` and
  `legacy`) and their REST helpers, which move to `commands/rest.rs`.
- **CI.** Both CI definitions also test the legacy build
  (`dytallix-cli --features legacy-network`, `dytallix-sdk --features
  network`); the SDK's own CI lints it too.
- **Distribution.** While the public testnet is the only public network,
  the published install command and the release archives build with
  `legacy-network`. The default source build has only the consensus-chain
  commands.

## Steps

| Step | Content |
| --- | --- |
| K-a | Builder safety fix; exact vendoring with a sync script and CI drift check; independent v2 and v3 vector generator |
| K-b | SDK ordinary v3 and the v3 fee profile source; CLI governance on v3 |
| K-c1 | Pinned chain; one-step send, stake, balance and governance with v1 addresses; first spend; later-height refresh; legacy REST commands under `dytallix legacy` |
| K-c2 | `legacy-network` non-default: a TLS-capable RPC feature without the legacy REST client; remaining REST commands gated |
| K-d | SDK state-proof verification; CLI balance and account reads verified |
