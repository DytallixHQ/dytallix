# Emergency custodian intake

Engineering task E05. This packet collects the public records for the
emergency authority, which freezes user transactions while consensus
continues and later resumes them
([emergency transaction freeze](../../../node/docs/mainnet/emergency-transaction-freeze.md)).
It does not appoint custodians, verify signatures or create production
configuration. A production node refuses a configuration without the
emergency freeze at schema 2
([root controls](../../approvals/P01_E05_ROOT_CONTROLS_2026-10-01.json)).

This intake comes first. The [upgrade](../upgrade/INTAKE.md) and
[genesis signer](../genesis/INTAKE.md) checkers compare their custodians and
keys with it and cannot finish without a complete emergency packet.

## Five key holders (P01, 7 October 2026)

[Root key holders](../../approvals/P01_E05_ROOT_KEY_HOLDERS_2026-10-07.json):
five people, the founder and four others, hold the five kits, one each, and
each makes their own kit ([key ceremony](../KEY_CEREMONY.md)). Each slot is
one holder:

- **Controller.** A distinct `controller_id` that names the kit, never the
  person (for example `kit-holder-3`). Names stay in the working packet in
  the custody system.
- **Kits.** The slot's `control_group`, and its keys' signer and backup
  groups, is `kit-N`, as before; the emergency, upgrade and genesis packets
  use the same five.
- **Backup.** A kit has no backup: its backup record says the key is on the
  holder's encrypted fob only, and a lost kit is replaced by the kit
  replacement control.
- **Model.** `"custody_model": "five_holders"`. Neither older model fits:
  `solo_kits` requires one controller in every slot, and `independent`
  refuses a holder or kit that also holds another role. Under
  `five_holders` slot N must be `kit-holder-N` in `kit-N`, the five holders
  must be the same in all three packets, and every slot's
  `independence_review` is null: the public disclosure in the trust model
  stands in for it (P01, 10 October 2026,
  [approval](../../approvals/P01_E05_HOLDER_DISCLOSURE_2026-10-10.json)). Check the three packets together with
  `five_holder_custody.py` (see [the tools](../../../node/tools/mainnet-preparation/README.md)).

## Solo launch profile (kits superseded on 7 October 2026)

P01 replaced the separate groups of independent people on 3 October 2026
([solo launch](../../approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json),
[trust model](../../TRUST_MODEL.md)). The founder holds every root key in five
key kits: kit N holds key N of each role (genesis, upgrade, freeze and
resume). Any three kits can act, and two can be lost or stolen safely. Keys
stay distinct per role, as the node requires, and the thresholds and
parameter set are unchanged.

Fill a solo packet with `"custody_model": "solo_kits"`:

- **Controller.** The same `controller_id`, name and organization in all five
  slots.
- **Kits.** Each slot's `control_group`, and the signer and backup groups of
  both its keys, is that slot's kit identifier (for example `kit-1` to
  `kit-5`). The five kits must be distinct, and the upgrade and genesis
  packets must use the same five.
- **Review.** `independence_review` is null in every slot; the public
  disclosure ([TRUST_MODEL.md](../../TRUST_MODEL.md)) replaces it. The
  appointment, key records, proof of possession and drill evidence are still
  required.

A packet with `"custody_model": "independent"` keeps the original rules: five
custodians with distinct controllers and control groups, each independently
reviewed.

## Approved policy

- **Authority** (D11-Q03, P01): three signatures from five custodians freeze,
  and three resume, with distinct freeze and resume keys. Resume binds the
  freeze it ends.
- **Parameter set** (P01, 30 September 2026,
  [custody approval](../../approvals/P01_E05_CUSTODY_2026-09-30.json)):
  SLH-DSA-SHAKE-256s, the set the node's root verifier implements: 64-byte
  public keys and 29,792-byte signatures.

The node's emergency policy (schema 2) has one authority epoch and two key
sets of exactly five keys, one for freeze and one for resume, each with
threshold three. So each custodian holds one freeze key and one resume key,
ten distinct keys in all. The node refuses a key that holds two of the
emergency, upgrade and genesis roles.

