# Security Model

This page summarizes the security model of the mainnet candidate. Decision
IDs (such as D07-Q01) refer to `mainnet/launch/MAINNET_DECISION_REGISTER.json`.
Row and conflict IDs refer to the
[E04 requirement triage](../../node/docs/mainnet/e04-requirement-triage.md).

## Cryptographic Model

Dytallix accounts use ML-DSA-65.

Key points:

- addresses are derived from ML-DSA-65 public keys
- addresses are encoded as Bech32m D-Addrs
- the D-Addr payload is a 32-byte hash of the public key
- normal account workflows do not use legacy or hybrid account modes

By role:

| Role | Algorithm |
| --- | --- |
| Accounts and transactions | ML-DSA-65 (FIPS 204) |
| Validators, votes and proposals | ML-DSA-65 |
| Peer transport and client channel | ML-KEM-768 (FIPS 203) key exchange, ML-DSA-65 identities, AES-256-GCM records |
| Genesis, upgrades and exceptional authorization | SLH-DSA (FIPS 205), under separate root keys |
| State root | Sparse Merkle tree over SHA3-256 |

No classical public-key cryptography is used anywhere in the stack, client
edges included (P01, 28 September 2026). CI refuses a classical or TLS crate
in any mainnet Rust lockfile.

The CLI rejects `crypto keygen --scheme slh-dsa`. SLH-DSA is used only for
root authorization, through a separate component.

See the [PQC architecture](../../launch/PQC_ARCHITECTURE.md),
[client channel v1](../../node/docs/architecture/client-channel-v1.md) and
[state root v2](../../node/docs/architecture/state-root-v2.md).

## Security Assumptions

Dytallix security depends on:

- post-quantum primitives remaining secure at the chosen parameter sets
- fewer than one third of validator voting power being Byzantine: consensus
  is the CometBFT state machine with a quorum of more than two thirds
  (CONS-001)
- block time, timeouts and fault assumptions, which are open (D06-Q02)

## Consensus Evidence

- The engine verifies duplicate-vote and light-client-attack evidence
  before a block carries it.
- A validator's first duplicate vote deducts a fixed share from all stake
  bonded to it at that height, the operator's and every delegator's,
  vesting-locked stake included, and removes the validator for good two
  blocks later. Its consensus key is never reused, and later evidence
  against it adds nothing (D09-Q04, P01, 30 September 2026).
- Light-client-attack evidence is recorded only. There is no downtime
  penalty and no jailing.
- Penalized DGT moves to an escrow that nothing can spend.
- An unbond can be withdrawn once both evidence age limits plus margins have
  passed and no penalty on it is unsettled (D09-Q05). The rate, limits and
  margins are genesis inputs.

See [penalties v1](../../node/docs/architecture/penalties-v1.md) and
[liveness v1](../../node/docs/architecture/liveness-v1.md).

## Bridge Boundary

- No bridge ships in the consensus build. The node has no cross-chain
  bridge, no wrapped assets, no Airlock and no multi-party custody.
- Assets cannot move between Dytallix and another chain through the
  protocol. The chain recognizes no bridge or custody key.
- `dytallix-comet-bridge` is the adapter between the consensus engine and
  the application. It is not a cross-chain bridge.
- Bridges, wrapped assets and the Airlock are POST MAINNET (D07-Q01, P01,
  29 September 2026). The whitepapers' MPC Airlock does not apply (BRG-001
  to BRG-003, AC-009).

## Oracle Handling

- The consensus build has no oracle and no oracle reporters.
- The only issuance input is the epoch observation. Every validator derives
  it from committed blocks: utilization is the epoch's transaction bytes
  (without the observation itself) divided by `epoch_blocks ×
  max_block_bytes`, and volatility is 0.
- CheckTx refuses a submitted observation. The proposer inserts the derived
  one, and execution rejects a block whose observation differs. The complete
  history check re-derives every committed observation.
- No party supplies issuance inputs, so there are no outlier reports to
  filter and no oracle slashing (ORC-003).
- The incident runbooks record oracle failure as not applicable.
- This observation contract is approved, and no external oracle is used
  at launch (D01-Q02, D07-Q01, P01, 29 September 2026; AC-010).

See [adaptive emission v1](../../node/docs/mainnet/adaptive-emission-v1.md)
(observation contract v1).

## Network And Client Access

- A node serves an allowlist of 18 CometBFT JSON-RPC methods on an
  owner-only local socket, and operator diagnostics on a separate socket.
  Search, mempool contents, commit-waiting broadcasts and subscriptions are
  served nowhere.
- A remote client reaches a node only through the post-quantum client
  channel: ML-KEM-768 key exchange, then an ML-DSA-65 signature by the
  endpoint's key, which the client pins in full. There is no TLS, no trust
  on first use and no plaintext fallback.
- Browsers use `dytallix gateway`, a companion on the user's own machine at
  a loopback address. It holds no keys.
- A dishonest node can make a transaction fail or cost up to the signer's
  maximum fee, but cannot change its recipient or amount.
- Public ingress topology and rate limits are open (D12-Q01).

See [RPC controls v1](../../node/docs/architecture/rpc-controls-v1.md) and
[client channel v1](../../node/docs/architecture/client-channel-v1.md).

## Operational Security

- Validator keys are ML-DSA-65 keys in owner-only files. The operator
  generates them on the validator host with `dytallix-validator-key`, which
  signs only the register or rotate proofs it builds.
- The CLI keystore encrypts each private key with AES-256-GCM under an
  Argon2id key derived from the user's passphrase.
- Upgrades, emergency freezes and restarts after a halt need root
  signatures under separate keys. The production authorities and thresholds
  are open (D11-Q03, D14-Q02).
- Production custody is not yet qualified.

See [key tooling v1](../../node/docs/architecture/key-tooling-v1.md),
[keystore v2](../../node/docs/architecture/keystore-v2.md) and
[restart v1](../../node/docs/architecture/restart-v1.md).

## Threat Categories In The Tokenomics Paper

The tokenomics paper lists these threat classes:

- external passive observation
- external active spam and DoS
- minority Byzantine validators
- economic cartel behavior
- gateway censorship or selective inclusion
- MPC custody or bridge-related key compromise

Its mitigations (dimensional gas, 100% slashing, oracle outlier filtering,
bonded gateways and MPC custody) are not in the mainnet candidate. What
applies instead:

- observation: peer and client channel records are encrypted
- spam and DoS: every committed transaction pays at least the minimum fee,
  and every fee is burned; the mempool refuses conflicting nonces and
  re-signed duplicates; RPC methods and limits are fixed
- Byzantine validators: the CometBFT quorum; double-signing is penalized,
  with removal (D09-Q04)
- gateways: there is no public HTTPS gateway; remote access goes through
  channel endpoints, which cannot alter a signed transaction
- bridge or MPC custody compromise: not applicable, since there is no bridge
  or MPC custody

See [Errata for the mainnet candidate](whitepapers.md#errata-for-the-mainnet-candidate).

## Earlier Research Modules

Earlier node snapshots contained draft bridge, oracle and multi-algorithm
modules. The mainnet candidate's node has no bridge or oracle code, and its
generic verifier represents ML-DSA-65 only.

## Security Guidance For Builders

- validate D-Addrs locally before submitting anything
- pin the chain ID and genesis digest from a source you trust, never from
  the node itself
- take an endpoint pin file from a source you trust, and compare its key
  fingerprint
- read the account and fee profile from the pinned node before signing, and
  set your own gas limit and maximum fee
- `dytallix balance` checks a state proof against the application hash in
  the pinned node's next block header; the header's signatures are not yet
  checked, so it still trusts that node
