# Authenticated state root (state model phase B, D3)

Status: approved (P01, 27 September 2026: decisions below); R-S1 to R-S4
done. Engineering task E04. Covers problems P3 and P9 of
[state model v2](state-model-v2.md). Paths are relative to
`dytallix-fast-launch/node/src/`.

## Problems

| ID | Problem | Where |
| --- | --- | --- |
| R1 | The state commitment reads every consensus key and hashes them all, about six times per block: twice each in process and finalize (`prepare` computes the parent and next digests), once at commit and once in the per-block check. Every account is read each time, so block cost grows with the number of accounts. | `consensus_settlement.rs:916, 4950, 4966, 5025, 5868` |
| R2 | One `emission:event:{height}` record is added to consensus state per block and never removed, so the same scans also grow with chain height (P9). | `block_lifecycle.rs` |
| R3 | The commitment supports no proofs: a wallet, explorer or light client must trust the node it queries, and state sync (phase C) has nothing to verify chunks against. | — |

## Proposed rules

1. **Tree.** A sparse Merkle tree over SHA3-256 of each consensus state key,
   holding SHA3-256 of the value. The keys are exactly today's committed keys
   (`STATE_PREFIXES`, the individual state keys, and `governance:v2:` when
   governance is configured). Indexes, block records and receipts outside
   those prefixes stay out.
2. **Incremental.** Each block applies its writes and deletions to the tree;
   the root is computed from the changed paths only, O(changed keys × tree
   depth). No block reads the whole state.
3. **Application hash v2** = SHA3-256(`dytallix-app-v2` ‖ state root ‖
   anchor digest). The anchor keeps parent linkage and the input and result
   digests, as today.
4. **Checks.** The complete check (startup, or after an outside write)
   rebuilds the root from every committed key and compares it with the head;
   the per-block check compares the stored root with the head. A property test
   compares the tree root with a naive reference over the same keys.
5. **Hash.** SHA3-256, as `dytallix_protocol_types::sha3_256`. The tree
   brings no signature scheme or key exchange, so G35 is unaffected.

## Options

**Tree implementation.**

- **A. Jellyfish Merkle Tree (`jmt` crate).** A versioned sparse Merkle tree
  used in production by Aptos and Penumbra, with inclusion and non-inclusion
  proofs and export by version for state sync. It adds roughly a dozen crates
  (none cryptographic beyond hashes), which must pass the G35 dependency
  check, the E01 source inventory and the independent review. It is not in the
  local crate cache, so evaluating it needs a download from crates.io.
- **B. In-house binary sparse Merkle tree.** About 800 lines with SHA3-256 and
  no new dependency. More consensus-critical code to write and review, and
  versioned export for state sync would be ours to build.

**Proof queries at launch** (open question 2 of state model v2).

- **Yes.** An ABCI query returns a value with its proof against the committed
  root, so wallets and the explorer need not trust the node.
- **No.** Root only at launch; proof queries later.

**State sync (phase C).**

- **Next, before launch.** ABCI snapshots over the tree: a new validator
  joins from a verified snapshot instead of replaying from genesis.
- **POST MAINNET.** New validators replay every block from genesis.

## Steps

| Step | Content | Output change |
| --- | --- | --- |
| R-S1 (done) | `jmt` 0.12.0 pinned, default features off (no `ics23`, no SHA-2); tree records under `merkle:`; replaced nodes deleted at commit | None |
| R-S2 (done) | Application hash from the incremental root in prepare and commit; per-block and complete checks; the full scans removed from the block path | App hash format |
| R-S3 (done) | Proof query `/state/proof/{hex key}` | New query |
| R-S4 (done) | Tests | Tests |

**Implementation notes.**

- `state_tree.rs` wraps the tree. A key's leaf is SHA3-256 of its value; the
  tree stores the latest value hash per key (`merkle:value:`) and its nodes
  (`merkle:node:`). Only the latest version is kept: the nodes a block
  replaces are deleted in the same storage transaction. Proofs are therefore
  served at the committed height only, which is also the only height queries
  accept.
- The state digest in the block head is the tree root in hex. The
  application hash keeps its existing formula over that digest and the
  anchor (`dytallix-cometbft-app-v1`, and `dytallix-cometbft-genesis-v1` at
  genesis), so only the digest changed. This differs from rule 3, which
  proposed a new SHA3 application hash; the existing formula already binds
  the root and the anchor, and changing it would add no security.
- Genesis builds version 0 from every genesis entry. Each block applies its
  selected writes and deletions as the next version; a block cannot write
  `merkle:` records itself.
- The complete check rebuilds the root in memory from every committed entry,
  compares it with the stored root and the head, then checks every entry's
  proof against the stored tree and that the tree holds no other leaf. The
  per-block check reads the stored root only.
- Emission events (R2) are no longer read per block; they still add one
  small record per block to storage.
- `jmt` adds 18 packages (serialization, hashing traits, error and tracing
  crates); the G35 dependency check passes at 132 packages and the module
  boundary check passes.
- For phase C: a snapshot at height H must capture state as of H (for
  example a storage checkpoint), because the tree keeps only the latest
  version; restore rebuilds the tree and checks its root against H's
  application hash.

## Tests

- Tree: incremental roots equal a rebuilt reference through 120 blocks of
  random inserts, updates and deletions; presence and absence proofs verify
  and forged values fail; the stored-tree check refuses a changed value, a
  missing key and a corrupted record; stored records stay near the live key
  count.
- Engine: committing ten blocks reads no committed state entry for the
  digest, and the committed root equals a rebuild; a proof for a committed
  balance verifies and, with the anchor, reproduces the committed
  application hash; an absent key has a non-existence proof; keys outside
  consensus state and malformed keys are refused.
- The existing suite runs on the new digest.

## Decisions (P01, approved 27 September 2026)

1. Tree implementation: A, the `jmt` crate, if its dependency graph passes
   the G35 check.
2. Proof queries at launch: yes.
3. State sync (phase C): the next step, before launch.
