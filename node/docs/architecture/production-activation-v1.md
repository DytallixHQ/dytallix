# Production activation v1

Engineering task E05, step e: the design for letting a node run a production
chain. Approved by P01 on 30 September 2026
(`launch/approvals/P01_E05_ACTIVATION_2026-09-30.json`). This document
selects the design and the order of work; it changes no code, and it does
not authorize a launch.

## Problem

Nothing can start a production chain today. More than 60 checks across the
node, engine, supervisors and tooling refuse production profiles, paths and
chain IDs. About half are name checks over complete code; the rest guard
paths that do not exist yet: a production root genesis, upgrade schema 2, a
production handover, a production supervisor mode and a production transport
profile. Every name and value is permanent at genesis, because the node
stores the exact configuration bytes and the root receipt and compares them
on every start.

## Decisions (P01, 30 September 2026)

1. **The switch is a signed genesis on a production build.** Release
   binaries are built without the development entry points. A node runs the
   production profiles only when the root genesis signatures verify over the
   exact genesis files and release. Root signers sign only after the P02
   review, E06 release acceptance and gate acceptance, so no unsigned
   production chain can start and no development path ships in a release.
2. **Root genesis is signed 3-of-5 by its own group** of five genesis
   signers, separate from the emergency and upgrade custodians.
3. **Upgrades and handovers sign over an anchored window**, the scheme
   emergency freeze v2 uses: a finalized anchor block and a bounded height
   window, so custodians have time to sign.
4. **The upgrade custodians control handover and restart**: the same five
   keys, three of five.
5. **Validators allow local Unix sockets only**: no network listener of any
   kind; owner-only sockets serve the supervisor and the console operator
   tool.
6. **The engine is checked at startup, then held by kernel limits**
   (AppArmor, seccomp, no-new-privileges), with no periodic pauses. Helpers
   and other short processes keep their paused checks.
7. **One IP per node**: every node listens on and sends from one address;
   public sentries use static 1:1 NAT. The engine's connection check is
   unchanged.
8. **A published partial mesh within 64 pins**: each sentry pins its own
   validator, the endpoints it serves and a fixed set of other sentries. The
   pin plan is a public record released with each network configuration.

Earlier decisions this design implements: the separate 3-of-5 upgrade
custodians and the 120,960-block notice (D11-Q03), the SLH-DSA-SHAKE-256s
parameter set, the node check keeping a basic transfer between 0.1 and 10 DRT
under governed fee changes, private validators behind sentries (D12-Q01),
console-only operator access, and operator-supplied light blocks for state
sync (state sync v1).

## Design

### A. Builds and the switch

- **Production build.** A `production` cargo feature (Rust) and build tag
  (Go) compile the production entry points and omit the development ones:
  `--development-root-config`, `--development-emergency-verifier-config`,
  `--development-candidate-config`, `--candidate-staging`, the loopback and
  private-seed transport profiles, and the fixture commands. Development
  builds keep today's paths for tests and staging and cannot start a
  production profile.
- **Mutual exclusion.** Each production check refuses development profiles,
  and each development check refuses production profiles, so neither build can
  open the other's chain.
- **Chain identity.** A production chain may name mainnet; a development or
  staging chain still may not, so no staging chain can pass for mainnet.
- **The root signatures are the authority.** The production entry verifies
  three of the five genesis signatures over the root bundle (application
  genesis, configuration bytes, engine genesis SHA-512, release manifest
  SHA-512) before opening, and stores a production receipt. Without them the
  node does not start.

### B. Profiles

New permanent names replace the local-qualification and development ones in
production builds: the consensus, lifecycle and penalty profiles, and the
reward and issuance profiles. Penalties' `production_activation` becomes true
with no other change (penalties v1 is complete); the emergency automatic
transition policy keeps its approved rule (continue previously approved
mandatory transitions) under a production name. The genesis builder gains a
production mode emitting these names, and the binding review accepts them.

### C. Root genesis, 3-of-5

- A genesis policy with five SLH-DSA-SHAKE-256s keys and threshold 3, keys
  disjoint from the emergency and upgrade sets; one envelope per signer over
  the same bundle digest; a receipt version 2 that lists the signing key IDs.
- An offline signer for custodians, producing one envelope from the bundle
  digest on the custodian's own device; no key leaves the custody system.
- The `production_qualified` field retires in a helper wire version 2. A
  production build accepts a VERIFIED result only from the helper pinned by
  the accepted release catalog; qualification is a property of the release
  (E06, T03), not of one helper run.

