# Root kit replacement v1

Engineering task E05. A root control, signed by three of the five key kits,
that replaces one seat's keys, so a lost fob or a departing holder is
replaced instead of being lost for the chain's life
([root key holders](../../../launch/approvals/P01_E05_ROOT_KEY_HOLDERS_2026-10-07.json)).

## Problem

- **The key sets are fixed.** The freeze and resume key sets are in the
  emergency policy, and the upgrade key set in the upgrade policy (schema
  2), both in the genesis-bound application configuration, each with an
  authority epoch. The release handover policy reuses the upgrade key set
  and epoch, and a restart is signed by the same keys. Every control is
  checked against the configuration, and nothing changes it.
- **Kits have no backup.** Each kit is on one fob (P01, 7 October 2026). One
  lost kit leaves four, two leave three with no margin, and three lost
  leave no one able to freeze, resume or upgrade the chain again.

## Decisions (P01, 8 October 2026)

[Approval](../../../launch/approvals/P01_E05_KIT_REPLACEMENT_2026-10-08.json):

1. **Signers.** One control, signed by three of the five current upgrade
   keys, replaces one seat's upgrade, freeze and resume keys together.
2. **Notice.** It takes effect after the release handover notice, 120,960
   blocks (seven days); until then the old keys keep working.
3. **Scope.** One seat per control. Five seats and the threshold of three
   stay fixed.

## Rule

### The authority record

- **State.** A consensus record, `consensus:root-authority:v1`, holds
  every authority epoch: its number, the first height its keys sign for,
  and its three key sets of five, upgrade, freeze and resume. The first
  replacement writes it; until then the configuration's key sets are in
  force, so the genesis and its state are unchanged. The configuration must
  give the emergency and upgrade policies the same authority epoch (1 at
  launch). No new genesis input.
- **Every check reads it.** Emergency freeze and resume, upgrades, release
  handovers and restarts take their keys and epoch from the record, not the
  configuration. Thresholds, windows, anchor ages, notices and size bounds
  stay in the configuration.
- **Genesis keys** are not in the record. They sign the genesis once and are
  not replaced.

### The replacement control

- **Payload** (`dytallix-root-kit-replacement-v1`):
  - the chain ID and genesis digest, and the upgrade policy's hash;
  - the current authority epoch, and the sequence, which is the epoch the
    replacement creates;
  - the seat, named by its three key IDs, one per role (upgrade, freeze and
    resume);
  - the new kit's three public keys (SLH-DSA-SHAKE-256s), each under its
    SHA-256 key ID;
  - an anchored window like the other schema 2 controls: anchor height and
    hash, first and last height, within the upgrade policy's validity and
    anchor-age bounds.
- **Signatures.** At least three distinct current upgrade keys sign it
  through the root helper under the `upgrade` action. Its artifact domain
  (`DYTALLIX/ROOT-KIT-REPLACEMENT/v1`) is its own, so no other control's
  signature can be reused for it.
- **Proofs of possession.** Each new key signs the same artifact, through
  the same helper action. The proof binds the chain, the replaced seat, the
  role's key and the next epoch, and shows that the new kit holds each key.
- **Ordering.** Its sequence is the epoch it creates, not an upgrade
  sequence number. Once it is admitted, no other is admitted until its epoch
  is in force, and from then on a control for the old epoch is refused; so
  none is applied twice.
- **Size.** Six SLH-DSA signatures of 29,792 bytes each. In hex, as the
  other controls carry them, they would not fit the 262,144-byte transaction
  bound, so this control carries them in base64: about 240 KB in all. The
  upgrade policy's control bound applies.

### Admission

The node admits a replacement only if:

- the chain is not frozen and the block has no emergency control (P01,
  8 October 2026,
  [approval](../../../launch/approvals/P01_E05_KIT_REPLACEMENT_FREEZE_2026-10-08.json));
- three distinct current upgrade keys signed it, within its window, with an
  anchor the node holds;
