# Upgrade custodian intake

Engineering task E05, step c. This packet collects the public records for the
approved upgrade authority. It does not appoint custodians, verify signatures
or create production configuration. Upgrade schema 2 and handover schema 2,
which enforce this authority, are implemented (production activation v1, step
A3); a production build requires them (step A4).

## Solo launch profile

P01 replaced the separate groups of independent people on 3 October 2026
([solo launch](../../approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json),
[trust model](../../TRUST_MODEL.md)). The founder holds every root key in five
key kits: kit N holds key N of each role (genesis, upgrade, freeze and
resume), on encrypted drives kept in up to five separate places. Any three
kits can act, and two can be lost or stolen safely. Keys stay distinct per
role, as the node requires, and the thresholds and parameter set are
unchanged.

Fill a solo packet with `"custody_model": "solo_kits"`:

- **Controller.** The same `controller_id`, name and organization in all five
  slots.
- **Kits.** Each slot's `control_group`, and its key's signer and backup
  groups, is that slot's kit identifier (for example `kit-1` to `kit-5`). The
  five kits must be distinct, and the emergency, upgrade and genesis packets
  must use the same five.
- **Review.** `independence_review` is null in every slot; the public
  disclosure ([TRUST_MODEL.md](../../TRUST_MODEL.md)) replaces it. The
  appointment, key records, proof of possession and drill evidence are still
  required.

The checker refuses a second controller, a repeated kit, a review reference,
or another controller or kit set in the other intakes. Keys stay distinct
across every role. A packet with `"custody_model": "independent"` keeps the
original rules.

## Approved policy

- **Custodians** (D11-Q03, P01, 30 September 2026,
  [operations approvals](../../approvals/P01_E04_OPERATIONS_2026-09-30.json)):
  five independent upgrade custodians, distinct from the emergency freeze and
  resume custodians. Three signatures admit an upgrade and three fresh ones
  activate it. Upgrades stay root-signed only.
- **Notice** (P01, 30 September 2026,
  [second value set](../../approvals/P01_E05_VALUES_2_2026-09-30.json)):
  activation at least 120,960 blocks (7 days) after admission.
- **Parameter set** (P01, 30 September 2026,
  [custody approval](../../approvals/P01_E05_CUSTODY_2026-09-30.json)):
  SLH-DSA-SHAKE-256s, the set the node's root verifier implements: 64-byte
  public keys and 29,792-byte signatures.

The node's upgrade authority (schema 2, `upgrade/v2/upgrade.rs`) is one key
set of exactly five keys with one authority epoch, so each custodian holds one
upgrade key. Admission and activation each need three signatures from that
set, each over an anchored window. Release handovers (schema 2) and halt
restarts use the same keys, threshold and epoch. The configuration check
refuses a key that also holds a freeze or resume role, and a handover
authority that differs from the upgrade authority.

## What stays out of this repository

This repository is public. The completed packet, custodian names and
organizations, and the evidence files stay in the approved custody system,
with the emergency intake. Never upload private keys, seeds, recovery shares,
passwords, signer endpoints, backup locations or access tokens anywhere in
this packet.

Only the checker's `authority_fragment` (public keys, key IDs, the epoch and
the threshold) and the digests of the completed packet and its check enter
the repository, through the genesis builder (E05-d).

## Order

1. The emergency custodian intake comes first. The checker compares the two
   groups and cannot finish without a complete emergency packet.
2. Proof of possession and drills bind the chain ID, now approved as
   `dytallix-mainnet-1` (D13-Q01, P01, 3 October 2026;
   [identity](../../genesis/IDENTITY.json)).

## Required inputs

1. Five custodian names or organization references. For each, one stable
   controller identifier and one control-group identifier. None may appear
   in the emergency intake.
2. Who can control each signer, backup and recovery process, including shared
   employers, owners, administrators, signing devices, vault services and
   recovery dependencies. Use public record identifiers, never secrets or
   access instructions.
3. An explicit upgrade-role appointment and acceptance from each custodian,
   stating the admission and activation scope, the authority epoch and the
   replacement procedure.
4. The approved key ceremony: five SLH-DSA-SHAKE-256s public keys, one per
   custodian, with public signer and backup procedure records.
5. Proof of possession and a drill for each key. The challenge binds the
   chain, the controller, the purpose `upgrade`, the authority epoch, the
   public-key digest and the ceremony session. A qualified verifier checks the
   actual signatures and records its version, result and evidence digest.
   This checker does not.
6. Independent review of control groups, key bindings, signing procedures and
   recovery access, by a person outside every upgrade custodian's control
   group. Resolve conflicts before acceptance.

## Fill the packet

Copy `PUBLIC_INTAKE.template.json` to a working packet in the custody system.
Complete every null field and keep `production_accepted` false. The
threshold, size and parameter set are fixed by the approvals above.

- **Control groups.** Use one control-group identifier for everything under
  common signing or recovery control. Five key pairs do not make five
  independent custodians. The checker requires five distinct groups, refuses
  a signer or backup in another custodian's group, and refuses any group that
  appears in the emergency intake. It cannot prove that declared groups are
  independent.
- **Keys.** Canonical base64 of the 64 public-key bytes. `key_id` is the
  lowercase SHA-256 of those bytes; the authority fragment uses the same
  identifier and sorts the keys by it, as the node requires.
- **Epoch.** `authority_epoch` is one explicit positive integer for the whole
  group. Never infer it from a file name.
- **Evidence.** One public JSON summary per reference, started from
  `PUBLIC_EVIDENCE.template.json`, listed in the packet's `evidence` map with
  its relative path and lowercase SHA-256. Files stay under 1 MiB. Absolute
  paths, parent paths, symbolic links, duplicate JSON keys and unreferenced
  evidence are refused.

| Kind | Binding |
| --- | --- |
| `profile_approval` | Parameter set. Controller, purpose, key and epoch are null. |
| `appointment` | Controller and parameter set. Purpose, key and epoch are null. |
| `independence_review` | Controller, parameter set and the reviewer's control group. Purpose, key and epoch are null. |
| `signer_record` | Controller, purpose `upgrade`, key ID, parameter set and authority epoch. |
| `backup_record` | The same. |
| `proof_of_possession` | The same. |
| `drill_record` | The same. |

`public_statement` holds the public summary and underlying record
identifiers. It is not a verified signature. A digest shows that a file
matches; it does not show who made or approved it.

## Run the checker

From `mainnet/node`:

```text
python3 -B tools/mainnet-preparation/upgrade_custodian_intake.py \
  UPGRADE_INTAKE.working.json \
  --emergency EMERGENCY_INTAKE.working.json \
  --output UPGRADE_INTAKE_CHECK.json
```

- `INCOMPLETE_OR_INVALID` (exit code 2) blocks handoff. The unchanged
  template returns it.
- `STRUCTURALLY_COMPLETE` (exit code 0) means only that fields, key
  encodings, emergency separation and local evidence bindings passed. The
  result then carries `authority_fragment` in the node's upgrade authority
  shape. It never reports production acceptance.

## Acceptance handoff

The custody lead coordinates intake. Custodians sign their own acceptances
and prove possession. An independent reviewer checks affiliation and control
claims. The production approver checks the completed evidence and signs the
final record under the accepted release process. One person cannot hold all
these roles.

Exercise unavailable custodians, replacement and recovery with the appointed
custodians, and record the exact epoch. Map the accepted output to the
D10-Q03 signing-role records, D14-Q03 role acceptances and D11-Q03 upgrade
authority. Upgrade clearance, full-halt and restart authorities remain
separate inputs; this packet grants none of them.