### D. Upgrade schema 2 and handover v2

- **Upgrade schema 2** is a new retained module beside v1, with a registry
  entry; v1 stays byte for byte for replay. It enforces five keys and
  threshold 3, disjoint from the emergency keys; activation at least 120,960
  blocks after admission; and anchored signing: each signature binds a
  finalized anchor (height and hash) and a window, within bounds measured with
  the emergency ones.
- **Handover v2** uses the same anchored window and the upgrade authority's
  keys and threshold, which the configuration check requires to be identical.
  Handover, restart and upgrade keep their distinct signing domains.
- **Restart** keeps its exact-height binding: the chain is halted, so there is
  no race.
- The registry change alters the migration registry digest that release
  manifests bind, so schema 2 lands before the E06 freeze and the root
  ceremony.

### E. Emergency

Freeze v2 is complete. Production builds accept `development_only: false`.
The validity window and anchor age come from measurements on dedicated hosts
(E03, T02, T03).

### F. Supervisor production mode

- **Roles.** A production mode with a node role: validator, sentry or
  endpoint.
  - Validators run no adapter or channel listener.
  - Endpoints run the HTTP adapter and client channel.
  - Sentries run neither.
  - Only validators load a validator signing key; sentries and endpoints use
    a key the engine refuses to find in the genesis set.
- **Identity.** Peer identity comes from the seed file (`pqc_peer_seed.bin`),
  generated on the host. A production peer-seed tool replaces `node_key.json`,
  which the production transport refuses.
- **Network.** The P2P listener is the node's single IP. With 1:1 NAT a
  sentry's public address is its only address, so the engine's same-IP check
  holds unchanged.
- **State sync.** Sentries and endpoints join by state sync from
  operator-supplied light blocks (state sync v1); the supervisor plumbs
  `--light-blocks`. Readiness waits for catch-up instead of failing after 60
  seconds.
- **Observation.** The engine is checked once at startup, then left to kernel
  enforcement with no pauses. Helpers keep paused checks under a measured
  budget.
- **Catalogs.** Production service profiles in the release catalog and the
  policy renderer, with a routed network mode and host firewall rules that
  drop unpinned sources before they reach the handshake.
- **Python supervisor.** `deploy/pqc-engine/supervise.py`, which cannot start
  the owner-guarded engine, is retired.

### G. Transport and the pin plan

- A production transport profile generalizes the production-candidate one:
  explicit IP literals (IPv4 or IPv6), strict admission, no discovery, no
  browser origins, seed identity.
  - Validators pin only their own sentries.
  - Sentries pin their validator, the endpoints they serve and a fixed set of
    other sentries.
  - Every node stays within 64 pins, enough for 32 validators with two
    sentries each.
- The pin plan is a public record: keys, addresses and each node's pin set,
  published with each network configuration. The engine binding (today's
  production-candidate binding, extended to the peer and validator public
  keys) ties each host's files to it, and the supervisor checks it at start.
  A pin change is a reviewed configuration update and a restart.

### H. Other approved checks

- **Fee range.** A fee-profile governance proposal is refused unless a
  reference basic Send, at fixed sizes, costs between 0.1 and 10 DRT under the
  proposed values.
- **Genesis size.** The engine's 2 MiB genesis bound rises to carry the 8 MiB
  application genesis the rest of the stack allows.

## Order of work

Each step is one PR from main, tested on development builds and on
production-profile staging chains signed with test keys:

1. **A1 builds and profiles:** production features and build tags, profile
   constants, mutual exclusion, the chain-ID rule.
2. **A2 root genesis 3-of-5:** threshold policy and receipt v2, the
   production entry, the offline signer, helper wire v2.
3. **A3 upgrade schema 2 and handover v2:** anchored windows, notice, the
   shared authority, registry v2.
4. **A4 production controls:** emergency, upgrade and handover accept
   production; the engine's production transport profile and extended
   binding.
5. **A5 supervisor production mode:** roles, seed identity, state sync,
   readiness, observation, catalogs and host rules; retire the Python
   supervisor.
6. **A6 fee range and genesis size.**
7. **A7 builder and review:** the genesis builder's production mode, and a
   production-profile rehearsal on a staging chain reviewed by
   `check_bindings.py`.

## Status

**A1 done** (builds and profiles):
- **Rust feature `production`** (`dytallix-fast-node`, `dytallix-native-supervisor`).
  - `consensus_stdio` drops its `--development-*` flags.
  - The library's development entry points refuse to run.
  - The supervisor has no mode until A5.
  - Test and qualification-only features fail to compile beside it.