- the epoch is the current one and no other replacement is pending;
- each replaced key ID is a current key of its role;
- each new key's ID is its SHA-256, the key differs from every key the
  record holds or has ever held, and its proof of possession verifies.

### Effect

- **Notice.** Admitted at height A, it takes effect at A + 120,960. Until
  then it is pending, public in state and in the node's query, and the old
  keys keep working.
- **At the effect height** the record replaces the seat's three keys and
  increments the epoch. From then on, a control or restart authorization
  bound to the old epoch is refused, and holders sign for the new one.
- **Controls already admitted** (for example a handover waiting for its own
  notice) keep their admission: they were valid under the authority that
  admitted them.
- **During a freeze** no replacement is admitted, as no upgrade or handover
  is (P01, 8 October 2026). The freeze stays a full brake on the upgrade
  keys, and no recovery is lost: the three kits that could sign a
  replacement can sign a resume first. A replacement admitted before the
  freeze still takes effect at its effect height.

### Implementation notes

- **Each epoch is a policy.** An epoch's emergency, upgrade and handover
  policies are the configuration's with that epoch's keys and number.
- **Policy hashes cover the rules, not the keys (K2a).** The three policies
  are hashed into their states, receipts and signed payloads
  (`policy_sha256`). A schema 2 policy's hash leaves out its key sets and
  authority epoch, so a replacement does not change it: states and
  receipts stay valid across epochs and nothing is bound again at the
  effect height. A payload still names its authority epoch, and only that
  epoch's keys verify it. A changed rule (a threshold, window or bound)
  still changes the hash and needs a migration. Schema 1 policies hash as
  before.
- **Height decides.** A state is checked against the policy in force at the
  committed height; a receipt, a replayed control and a block plan against
  the one in force at their block. So a replay of history checks each
  control with the keys it was signed under. Decoding a control checks only
  sizes and counts, which stay the configuration's.
- **Built in K1:** `root_authority` (the record, its validation, the epoch in
  force at a height, and each policy for an epoch: the configuration's,
  borrowed, while there is no record) and the configuration rule that the
  emergency and upgrade epochs agree.
- **Built in K2a:** the rules hash, and every check through the policy in
  force at its height: the emergency, upgrade and handover block plans
  (which admission and finalize both run), handover admission, the startup
  replay of emergency receipts, the handover, restart and upgrade history
  replays, and the restart check and payload. Nothing writes the record
  yet, and every block, admission and startup refuses a stored one as an
  unknown consensus record; K2b makes it committed state and replays the
  replacements that wrote it.
- **Built in K2b:** the replacement control (`kit_replacement`):
  - Admission and finalize take it in a block of its own, as the other root
    controls.
  - It writes the record, starting from the configuration's epoch at
    height 0, plus a receipt per replacement
    (`consensus:root-authority:receipt:{sequence}`), all in the committed
    state.
  - A pending replacement is the record's last epoch while its first height
    is still ahead. The status query reports the epoch in force and the
    pending one.
  - Startup and the history check rebuild the record and receipts from the
    committed replacements, byte for byte, and with the root helper at
    startup. Blocks with a replacement, and the anchors replacements name,
    are kept below the retained window.
  - The structural replay of emergency receipts, and the emergency history
    that upgrades check, also take each record's keys from its block's
    epoch (`recover_recorded_at`).
- **Built in K3:** the tools.
  - The status reports the keys of the epoch in force.
  - `control_request` prepares every control with that epoch and its keys,
    ends a window the block before a pending replacement's effect height,
    and assembles against the status's keys (`assemble --status`), refusing
    a request for another epoch.
  - The `kit-replacement` operation, the signer's control kind, and the
    CLI's.
