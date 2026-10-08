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

- **Payload.** The chain ID and genesis digest; the current authority epoch;
  the seat; the new upgrade, freeze and resume public keys
  (SLH-DSA-SHAKE-256s); the new kit's three proofs of possession, each
  binding the chain, the seat's controller, its purpose and the next epoch;
  the next upgrade sequence number; and an anchored window like the other
  schema 2 controls (anchor height and hash, first and last height, within
  the upgrade policy's validity and anchor-age bounds).
- **Signatures.** Three distinct current upgrade keys sign it through the
  root helper under the `upgrade` action, with an artifact domain of its own,
  so no other control's signature can be reused for it.
- **Ordering.** It consumes one upgrade sequence number, so it is ordered
  with upgrades and handovers and cannot be replayed.
- **Size.** Six SLH-DSA signatures (three authorizing, three proofs) of
  29,792 bytes each, plus the payload: about 180 KB. Its own size bound is
  derived from that, like the other controls' bounds.

### Admission

The node admits a replacement only if:

- three distinct current upgrade keys signed it, within its window, with an
  anchor the node holds;
- the epoch is the current one and no other replacement is pending;
- the seat is 1 to 5;
- each new key is a valid public key, differs from every key the record
  holds or has ever held, and its proof of possession verifies.

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
- **During a freeze** a replacement is admitted and takes effect like any
  other root control; a freeze stops ordinary transactions, not the root
  authority.

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

## What does not change

Five seats, the threshold of three per role, the SLH-DSA parameter set, the
genesis keys, and every window, notice and size bound in the configuration.

## Code

- **Node.** The authority record and the replacement control, with its
  admission, notice and effect, in the consensus application. The upgrade
  schema 2 module is hash-pinned (`V2_SHA256` and the registry); K2a
  re-pinned it for the rules hash, and K2b re-pins it if it changes it.
- **Root helper.** Unchanged: it verifies single SLH-DSA signatures with the
  keys the node passes it, now taken from the record.
- **Tools.** `dytallix-control prepare` gains the replacement, and
  `dytallix-root-sign show-control` and `sign-control` show the seat and the
  new key IDs before signing. The new holder makes their kit and proofs in
  the [key ceremony](../../../launch/custody/KEY_CEREMONY.md) with the next
  epoch.
- **Genesis builder and binding review.** They check that the emergency and
  upgrade epochs are equal; the record's starting value derives from the
  configuration.

## Steps

| Step | Content | Output change |
| --- | --- | --- |
| K1 | The authority record, its epochs and each epoch's policies; one authority epoch in the configuration | None: no record until a replacement |
| K2a | Schema 2 policy hashes over the rules, not the keys; every check through the policy in force at its height | Schema 2 policy hashes (no chain runs them yet) |
| K2b | The replacement control: admission, notice, effect (the record and the epoch) | New control |
| K3 | Tools: prepare, show, sign and assemble a replacement; proofs for the next epoch | New tool operations |
| K4 | Staging drill: replace a seat; the old kit is refused after the effect height and the new kit signs a freeze and a resume | Evidence |

## Tests

- K1: a chain's controls verify exactly as before; a restart and a node
  restart read the record.
- K2a: a schema 2 state, receipt and payload keep their policy hash across
  epochs and lose it with a changed rule; a replay checks each receipt with
  the keys in force at its block; a block before the effect height takes
  the old keys, and from it only the new ones, under either epoch number;
  a stored record is refused until K2b.
- K2b: admission refuses two signers, a repeated signer, a freeze or resume
  key, an old epoch, a stale anchor, a closed window, a seat outside 1 to 5,
  a reused key, a bad proof and a second pending replacement; the notice
  holds the old keys until the effect height; after it the old keys are
  refused and the new ones accepted; a replayed replacement is refused.
- K3: the signed fixture runs prepare, sign and assemble end to end.
