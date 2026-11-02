# Crypto-agility spike: adding a hash-based signature algorithm

Status: spike on branch `spike/slhdsa-blast-radius`, 2 November 2026. Not for
merge as is. Mainnet stays **NO GO** and mainnet policy is unchanged: every
mainnet check still accepts ML-DSA-65 only.

## Question

If ML-DSA (lattice) had to be replaced, how much of the stack would have to
change, and what would the replacement cost at run time? The spike adds FIPS
205 SLH-DSA, a hash-based scheme that survives a lattice break, as a second
algorithm in two places:

- **Accounts:** SLH-DSA-SHAKE-192s as algorithm code 3 (category 3, like
  ML-DSA-65), through the ordinary, ordinary-v3, recovery, sponsor and fee
  codecs and verifiers.
- **Validators:** SLH-DSA-SHAKE-192f as a CometBFT key type in the engine fork.

## Result in one paragraph

Accounts can take a second algorithm with a contained change. After this
spike's refactor it takes a registry row, a verifier arm, and an address
variant with its name mapping, plus the policy constant once mainnet approves
the algorithm. Before the refactor, the same change touched 27 places:
**10 lookup functions, 2 fixed key-size limits and the `OriginKeyAlgorithm`
enum in `protocol-types`, 3 tables in the node, 4 separate verifier
dispatches, and 7 copies of the mainnet "ML-DSA-65 only" rule**. Validators are the hard part. Hash-based votes
multiply commit size about 10 times and take 118 ms to sign, and they make the
engine's default block budget invalid. Part 2 adds the validator proto field,
SDK and wallet support, a lattice-independent peer transport, and an
in-process drill. The drill swaps the validator set to SLH-DSA, halts,
root-signs a checkpoint and restarts from a replacement genesis. The full
stack (bridge, engine loader, Rust app) still refuses every step of that
restart; see [Part 2](#part-2-wallet-transport-and-client-channel) and
[the drill](#drill-halt-re-key-and-restart).

## What changed

| Area | Before | After the spike |
|---|---|---|
| Algorithm tables (`protocol-types`) | 10 separate `match` lookups (name to code and code to name) in `ordinary`, `ordinary_v3`, `recovery_wire`, `recovery_sponsor`, `ordinary_fees`, and two `+ 2592` key-ID buffer limits | One registry, [`algorithms.rs`](../node/crates/protocol-types/src/algorithms.rs); the old functions delegate to it with their error messages unchanged |
| Verifier dispatch (`runtime-crypto`) | 4 copies of the ML-DSA-65 decode-and-verify block, one per account verifier | One `verify_exact` in [`pqc_verify.rs`](../node/crates/runtime-crypto/src/pqc_verify.rs); `ordinary`, `ordinary_v3`, `recovery` and `recovery_sponsor` call it. The older `pqc_verify::verify` API keeps its own ML-DSA-65-only path |
| Mainnet policy | `network == 1 && algorithm != "mldsa65"` in 4 verifiers, plus 3 checks in `ordinary_state.rs` | `algorithms::MAINNET_APPROVED` (still `["mldsa65"]`), read by all 7 |
| Node configuration tables | Key-length tables in `ordinary_state.rs` and `recovery_fees.rs`; the name-to-origin map in `ordinary_authority.rs` | Read the registry; origin map has the new variant |
| Address origin | `OriginKeyAlgorithm` without SLH-DSA | `SlhDsaShake192s`, code 3; the address commits to it |
| Validator key (Go fork) | `mldsa65` only in keytypes, codec, ABCI names and `MaxSignatureSize` | New [`crypto/slhdsa192f`](../node/consensus/cometbft/upstream/crypto/slhdsa192f/key.go) registered in all four. Part 1 left the proto oneof unchanged; Part 2 adds `slhdsa_shake_192f = 6` and both codec directions |
| Dependencies | `fips204` | Adds `fips205` 0.5 (Rust) and CIRCL `sign/slhdsa` to the engine's `go.sum` (already used by the root helper) |

Part 1 changed 29 files under `node/`, 4 of them new, plus this report. After Part 2 the branch changes 64 paths: 39 under `node/`, 24 under `sdk/` (most of them the re-vendored `protocol-types`) and this report. The engine's `go.mod` gains `golang.org/x/crypto` as an indirect dependency, because CIRCL's `slhdsa` uses `x/crypto/cryptobyte`.

## Measurements

Apple Silicon, release builds. Rust: `fips204` 0.4.6 and `fips205` 0.5.0.
Go: CIRCL 1.6.3. Single core, averages of 5 runs.

| Algorithm | Public key | Signature | Sign | Verify |
|---|---:|---:|---:|---:|
| ML-DSA-65 (today) | 1,952 B | 3,309 B | 0.21 ms | 0.08 ms |
| SLH-DSA-SHAKE-128f | 32 B | 17,088 B | 38 ms | 2.2 ms |
| SLH-DSA-SHAKE-128s | 32 B | 7,856 B | 778 ms | 0.77 ms |
| SLH-DSA-SHAKE-192f | 48 B | 35,664 B | 60 ms (Go: 118 ms) | 3.2 ms (Go: 6.4 ms) |
| SLH-DSA-SHAKE-192s | 48 B | 16,224 B | 1.40 s | 1.2 ms |
| SLH-DSA-SHAKE-256f | 64 B | 49,856 B | 130 ms | 3.4 ms |
| SLH-DSA-SHAKE-256s | 64 B | 29,792 B | 1.26 s | 1.8 ms |

- **Accounts.** An ordinary-v2 transaction with one `Send` signed with
  SLH-DSA-SHAKE-192s is 16,570 bytes, against 3,559 bytes with ML-DSA-65 (computed from the
  same framing, not measured). A 1.4 s signature is acceptable for a wallet.
  The qualification fixture's mempool cap (262,144 bytes) admits it. The
  production `max_tx_bytes`, the account roles' `max_wire_bytes` and the
  per-signature gas cost are still unset and must account for it.
- **Validators.** The "s" sets take over a second to sign and cannot keep up
  with voting. With SLH-DSA-SHAKE-192f, one commit carries:

  | Validators | ML-DSA-65 | SLH-DSA-SHAKE-192f | Verifying one commit, one core |
  |---:|---:|---:|---:|
  | 4 | 0.01 MiB | 0.14 MiB | 26 ms |
  | 21 | 0.07 MiB | 0.72 MiB | 134 ms |
  | 50 | 0.16 MiB | 1.70 MiB | 320 ms |
  | 100 | 0.32 MiB | 3.40 MiB | 640 ms |

  Every block stores its last commit, and light clients download it.
  Raising `MaxSignatureSize` to 35,664 also raises `MaxCommitSigBytes`, which
  sizes the default block budget (`DefaultBlockParams`) for 10,000 validators.

## Tests

| Suite | Baseline (HEAD `6505993`) | Spike |
|---|---|---|
| Node Rust workspace (`cargo test --workspace --locked`) | 1,012 passed, 0 failed, 34 ignored | First run: 1,015 passed, 3 failed, 35 ignored. One test asserted code 3 is invalid and now uses 4; the other two (`root_genesis::tests::helper_*`) timed out while the Go suites shared the CPU and pass on rerun |
| Spike tests (`runtime-crypto/tests/slhdsa_agility.rs`) | — | 5 passed, 1 benchmark ignored by default |
| Engine Go module (`go test ./...` in `node/consensus/cometbft`) | 17 of 17 packages ok | 14 of 17 ok. `dytallix-comet-fixture`, `enginepqc` and `lightblocks` fail: `block.MaxBytes is too big. 379150190 > 104857600` |
| Engine upstream packages (types, crypto, keytypes, privval, state, light, consensus, evidence) | 18 of 18 ok | 16 of 19 ok (the new `crypto/slhdsa192f` passes). `types`, `state` and `consensus` fail on the same block budget, plus expected commit sizes computed for ML-DSA-65 |
| SDK (`cargo test --workspace --locked`) | 243 passed | Not run in Part 1, which did not change the SDK. See Part 2 |

The spike tests show:
- A development-network transaction signed with SLH-DSA verifies.
- The same transaction on mainnet is refused (`UnsupportedAlgorithm`).
- Altered and truncated signatures are refused.
- An account role that does not list SLH-DSA refuses it.
- The address commits to algorithm code 3, so the same 48 bytes cannot pose as
  an ML-DSA key.

The SLH-DSA tests cover the ordinary-v2 verifier only. The ordinary-v3,
recovery and sponsor verifiers now share `verify_exact` and the mainnet
constant; for those three, only their existing ML-DSA-65 tests ran.

## Main finding: the default block budget breaks

Raising `MaxSignatureSize` to SLH-DSA-SHAKE-192f's 35,664 bytes is enough to
make the engine's default consensus parameters invalid. `DefaultBlockParams`
budgets 21 MiB of data plus a commit for `MaxVotesCount` (10,000) validators at
`MaxSignatureSize`: 379,150,190 bytes, above `MaxBlockSizeBytes` (100 MiB).
Every failing Go test traces to this or to expected sizes derived from it.

Fixing it is a protocol decision, not a code fix, so the spike leaves it
failing.

**The evidence budget breaks too.** The approved production value
`engine_evidence_max_bytes` is 65,536 bytes
(`approvals/P01_E05_VALUES_4_2026-10-02.json`). The drill measures one
duplicate-vote equivocation as protobuf evidence:

| Validator key | Duplicate-vote evidence |
|---|---:|
| ML-DSA-65 | 6,886 B |
| SLH-DSA-SHAKE-192f | 71,601 B |

The evidence pool (`upstream/evidence/pool.go:383`) walks pending evidence
oldest-first and stops at the first item that does not fit. Under today's
value, SLH-DSA equivocation evidence would therefore never be proposed, so a
double-signing validator could never be punished. It would also block every
newer piece of evidence until it expired. Block size, validator count and
evidence size must be decided together. The options are to cap the validator count for the chain, to set an
explicit block budget in genesis instead of the default, or both. Whichever
is chosen must be in the release before an incident, because consensus
parameters change through the same restart this would be needed for.

## Gates that caught the change (working as intended)

- `scripts/e01_inventory.py --check` flags all 11 changed crypto-routed source
  files until the inventory is reviewed and rewritten.
- `sdk/scripts/check_protocol_vendor.py --node-root ../node` fails with
  `source hash differs: .../address.rs`: the SDK's byte-for-byte copy of
  `protocol-types` must be re-vendored with its manifest.
- The engine's `go test -mod=readonly` fails at setup until `go.sum` lists
  CIRCL's `sign/slhdsa` package, so the consensus Go graph changes visibly.

Two gates **pass** and so did not notice the change:
`check_consensus_cargo_profile.py` (111 packages, no prohibited matches, no
unexpected FIPS 204 features) and `check_consensus_go_graph.py`. Both ban
known-bad packages rather than allow-listing approved ones. A new signature
crate (`fips205`) and a new module (`golang.org/x/crypto`) therefore enter the
consensus graph without review. An allow-list of approved cryptographic
packages would catch both.

## Blockers found in Part 1

Part 2 resolves 1 and 3, partly resolves 4 and exercises 5 in a test; see the
status after each.

1. **Validator keys cannot cross ABCI.** `keys.proto`'s `PublicKey` oneof needs
   a new field, and `keys.pb.go` must be regenerated with gogoproto. Part 1
   said no toolchain was installed. That was wrong: `protoc` is installed, and
   `protoc-gen-gogofaster` builds offline from the module cache
   (`gogoproto@v1.7.2`). Regenerating the unchanged file reproduces
   `keys.pb.go` byte for byte. **Resolved in Part 2:** field
   `slhdsa_shake_192f = 6`, and both codec directions.
2. **Validator registration and rotation are ML-DSA only.** The node meters
   `validator_proof(..., "mldsa65")` (`ordinary_execution.rs`), and the
   validator role profile is fixed to `{"mldsa65"}` (`ordinary_state.rs`).
3. **The SDK and wallet.** 12 SDK source files name `mldsa65`. The CLI rejects
   other schemes, and the SDK's `KeyScheme::SlhDsa` is legacy SPHINCS+-192s,
   not FIPS 205. Its 48-byte keys look like SLH-DSA-SHAKE-192s keys and must
   never be relabeled as them. **Resolved in Part 2** for the Rust SDK and
   CLI; the browser wallet is unchanged.
4. **Transport.** `pqcp2p` and the client channel each have one fixed suite
   (ML-KEM-768 + ML-DSA-65) and no negotiation. **Part 2:** `pqcp2p` wire
   version 3 mixes a per-pair PSK; the client channel is assessed and left
   unchanged.
5. **Restart under a new algorithm.** Cross-binary upgrades are not
   qualified, and nothing re-keys a validator set from a halted height with
   root authority. Root keys (SLH-DSA-SHAKE-256s) are the only authority that
   survives a lattice break, and they have no replacement procedure.
   **Part 2:** the drill runs this path in the engine fork's own consensus
   code. The full stack still refuses it.

## Recommendations

1. Keep the registry, the single `verify_exact` dispatch and the
   `MAINNET_APPROVED` constant whatever happens to the rest of this spike.
   They turn the next account algorithm from 27 edits into 4, with no
   change to mainnet behavior.
2. Before genesis, add the validator proto field for a hash-based key, behind
   configuration, so it never needs regenerating during an incident. Part 2
   does this on the spike branch.
3. Before genesis, let each account and validator commit to the SHA3-256 hash
   of an SLH-DSA backup key, activatable only by root authority. Accounts that
   have spent expose their ML-DSA keys; this is their only lattice-free proof
   of ownership after a break.
4. Run the devnet drill: halt at H, root-signed checkpoint, re-keyed
   SLH-DSA-SHAKE-192f validator set, restart, migrate accounts, resume. Part 2
   runs it in process with 4 validators. The full-stack version (Linux or
   Docker) and 21 validators remain.
5. Decide validator parameters from those numbers: 128f halves commit size
   at category 1, and stateful hash-based schemes (LMS/XMSS, SP 800-208) give
   signatures of roughly 2.5 KB (not measured here) but need state that a restore must never reuse.

# Part 2: wallet, transport and client channel

## SDK and wallet

The SDK can now hold and use a FIPS 205 account key in development builds.

- **New scheme.** `KeyScheme::SlhDsaShake192s` (`slhdsa-shake-192s`) sits
  beside ML-DSA-65 in `dytallix-core`, behind a `fips205` feature that the
  default (`compatibility`) build enables. The strict ML-DSA-65 graph
  (`strict-local-mldsa65`) does not contain `fips205`, as `cargo tree`
  confirms.
- **Legacy look-alike.** The legacy `KeyScheme::SlhDsa` is SPHINCS+-192s and
  has exactly the same lengths (48, 96 and 16,224 bytes). The new scheme is
  never chosen by length. `from_private_key` still reads 96 bytes as legacy;
  a test shows that the legacy verifier rejects a FIPS 205 signature, and that
  only `from_keypair(KeyScheme::SlhDsaShake192s, ...)` imports the new key.
- **Keystore.** The keystore accepted only ML-DSA-65
  (`require_production_operational`), and its entries carry a legacy D-Addr
  derived only from 1,952-byte keys. It now:
  - accepts ML-DSA-65, and SLH-DSA-SHAKE-192s in builds that include it
    (`require_keystore`);
  - derives the entry address with `DAddr::from_scheme_public_key`. ML-DSA-65
    addresses are unchanged. An SLH-DSA key hashes its algorithm name with
    the key, so 48 bytes never give one address under two algorithms.
  Legacy SPHINCS+ keys are still refused.
- **CLI.** `dytallix wallet create --scheme slhdsa-shake-192s` and
  `dytallix crypto keygen slhdsa-shake-192s` create the key. The old
  `slh-dsa` name is still refused and now points to the new one. Ordinary
  signing, consensus key identities and manifest checks accept the new
  algorithm; the strict CLI build refuses it.
- **Protocol types.** The SDK's vendored `protocol-types` was re-synced with
  `sync_protocol_vendor.py`, and `check_protocol_vendor.py --node-root ../node`
  passes again.
- **Tests.** `sdk/crates/dytallix-sdk/tests/slhdsa_fallback.rs`: key
  generation, the legacy look-alike guard, possession-checked import, the
  encrypted keystore round trip (and refusal of legacy SPHINCS+), and a
  development ordinary-v2 transaction signed and verified, with tampering
  rejected.

Still open: the SDK keeps its own copies of the algorithm tables
(`origin_algorithm` and `verify_signature` in `ordinary_v2.rs`, `exact_scheme`
in the CLI); the browser wallet (`ordinary-browser`) is unchanged; and wallet
custody is not production qualified.

## Peer transport (pqcp2p wire version 3)

A pre-shared key per pair of pinned peers now goes into the transport's key
schedule. This is the one change that keeps the validator network private and
authenticated if both ML-KEM-768 and ML-DSA-65 fail, with no new public-key
algorithm.

- **Protocol.** Suite
  `dytallix-pqcp2p-component-v3/mlkem768/mldsa65/psk/hkdfsha256/aes256gcm`,
  `WireVersion = 3`. HKDF input is now the ML-KEM secret followed by the
  32-byte PSK (both fixed lengths). The traffic keys and both
  key-confirmation keys come from that one HKDF call, so a peer without the
  PSK fails confirmation even if it can forge every ML-DSA signature.
- **No downgrade.** Version 2 is refused. If a node accepted both, an attacker
  holding an ML-DSA forgery would simply speak version 2 and skip the PSK.
  Moving to version 3 is therefore a single network-wide switch.
- **API.** `NewInitiatorPSK`, `NewResponderPSK` and `UpgradePSK` take the key;
  the old functions call them with the zero PSK. The responder looks up the
  key from the initiator's full pin in the hello, before any key generation.
- **Engine.** An optional owner-only file `config/pqc_peer_psk.json`
  (`{"version":1,"keys":{"<full pin, base64>":"<32 bytes, base64>"}}`) is
  loaded next to the peer seed. Each key must name a pinned peer and be 32
  nonzero bytes. A pair with no entry uses the zero PSK.
- **Tests.** `internal/pqcp2p/psk_test.go`:
  - matching PSKs carry traffic;
  - a peer holding the real ML-DSA identity (standing in for a forgery) with
    a wrong or zero PSK is refused;
  - a recovered ML-KEM secret without the PSK does not reproduce the keys;
  - a version 2 header is refused.

  `internal/enginepqc/peer_psk_test.go` checks the file rules and runs the
  same accept and refuse cases through the engine's own `Upgrade`.

Still open:
- The PSK file is optional; a production profile should require it.
- PSK provisioning: one key per pair, generated once and sealed into both
  hosts' bundles. It must be added to `host_keys.py` `SECRETS`, the expected
  `secret_files` in `host_files.py` and the supervisor's engine-input set
  (`native-supervisor/src/config.rs`).
- Suite strings in `EXPERIMENTAL_PROTOCOL.md`, `PQC_ARCHITECTURE.md` and
  `interfaces-v1.json` still name version 2.
- A responder still generates an ML-KEM key and signs before the PSK is
  proven; the existing handshake slot cap bounds that.

## Client channel: assessment, no code change

The client channel (`node/crates/client-channel`, used by the SDK and the
supervisor probe) authenticates the endpoint with ML-DSA-65 and exchanges keys
with ML-KEM-768. The client is anonymous.

- **A PSK cannot protect it.** A key shared with every public client is
  public.
- **What a break costs.**
  - Breaking ML-KEM alone exposes traffic. On this chain that is
    transactions, which are public once included, and queries.
  - Forging ML-DSA alone lets an attacker impersonate the endpoint. Clients
    could then be fed false state or have transactions dropped. Transactions
    stay safe, because they are signed by the account key.
- **The fix is above the channel, not in it.**
  - Transactions are already self-authenticating.
  - Reads already come with state proofs: a SHA3 Merkle path to the app hash
    (`protocol-types/src/state_proof.rs`). That path is hash-based. What is
    lattice-based today is how a client trusts the app hash (validator
    signatures).
  - After a break, a root-signed (SLH-DSA) checkpoint of the app hash, as in
    the drill below, gives clients a lattice-free anchor. The channel then
    only needs to deliver bytes.
- **Dual-signing the handshake was measured and rejected for now.** Adding an
  SLH-DSA signature beside ML-DSA in the endpoint's offer does fit the
  65,535-byte frame:

  | Offer contents | Size |
  |---|---:|
  | Today | 4,429 B |
  | With SLH-DSA-SHAKE-128f added | 21,517 B |
  | With SLH-DSA-SHAKE-192f added | 40,093 B |

  But the endpoint would sign before the anonymous client proves anything.
  At 38–60 ms per signature, one core serves about 16–26 handshakes a second,
  so it becomes a cheap denial-of-service target. It would also need a pin
  file version 2, new suite and vectors in Rust and Go, and a re-vendor.
- **Confidentiality after a full break** needs a KEM that is not
  lattice-based (HQC, or Classic McEliece). There is no implementation in the
  current dependency graph. CIRCL has only lattice KEMs and X25519 hybrids,
  which the no-classical rule excludes.

## Drill: halt, re-key and restart

[`consensus/lattice_drill_test.go`](../node/consensus/cometbft/upstream/consensus/lattice_drill_test.go)
in the engine fork runs the lattice-break response in process. One real
consensus `State` runs with the kvstore application; the other validators are
signing stubs. Networked reactors are skipped in this fork's tests
(`SkipWithoutPQCUpgrade`), so they are not exercised. Run it with:

```sh
cd node/consensus/cometbft/upstream
go test -mod=readonly -run TestLatticeBreakDrill -v ./consensus/
```

**What it does**

1. **Live swap.** Four ML-DSA-65 validators run the chain. Block 2 carries
   eight validator updates: four SLH-DSA-SHAKE-192f keys added and four
   ML-DSA-65 keys removed. CometBFT applies updates two heights later, so
   blocks 2 and 3 are still ML-DSA-signed. The node's own key moves to
   SLH-DSA with its first vote at height 4.
2. **Hash-based consensus.** From height 4 every validator is SLH-DSA. For each
   height the test checks:
   - every commit signature is 35,664 bytes;
   - equivocation evidence is measured (see the evidence budget above);
   - `VerifyCommit` passes;
   - the header's `validators_hash` matches the stored set.
3. **Halt and checkpoint.** Signing stops after height 6. One freshly
   generated SLH-DSA-SHAKE-256s key (the root authority's algorithm), called
   through CIRCL directly, signs a checkpoint. This shows what the payload
   must bind and what it costs. It is not a qualified root authorization: it
   does not use the root-authorization helper, its envelope, sequence
   numbers or the 3-of-5 threshold. The checkpoint binds:
   - the chain ID and halted height;
   - block 6's hash and the app hash after it;
   - the next validator set's hash;
   - the restart height.
   A checkpoint with a substituted validator hash fails verification.
4. **Replacement genesis.** A new genesis starts at height 7 with the SLH-DSA
   set and the checkpoint's app hash, over a fresh application process opened
   on the same application data. CometBFT's handshake skips `InitChain`
   because the application already has state, and only checks the app hash.
   - A genesis with any other app hash is refused (the handshake panics).
   - With the checkpoint's hash, heights 7 and 8 commit under SLH-DSA.
5. **The broken hash chain.** Block 7 carries the checkpoint's app hash, but its
   `LastBlockID` is empty: nothing in the chain links it to block 6. The test
   asserts this. Only the root-signed checkpoint joins the two histories.

**Measured** (4 validators, Apple Silicon, CIRCL):

| | ML-DSA-65 (height 3) | SLH-DSA-SHAKE-192f (heights 4–6) |
|---|---:|---:|
| Commit size | 10,152 B | 107,222 B |
| Three votes signed and added, per height | — | 0.40–0.58 s |
| Root checkpoint signature (SLH-DSA-SHAKE-256s) | — | 29,792 B, 1.1 s |

**What the full stack still refuses.** The in-process drill shows the
consensus engine itself can do this. The production path cannot yet:

- **Halting.** There is no halt-height option anywhere. Operators stop the
  processes after H commits.
- **The comet bridge**
  (`cmd/dytallix-comet-bridge/application.go`) requires validator key type
  exactly `[ml_dsa_65]` in genesis parameters (:118), in initial validators
  (:328) and in updates (:349, :356). It also requires initial height 1
  (:114).
- **The engine loader** (`internal/enginepqc/engine.go`) requires:
  - `InitialHeight == 1` (:384);
  - ML-DSA-65 key types and genesis validators (:391, :395);
  - an ML-DSA-65 `priv_validator_key.json` (:519).
- **The Rust app** checks that the engine's validator set matches its stored
  configuration (`consensus_settlement.rs:5040`) and refuses any change to
  that configuration (:877). It hard-codes 1,952-byte ML-DSA validator keys
  (:459, `validator_lifecycle.rs:298`) and `pubkey_type: "ml_dsa_65"`
  (`validator_lifecycle.rs:464, :721`). It has no app-state export or import
  at a height.
- **Policy.** The restart authorization (`release_handover/restart.rs`)
  switches the release at H and nothing else. It has no validator set and no
  initial height. `halt.md` forbids a replacement genesis or a change of
  chain identity. Using this path needs a new schema version carrying the
  checkpoint fields above, and a P01 decision against that rule.
- **The live swap is not a response to practical forgery.** The new
  validator set is itself authorized by ML-DSA: block 2 carries the updates
  and is ML-DSA-signed, and so is block 3. If ML-DSA is
  already forgeable, an attacker can put its own keys into that update. The
  live swap fits only the "security margin eroded" case. Practical forgery
  needs the halt and the root-signed replacement-genesis path.
- **Not covered by the drill:** the networked p2p reactors, account migration,
  and 21 validators.

**Recommended path to the full-stack drill.** Fork the existing Linux
orchestrator (`native-supervisor/src/bin/state_sync_join.rs`, run on macOS
through `tools/state-sync-join/run-in-docker.sh`). First make the bridge,
engine and app accept an SLH-DSA validator set and a restart height behind a
development-only flag. Then define the restart-authorization fields this test
signs.

## Part 2 tests and gates

| Suite | Baseline | After Part 2 |
|---|---|---|
| SDK (`cargo test --workspace --locked`) | 243 passed | 247 passed, 0 failed |
| CLI, strict ML-DSA-65 build | — | 20 passed |
| CLI, HTTP-only build | — | 19 passed |
| `pqcp2p` | ok | ok, including 4 new PSK tests |
| `enginepqc` | ok | 2 new PSK tests pass. The package still fails on the block-budget test from Part 1 |
| Drill (`TestLatticeBreakDrill`) | — | passes: 5 runs in a row, inside the full `consensus` package, and 3 runs under `-race` with no data race reported |
| Engine Go module | 17 of 17 ok | 14 of 17. Same three failures as Part 1, all the block budget |
| Engine upstream packages | 18 of 18 ok (Part 1 subset) | 21 of 24 ok over a wider subset (adds `abci/...`). `types`, `state` and `consensus` fail as in Part 1; the drill passes |

The Go failures after Part 2 are exactly the Part 1 set: the same test names,
all `block.MaxBytes is too big`. Part 2 adds none.

An early version of the drill was flaky. It submitted the swap transactions
at the start of height 2, after a self-proposed block 2 was already built, so
the swap moved a height. It now submits them once block 1's proposal is
complete and asserts block 2 carries all eight. Its waits are 30 seconds instead of the
shared 200 ms helper timeout. Under the race detector, the node's own
verification of the SLH-DSA votes is slower than 200 ms.

**Gates after Part 2**

- `e01_inventory.py --check` flags 14 changed crypto-routed files, now
  including `pqcp2p/connection.go`, `pqcp2p/establishment.go` and
  `enginepqc/engine.go`.
- `check_interface_inventory.py` refuses the transport change:
  `symbol 'dytallix-pqcp2p-component-v2' not in .../establishment.go` and
  `WireVersion = 2 not in .../connection.go`.
- **Gap:** E01 does not route the engine fork's validator key plumbing. The
  regenerated `keys.pb.go`, `crypto/encoding/codec_pqc.go`,
  `types/signable_pqc.go` and the new `crypto/slhdsa192f` package all changed
  without it noticing. These files decide which validator signatures
  consensus accepts and belong in the inventory.
- `check_protocol_vendor.py --node-root ../node` and the SDK's
  `check_no_classical.py` pass. `check_consensus_go_graph.py` still passes.
