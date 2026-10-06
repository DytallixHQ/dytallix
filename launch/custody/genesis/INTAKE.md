# Genesis signer intake

Engineering task E05, records. This packet collects the public records for the
five genesis signers who sign the root genesis three of five (production
activation v1, step A2). It does not appoint signers, verify signatures or
authorize a launch. On a complete packet the checker emits the public signer
policy that [root genesis signing](../../../node/docs/mainnet/root-genesis-signing.md)
uses.

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

- **Signers** (P01, 30 September 2026,
  [activation approval](../../approvals/P01_E05_ACTIVATION_2026-09-30.json)):
  five genesis signers in their own group, separate from the emergency freeze
  and resume custodians and from the upgrade custodians. Three signatures over
  the root bundle start the chain.
- **Parameter set** (P01, 30 September 2026,
  [custody approval](../../approvals/P01_E05_CUSTODY_2026-09-30.json)):
  SLH-DSA-SHAKE-256s, which the root verifier implements: 64-byte public keys
  and 29,792-byte signatures.
- **When they sign** (P01, 30 September 2026): only after the P02 review, E06
  release acceptance and gate acceptance. Signing is the last step. Under the
  solo launch profile the P02 review is the 30-day public review and the
  founder's sign-off (P01, 3 October 2026).

The node refuses a genesis signer key that also holds an emergency, upgrade or
handover role. This checker also refuses a shared controller or control group.

## What stays out of this repository

This repository is public. The completed packet, signer names and
organizations, and the evidence files stay in the approved custody system,
with the emergency and upgrade intakes. Never upload private keys, seeds,
recovery shares, passwords, signer endpoints, backup locations or access
tokens anywhere in this packet.

Only the checker's `signer_policy` (public keys, key IDs, the chain ID and the
threshold), its SHA-256 and the digests of the completed packet and its check
are published. The policy is a public record every node reads.

## Order

1. The emergency custodian intake, then the upgrade custodian intake. The
   checker compares all three groups and cannot finish without both.
2. The chain identity (D13-Q01), approved as `dytallix-mainnet-1`. The policy
   names the chain, and proof of possession binds it.
3. Proof of possession and drills, after the key ceremony.

## Required inputs

1. Five signer names or organization references. For each, one stable
   controller identifier and one control-group identifier. None may appear in
   the emergency or upgrade intake.
2. Who can control each signer, backup and recovery process, including shared
   employers, owners, administrators, signing devices, vault services and
   recovery dependencies. Use public record identifiers, never secrets or
   access instructions.
3. An explicit genesis-signer appointment and acceptance from each signer,
   stating that they sign one root genesis bundle for the named chain only
   after the review, release and gate acceptances above.
4. The approved key ceremony: each signer runs `dytallix-root-sign keygen` on
   their own device and hands over only the public record it writes,
   `{key_id, public_key_hex}`.
5. Proof of possession and a drill for each key. The challenge binds the
   chain, the controller, the purpose `genesis`, the public-key digest and the
   ceremony session. A qualified verifier checks the actual signatures and
   records its version, result and evidence digest. This checker does not.
6. Independent review of control groups, key bindings, signing procedures and
   recovery access, by a person outside every signer's control group.

## Fill the packet

Copy `PUBLIC_INTAKE.template.json` to a working packet in the custody system.
Complete every null field and keep `production_accepted` false. The
threshold, size and parameter set are fixed by the approvals above.

- **Chain.** `chain_id` is the approved chain identity, `dytallix-mainnet-1`
  (D13-Q01, P01, 3 October 2026; [identity](../../genesis/IDENTITY.json)). The
  checker accepts letters, digits, `.`, `_` and `-`, at most 128 characters,
  so a staging rehearsal can use its own chain.
- **Keys.** Copy `key_id` and `public_key_hex` from each signer's public key
  record unchanged: 128 lowercase hex characters, and the key ID is their
  SHA-256.
- **Control groups.** One control-group identifier for everything under
  common signing or recovery control. The checker requires five distinct
  groups, and refuses a signer or backup in another signer's group or in any
  emergency or upgrade group. It cannot prove that declared groups are
  independent.
- **Evidence.** One public JSON summary per reference, from
  `PUBLIC_EVIDENCE.template.json`, listed in the packet's `evidence` map with
  its relative path and lowercase SHA-256. The evidence rules match the
  upgrade intake, except that the keyed records name the purpose `genesis`
  and have no epoch.

| Kind | Binding |
| --- | --- |
| `profile_approval` | Parameter set. Controller, purpose, key and epoch are null. |
| `appointment` | Controller and parameter set. Purpose, key and epoch are null. |
| `independence_review` | Controller, parameter set and the reviewer's control group. Purpose, key and epoch are null. |
| `signer_record`, `backup_record`, `proof_of_possession`, `drill_record` | Controller, purpose `genesis`, key ID and parameter set. Epoch is null. |

## Run the checker

From `node`:

```text
python3 -B tools/mainnet-preparation/genesis_signer_intake.py \
  GENESIS_INTAKE.working.json \
  --emergency EMERGENCY_INTAKE.working.json \
  --upgrade UPGRADE_INTAKE.working.json \
  --output GENESIS_INTAKE_CHECK.json --policy-out root-genesis-policy.json
```

- `INCOMPLETE_OR_INVALID` (exit code 2) blocks handoff. The unchanged
  template returns it.
- `STRUCTURALLY_COMPLETE` (exit code 0) means only that fields, key
  encodings, separation and local evidence bindings passed. The result
  carries `signer_policy`: the public record, byte for byte what
  `dytallix-root-sign policy` writes from the same five public key records.
  `--policy-out` writes it to a new file. It never reports production
  acceptance.

## Acceptance handoff

The custody lead coordinates intake. Signers sign their own acceptances and
prove possession. An independent reviewer checks affiliation and control
claims. The production approver checks the completed evidence and signs the
final record. One person cannot hold all these roles. Map the accepted output
to the D10-Q03 signing-role records and the D14-Q03 role acceptances.