- **Built in K4:** the drill, in the CI staging network on every change
  (P01, 8 October 2026,
  [approval](../../../launch/approvals/P01_E05_KIT_REPLACEMENT_DRILL_2026-10-08.json);
  `staging_network.py kit-drill`). The staging chain's freeze, resume and
  upgrade keys come from five throwaway kits made as the key ceremony makes
  them, and its upgrade and handover notice is a staging-only 20 blocks.
  From the runner, with the release's tools, over the endpoint's client
  channel:
  - pin the chain (`dytallix config pin-chain`);
  - a new kit for seat 5;
  - prepare, show, sign (kits 1, 3 and 4, and the new kit's three keys),
    assemble, check and submit the replacement;
  - wait for its effect height;
  - a freeze signed with the old kit is refused by assembly and by the
    node;
  - the new kit freezes and then resumes the chain.

  It prints the evidence: the key IDs, the control digests and the
  heights.

## What does not change

Five seats, the threshold of three per role, the SLH-DSA parameter set, the
genesis keys, and every window, notice and size bound in the configuration.

## Code

- **Node.** The authority record and the replacement control, with its
  admission, notice and effect, in the consensus application. The upgrade
  schema 2 module is hash-pinned (`V2_SHA256` and the registry); K2a
  re-pinned it for the rules hash, and K2b for the per-epoch emergency
  history.
- **Root helper.** Unchanged: it verifies single SLH-DSA signatures with the
  keys the node passes it, now taken from the record.
- **Tools** ([control signing](../mainnet/control-signing.md#kit-replacement)).
  `dytallix-control prepare kit-replacement` builds the request from the
  leaving and the new kit's public key records, and `dytallix-root-sign
  show-control` and `sign-control` show the seat and the new key IDs before
  signing. The new holder makes their kit with `dytallix-root-sign kit` in
  the [key ceremony](../../../launch/custody/KEY_CEREMONY.md), with its
  custody proofs for the next epoch.
- **Genesis builder and binding review.** They check that the emergency and
  upgrade epochs are equal; the record's starting value derives from the
  configuration.

## Steps

| Step | Content | Output change |
| --- | --- | --- |
| K1 | The authority record, its epochs and each epoch's policies; one authority epoch in the configuration | None: no record until a replacement |
| K2a | Schema 2 policy hashes over the rules, not the keys; every check through the policy in force at its height | Schema 2 policy hashes (no chain runs them yet) |
| K2b | The replacement control: admission, notice, effect (the record and the epoch); no replacement while frozen | New control and committed state |
| K3 | Tools: prepare, show, sign and assemble a replacement; every control prepared and assembled with the epoch in force | New tool operation; `assemble` takes the status |
| K4 | Staging drill in CI: replace a seat; the old kit is refused after the effect height and the new kit signs a freeze and a resume | Evidence on every CI run |

## Tests

- K1: a chain's controls verify exactly as before; a restart and a node
  restart read the record.
- K2a: a schema 2 state, receipt and payload keep their policy hash across
  epochs and lose it with a changed rule; a replay checks each receipt with
  the keys in force at its block; a block before the effect height takes
  the old keys, and from it only the new ones, under either epoch number;
  a stored record is refused until K2b.
- K2b: admission refuses two signers, a repeated signer, a freeze or resume
  key, an old epoch, a stale anchor, a closed window, a key outside its
  role, a reused key, a bad proof, a second pending replacement and a
  replacement while frozen; the notice holds the old keys until the effect
  height; after it the old keys are refused and the new ones accepted; a
  replayed replacement is refused; a restart replays the replacement, and
  an upgrade after it checks emergency history across both epochs.
- K3: the signed fixture runs prepare, show, sign and assemble end to end
  with real kits: a replacement of seat 5, then a freeze signed with the new
  kit after the effect height; a window prepared while the replacement is
  pending ends before its effect height; the signer reads a request the
  node wrote; assembly refuses a key outside the epoch in force, a missing
  proof and a request for another epoch.
- K4: the staging network drill in CI, with the release's binaries,
  bundles and root helper on three hosts (the validator on the runner, the
  sentry and the endpoint in VMs).