## What stays out of this repository

This repository is public. The completed packet, custodian names and
organizations, and the evidence files stay in the custody system. Never
upload private keys, seeds, recovery shares, passwords, signer endpoints,
backup locations or access tokens anywhere in this packet.

Only the checker's `authority_fragment` (public keys, key IDs, the epoch and
the thresholds) and the digests of the completed packet and its check enter
the repository, as `root.emergency` in the genesis records.

## Required inputs

1. Five custodians, or for solo kits the founder in five slots. For each
   slot, one stable controller identifier and one control-group identifier.
2. Who can control each signer, backup and recovery process, as public record
   identifiers, never secrets or access instructions.
3. An explicit emergency-role appointment and acceptance per slot, stating
   the freeze and resume scope, the authority epoch and the replacement
   procedure.
4. The key ceremony: ten SLH-DSA-SHAKE-256s public keys, a freeze key and a
   resume key per slot, with public signer and backup procedure records.
5. Proof of possession and a drill for each key. The challenge binds the
   chain (`dytallix-mainnet-1`), the controller, the purpose (`freeze` or
   `resume`), the authority epoch, the public-key digest and the ceremony
   session. A verifier checks the actual signatures and records its version,
   result and evidence digest. This checker does not.
6. For independent custodians, a review of control groups, key bindings,
   signing procedures and recovery access by a person outside every
   custodian's control group.

## Fill the packet

Copy `PUBLIC_INTAKE.template.json` to a working packet in the custody system.
Complete every null field and keep `production_accepted` false. The
thresholds, size and parameter set are fixed by the approvals above.

- **Keys.** Canonical base64 of the 64 public-key bytes. `key_id` is the
  lowercase SHA-256 of those bytes; the authority fragment uses the same
  identifier and sorts each key set by it, as the node requires.
- **Epoch.** `authority_epoch` is one explicit positive integer for the whole
  authority. Never infer it from a file name.
- **Evidence.** One public JSON summary per reference, started from
  `PUBLIC_EVIDENCE.template.json`, listed in the packet's `evidence` map with
  its relative path and lowercase SHA-256. Files stay under 1 MiB. Absolute
  paths, parent paths, symbolic links, duplicate JSON keys and unreferenced
  evidence are refused.

| Kind | Binding |
| --- | --- |
| `profile_approval` | Parameter set. Controller, purpose, key and epoch are null. |
| `appointment` | Controller and parameter set. Purpose, key and epoch are null. |
| `independence_review` | Controller, parameter set and the reviewer's control group. Purpose, key and epoch are null. Not used for solo kits. |
| `signer_record` | Controller, purpose `freeze` or `resume`, that key's ID, parameter set and authority epoch. |
| `backup_record` | The same. |
| `proof_of_possession` | The same. |
| `drill_record` | The same. |

`public_statement` holds the public summary and underlying record
identifiers. It is not a verified signature. A digest shows that a file
matches; it does not show who made or approved it.

## Run the checker

From `node`:

```text
python3 -B tools/mainnet-preparation/emergency_custodian_intake.py \
  EMERGENCY_INTAKE.working.json --output EMERGENCY_INTAKE_CHECK.json
```

- `INCOMPLETE_OR_INVALID` (exit code 2) blocks handoff. The unchanged
  template returns it.
- `STRUCTURALLY_COMPLETE` (exit code 0) means only that fields, key
  encodings, distinct keys, control groups and local evidence bindings
  passed. The result then carries `authority_fragment` in the shape of
  `root.emergency` in the genesis records. It never reports production
  acceptance.

Then pass the same working packet to the upgrade and genesis checkers with
`--emergency`.

## Acceptance

Under the solo launch profile the founder fills, checks and signs off this
packet, and the public review and labeled AI review replace independent
review ([trust model](../../TRUST_MODEL.md)). With independent custodians,
each signs their own acceptance and proves possession, and an independent
reviewer checks affiliation and control claims before the production
approver signs the final record.

This packet grants only the freeze and resume roles. Upgrade, handover and
genesis authority are separate inputs.