- **Go build tag `production`.**
  - The engine refuses every development and staging transport profile and the
    candidate staging mode.
  - The fixture and peer-probe commands are excluded.
- **Profiles** (`src/build_profile.rs`). Each build accepts only its own names:

  | Profile | Production name |
  | --- | --- |
  | Consensus and penalty | `cometbft-production-v1` |
  | Lifecycle | `cometbft-lifecycle-production-v1` |
  | Reward and issuance | `production` |

  - A production configuration must carry recovery, ordinary and governance,
    with penalties activated.
- **Chain ID.** A development build refuses a chain ID naming mainnet or
  production, in the node as well as the engine and supervisor; a production
  build allows it.
- **Fail closed.** A production build opens no chain until A2 and refuses root
  controls until A4.
- **CI** checks both builds.

**A2 done** (root genesis 3-of-5):
- **Threshold root** (`src/root_genesis/threshold.rs`). Two public records
  reach every node unchanged:
  - the signer policy: five SLH-DSA-SHAKE-256s keys, threshold 3;
  - the combined signatures file: three to five signatures over one genesis
    envelope, sorted by key ID.
  - The node verifies every listed signature through the pinned helper, one
    run per signature, on every start.
- **Receipt v2** lists the policy SHA-256, the threshold, the signing key IDs
  and the signatures file SHA-256. It is consensus state, so every node must
  read the same signatures file.
- **Production entry.** `ConsensusApplication::open_with_root` and
  `consensus_stdio --root-config` exist in both builds.
  - A production build opens only through them; `production_open` accepts
    only a three-of-five receipt.
  - Emergency, upgrade and handover controls stay refused on this path
    until A4, which also adds the check that the genesis keys are disjoint
    from them.
- **Offline signer** `dytallix-root-sign` (keygen, policy, digest, sign,
  combine, verify). The procedure is in
  [root genesis signing](../mainnet/root-genesis-signing.md).
- **Helper wire v2.**
  - `VerificationResult` drops `ProductionQualified`.
  - READY and ACK end in `v2`, so a node and helper of different versions
    fail at READY.
  - Qualification belongs to the accepted release that pins the helper.
- **Tests.**
  - A signed three-of-five test (five generated keys, three and four
    signatures, restart, refusals) runs on development and production builds
    in CI.
  - Unit tests cover the record rules and helper refusals.

**A3 done** (upgrade schema 2 and handover v2):
- **Upgrade schema 2** (`src/upgrade/v2/upgrade.rs`) is a second retained
  implementation beside v1, which is unchanged byte for byte.
  - Exactly five keys, threshold 3, key IDs the SHA-256 of the public keys,
    disjoint from the emergency keys.
  - Activation at least `min_notice_blocks` after admission.
  - Every signature binds a finalized anchor (height and application hash)
    and a window, checked against `max_validity_blocks` and
    `max_anchor_age_blocks`. The root envelope's validity is the same window.
  - The migration runs in the active release that committed handover history
    selects; schema 1 bound the genesis release.
- **Registry.** The registry lists the v2 implementation beside v1, so its
  digest, which release manifests bind, changed before the E06 freeze.
  `upgrade.rs` pins both sources and dispatches between them.
- **Handover schema 2** (`src/release_handover.rs`, the emergency freeze v2
  pattern: optional `v2` members, so schema 1 bytes are unchanged).
  - The same anchored window and the same notice (P01, 1 October 2026: the
    notice applies to handovers).
  - The configuration check requires the upgrade authority's keys, threshold
    and epoch, and pairs schema 2 handovers only with schema 2 upgrades.
  - Restart keeps its exact-height binding.
- **Adapter.** Planning, CheckTx and history replay read each schema 2
  anchor from committed history; an anchor beyond its age bound is a
  deterministic refusal. Recorded anchors are kept with the block history,
  and the retention floor covers the largest anchor age.
- **Tests.** Unit tests for both schemas, and signed tests through the
  adapter: an anchored upgrade with its notice, migration and restart replay,
  and a handover paired with the migration after its notice.
- **Not yet.** Production builds still refuse these controls until A4; the
  window bounds are measured values (E05).

## Still required after activation

Every record (operators, custodians, genesis signers, beneficiaries, chain
identity, hosts, the pin plan), the operating-value batch, the measured
windows and budgets, T03 native qualification, the E06 release freeze, the P02
independent review by a person, the T-suites and gate acceptance. The root
signers sign last.
